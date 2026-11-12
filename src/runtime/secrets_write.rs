//! The menu save of `secrets.toml` (openspec `mcp-secrets`, Menu save; D22):
//! the one write lazarobox makes to that file, and the deletion of its
//! backup. It works on open handles, not paths: the config folder is opened
//! `O_DIRECTORY | O_NOFOLLOW` and judged by `fstat`, the file and the
//! temporary file are opened relative to it, and the swap is a `renameat` on
//! it. Called only from the Connectors area's confirmed secret field (S6a2):
//! no model, tool, MCP call or config value can reach it. All I/O goes through
//! [`SaveFs`] so a test can make a step fail.

use std::{
    ffi::CString,
    fs::{DirBuilder, File, Metadata},
    io::{self, ErrorKind, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::fs::{DirBuilderExt, MetadataExt},
    },
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};

use tokio::sync::mpsc;

use super::config_append::connect_with;
use crate::{
    app::AppEvent,
    core::{
        ai::mcp_secrets::{
            BACKUP, CLIENT_BACKUP, CLIENT_FILE, FILE, MAX_BYTES, Refusal, SecretText, client_json,
            edit, edit_all, line_of,
        },
        config_append::{Connect, Connected},
    },
};

/// The file a write replaces, the backup it keeps, its size bound and its
/// mode rule.
#[derive(Clone, Copy)]
pub(super) struct Target {
    pub(super) file: &'static str,
    pub(super) backup: &'static str,
    pub(super) max: u64,
    /// `false`: the file must be 0600 and stays 0600. `true`: any mode
    /// without group or world write is kept (`config.toml`); a new file is
    /// 0600.
    pub(super) keep_mode: bool,
}

const SECRETS: Target = Target {
    file: FILE,
    backup: BACKUP,
    max: MAX_BYTES,
    keep_mode: false,
};
const CLIENT: Target = Target {
    file: CLIENT_FILE,
    backup: CLIENT_BACKUP,
    max: MAX_BYTES,
    keep_mode: false,
};

/// What a save did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    pub name: String,
    pub backed_up: bool,
    /// The OAuth client file, not `secrets.toml`.
    pub client: bool,
}

impl Saved {
    /// The success notice.
    pub fn notice(&self) -> String {
        let name = &self.name;
        if self.backed_up && self.client {
            format!("saved {name}; the previous file stays in {CLIENT_BACKUP} (0600)")
        } else if self.backed_up {
            format!("saved {name}; the previous values stay in {BACKUP} (0600) until you delete it")
        } else {
            format!("saved {name}")
        }
    }
}

/// What `fstat` or `fstatat` said about a name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stat {
    pub dev: u64,
    pub ino: u64,
    pub len: u64,
    pub mtime: (i64, i64),
    /// The permission bits only.
    pub mode: u32,
    pub uid: u32,
    pub nlink: u64,
    pub regular: bool,
}

impl Stat {
    fn same(&self, other: &Stat) -> bool {
        (self.dev, self.ino, self.len, self.mtime) == (other.dev, other.ino, other.len, other.mtime)
    }
}

impl From<Metadata> for Stat {
    fn from(m: Metadata) -> Self {
        Self {
            dev: m.dev(),
            ino: m.ino(),
            len: m.len(),
            mtime: (m.mtime(), m.mtime_nsec()),
            mode: m.mode() & 0o7777,
            uid: m.uid(),
            nlink: m.nlink(),
            regular: m.is_file(),
        }
    }
}

/// Every I/O step of the save, so a test can fail any of them.
pub trait SaveFs {
    fn euid(&self) -> u32;
    /// Fresh bits for a temporary name.
    fn random(&self) -> u64;
    /// The folder, created with mode 0700 when missing; `O_DIRECTORY | O_NOFOLLOW`.
    fn open_dir(&self, path: &Path) -> io::Result<OwnedFd>;
    fn fstat(&self, fd: &OwnedFd) -> io::Result<Stat>;
    /// `O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_NOCTTY`, relative to `dir`.
    fn open_read(&self, dir: &OwnedFd, name: &str) -> io::Result<OwnedFd>;
    /// At most `max` bytes.
    fn read(&self, fd: &OwnedFd, max: u64) -> io::Result<Vec<u8>>;
    /// `fstatat` without following a link.
    fn stat_at(&self, dir: &OwnedFd, name: &str) -> io::Result<Stat>;
    /// `O_CREAT | O_EXCL | O_NOFOLLOW | O_WRONLY`, mode 0600.
    fn create_excl(&self, dir: &OwnedFd, name: &str) -> io::Result<OwnedFd>;
    /// `fchmod` to 0600, whatever the umask.
    fn private(&self, fd: &OwnedFd) -> io::Result<()>;
    /// `fchmod` to `mode`, for a file that keeps its mode.
    fn set_mode(&self, fd: &OwnedFd, mode: u32) -> io::Result<()> {
        // SAFETY: `fd` is open.
        ok(unsafe { libc::fchmod(fd.as_raw_fd(), mode as libc::mode_t) })
    }
    fn write_all(&self, fd: &OwnedFd, bytes: &[u8]) -> io::Result<()>;
    fn fsync(&self, fd: &OwnedFd) -> io::Result<()>;
    fn rename(&self, dir: &OwnedFd, from: &str, to: &str) -> io::Result<()>;
    fn unlink(&self, dir: &OwnedFd, name: &str) -> io::Result<()>;
}

/// The real thing.
pub struct RealFs;

fn cstr(text: &str) -> io::Result<CString> {
    CString::new(text).map_err(|_| io::Error::from(ErrorKind::InvalidInput))
}

fn owned(ret: libc::c_int) -> io::Result<OwnedFd> {
    if ret < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `ret` is a fresh descriptor that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(ret) })
}

fn ok(ret: libc::c_int) -> io::Result<()> {
    if ret < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// The file of a duplicate of `fd`: std does the reads, writes and syncs.
fn file(fd: &OwnedFd) -> io::Result<File> {
    fd.try_clone().map(File::from)
}

impl SaveFs for RealFs {
    fn euid(&self) -> u32 {
        // SAFETY: no arguments, cannot fail.
        unsafe { libc::geteuid() }
    }

    fn random(&self) -> u64 {
        use std::hash::{BuildHasher, Hasher};
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u32(std::process::id());
        hasher.finish()
    }

    fn open_dir(&self, path: &Path) -> io::Result<OwnedFd> {
        let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
        let open = || {
            let c = cstr(&path.to_string_lossy())?;
            // SAFETY: `c` is a valid C string.
            owned(unsafe { libc::open(c.as_ptr(), flags) })
        };
        match open() {
            Err(e) if e.kind() == ErrorKind::NotFound => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                DirBuilder::new().mode(0o700).create(path)?;
                open()
            }
            other => other,
        }
    }

    fn fstat(&self, fd: &OwnedFd) -> io::Result<Stat> {
        file(fd)?.metadata().map(Stat::from)
    }

    fn open_read(&self, dir: &OwnedFd, name: &str) -> io::Result<OwnedFd> {
        let flags =
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_NOCTTY | libc::O_CLOEXEC;
        let c = cstr(name)?;
        // SAFETY: `dir` is open and `c` is a valid C string.
        owned(unsafe { libc::openat(dir.as_raw_fd(), c.as_ptr(), flags) })
    }

    fn read(&self, fd: &OwnedFd, max: u64) -> io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        file(fd)?.take(max).read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    #[allow(clippy::unnecessary_cast)] // the widths differ between platforms
    fn stat_at(&self, dir: &OwnedFd, name: &str) -> io::Result<Stat> {
        let c = cstr(name)?;
        // SAFETY: an all-zero `stat` is a valid value for the call to fill.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: `dir` is open, `c` is valid and `st` is writable.
        ok(unsafe {
            libc::fstatat(
                dir.as_raw_fd(),
                c.as_ptr(),
                &mut st,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        })?;
        Ok(Stat {
            dev: st.st_dev as u64,
            ino: st.st_ino as u64,
            len: st.st_size as u64,
            mtime: (st.st_mtime as i64, st.st_mtime_nsec as i64),
            mode: st.st_mode as u32 & 0o7777,
            uid: st.st_uid as u32,
            nlink: st.st_nlink as u64,
            regular: st.st_mode as u32 & libc::S_IFMT as u32 == libc::S_IFREG as u32,
        })
    }

    fn create_excl(&self, dir: &OwnedFd, name: &str) -> io::Result<OwnedFd> {
        let flags =
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC;
        let c = cstr(name)?;
        // SAFETY: `dir` is open and `c` is a valid C string.
        owned(unsafe { libc::openat(dir.as_raw_fd(), c.as_ptr(), flags, 0o600 as libc::c_uint) })
    }

    fn private(&self, fd: &OwnedFd) -> io::Result<()> {
        // SAFETY: `fd` is open.
        ok(unsafe { libc::fchmod(fd.as_raw_fd(), 0o600) })
    }

    fn write_all(&self, fd: &OwnedFd, bytes: &[u8]) -> io::Result<()> {
        file(fd)?.write_all(bytes)
    }

    fn fsync(&self, fd: &OwnedFd) -> io::Result<()> {
        file(fd)?.sync_all()
    }

    fn rename(&self, dir: &OwnedFd, from: &str, to: &str) -> io::Result<()> {
        let (from, to) = (cstr(from)?, cstr(to)?);
        let fd = dir.as_raw_fd();
        // SAFETY: `dir` is open and both names are valid C strings.
        ok(unsafe { libc::renameat(fd, from.as_ptr(), fd, to.as_ptr()) })
    }

    fn unlink(&self, dir: &OwnedFd, name: &str) -> io::Result<()> {
        let c = cstr(name)?;
        // SAFETY: `dir` is open and `c` is a valid C string.
        ok(unsafe { libc::unlinkat(dir.as_raw_fd(), c.as_ptr(), 0) })
    }
}

/// Answers each `Effect::SaveSecret` and `Effect::DeleteBackup` of the
/// Connectors area with one event, on a blocking task of its own. The notice
/// it sends names a server and a name at most, never a value.
pub struct Writer {
    secrets: Option<PathBuf>,
    events: mpsc::Sender<AppEvent>,
    /// One write at a time: two saves would read the same file and the second
    /// would see "changed on disk".
    turn: Arc<Mutex<()>>,
}

impl Writer {
    pub fn new(secrets: Option<PathBuf>, events: mpsc::Sender<AppEvent>) -> Self {
        Self {
            secrets,
            events,
            turn: Arc::default(),
        }
    }

    pub fn save(&self, server: &'static str, name: &'static str, value: SecretText) {
        let (secrets, events) = (self.secrets.clone(), self.events.clone());
        let turn = Arc::clone(&self.turn);
        tokio::spawn(async move {
            let done = tokio::task::spawn_blocking(move || {
                let _turn = turn.lock().unwrap_or_else(PoisonError::into_inner);
                save_secret(secrets.as_deref(), server, name, value.expose())
            })
            .await;
            let reply = match done {
                Ok(Ok(saved)) => Ok(saved.notice()),
                Ok(Err(refusal)) => Err(refusal.notice()),
                Err(_) => Err("secrets.toml: not saved (the task failed)".to_owned()),
            };
            let _ = events.send(AppEvent::SecretSaved(reply)).await;
        });
    }

    /// Writes the OAuth client file; the answer is the same event as a save.
    pub fn save_client(&self, id: String, secret: SecretText, replace: bool) {
        let (secrets, events) = (self.secrets.clone(), self.events.clone());
        let turn = Arc::clone(&self.turn);
        tokio::spawn(async move {
            let done = tokio::task::spawn_blocking(move || {
                let _turn = turn.lock().unwrap_or_else(PoisonError::into_inner);
                let fs = &RealFs;
                save_client_with(fs, secrets.as_deref(), &id, secret.expose(), replace)
            })
            .await;
            let reply = match done {
                Ok(Ok(saved)) => Ok(saved.notice()),
                Ok(Err(refusal)) => Err(refusal.notice_of(CLIENT_FILE)),
                Err(_) => Err(format!("{CLIENT_FILE}: not saved (the task failed)")),
            };
            let _ = events.send(AppEvent::SecretSaved(reply)).await;
        });
    }

    /// Connect Google's write (`Effect::ConnectGoogle`): the values, then the
    /// entries appended to `config.toml` next to `secrets.toml`; the answer
    /// is one `AppEvent::GoogleConnected`.
    pub fn connect(&self, write: Box<Connect>) {
        let (secrets, events) = (self.secrets.clone(), self.events.clone());
        let turn = Arc::clone(&self.turn);
        tokio::spawn(async move {
            let done = tokio::task::spawn_blocking(move || {
                let _turn = turn.lock().unwrap_or_else(PoisonError::into_inner);
                connect_with(&RealFs, secrets.as_deref(), &write)
            })
            .await;
            let reply = done.unwrap_or_else(|_| {
                Connected::Refused(
                    "connect: the task failed; check secrets.toml and config.toml".to_owned(),
                )
            });
            let _ = events.send(AppEvent::GoogleConnected(reply)).await;
        });
    }

    pub fn delete_backup(&self) {
        let (secrets, events) = (self.secrets.clone(), self.events.clone());
        tokio::spawn(async move {
            let done = tokio::task::spawn_blocking(move || delete_backup(secrets.as_deref())).await;
            let reply = match done {
                Ok(Ok(())) => Ok(format!("deleted {BACKUP}")),
                Ok(Err(refusal)) => Err(refusal.notice()),
                Err(_) => Err(format!("{BACKUP}: not deleted (the task failed)")),
            };
            let _ = events.send(AppEvent::BackupDeleted(reply)).await;
        });
    }
}

/// Saves `name` of `server`, in the file at `secrets` (`None`: no config
/// folder was found).
pub fn save_secret(
    secrets: Option<&Path>,
    server: &str,
    name: &str,
    value: &str,
) -> Result<Saved, Refusal> {
    save_with(&RealFs, secrets, server, name, value)
}

/// Deletes the backup of the file at `secrets`.
pub fn delete_backup(secrets: Option<&Path>) -> Result<(), Refusal> {
    delete_with(&RealFs, secrets)
}

pub fn save_with(
    fs: &impl SaveFs,
    secrets: Option<&Path>,
    server: &str,
    name: &str,
    value: &str,
) -> Result<Saved, Refusal> {
    // Refuses a name the entry does not list, or a bad value, before any I/O.
    edit("", server, name, value)?;
    let backed_up = replace_with(fs, secrets, SECRETS, |current| {
        let text = match current {
            Some(bytes) => std::str::from_utf8(bytes)
                .map_err(|e| Refusal::NotToml(line_of(bytes, e.valid_up_to())))?,
            None => "",
        };
        edit(text, server, name, value)
    })?;
    Ok(Saved {
        name: name.to_owned(),
        backed_up,
        client: false,
    })
}

/// Saves every (server, name, value) of `values` in one write and one backup
/// generation, by the rules of a save of one: the first refusal writes
/// nothing.
pub fn save_all_with(
    fs: &impl SaveFs,
    secrets: Option<&Path>,
    values: &[(&str, &str, &str)],
) -> Result<Saved, Refusal> {
    edit_all("", values)?;
    let backed_up = replace_with(fs, secrets, SECRETS, |current| {
        let text = match current {
            Some(bytes) => std::str::from_utf8(bytes)
                .map_err(|e| Refusal::NotToml(line_of(bytes, e.valid_up_to())))?,
            None => "",
        };
        edit_all(text, values)
    })?;
    let name = match values {
        [(_, name, _)] => (*name).to_owned(),
        _ => format!("{} values", values.len()),
    };
    Ok(Saved {
        name,
        backed_up,
        client: false,
    })
}

/// Writes the OAuth client file next to `secrets`, by the same steps as a
/// save. An existing file is replaced only when `replace` (the user's `y`).
pub fn save_client_with(
    fs: &impl SaveFs,
    secrets: Option<&Path>,
    id: &str,
    secret: &str,
    replace: bool,
) -> Result<Saved, Refusal> {
    let json = client_json(id, secret)?;
    let backed_up = replace_with(fs, secrets, CLIENT, |current| match current {
        Some(_) if !replace => Err(Refusal::Exists),
        _ => Ok(json),
    })?;
    Ok(Saved {
        name: CLIENT_FILE.to_owned(),
        backed_up,
        client: true,
    })
}

/// Replaces `target` in the folder of `secrets` (any file of that folder)
/// with what `build` makes from the bytes it holds (`None`: it does not
/// exist): folder judged, file read and judged on its handle, temp file,
/// backup, swap. `true` when a backup was made.
pub(super) fn replace_with<E: From<Refusal>>(
    fs: &impl SaveFs,
    secrets: Option<&Path>,
    target: Target,
    build: impl FnOnce(Option<&[u8]>) -> Result<String, E>,
) -> Result<bool, E> {
    let dir = open_folder(fs, secrets)?;
    let folder = secrets.and_then(Path::parent).unwrap_or(Path::new(""));
    let current = read_current(fs, &dir, &folder.join(target.file), target)?;
    let new = build(current.as_ref().map(|(bytes, _)| bytes.as_slice()))?;
    // Defense in depth: `edit` already refuses a result over the cap.
    if new.len() as u64 > target.max {
        return Err(Refusal::TooLarge.into());
    }
    let before = current.as_ref().map(|(_, stat)| stat);
    if current.is_some() {
        match fs.stat_at(&dir, target.backup) {
            Ok(stat) if !stat.regular => return Err(Refusal::BackupUnsafe.into()),
            Err(e) if e.kind() != ErrorKind::NotFound => {
                return Err(Refusal::Io(e.kind()).into());
            }
            _ => {}
        }
    }
    let mode = match before {
        Some(stat) if target.keep_mode => stat.mode & 0o777,
        _ => 0o600,
    };
    let temp = write_temp(fs, &dir, target, new.as_bytes(), mode)?;
    let old = current.as_ref().map(|(bytes, _)| bytes.as_slice());
    let done = swap(fs, &dir, target, &temp, before, old);
    if done.is_err() {
        let _ = fs.unlink(&dir, &temp);
    }
    done.map(|()| current.is_some()).map_err(E::from)
}

/// The config folder, opened and judged on the handle.
fn open_folder(fs: &impl SaveFs, secrets: Option<&Path>) -> Result<OwnedFd, Refusal> {
    let folder = secrets.and_then(Path::parent).ok_or(Refusal::NoFolder)?;
    let dir = fs.open_dir(folder).map_err(|e| {
        let link = std::fs::symlink_metadata(folder).is_ok_and(|m| m.is_symlink());
        if link {
            Refusal::FolderSymlink
        } else {
            Refusal::Io(e.kind())
        }
    })?;
    let stat = fs.fstat(&dir).map_err(|e| Refusal::Io(e.kind()))?;
    if stat.uid != fs.euid() {
        return Err(Refusal::FolderOwner);
    }
    if stat.mode & 0o022 != 0 {
        return Err(Refusal::FolderWritable(folder.to_path_buf()));
    }
    Ok(dir)
}

/// The bytes of the file and its `fstat`, `None` when it does not exist;
/// judged on the open handle.
fn read_current(
    fs: &impl SaveFs,
    dir: &OwnedFd,
    path: &Path,
    target: Target,
) -> Result<Option<(Vec<u8>, Stat)>, Refusal> {
    let fd = match fs.open_read(dir, target.file) {
        Ok(fd) => fd,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) if e.raw_os_error() == Some(libc::ELOOP) => return Err(Refusal::FileSymlink),
        Err(_) => return Err(Refusal::Unreadable),
    };
    let stat = fs.fstat(&fd).map_err(|_| Refusal::Unreadable)?;
    if !stat.regular {
        return Err(Refusal::NotRegular);
    }
    if stat.uid != fs.euid() {
        return Err(Refusal::FileOwner);
    }
    if stat.nlink > 1 {
        return Err(Refusal::HardLinked);
    }
    if target.keep_mode {
        if stat.mode & 0o022 != 0 {
            return Err(Refusal::FileWritable(path.to_path_buf()));
        }
        if stat.mode & 0o200 == 0 {
            return Err(Refusal::ReadOnly(path.to_path_buf()));
        }
    } else {
        // Only the owner's read and write: 0400 is read-only, anything else loose.
        if stat.mode & 0o200 == 0 && stat.mode & 0o7077 == 0 {
            return Err(Refusal::ReadOnly(path.to_path_buf()));
        }
        if stat.mode != 0o600 {
            return Err(Refusal::FileMode(path.to_path_buf()));
        }
    }
    if stat.len > target.max {
        return Err(Refusal::TooLarge);
    }
    let bytes = fs
        .read(&fd, target.max + 1)
        .map_err(|_| Refusal::Unreadable)?;
    if bytes.len() as u64 > target.max {
        return Err(Refusal::TooLarge);
    }
    Ok(Some((bytes, stat)))
}

/// A new file of mode `mode` (0600 for every file but a kept `config.toml`)
/// in the folder under a random name, written and synced: `O_EXCL`, so a name
/// that exists (a planted link too) is never opened or removed, and another
/// is tried, three times.
fn write_temp(
    fs: &impl SaveFs,
    dir: &OwnedFd,
    target: Target,
    bytes: &[u8],
    mode: u32,
) -> Result<String, Refusal> {
    for _ in 0..3 {
        let name = format!(
            ".{}.{}.{:016x}.tmp",
            target.file,
            std::process::id(),
            fs.random()
        );
        match fs.create_excl(dir, &name) {
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(Refusal::Io(e.kind())),
            Ok(fd) => {
                let chmod = if mode == 0o600 {
                    fs.private(&fd)
                } else {
                    fs.set_mode(&fd, mode)
                };
                let written = chmod
                    .and_then(|()| fs.write_all(&fd, bytes))
                    .and_then(|()| fs.fsync(&fd));
                if let Err(e) = written {
                    let _ = fs.unlink(dir, &name);
                    return Err(Refusal::Io(e.kind()));
                }
                return Ok(name);
            }
        }
    }
    Err(Refusal::TempExists)
}

/// Whether the name still is the file that was read (`None`: still missing).
fn unchanged(
    fs: &impl SaveFs,
    dir: &OwnedFd,
    file: &str,
    before: Option<&Stat>,
) -> Result<(), Refusal> {
    match (fs.stat_at(dir, file), before) {
        (Ok(now), Some(before)) if now.same(before) => Ok(()),
        (Err(e), None) if e.kind() == ErrorKind::NotFound => Ok(()),
        _ => Err(Refusal::Changed),
    }
}

/// Backup of what was read, then `temp` over the file, then the folder synced.
fn swap(
    fs: &impl SaveFs,
    dir: &OwnedFd,
    target: Target,
    temp: &str,
    before: Option<&Stat>,
    old: Option<&[u8]>,
) -> Result<(), Refusal> {
    let io = |e: io::Error| Refusal::Io(e.kind());
    unchanged(fs, dir, target.file, before)?;
    if let Some(old) = old {
        let backup = write_temp(fs, dir, target, old, 0o600)?;
        if let Err(e) = fs.rename(dir, &backup, target.backup) {
            let _ = fs.unlink(dir, &backup);
            return Err(io(e));
        }
        unchanged(fs, dir, target.file, before)?;
    }
    fs.rename(dir, temp, target.file).map_err(io)?;
    // The file is in place: a folder that cannot be synced changes nothing.
    let _ = fs.fsync(dir);
    Ok(())
}

pub fn delete_with(fs: &impl SaveFs, secrets: Option<&Path>) -> Result<(), Refusal> {
    let dir = open_folder(fs, secrets)?;
    match fs.stat_at(&dir, BACKUP) {
        Err(e) if e.kind() == ErrorKind::NotFound => Err(Refusal::NoBackup),
        Err(e) => Err(Refusal::Io(e.kind())),
        Ok(stat) if !stat.regular || stat.uid != fs.euid() => Err(Refusal::BackupUnsafe),
        Ok(_) => fs.unlink(&dir, BACKUP).map_err(|e| Refusal::Io(e.kind())),
    }
}

#[cfg(test)]
mod tests;
