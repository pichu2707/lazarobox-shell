//! Changing the disk below HOME without following a single symlink.
//!
//! `resolve_write` is an earlier check, not a lock; these calls are the lock.
//! Every call opens the parent folder beneath HOME with no symlink (the read
//! side's `open_checked`), judges the descriptor's real path again, and only
//! then acts relative to that descriptor. A file is never written in place: the
//! bytes go to an `O_EXCL` temp file in the same folder, are synced, and are
//! renamed into place, so a hard link or a swapped-in link is replaced, not
//! written through. The folder is synced best effort afterwards (some
//! filesystems refuse it).
//!
//! Kept out of `open.rs` on purpose: the audit in `audit.rs` bans every write
//! API from the read-side tool files, and this file is the one place that may
//! use them. Nothing in `fs.rs` may reach it. Linux and macOS: elsewhere every call
//! returns [`WriteError::Unsupported`].
//!
//! Only three things differ per system, each in a small seam: the random bytes
//! of a temp name (`getrandom` / `getentropy`), the rename that refuses a taken
//! name (`renameat2(RENAME_NOREPLACE)` / `renameatx_np(RENAME_EXCL)`) and which
//! errors mean "denied". macOS has no fallback for a filesystem without
//! `RENAME_EXCL` (exFAT, SMB, NFS): there a create answers `Unsupported`.

use crate::core::ai::tools::paths::SecretRoots;

/// Why a write primitive did nothing (or stopped).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WriteError {
    /// The path, a link, a hard link, a non-regular file or the policy said no.
    Refused,
    /// A create found the name taken.
    Exists,
    /// A replace found no file.
    Missing,
    /// A folder removal found the folder not empty.
    NotEmpty,
    /// No safe write on this platform or volume.
    Unsupported,
    /// The system denied the change (`EPERM`/`EACCES`, for example macOS TCC).
    /// Not drift and not `Unsupported`: the batch fails, the user is told.
    Denied,
    /// A replace found other bytes than the ones the user reviewed.
    Drifted,
    /// Anything else the system reported.
    Io,
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub use other::*;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use unix::*;

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix {
    #[cfg(target_os = "linux")]
    use super::linux_sys::{fill_random, rename_noreplace};
    #[cfg(target_os = "macos")]
    use super::macos_sys::{fill_random, rename_noreplace};
    use super::{SecretRoots, WriteError};
    use crate::runtime::tools::{
        fs::{Failure, open_checked},
        open::{cstring, fd_path, open_existing_at, owned},
    };
    use std::{
        ffi::CString,
        fs::{File, Permissions},
        io::{self, Read, Write},
        os::{
            fd::{AsRawFd, OwnedFd, RawFd},
            unix::{
                ffi::OsStrExt,
                fs::{MetadataExt, PermissionsExt},
            },
        },
        path::Path,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    /// The parent folder of `real`, opened beneath HOME and judged on its own
    /// descriptor, plus the file name. `real` is judged first as well.
    fn open_parent(roots: &SecretRoots, real: &Path) -> Result<(OwnedFd, CString), WriteError> {
        let (Some(dir), Some(name)) = (real.parent(), real.file_name()) else {
            return Err(WriteError::Refused);
        };
        if !roots.judge_real(real) {
            return Err(WriteError::Refused);
        }
        let fd = open_checked(roots, dir, true).map_err(|failure| match failure {
            Failure::Unreadable => WriteError::Unsupported,
            _ => WriteError::Refused,
        })?;
        Ok((
            fd,
            cstring(name.as_bytes()).map_err(|_| WriteError::Refused)?,
        ))
    }

    fn io_error<E>(_: E) -> WriteError {
        WriteError::Io
    }

    /// Opens the existing file `name` of `dir` for reading and proves it is the
    /// file `real`: not a link, regular, single linked, and still allowed by the
    /// policy at the path of the opened descriptor. `None` when it is absent.
    fn open_in(
        roots: &SecretRoots,
        dir: &OwnedFd,
        name: &CString,
        real: &Path,
    ) -> Result<Option<File>, WriteError> {
        let fd = match open_existing_at(dir.as_raw_fd(), name) {
            Ok(fd) => fd,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(WriteError::Refused), // ELOOP: a link in the last place
        };
        match fd_path(&fd) {
            Ok(opened) if opened == real && roots.judge_real(&opened) => {}
            _ => return Err(WriteError::Refused),
        }
        let file = File::from(fd);
        let meta = file.metadata().map_err(io_error)?;
        if meta.is_file() && meta.nlink() == 1 {
            Ok(Some(file))
        } else {
            Err(WriteError::Refused)
        }
    }

    /// Opens the file `real` for reading, or `None` when it is absent. Refuses a
    /// link anywhere on the way, a hard-linked or non-regular file, and a path
    /// the policy denies. The preflight of an apply reads the bytes from it.
    pub fn open_target(roots: &SecretRoots, real: &Path) -> Result<Option<File>, WriteError> {
        let (dir, name) = open_parent(roots, real)?;
        open_in(roots, &dir, &name, real)
    }

    /// Makes the folder `real` (one level: its parent must exist) with the
    /// default mode. The parent is opened with no symlink; `mkdirat` itself
    /// never follows one.
    pub fn make_dir(roots: &SecretRoots, real: &Path) -> Result<(), WriteError> {
        let (dir, name) = open_parent(roots, real)?;
        // SAFETY: `name` is NUL-terminated and `dir` is an open descriptor.
        if unsafe { libc::mkdirat(dir.as_raw_fd(), name.as_ptr(), 0o777) } != 0 {
            let errno = io::Error::last_os_error().raw_os_error();
            return Err(match errno {
                Some(libc::EEXIST) => WriteError::Exists,
                _ => denied_or(errno, WriteError::Io),
            });
        }
        sync_dir(&dir);
        Ok(())
    }

    /// Creates the file `real` with `bytes`. Never replaces: the final
    /// `renameat2(RENAME_NOREPLACE)` fails with [`WriteError::Exists`] when the
    /// name is taken, even by a dangling link.
    pub fn create_file(roots: &SecretRoots, real: &Path, bytes: &[u8]) -> Result<(), WriteError> {
        create_file_with(roots, real, bytes, || {})
    }

    /// [`create_file`] with a seam that runs after the staging (for tests).
    fn create_file_with(
        roots: &SecretRoots,
        real: &Path,
        bytes: &[u8],
        after_stage: impl FnOnce(),
    ) -> Result<(), WriteError> {
        let (dir, name) = open_parent(roots, real)?;
        let mut temp = stage(&dir, bytes, 0o666, None)?;
        after_stage();
        same_folder(&dir, real)?;
        rename(&dir, &mut temp, &name, true)?;
        sync_dir(&dir);
        Ok(())
    }

    /// Replaces the existing file `real` with `bytes`, keeping its permission
    /// bits (never setuid, setgid or sticky). `expected` is what the user
    /// reviewed: the bytes read through the opened file must still be those,
    /// or the answer is [`WriteError::Drifted`]. The old file is unlinked from
    /// the name, never written, so another hard link keeps the old bytes (and
    /// is refused before that anyway).
    pub fn replace_file(
        roots: &SecretRoots,
        real: &Path,
        expected: &[u8],
        bytes: &[u8],
    ) -> Result<(), WriteError> {
        let (dir, name) = open_parent(roots, real)?;
        let old = open_in(roots, &dir, &name, real)?.ok_or(WriteError::Missing)?;
        holds(&old, expected)?;
        let mode = old.metadata().map_err(io_error)?.mode() & 0o777;
        let mut temp = stage(&dir, bytes, mode, Some(mode))?;
        same_folder(&dir, real)?;
        rename(&dir, &mut temp, &name, false)?;
        sync_dir(&dir);
        Ok(())
    }

    /// Removes the file `real`, but only while it still holds `expected`: the
    /// undo of a create must not delete a file someone else put there.
    pub fn delete_file(
        roots: &SecretRoots,
        real: &Path,
        expected: &[u8],
    ) -> Result<(), WriteError> {
        let (dir, name) = open_parent(roots, real)?;
        let old = open_in(roots, &dir, &name, real)?.ok_or(WriteError::Missing)?;
        holds(&old, expected)?;
        same_folder(&dir, real)?;
        unlink(&dir, &name, 0)?;
        sync_dir(&dir);
        Ok(())
    }

    /// Removes the empty folder `real`. A link, a file or a folder with
    /// content is left alone (`unlinkat` with `AT_REMOVEDIR` never follows).
    pub fn delete_dir(roots: &SecretRoots, real: &Path) -> Result<(), WriteError> {
        let (dir, name) = open_parent(roots, real)?;
        unlink(&dir, &name, libc::AT_REMOVEDIR)?;
        sync_dir(&dir);
        Ok(())
    }

    /// `unlinkat` relative to `dir`; the name is not followed.
    fn unlink(dir: &OwnedFd, name: &CString, flags: i32) -> Result<(), WriteError> {
        // SAFETY: `name` is NUL-terminated and `dir` is an open descriptor.
        if unsafe { libc::unlinkat(dir.as_raw_fd(), name.as_ptr(), flags) } == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        Err(match error.kind() {
            io::ErrorKind::NotFound => WriteError::Missing,
            io::ErrorKind::DirectoryNotEmpty => WriteError::NotEmpty,
            _ => denied_or(error.raw_os_error(), WriteError::Io),
        })
    }

    /// [`WriteError::Drifted`] unless the opened file holds exactly `expected`.
    fn holds(file: &File, expected: &[u8]) -> Result<(), WriteError> {
        let mut current = Vec::new();
        file.take(expected.len() as u64 + 1)
            .read_to_end(&mut current)
            .map_err(io_error)?;
        if current == expected {
            Ok(())
        } else {
            Err(WriteError::Drifted)
        }
    }

    /// Refuses when the folder behind `dir` is no longer the parent of `real`
    /// (it was moved or renamed after it was judged).
    fn same_folder(dir: &OwnedFd, real: &Path) -> Result<(), WriteError> {
        match fd_path(dir) {
            Ok(now) if Some(now.as_path()) == real.parent() => Ok(()),
            _ => Err(WriteError::Refused),
        }
    }

    /// A temp file that is unlinked when dropped unless it was renamed away.
    struct Temp {
        dir: RawFd,
        name: CString,
        renamed: bool,
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            if !self.renamed {
                // SAFETY: `dir` outlives the guard (it is declared first) and
                // `name` is NUL-terminated. A failure leaves nothing to do.
                unsafe { libc::unlinkat(self.dir, self.name.as_ptr(), 0) };
            }
        }
    }

    /// `.lbx-<32 random hex digits>.tmp`: unpredictable, and short enough for
    /// any file name limit whatever the target is called.
    fn temp_name() -> io::Result<CString> {
        temp_name_from(random_bytes(fill_random))
    }

    fn temp_name_from(random: [u8; 16]) -> io::Result<CString> {
        let hex: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        cstring(format!(".lbx-{hex}.tmp").as_bytes())
    }

    /// 16 bytes from `fill`, retried on EINTR. On any other error (EAGAIN: the
    /// pool is not ready yet) a nonce stands in: the name only has to be
    /// unique, and `O_EXCL` guarantees nothing is clobbered.
    fn random_bytes(mut fill: impl FnMut(&mut [u8; 16]) -> io::Result<()>) -> [u8; 16] {
        let mut random = [0u8; 16];
        loop {
            match fill(&mut random) {
                Ok(()) => return random,
                Err(error) if error.raw_os_error() == Some(libc::EINTR) => {}
                Err(_) => return nonce(),
            }
        }
    }

    /// Time, process id and a counter: distinct for every call in a process.
    fn nonce() -> [u8; 16] {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos() as u64);
        // SAFETY: `getpid` takes no argument and cannot fail.
        let pid = unsafe { libc::getpid() } as u32;
        let count = COUNTER.fetch_add(1, Ordering::Relaxed) as u32;
        let mut out = [0u8; 16];
        out[..8].copy_from_slice(&nanos.to_le_bytes());
        out[8..12].copy_from_slice(&pid.to_le_bytes());
        out[12..].copy_from_slice(&count.to_le_bytes());
        out
    }

    /// Writes `bytes` to a new `O_EXCL` temp file in `dir` and syncs it. On any
    /// error the guard removes the file. `chmod` sets the exact mode (the open
    /// mode is cut by the umask).
    fn stage(
        dir: &OwnedFd,
        bytes: &[u8],
        open_mode: u32,
        chmod: Option<u32>,
    ) -> Result<Temp, WriteError> {
        let create = libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW;
        for _ in 0..4 {
            let name = temp_name().map_err(io_error)?;
            // SAFETY: `name` is NUL-terminated; the mode is a plain integer.
            let fd = unsafe {
                libc::openat(
                    dir.as_raw_fd(),
                    name.as_ptr(),
                    create | libc::O_CLOEXEC,
                    open_mode as libc::c_uint,
                )
            };
            match owned(fd) {
                Err(error) if error.raw_os_error() == Some(libc::EEXIST) => continue,
                Err(error) => return Err(denied_or(error.raw_os_error(), WriteError::Io)),
                Ok(fd) => {
                    let temp = Temp {
                        dir: dir.as_raw_fd(),
                        name,
                        renamed: false,
                    };
                    let mut file = File::from(fd);
                    file.write_all(bytes).map_err(io_error)?;
                    if let Some(mode) = chmod {
                        file.set_permissions(Permissions::from_mode(mode))
                            .map_err(io_error)?;
                    }
                    file.sync_all().map_err(io_error)?;
                    return Ok(temp);
                }
            }
        }
        Err(WriteError::Io)
    }

    /// Moves the temp file onto `to` in the same folder. `noreplace` refuses a
    /// taken name (`renameat2`); otherwise the name is replaced (`renameat`).
    fn rename(
        dir: &OwnedFd,
        temp: &mut Temp,
        to: &CString,
        noreplace: bool,
    ) -> Result<(), WriteError> {
        let fd = dir.as_raw_fd();
        let done = if noreplace {
            rename_noreplace(fd, &temp.name, to)
        } else {
            // SAFETY: both names are NUL-terminated and `fd` is an open descriptor.
            if unsafe { libc::renameat(fd, temp.name.as_ptr(), fd, to.as_ptr()) } == 0 {
                Ok(())
            } else {
                // Read at once: nothing may run between the call and `errno`.
                let errno = io::Error::last_os_error().raw_os_error();
                Err(denied_or(errno, WriteError::Io))
            }
        };
        if done.is_ok() {
            // The temp name is gone: the guard has nothing to remove.
            temp.renamed = true;
        }
        done
    }

    /// [`WriteError::Denied`] when `errno` (read right next to the failing call)
    /// is a denial macOS raises (`EPERM`/`EACCES`: TCC, a data vault), else
    /// `otherwise`. A denial is neither drift nor `Unsupported`, which `apply`
    /// would turn into a whole-batch answer. On Linux it is always `otherwise`.
    fn denied_or(errno: Option<i32>, otherwise: WriteError) -> WriteError {
        if cfg!(target_os = "macos") && matches!(errno, Some(libc::EPERM | libc::EACCES)) {
            WriteError::Denied
        } else {
            otherwise
        }
    }

    /// Best effort: some filesystems cannot sync a folder.
    fn sync_dir(dir: &OwnedFd) {
        // SAFETY: `dir` is an open descriptor.
        unsafe { libc::fsync(dir.as_raw_fd()) };
    }

    #[cfg(test)]
    mod write_tests {
        use super::*;
        use crate::core::ai::tools::paths::SecretRoots;
        use std::{
            io::Read,
            os::unix::fs::{MetadataExt, PermissionsExt, symlink},
            path::PathBuf,
        };

        struct Fx {
            _dir: tempfile::TempDir,
            home: PathBuf,
            roots: SecretRoots,
        }

        fn fx() -> Fx {
            let dir = tempfile::tempdir().unwrap();
            let home = dir.path().canonicalize().unwrap();
            std::fs::create_dir_all(home.join("proj/src")).unwrap();
            std::fs::create_dir_all(home.join("elsewhere")).unwrap();
            let roots = SecretRoots::load(&home, &[]).unwrap();
            Fx {
                _dir: dir,
                home,
                roots,
            }
        }

        fn names(dir: &Path) -> Vec<String> {
            let mut all: Vec<String> = std::fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            all.sort();
            all
        }

        // Wb.1: opening a target

        #[test]
        fn a_regular_file_opens_and_an_absent_one_is_none() {
            let f = fx();
            std::fs::write(f.home.join("proj/a.txt"), "hello").unwrap();
            let mut file = open_target(&f.roots, &f.home.join("proj/a.txt"))
                .unwrap()
                .unwrap();
            let mut text = String::new();
            file.read_to_string(&mut text).unwrap();
            assert_eq!(text, "hello");
            assert!(
                open_target(&f.roots, &f.home.join("proj/none.txt"))
                    .unwrap()
                    .is_none()
            );
        }

        #[test]
        fn a_final_symlink_is_refused_even_when_it_points_inside_or_dangles() {
            let f = fx();
            std::fs::write(f.home.join("proj/real.txt"), "x").unwrap();
            symlink(f.home.join("proj/real.txt"), f.home.join("proj/inside")).unwrap();
            symlink(f.home.join("proj/gone"), f.home.join("proj/dangling")).unwrap();
            for name in ["inside", "dangling"] {
                let got = open_target(&f.roots, &f.home.join("proj").join(name));
                assert_eq!(got.unwrap_err(), WriteError::Refused, "{name}");
            }
        }

        #[test]
        fn a_folder_swapped_for_a_symlink_after_the_check_is_refused() {
            let f = fx();
            std::fs::write(f.home.join("elsewhere/a.txt"), "x").unwrap();
            let real = f.home.join("proj/src/a.txt"); // judged while src was a folder
            std::fs::remove_dir(f.home.join("proj/src")).unwrap();
            symlink(f.home.join("elsewhere"), f.home.join("proj/src")).unwrap();
            assert_eq!(
                open_target(&f.roots, &real).unwrap_err(),
                WriteError::Refused
            );
        }

        #[test]
        fn a_hard_linked_file_is_refused_and_a_single_link_is_not() {
            let f = fx();
            std::fs::write(f.home.join("proj/a.txt"), "x").unwrap();
            assert!(open_target(&f.roots, &f.home.join("proj/a.txt")).is_ok());
            std::fs::hard_link(f.home.join("proj/a.txt"), f.home.join("elsewhere/b")).unwrap();
            let got = open_target(&f.roots, &f.home.join("proj/a.txt"));
            assert_eq!(got.unwrap_err(), WriteError::Refused);
        }

        #[test]
        fn a_folder_and_a_fifo_are_not_regular_files() {
            let f = fx();
            let fifo = CString::new(f.home.join("proj/pipe").as_os_str().as_bytes()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
            for name in ["src", "pipe"] {
                let got = open_target(&f.roots, &f.home.join("proj").join(name));
                assert_eq!(got.unwrap_err(), WriteError::Refused, "{name}");
            }
        }

        #[test]
        fn a_path_the_policy_denies_is_refused_even_if_it_exists() {
            let f = fx();
            std::fs::create_dir_all(f.home.join(".ssh")).unwrap();
            std::fs::write(f.home.join(".ssh/config"), "x").unwrap();
            let got = open_target(&f.roots, &f.home.join(".ssh/config"));
            assert_eq!(got.unwrap_err(), WriteError::Refused);
            let outside = open_target(&f.roots, Path::new("/etc/hostname"));
            assert_eq!(outside.unwrap_err(), WriteError::Refused);
        }

        // Wb.2: the writes

        #[test]
        fn make_dir_creates_one_folder_and_refuses_an_existing_or_missing_parent() {
            let f = fx();
            make_dir(&f.roots, &f.home.join("proj/new")).unwrap();
            assert!(f.home.join("proj/new").is_dir());
            assert_eq!(
                make_dir(&f.roots, &f.home.join("proj/new")).unwrap_err(),
                WriteError::Exists
            );
            assert_eq!(
                make_dir(&f.roots, &f.home.join("proj/a/b")).unwrap_err(),
                WriteError::Refused
            );
        }

        #[test]
        fn make_dir_never_goes_through_a_symlinked_parent() {
            let f = fx();
            symlink(f.home.join("elsewhere"), f.home.join("proj/link")).unwrap();
            let got = make_dir(&f.roots, &f.home.join("proj/link/new"));
            assert_eq!(got.unwrap_err(), WriteError::Refused);
            assert!(!f.home.join("elsewhere/new").exists());
        }

        #[test]
        fn create_writes_the_bytes_and_leaves_no_temp_file() {
            let f = fx();
            let bytes = b"\xef\xbb\xbfno final newline";
            create_file(&f.roots, &f.home.join("proj/src/new.rs"), bytes).unwrap();
            assert_eq!(
                std::fs::read(f.home.join("proj/src/new.rs")).unwrap(),
                bytes
            );
            assert_eq!(names(&f.home.join("proj/src")), ["new.rs"]);
            create_file(&f.roots, &f.home.join("proj/empty"), b"").unwrap();
            assert_eq!(
                std::fs::metadata(f.home.join("proj/empty")).unwrap().len(),
                0
            );
        }

        #[test]
        fn create_never_replaces_an_existing_file_and_cleans_its_temp() {
            let f = fx();
            std::fs::write(f.home.join("proj/a.txt"), "mine").unwrap();
            let got = create_file(&f.roots, &f.home.join("proj/a.txt"), b"theirs");
            assert_eq!(got.unwrap_err(), WriteError::Exists);
            assert_eq!(std::fs::read(f.home.join("proj/a.txt")).unwrap(), b"mine");
            assert_eq!(names(&f.home.join("proj")), ["a.txt", "src"]);
        }

        #[test]
        fn create_refuses_a_symlink_in_the_way_and_a_missing_parent() {
            let f = fx();
            symlink(f.home.join("elsewhere"), f.home.join("proj/link")).unwrap();
            let got = create_file(&f.roots, &f.home.join("proj/link/x"), b"x");
            assert_eq!(got.unwrap_err(), WriteError::Refused);
            symlink(f.home.join("elsewhere/t"), f.home.join("proj/dangling")).unwrap();
            let got = create_file(&f.roots, &f.home.join("proj/dangling"), b"x");
            assert_eq!(got.unwrap_err(), WriteError::Exists);
            assert!(!f.home.join("elsewhere/t").exists());
            let got = create_file(&f.roots, &f.home.join("proj/no/dir/x"), b"x");
            assert_eq!(got.unwrap_err(), WriteError::Refused);
        }

        #[test]
        fn replace_swaps_the_content_keeps_the_mode_and_leaves_no_temp_file() {
            let f = fx();
            let path = f.home.join("proj/run.sh");
            for mode in [0o750, 0o640] {
                std::fs::write(&path, "old").unwrap();
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
                replace_file(&f.roots, &path, b"old", b"new").unwrap();
                assert_eq!(std::fs::read(&path).unwrap(), b"new");
                let meta = std::fs::metadata(&path).unwrap();
                assert_eq!(meta.mode() & 0o7777, mode, "{mode:o}");
                assert_eq!(names(&f.home.join("proj")), ["run.sh", "src"]);
            }
        }

        #[test]
        fn replace_refuses_a_hard_link_a_symlink_and_an_absent_file() {
            let f = fx();
            std::fs::write(f.home.join("proj/a.txt"), "old").unwrap();
            std::fs::hard_link(f.home.join("proj/a.txt"), f.home.join("elsewhere/b")).unwrap();
            let got = replace_file(&f.roots, &f.home.join("proj/a.txt"), b"old", b"new");
            assert_eq!(got.unwrap_err(), WriteError::Refused);
            assert_eq!(std::fs::read(f.home.join("elsewhere/b")).unwrap(), b"old");
            std::fs::write(f.home.join("elsewhere/t"), "old").unwrap();
            symlink(f.home.join("elsewhere/t"), f.home.join("proj/l")).unwrap();
            let got = replace_file(&f.roots, &f.home.join("proj/l"), b"old", b"new");
            assert_eq!(got.unwrap_err(), WriteError::Refused);
            assert_eq!(std::fs::read(f.home.join("elsewhere/t")).unwrap(), b"old");
            let got = replace_file(&f.roots, &f.home.join("proj/none"), b"old", b"new");
            assert_eq!(got.unwrap_err(), WriteError::Missing);
            assert_eq!(names(&f.home.join("proj")), ["a.txt", "l", "src"]);
        }

        // Review fixes

        #[test]
        fn replace_refuses_a_file_that_changed_since_it_was_reviewed() {
            let f = fx();
            let path = f.home.join("proj/a.txt");
            std::fs::write(&path, "reviewed").unwrap();
            std::fs::write(&path, "changed meanwhile").unwrap();
            let got = replace_file(&f.roots, &path, b"reviewed", b"new");
            assert_eq!(got.unwrap_err(), WriteError::Drifted);
            assert_eq!(std::fs::read(&path).unwrap(), b"changed meanwhile");
            assert_eq!(names(&f.home.join("proj")), ["a.txt", "src"]);
            // A longer file that starts with the reviewed bytes drifted too.
            std::fs::write(&path, "reviewed and more").unwrap();
            let got = replace_file(&f.roots, &path, b"reviewed", b"new");
            assert_eq!(got.unwrap_err(), WriteError::Drifted);
            assert_eq!(std::fs::read(&path).unwrap(), b"reviewed and more");
        }

        #[test]
        fn replace_never_copies_setuid_setgid_or_sticky() {
            let f = fx();
            let path = f.home.join("proj/tool");
            for (from, to) in [(0o4755, 0o755), (0o2750, 0o750)] {
                assert_replace_drops_special_bits(&f, &path, from, to);
            }
        }

        // Linux only: a regular file with the sticky bit is refused by chmod on
        // macOS for a non-root user (EFTYPE).
        #[cfg(target_os = "linux")]
        #[test]
        fn replace_never_copies_the_sticky_bit() {
            let f = fx();
            assert_replace_drops_special_bits(&f, &f.home.join("proj/tool"), 0o1777, 0o777);
        }

        fn assert_replace_drops_special_bits(f: &Fx, path: &Path, from: u32, to: u32) {
            std::fs::write(path, "old").unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(from)).unwrap();
            replace_file(&f.roots, path, b"old", b"new").unwrap();
            let mode = std::fs::metadata(path).unwrap().mode() & 0o7777;
            assert_eq!(mode, to, "{from:o}");
        }

        #[test]
        fn a_leftover_temp_is_not_a_target_and_does_not_break_the_next_create() {
            let f = fx();
            let temp = f
                .home
                .join("proj/.lbx-0123456789abcdef0123456789abcdef.tmp");
            std::fs::write(&temp, "crashed").unwrap();
            assert_eq!(
                open_target(&f.roots, &temp).unwrap_err(),
                WriteError::Refused
            );
            assert_eq!(
                create_file(&f.roots, &temp, b"x").unwrap_err(),
                WriteError::Refused
            );
            create_file(&f.roots, &f.home.join("proj/new.txt"), b"x").unwrap();
            assert_eq!(std::fs::read(&temp).unwrap(), b"crashed");
            assert_eq!(names(&f.home.join("proj")).len(), 3);
        }

        #[test]
        fn a_moved_folder_is_refused_between_the_stage_and_the_rename() {
            let f = fx();
            let real = f.home.join("proj/src/new.rs");
            let moved = f.home.join("proj/moved");
            let got = create_file_with(&f.roots, &real, b"x", || {
                std::fs::rename(f.home.join("proj/src"), &moved).unwrap();
            });
            assert_eq!(got.unwrap_err(), WriteError::Refused);
            assert_eq!(names(&moved), Vec::<String>::new());
            assert!(!f.home.join("proj/src").exists());
        }

        #[test]
        fn a_missing_random_pool_or_a_signal_never_blocks_or_fails_the_name() {
            let mut calls = 0;
            let bytes = random_bytes(|buf| {
                calls += 1;
                if calls < 3 {
                    return Err(io::Error::from_raw_os_error(libc::EINTR));
                }
                buf.fill(0xab);
                Ok(())
            });
            assert_eq!((calls, bytes), (3, [0xab; 16]));
            // The pool is not ready (EAGAIN): a nonce stands in, still unique.
            let pool = || Err(io::Error::from_raw_os_error(libc::EAGAIN));
            assert_ne!(random_bytes(|_| pool()), random_bytes(|_| pool()));
            let name = temp_name_from(random_bytes(|_| pool())).unwrap();
            assert!(crate::core::ai::tools::paths::is_temp_name(
                name.to_str().unwrap()
            ));
        }

        #[test]
        fn only_macos_reads_eperm_and_eacces_as_a_denial() {
            let expected = if cfg!(target_os = "macos") {
                WriteError::Denied
            } else {
                WriteError::Io
            };
            for errno in [libc::EPERM, libc::EACCES] {
                assert_eq!(denied_or(Some(errno), WriteError::Io), expected);
            }
            assert_eq!(
                denied_or(Some(libc::ENOSPC), WriteError::Io),
                WriteError::Io
            );
            assert_eq!(denied_or(None, WriteError::Io), WriteError::Io);
        }

        // macOS only (unverified on a Mac): a denial such as TCC's EPERM/EACCES is
        // `Denied`, never `Unsupported`, which `apply` would turn into a
        // whole-batch answer. A read-only folder stands in for the denial.
        #[cfg(target_os = "macos")]
        #[test]
        fn a_denied_write_is_a_refusal_and_never_unsupported() {
            let f = fx();
            let locked = f.home.join("proj/locked");
            std::fs::create_dir(&locked).unwrap();
            std::fs::set_permissions(&locked, Permissions::from_mode(0o555)).unwrap();
            let got = create_file(&f.roots, &locked.join("a.txt"), b"x");
            // Root is never denied: a plain write into the folder tells.
            let denied = std::fs::write(locked.join("probe"), "").is_err();
            std::fs::set_permissions(&locked, Permissions::from_mode(0o755)).unwrap();
            if denied {
                assert_eq!(got.unwrap_err(), WriteError::Denied);
            }
        }

        // We2.2: the undo primitives

        #[test]
        fn delete_file_removes_only_a_file_that_still_holds_what_was_written() {
            let f = fx();
            let real = f.home.join("proj/a.txt");
            std::fs::write(&real, "ours").unwrap();
            // Someone else replaced it: it stays.
            assert_eq!(
                delete_file(&f.roots, &real, b"other").unwrap_err(),
                WriteError::Drifted
            );
            assert_eq!(std::fs::read_to_string(&real).unwrap(), "ours");
            delete_file(&f.roots, &real, b"ours").unwrap();
            assert!(!real.exists());
            assert_eq!(
                delete_file(&f.roots, &real, b"ours").unwrap_err(),
                WriteError::Missing
            );
        }

        #[test]
        fn delete_file_refuses_a_link_a_hard_link_a_folder_and_a_denied_path() {
            let f = fx();
            std::fs::write(f.home.join("proj/real.txt"), "x").unwrap();
            symlink(f.home.join("proj/real.txt"), f.home.join("proj/link")).unwrap();
            std::fs::write(f.home.join("proj/h"), "x").unwrap();
            std::fs::hard_link(f.home.join("proj/h"), f.home.join("elsewhere/h2")).unwrap();
            for name in ["link", "h", "src"] {
                let got = delete_file(&f.roots, &f.home.join("proj").join(name), b"x");
                assert_eq!(got.unwrap_err(), WriteError::Refused, "{name}");
            }
            assert!(f.home.join("proj/real.txt").exists());
            assert!(f.home.join("proj/h").exists());
            let outside = delete_file(&f.roots, Path::new("/etc/hostname"), b"x");
            assert_eq!(outside.unwrap_err(), WriteError::Refused);
        }

        #[test]
        fn delete_dir_removes_an_empty_folder_and_nothing_else() {
            let f = fx();
            std::fs::create_dir(f.home.join("proj/empty")).unwrap();
            std::fs::create_dir(f.home.join("proj/full")).unwrap();
            std::fs::write(f.home.join("proj/full/a"), "x").unwrap();
            symlink(f.home.join("elsewhere"), f.home.join("proj/link")).unwrap();
            delete_dir(&f.roots, &f.home.join("proj/empty")).unwrap();
            assert!(!f.home.join("proj/empty").exists());
            let full = delete_dir(&f.roots, &f.home.join("proj/full"));
            assert_eq!(full.unwrap_err(), WriteError::NotEmpty);
            let link = delete_dir(&f.roots, &f.home.join("proj/link"));
            assert!(link.is_err());
            assert!(f.home.join("proj/full/a").exists());
            assert!(f.home.join("elsewhere").is_dir());
            assert_eq!(
                delete_dir(&f.roots, &f.home.join("proj/empty")).unwrap_err(),
                WriteError::Missing
            );
            let outside = delete_dir(&f.roots, Path::new("/etc"));
            assert_eq!(outside.unwrap_err(), WriteError::Refused);
        }

        #[test]
        fn temp_names_are_unpredictable_and_short() {
            let first = temp_name().unwrap();
            let second = temp_name().unwrap();
            assert_ne!(first, second);
            for name in [&first, &second] {
                let text = name.to_str().unwrap();
                assert!(
                    text.starts_with(".lbx-") && text.ends_with(".tmp"),
                    "{text}"
                );
                assert!(text.len() < 64, "{text}");
            }
        }
    }
}

/// The Linux seams: `getrandom` and `renameat2(RENAME_NOREPLACE)`.
#[cfg(target_os = "linux")]
mod linux_sys {
    use super::WriteError;
    use std::{ffi::CString, io, os::fd::RawFd};

    /// Fills `buf` from `getrandom`. `GRND_NONBLOCK` keeps an uninitialised
    /// pool from blocking the call.
    pub fn fill_random(buf: &mut [u8; 16]) -> io::Result<()> {
        // SAFETY: the buffer is valid for `len` bytes.
        let got =
            unsafe { libc::getrandom(buf.as_mut_ptr().cast(), buf.len(), libc::GRND_NONBLOCK) };
        if usize::try_from(got) == Ok(buf.len()) {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// Renames `from` to `to` in `fd`, refusing a taken name (even a dangling
    /// link) with [`WriteError::Exists`].
    pub fn rename_noreplace(fd: RawFd, from: &CString, to: &CString) -> Result<(), WriteError> {
        // SAFETY: both names are NUL-terminated and `fd` is an open descriptor.
        let done = unsafe {
            libc::syscall(
                libc::SYS_renameat2,
                fd,
                from.as_ptr(),
                fd,
                to.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if done == 0 {
            return Ok(());
        }
        Err(match io::Error::last_os_error().raw_os_error() {
            Some(libc::EEXIST) => WriteError::Exists,
            _ => WriteError::Io,
        })
    }
}

/// The macOS seams: `getentropy` and `renameatx_np(RENAME_EXCL)`. Written
/// without a macOS compiler at hand (type-checked in a scratch crate): keep it
/// small. `File::sync_all` already issues `F_FULLFSYNC` on Apple targets.
#[cfg(target_os = "macos")]
mod macos_sys {
    use super::WriteError;
    use std::{ffi::CString, io, os::fd::RawFd};

    /// Fills `buf` from `getentropy` (it never blocks and takes up to 256 bytes).
    pub fn fill_random(buf: &mut [u8; 16]) -> io::Result<()> {
        // SAFETY: the buffer is valid for 16 bytes.
        if unsafe { libc::getentropy(buf.as_mut_ptr().cast(), buf.len()) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// Renames `from` to `to` in `fd`, refusing a taken name with
    /// [`WriteError::Exists`]. A filesystem without `RENAME_EXCL` (exFAT, SMB,
    /// NFS) answers `ENOTSUP`, `EOPNOTSUPP` or `EINVAL`:
    /// [`WriteError::Unsupported`], with no `linkat` fallback. A denial (TCC) is
    /// [`WriteError::Denied`].
    pub fn rename_noreplace(fd: RawFd, from: &CString, to: &CString) -> Result<(), WriteError> {
        // SAFETY: both names are NUL-terminated and `fd` is an open descriptor.
        let done =
            unsafe { libc::renameatx_np(fd, from.as_ptr(), fd, to.as_ptr(), libc::RENAME_EXCL) };
        if done == 0 {
            return Ok(());
        }
        Err(match io::Error::last_os_error().raw_os_error() {
            Some(libc::EEXIST) => WriteError::Exists,
            Some(libc::ENOTSUP | libc::EOPNOTSUPP | libc::EINVAL) => WriteError::Unsupported,
            Some(libc::EPERM | libc::EACCES) => WriteError::Denied,
            _ => WriteError::Io,
        })
    }
}

/// Without `openat2` and `renameat2` nothing can be written safely.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod other {
    use super::{SecretRoots, WriteError};
    use std::{fs::File, path::Path};

    /// Always fails: no safe write without `openat2` and `renameat2`.
    pub fn open_target(_roots: &SecretRoots, _real: &Path) -> Result<Option<File>, WriteError> {
        Err(WriteError::Unsupported)
    }

    /// Always fails: see [`open_target`].
    pub fn make_dir(_roots: &SecretRoots, _real: &Path) -> Result<(), WriteError> {
        Err(WriteError::Unsupported)
    }

    /// Always fails: see [`open_target`].
    pub fn create_file(
        _roots: &SecretRoots,
        _real: &Path,
        _bytes: &[u8],
    ) -> Result<(), WriteError> {
        Err(WriteError::Unsupported)
    }

    /// Always fails: see [`open_target`].
    pub fn replace_file(
        _roots: &SecretRoots,
        _real: &Path,
        _expected: &[u8],
        _bytes: &[u8],
    ) -> Result<(), WriteError> {
        Err(WriteError::Unsupported)
    }

    /// Always fails: see [`open_target`].
    pub fn delete_file(
        _roots: &SecretRoots,
        _real: &Path,
        _expected: &[u8],
    ) -> Result<(), WriteError> {
        Err(WriteError::Unsupported)
    }

    /// Always fails: see [`open_target`].
    pub fn delete_dir(_roots: &SecretRoots, _real: &Path) -> Result<(), WriteError> {
        Err(WriteError::Unsupported)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn every_write_primitive_refuses_cleanly() {
            let real = Path::new("/home/u/p/a");
            // Not `/tmp`: it is a system folder, so `load` refuses it.
            let home = tempfile::tempdir().unwrap();
            let roots = SecretRoots::load(home.path(), &[]).unwrap();
            assert_eq!(
                open_target(&roots, real).unwrap_err(),
                WriteError::Unsupported
            );
            assert_eq!(make_dir(&roots, real).unwrap_err(), WriteError::Unsupported);
            assert_eq!(
                create_file(&roots, real, b"x").unwrap_err(),
                WriteError::Unsupported
            );
            assert_eq!(
                replace_file(&roots, real, b"o", b"x").unwrap_err(),
                WriteError::Unsupported
            );
        }
    }
}
