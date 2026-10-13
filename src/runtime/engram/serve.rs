//! Starting `engram serve` for the user: finding the binary, spawning it
//! detached and keeping its log. Nothing here ever signals the server.

use std::{
    ffi::OsString,
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Stdio,
    sync::Mutex,
    time::Duration,
};

use tokio::{process::Command, sync::oneshot};

use crate::{
    core::ai::{
        memory::offer::{Binary, last_log_line},
        parse::parse_version,
    },
    runtime::{
        ai_detect,
        chat_store::{DirProblem, secure_dir},
    },
};

/// The log file name under the state directory.
const LOG_NAME: &str = "engram-serve.log";
/// A log larger than this is emptied before the next start.
const MAX_LOG: u64 = 1024 * 1024;
/// How much of the log's end is read for the failure line.
const TAIL: u64 = 4096;

/// A server that was spawned. Dropping it leaves the process running; the
/// only thing that ever ends it is the user (or the server itself).
#[derive(Debug)]
pub struct Running {
    pub pid: u32,
    /// Resolves when the child has exited and been reaped.
    pub exit: oneshot::Receiver<()>,
    /// Where this start's output begins in the log.
    pub log_from: u64,
}

/// The state directory for the log: `$XDG_STATE_HOME/lazarobox` when that is
/// set to an absolute path, else `$HOME/.local/state/lazarobox`.
pub fn state_dir(xdg: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let absolute = |value: OsString| Some(PathBuf::from(value)).filter(|p| p.is_absolute());
    let base = xdg
        .and_then(absolute)
        .or_else(|| home.and_then(absolute).map(|h| h.join(".local/state")))?;
    Some(base.join("lazarobox"))
}

/// The error of a spawn refused because the binary is not the file that was
/// looked up and shown to the user (carried inside an `io::Error`).
#[derive(Debug)]
pub struct BinaryChanged;

impl fmt::Display for BinaryChanged {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the binary changed since it was offered")
    }
}

impl std::error::Error for BinaryChanged {}

impl BinaryChanged {
    /// Whether `error` is a refusal because the binary changed.
    pub fn is(error: &io::Error) -> bool {
        error
            .get_ref()
            .is_some_and(|inner| inner.is::<BinaryChanged>())
    }
}

/// Which file a path was: device, inode, size and modification time. A file
/// replaced or rewritten after the lookup differs in at least one of them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Identity {
    dev: u64,
    ino: u64,
    size: u64,
    mtime: (i64, i64),
}

impl Identity {
    /// Of the file the path leads to (symlinks followed), `None` if unreadable.
    fn of(path: &Path) -> Option<Self> {
        let meta = fs::metadata(path).ok()?;
        Some(Self {
            dev: meta.dev(),
            ino: meta.ino(),
            size: meta.size(),
            mtime: (meta.mtime(), meta.mtime_nsec()),
        })
    }
}

/// What the worker needs to start a server. A trait so the worker's logic is
/// tested without processes; `Server` is the real one.
pub trait Launcher: Send + Sync + 'static {
    /// The first `engram` on the PATH and its version output.
    fn find(&self) -> impl Future<Output = Binary> + Send;
    /// Spawns `<binary> serve <port>` detached.
    fn spawn(&self, binary: &Path, port: u16) -> impl Future<Output = io::Result<Running>> + Send;
    /// The last log line written since `from`.
    fn log_line(&self, from: u64) -> impl Future<Output = Option<String>> + Send;
}

/// A launcher that does not look and cannot start: a worker without the
/// `engram serve` offer.
pub struct NoServer;

impl Launcher for NoServer {
    async fn find(&self) -> Binary {
        Binary::NotChecked
    }

    async fn spawn(&self, _binary: &Path, _port: u16) -> io::Result<Running> {
        Err(io::Error::other("starting a server is not available"))
    }

    async fn log_line(&self, _from: u64) -> Option<String> {
        None
    }
}

impl Launcher for Server {
    async fn find(&self) -> Binary {
        Server::find(self).await
    }

    async fn spawn(&self, binary: &Path, port: u16) -> io::Result<Running> {
        Server::spawn(self, binary, port).await
    }

    async fn log_line(&self, from: u64) -> Option<String> {
        Server::log_line(self, from).await
    }
}

/// Finds, starts and reads the log of the user's `engram serve`.
pub struct Server {
    path: OsString,
    state: Option<PathBuf>,
    /// The binary the last lookup found and which file it was: the only one
    /// `spawn` accepts, and only while it is still that file.
    seen: Mutex<Option<(PathBuf, Identity)>>,
}

impl Server {
    /// A server for the process environment.
    pub fn from_env() -> Self {
        Self {
            path: std::env::var_os("PATH").unwrap_or_default(),
            state: state_dir(std::env::var_os("XDG_STATE_HOME"), std::env::var_os("HOME")),
            seen: Mutex::new(None),
        }
    }

    pub fn new(path: OsString, state: PathBuf) -> Self {
        Self {
            path,
            state: Some(state),
            seen: Mutex::new(None),
        }
    }

    /// The first `engram` on the PATH and what its version command says.
    pub async fn find(&self) -> Binary {
        let Some(path) = ai_detect::find_on_path("engram", &self.path) else {
            return Binary::Missing;
        };
        // A path that is not UTF-8 cannot be shown to the user as it is.
        let Some(shown) = path.to_str().map(str::to_string) else {
            return Binary::Missing;
        };
        let Some(identity) = Identity::of(&path) else {
            return Binary::Missing;
        };
        *self.seen.lock().unwrap_or_else(|e| e.into_inner()) = Some((path.clone(), identity));
        let version = ai_detect::version(&path)
            .await
            .and_then(|out| parse_version(&out));
        Binary::Found {
            path: shown,
            version,
        }
    }

    /// Spawns `<binary> serve <port>` detached, its output appended to the
    /// log. The caller probes health; this only starts the process. Only the
    /// binary the last lookup found is spawned, and only if it is still the
    /// same file; anything else is refused with `BinaryChanged`.
    pub async fn spawn(&self, binary: &Path, port: u16) -> io::Result<Running> {
        let unchanged = self
            .seen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|(path, identity)| {
                path == binary && Identity::of(binary) == Some(*identity)
            });
        if !unchanged {
            return Err(io::Error::other(BinaryChanged));
        }
        let state = self
            .state
            .as_deref()
            .ok_or_else(|| io::Error::other("no state directory"))?;
        let (log, log_from) = open_log(state)?;
        let mut child = None;
        for attempt in 0..5 {
            // A binary that is still being written fails with ETXTBSY for a
            // moment (an upgrade): that one error is retried briefly.
            match command(binary, port, &log)?.spawn() {
                Ok(spawned) => {
                    child = Some(spawned);
                    break;
                }
                Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) && attempt < 4 => {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                Err(e) => return Err(e),
            }
        }
        let mut child = child.ok_or_else(|| io::Error::other("could not spawn"))?;
        let pid = child.id().ok_or_else(|| io::Error::other("no pid"))?;
        let (done, exit) = oneshot::channel();
        // The reaper: an early exit is waited for, so no zombie is left.
        tokio::spawn(async move {
            let _ = child.wait().await;
            let _ = done.send(());
        });
        Ok(Running {
            pid,
            exit,
            log_from,
        })
    }

    /// The last line this start wrote to the log, sanitized and capped.
    pub async fn log_line(&self, from: u64) -> Option<String> {
        let file = self.state.as_ref()?.join(LOG_NAME);
        tokio::task::spawn_blocking(move || {
            // Never through a symlink, never blocking on a FIFO.
            let mut file = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(file)
                .ok()?;
            let meta = file.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            let len = meta.len();
            file.seek(SeekFrom::Start(from.max(len.saturating_sub(TAIL))))
                .ok()?;
            let mut tail = Vec::new();
            file.take(TAIL).read_to_end(&mut tail).ok()?;
            last_log_line(&tail)
        })
        .await
        .ok()?
    }
}

/// Opens the log for appending: directory 0700 and owned by this user (like
/// the chat store's), file 0600, never through a symlink, never blocking on a
/// FIFO, emptied when over `MAX_LOG`. Returns it and where this start's
/// output begins.
fn open_log(dir: &Path) -> io::Result<(File, u64)> {
    // SAFETY: geteuid has no preconditions and cannot fail.
    open_log_as(dir, unsafe { libc::geteuid() })
}

fn open_log_as(dir: &Path, uid: u32) -> io::Result<(File, u64)> {
    secure_dir(dir, uid).map_err(|problem| match problem {
        DirProblem::NotADirectory => io::Error::other("the log directory is not a directory"),
        DirProblem::ForeignOwner => io::Error::other("the log directory has a foreign owner"),
        DirProblem::Io(err) => err,
    })?;
    // O_NONBLOCK so that opening a FIFO planted at the path returns at once
    // (it is refused below); it has no effect on a regular file.
    let log = OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(dir.join(LOG_NAME))?;
    let meta = log.metadata()?;
    if !meta.is_file() {
        return Err(io::Error::other("the log is not a regular file"));
    }
    if meta.uid() != uid {
        return Err(io::Error::other("the log has a foreign owner"));
    }
    if meta.permissions().mode() & 0o777 != 0o600 {
        log.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    if meta.len() > MAX_LOG {
        log.set_len(0)?;
    }
    let from = log.metadata()?.len();
    Ok((log, from))
}

/// The spawn command: argv exactly `serve <port>`, no shell, no stdin, both
/// output streams on the log, `ENGRAM_PORT` removed (the argument wins),
/// everything else (including `ENGRAM_DATA_DIR`) inherited as is, its own
/// session, and left running if the handle is dropped.
fn command(binary: &Path, port: u16, log: &File) -> io::Result<Command> {
    let mut cmd = Command::new(binary);
    cmd.arg("serve")
        .arg(port.to_string())
        .env_remove("ENGRAM_PORT")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log.try_clone()?))
        .kill_on_drop(false);
    // SAFETY: the closure only calls `setsid`, which is async-signal-safe: a
    // terminal hang-up must not reach the server.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    Ok(cmd)
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsString,
        fs,
        os::unix::fs::{MetadataExt, PermissionsExt, symlink},
        path::{Path, PathBuf},
        time::Duration,
    };

    use super::*;
    use crate::core::ai::memory::offer::Binary;

    /// Kills the pid it holds when dropped, so a failing test leaks nothing.
    struct Reap(Option<u32>);

    impl Reap {
        fn new(pid: u32) -> Self {
            Self(Some(pid))
        }
    }

    impl Drop for Reap {
        fn drop(&mut self) {
            if let Some(pid) = self.0.take() {
                // SAFETY: a plain signal to a pid this test spawned.
                unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
            }
        }
    }

    fn alive(pid: u32) -> bool {
        // SAFETY: signal 0 only checks that the pid exists.
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }

    /// A fake `engram` that reports a version, or records how it was run and
    /// then stays up (`exec sleep`, so the pid stays the script's).
    fn fake(dir: &Path, version: &str, record: &Path, tail: &str) -> PathBuf {
        let path = dir.join("engram");
        let body = format!(
            "#!/bin/sh\n\
             if [ \"$1\" = \"--version\" ]; then echo '{version}'; exit 0; fi\n\
             {{ echo \"argc=$#\"; for a in \"$@\"; do echo \"arg=$a\"; done; \
             echo \"stdin=$(readlink /proc/$$/fd/0)\"; env | sort; }} > '{rec}.tmp'\n\
             mv '{rec}.tmp' '{rec}'\n\
             {tail}\n",
            rec = record.display()
        );
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    struct Rig {
        _dir: tempfile::TempDir,
        bin_dir: PathBuf,
        state: PathBuf,
        record: PathBuf,
    }

    fn rig() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir(&bin_dir).unwrap();
        let state = dir.path().join("state");
        let record = dir.path().join("record");
        Rig {
            _dir: dir,
            bin_dir,
            state,
            record,
        }
    }

    impl Rig {
        fn server(&self) -> Server {
            Server::new(self.bin_dir.clone().into(), self.state.clone())
        }

        async fn recorded(&self) -> String {
            for _ in 0..200 {
                if let Ok(text) = fs::read_to_string(&self.record) {
                    return text;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            panic!("the fake never recorded its run");
        }
    }

    /// What the worker does: look the binary up, then start what it found.
    async fn start(server: &Server, script: &Path, port: u16) -> io::Result<Running> {
        server.find().await;
        server.spawn(script, port).await
    }

    impl Rig {
        /// A directory outside the PATH, for a binary the lookup never saw.
        fn outside_path(&self) -> PathBuf {
            let dir = self._dir.path().join("elsewhere");
            fs::create_dir_all(&dir).unwrap();
            dir
        }
    }

    fn lines_of(record: &str, key: &str) -> Vec<String> {
        record
            .lines()
            .filter_map(|l| l.strip_prefix(&format!("{key}=")).map(str::to_string))
            .collect()
    }

    #[test]
    fn the_state_dir_is_xdg_state_home_then_home() {
        let dir = |xdg: Option<&str>, home: Option<&str>| {
            state_dir(xdg.map(OsString::from), home.map(OsString::from))
        };
        assert_eq!(
            dir(Some("/x/state"), Some("/h")),
            Some(PathBuf::from("/x/state/lazarobox"))
        );
        assert_eq!(
            dir(None, Some("/h")),
            Some(PathBuf::from("/h/.local/state/lazarobox"))
        );
        // An empty or relative XDG value is ignored, as the XDG spec says.
        assert_eq!(
            dir(Some(""), Some("/h")),
            Some(PathBuf::from("/h/.local/state/lazarobox"))
        );
        assert_eq!(
            dir(Some("rel"), Some("/h")),
            Some(PathBuf::from("/h/.local/state/lazarobox"))
        );
        assert_eq!(dir(None, None), None);
        assert_eq!(dir(None, Some("relative-home")), None);
    }

    #[tokio::test]
    async fn find_gives_the_absolute_path_and_the_version_output() {
        let rig = rig();
        let script = fake(&rig.bin_dir, "engram 3.4.0", &rig.record, "exit 0");
        let found = rig.server().find().await;
        assert_eq!(
            found,
            Binary::Found {
                path: script.to_string_lossy().into_owned(),
                version: Some("engram 3.4.0".into()),
            }
        );
    }

    #[tokio::test]
    async fn find_reports_a_missing_binary_and_an_unreadable_version() {
        let rig = rig();
        assert_eq!(rig.server().find().await, Binary::Missing);
        let script = rig.bin_dir.join("engram");
        fs::write(&script, "#!/bin/sh\nexit 1\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            rig.server().find().await,
            Binary::Found {
                path: script.to_string_lossy().into_owned(),
                version: None,
            }
        );
    }

    #[tokio::test]
    async fn find_treats_a_path_that_is_not_utf8_as_missing() {
        use std::os::unix::ffi::OsStrExt;
        let rig = rig();
        let odd = rig.bin_dir.join(std::ffi::OsStr::from_bytes(b"caf\xe9"));
        fs::create_dir(&odd).unwrap();
        fake(&odd, "engram 3.4.0", &rig.record, "exit 0");
        let server = Server::new(odd.into_os_string(), rig.state.clone());
        assert_eq!(server.find().await, Binary::Missing);
    }

    #[tokio::test]
    async fn spawn_runs_serve_port_with_the_documented_environment() {
        let rig = rig();
        let script = fake(&rig.bin_dir, "engram 3.4.0", &rig.record, "exec sleep 30");
        let running = start(&rig.server(), &script, 8123).await.unwrap();
        let _reap = Reap::new(running.pid);
        let record = rig.recorded().await;
        assert_eq!(lines_of(&record, "argc"), ["2"]);
        assert_eq!(lines_of(&record, "arg"), ["serve", "8123"]);
        assert_eq!(lines_of(&record, "stdin"), ["/dev/null"]);
        assert!(
            lines_of(&record, "ENGRAM_PORT").is_empty(),
            "ENGRAM_PORT must not reach the child"
        );
        // ENGRAM_DATA_DIR is neither added nor changed.
        let inherited = std::env::var("ENGRAM_DATA_DIR").ok();
        assert_eq!(
            lines_of(&record, "ENGRAM_DATA_DIR"),
            inherited.into_iter().collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_command_removes_engram_port_and_never_sets_the_data_dir() {
        let rig = rig();
        let log = fs::File::create(rig.record.with_extension("log")).unwrap();
        let cmd = command(Path::new("/bin/true"), 8123, &log).unwrap();
        let std_cmd = cmd.as_std();
        let envs: Vec<_> = std_cmd.get_envs().collect();
        assert!(
            envs.iter().any(|(k, v)| *k == "ENGRAM_PORT" && v.is_none()),
            "{envs:?}"
        );
        assert!(
            envs.iter().all(|(k, _)| *k != "ENGRAM_DATA_DIR"),
            "{envs:?}"
        );
        assert_eq!(std_cmd.get_program(), "/bin/true");
        let args: Vec<_> = std_cmd.get_args().collect();
        assert_eq!(args, ["serve", "8123"]);
    }

    #[tokio::test]
    async fn the_child_is_its_own_session_leader() {
        let rig = rig();
        let script = fake(&rig.bin_dir, "engram 3.4.0", &rig.record, "exec sleep 30");
        let running = start(&rig.server(), &script, 8123).await.unwrap();
        let _reap = Reap::new(running.pid);
        rig.recorded().await;
        let stat = fs::read_to_string(format!("/proc/{}/stat", running.pid)).unwrap();
        // pid (comm) state ppid pgrp session ...; comm may hold spaces.
        let after = stat.rsplit_once(')').unwrap().1;
        let fields: Vec<&str> = after.split_whitespace().collect();
        let (pgrp, session) = (fields[2], fields[3]);
        assert_eq!(session, running.pid.to_string(), "sid == pid");
        assert_eq!(pgrp, running.pid.to_string());
    }

    /// Sets the process umask and puts the old one back when dropped, so a
    /// panic inside the guarded section cannot leave it changed.
    ///
    /// The umask is process-wide and tests run on parallel threads: keep the
    /// guarded section short and synchronous (never an `.await`), and never
    /// assert the mode of a file another test created with default options,
    /// since it may have been created inside this window.
    struct Umask(libc::mode_t);

    impl Umask {
        fn set(mask: libc::mode_t) -> Self {
            // SAFETY: umask only changes this process's file mode mask.
            Self(unsafe { libc::umask(mask) })
        }
    }

    impl Drop for Umask {
        fn drop(&mut self) {
            // SAFETY: restores the mask read in `set`.
            unsafe { libc::umask(self.0) };
        }
    }

    #[test]
    fn the_umask_guard_restores_the_mask_even_when_the_section_panics() {
        let before = Umask::set(0o022);
        let outcome = std::panic::catch_unwind(|| {
            let _guard = Umask::set(0);
            panic!("inside the guarded section");
        });
        assert!(outcome.is_err());
        // SAFETY: reads the current mask by setting it back to itself.
        let now = unsafe { libc::umask(0o022) };
        assert_eq!(now, 0o022, "the panicking section left the umask changed");
        drop(before);
    }

    #[test]
    fn the_log_is_private_under_a_permissive_umask() {
        let rig = rig();
        let opened = {
            let _umask = Umask::set(0);
            open_log(&rig.state)
        };
        opened.unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().mode() & 0o777;
        assert_eq!(mode(&rig.state), 0o700);
        assert_eq!(mode(&rig.state.join("engram-serve.log")), 0o600);
    }

    #[tokio::test]
    async fn the_log_gets_both_output_streams_of_the_child() {
        let rig = rig();
        let script = fake(
            &rig.bin_dir,
            "engram 3.4.0",
            &rig.record,
            "echo to-stdout; echo to-stderr >&2; exec sleep 30",
        );
        let running = start(&rig.server(), &script, 8123).await.unwrap();
        let _reap = Reap::new(running.pid);
        rig.recorded().await;
        let log = rig.state.join("engram-serve.log");
        for _ in 0..100 {
            let text = fs::read_to_string(&log).unwrap();
            if text.contains("to-stdout") && text.contains("to-stderr") {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("the child's output never reached the log");
    }

    #[tokio::test]
    async fn an_existing_loose_log_is_tightened_to_0600() {
        let rig = rig();
        fs::create_dir_all(&rig.state).unwrap();
        let log = rig.state.join("engram-serve.log");
        fs::write(&log, "old\n").unwrap();
        fs::set_permissions(&log, fs::Permissions::from_mode(0o666)).unwrap();
        let script = fake(&rig.bin_dir, "engram 3.4.0", &rig.record, "exec sleep 30");
        let running = start(&rig.server(), &script, 8123).await.unwrap();
        let _reap = Reap::new(running.pid);
        assert_eq!(fs::metadata(&log).unwrap().mode() & 0o777, 0o600);
        assert!(fs::read_to_string(&log).unwrap().starts_with("old\n"));
    }

    #[tokio::test]
    async fn a_symlinked_log_is_refused_and_nothing_is_spawned() {
        let rig = rig();
        fs::create_dir_all(&rig.state).unwrap();
        let target = rig.state.join("victim");
        fs::write(&target, "precious").unwrap();
        symlink(&target, rig.state.join("engram-serve.log")).unwrap();
        let script = fake(&rig.bin_dir, "engram 3.4.0", &rig.record, "exec sleep 30");
        let result = start(&rig.server(), &script, 8123).await;
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "precious");
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(!rig.record.exists(), "the binary must not have run");
    }

    #[tokio::test]
    async fn a_log_over_1_mib_is_truncated_and_one_at_exactly_1_mib_is_kept() {
        for (len, kept) in [(MAX_LOG + 1, false), (MAX_LOG, true)] {
            let rig = rig();
            fs::create_dir_all(&rig.state).unwrap();
            let log = rig.state.join("engram-serve.log");
            fs::write(&log, vec![b'x'; len as usize]).unwrap();
            let script = fake(&rig.bin_dir, "engram 3.4.0", &rig.record, "exec sleep 30");
            let running = start(&rig.server(), &script, 8123).await.unwrap();
            let _reap = Reap::new(running.pid);
            let size = fs::metadata(&log).unwrap().len();
            if kept {
                assert!(size >= MAX_LOG, "{size}");
                assert_eq!(running.log_from, MAX_LOG);
            } else {
                assert!(size < 1024, "truncated, got {size}");
                assert_eq!(running.log_from, 0);
            }
        }
    }

    #[tokio::test]
    async fn the_child_survives_dropping_every_handle() {
        let rig = rig();
        let script = fake(&rig.bin_dir, "engram 3.4.0", &rig.record, "exec sleep 30");
        let server = rig.server();
        let running = start(&server, &script, 8123).await.unwrap();
        let pid = running.pid;
        let _reap = Reap::new(pid);
        rig.recorded().await;
        drop(running);
        drop(server);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(alive(pid), "dropping the handle must not kill the server");
    }

    #[tokio::test]
    async fn a_child_that_exits_is_reaped_and_reported() {
        let rig = rig();
        let script = fake(
            &rig.bin_dir,
            "engram 3.4.0",
            &rig.record,
            "echo 'bind: address already in use' >&2; exit 1",
        );
        let running = start(&rig.server(), &script, 8123).await.unwrap();
        let (pid, from) = (running.pid, running.log_from);
        tokio::time::timeout(Duration::from_secs(5), running.exit)
            .await
            .expect("the exit is reported")
            .expect("the reaper task ran");
        // Reaped: no zombie is left under the test process.
        assert!(
            !Path::new(&format!("/proc/{pid}")).exists()
                || fs::read_to_string(format!("/proc/{pid}/stat"))
                    .map(|s| !s.contains(") Z "))
                    .unwrap_or(true)
        );
        assert_eq!(
            rig.server().log_line(from).await.as_deref(),
            Some("bind: address already in use")
        );
    }

    #[tokio::test]
    async fn the_log_line_ignores_what_was_there_before_this_start() {
        let rig = rig();
        fs::create_dir_all(&rig.state).unwrap();
        let log = rig.state.join("engram-serve.log");
        fs::write(&log, "stale failure from an older run\n").unwrap();
        let from = fs::metadata(&log).unwrap().len();
        assert_eq!(rig.server().log_line(from).await, None);
        fs::write(&log, "stale failure from an older run\nnew failure\n").unwrap();
        assert_eq!(
            rig.server().log_line(from).await.as_deref(),
            Some("new failure")
        );
    }

    #[tokio::test]
    async fn a_path_with_spaces_and_shell_characters_needs_no_shell() {
        let rig = rig();
        let odd = rig.bin_dir.join("a b;$(touch pwned)");
        fs::create_dir(&odd).unwrap();
        let script = fake(&odd, "engram 3.4.0", &rig.record, "exec sleep 30");
        let server = Server::new(odd.clone().into_os_string(), rig.state.clone());
        let running = start(&server, &script, 8123).await.unwrap();
        let _reap = Reap::new(running.pid);
        let record = rig.recorded().await;
        assert_eq!(lines_of(&record, "arg"), ["serve", "8123"]);
        assert!(!Path::new("pwned").exists());
    }

    /// Runs `f` on its own thread and fails the test instead of hanging when
    /// it does not return (a blocking `open` on a FIFO would never return).
    fn within<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        rx.recv_timeout(Duration::from_secs(3))
            .expect("the call blocked instead of returning")
    }

    fn mkfifo(path: &Path) {
        let c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: `c` is a valid NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    }

    fn euid() -> u32 {
        // SAFETY: geteuid has no preconditions and cannot fail.
        unsafe { libc::geteuid() }
    }

    #[test]
    fn a_fifo_at_the_log_path_is_refused_without_blocking() {
        // No reader: a blocking open of a FIFO for writing would wait forever.
        let rig = rig();
        fs::create_dir_all(&rig.state).unwrap();
        let log = rig.state.join("engram-serve.log");
        mkfifo(&log);
        let state = rig.state.clone();
        assert!(within(move || open_log(&state)).is_err());
        // With a reader the open itself would succeed: the kind check refuses.
        let _reader = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&log)
            .unwrap();
        let state = rig.state.clone();
        assert!(within(move || open_log(&state)).is_err());
    }

    #[tokio::test]
    async fn a_fifo_log_means_nothing_is_spawned() {
        let rig = rig();
        let script = fake(&rig.bin_dir, "engram 3.4.0", &rig.record, "exec sleep 30");
        let server = rig.server();
        server.find().await;
        fs::create_dir_all(&rig.state).unwrap();
        mkfifo(&rig.state.join("engram-serve.log"));
        let result = within(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async { server.spawn(&script, 8123).await.map(|r| r.pid) })
        });
        assert!(result.is_err());
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(!rig.record.exists(), "the binary must not have run");
    }

    #[test]
    fn a_symlinked_log_directory_is_refused() {
        let rig = rig();
        let real = rig._dir.path().join("real");
        fs::create_dir(&real).unwrap();
        symlink(&real, &rig.state).unwrap();
        assert!(open_log(&rig.state).is_err());
        assert!(!real.join("engram-serve.log").exists());
    }

    #[test]
    fn a_log_directory_of_another_owner_is_refused() {
        let rig = rig();
        fs::create_dir_all(&rig.state).unwrap();
        assert!(open_log_as(&rig.state, euid() + 1).is_err());
        assert!(!rig.state.join("engram-serve.log").exists());
        assert!(open_log_as(&rig.state, euid()).is_ok());
    }

    #[test]
    fn a_loose_log_directory_is_tightened_to_0700() {
        let rig = rig();
        fs::create_dir_all(&rig.state).unwrap();
        fs::set_permissions(&rig.state, fs::Permissions::from_mode(0o755)).unwrap();
        open_log(&rig.state).unwrap();
        assert_eq!(fs::metadata(&rig.state).unwrap().mode() & 0o7777, 0o700);
    }

    #[tokio::test]
    async fn the_log_line_does_not_follow_a_symlink_or_open_a_fifo() {
        let rig = rig();
        fs::create_dir_all(&rig.state).unwrap();
        let target = rig.state.join("victim");
        fs::write(&target, "secret failure\n").unwrap();
        let log = rig.state.join("engram-serve.log");
        symlink(&target, &log).unwrap();
        assert_eq!(rig.server().log_line(0).await, None);
        fs::remove_file(&log).unwrap();
        mkfifo(&log);
        let server = rig.server();
        let line = within(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(server.log_line(0))
        });
        assert_eq!(line, None);
    }

    fn is_changed(error: &io::Error) -> bool {
        BinaryChanged::is(error)
    }

    #[tokio::test]
    async fn a_binary_replaced_after_the_lookup_is_not_spawned() {
        let rig = rig();
        let script = fake(&rig.bin_dir, "engram 3.4.0", &rig.record, "exec sleep 30");
        let server = rig.server();
        server.find().await;
        // The same path now holds another file (an upgrade, or an attack).
        let other = fake(
            &rig.outside_path(),
            "engram 3.4.0",
            &rig.record,
            "exec sleep 31",
        );
        fs::rename(&other, &script).unwrap();
        let error = server.spawn(&script, 8123).await.unwrap_err();
        assert!(is_changed(&error), "{error:?}");
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(!rig.record.exists(), "the replacement must not have run");
        assert!(!rig.state.exists(), "nothing was prepared either");
    }

    #[tokio::test]
    async fn a_binary_rewritten_in_place_is_not_spawned() {
        let rig = rig();
        let script = fake(&rig.bin_dir, "engram 3.4.0", &rig.record, "exec sleep 30");
        let server = rig.server();
        server.find().await;
        let mut body = fs::read_to_string(&script).unwrap();
        body.push_str("# changed\n");
        fs::write(&script, body).unwrap();
        let error = server.spawn(&script, 8123).await.unwrap_err();
        assert!(is_changed(&error), "{error:?}");
        assert!(!rig.record.exists());
    }

    #[tokio::test]
    async fn only_a_binary_that_was_looked_up_can_be_spawned() {
        let rig = rig();
        let script = fake(&rig.bin_dir, "engram 3.4.0", &rig.record, "exec sleep 30");
        let error = rig.server().spawn(&script, 8123).await.unwrap_err();
        assert!(is_changed(&error), "{error:?}");
        let server = rig.server();
        server.find().await;
        let elsewhere = fake(
            &rig.outside_path(),
            "engram 3.4.0",
            &rig.record,
            "exec sleep 30",
        );
        let error = server.spawn(&elsewhere, 8123).await.unwrap_err();
        assert!(is_changed(&error), "{error:?}");
        assert!(!rig.record.exists());
    }
}
