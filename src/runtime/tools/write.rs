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
//! use them. Nothing in `fs.rs` may reach it. Linux only: elsewhere every call
//! returns [`WriteError::Unsupported`].

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
    /// No safe write on this platform.
    Unsupported,
    /// Anything else the system reported.
    Io,
}

#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(not(target_os = "linux"))]
pub use other::*;

#[cfg(target_os = "linux")]
mod linux {
    use super::{SecretRoots, WriteError};
    use crate::runtime::tools::{
        fs::{Failure, open_checked},
        open::{cstring, fd_path, flags, open_raw},
    };
    use std::{
        ffi::CString,
        fs::File,
        os::{
            fd::{AsRawFd, OwnedFd},
            unix::{ffi::OsStrExt, fs::MetadataExt},
        },
        path::Path,
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
        let fd = match open_raw(dir.as_raw_fd(), name, flags(false)) {
            Ok(fd) => fd,
            Err(error) if error.raw_os_error() == Some(libc::ENOENT) => return Ok(None),
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

    #[cfg(test)]
    mod write_tests {
        use super::*;
        use crate::core::ai::tools::paths::SecretRoots;
        use std::{io::Read, os::unix::fs::symlink, path::PathBuf};

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
    }
}

/// Without `openat2` and `renameat2` nothing can be written safely.
#[cfg(not(target_os = "linux"))]
mod other {
    use super::{SecretRoots, WriteError};
    use std::{fs::File, path::Path};

    /// Always fails: no safe write without `openat2` and `renameat2`.
    pub fn open_target(_roots: &SecretRoots, _real: &Path) -> Result<Option<File>, WriteError> {
        Err(WriteError::Unsupported)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn every_write_primitive_refuses_cleanly() {
            let real = Path::new("/home/u/p/a");
            let roots = SecretRoots::load(Path::new("/tmp"), &[]).unwrap();
            assert_eq!(
                open_target(&roots, real).unwrap_err(),
                WriteError::Unsupported
            );
        }
    }
}
