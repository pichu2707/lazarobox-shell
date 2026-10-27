//! Opening a path below HOME without following a single symlink.
//!
//! The path policy judges names; this module is what stops a link swapped in
//! between the judgment and the open. Linux only: elsewhere every open fails and
//! the tools report "could not read" (there is no `/proc/self/fd` to re-check).
//!
//! Mounts: `RESOLVE_NO_XDEV` is deliberately NOT used. A bind or network mount
//! below HOME (a projects disk, a synced folder) is legitimate, so crossing a
//! mount point is allowed; what is refused is any symlink, and any real path
//! that the policy judges again on the opened descriptor.

#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(not(target_os = "linux"))]
pub use other::*;

#[cfg(target_os = "linux")]
mod linux {
    use std::{
        ffi::CString,
        io,
        os::{
            fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
            unix::ffi::OsStrExt,
        },
        path::{Component, Path, PathBuf},
    };

    /// How a path is opened. `Openat2` is preferred; `Walk` is the fallback for a
    /// kernel (or a seccomp filter) without `openat2`.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum Strategy {
        Openat2,
        Walk,
    }

    /// Opens `rel` (a relative path of plain names) below `home`, never following
    /// a symlink in any component. `dir` demands a directory.
    ///
    /// Tries `openat2` with `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS` and falls back
    /// to one `O_NOFOLLOW` open per component when the kernel lacks it.
    pub fn open_beneath(home: &Path, rel: &Path, dir: bool) -> io::Result<OwnedFd> {
        with_fallback(open_with(Strategy::Openat2, home, rel, dir), home, rel, dir)
    }

    /// Keeps the `openat2` outcome unless it says the call itself is unavailable.
    fn with_fallback(
        first: io::Result<OwnedFd>,
        home: &Path,
        rel: &Path,
        dir: bool,
    ) -> io::Result<OwnedFd> {
        match first {
            Err(error)
                if matches!(
                    error.raw_os_error(),
                    Some(libc::ENOSYS | libc::EPERM | libc::EINVAL | libc::E2BIG)
                ) =>
            {
                open_with(Strategy::Walk, home, rel, dir)
            }
            other => other,
        }
    }

    /// [`open_beneath`] with a fixed strategy (a seam for tests).
    pub fn open_with(
        strategy: Strategy,
        home: &Path,
        rel: &Path,
        dir: bool,
    ) -> io::Result<OwnedFd> {
        let names = plain_names(rel)?;
        let base = open_raw(libc::AT_FDCWD, &cstring(home.as_os_str().as_bytes())?, {
            libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW
        })?;
        match strategy {
            Strategy::Openat2 => {
                let joined = names.join(&b"/"[..]);
                let path = cstring(if joined.is_empty() { b"." } else { &joined })?;
                openat2(base.as_raw_fd(), &path, flags(dir))
            }
            Strategy::Walk => walk(base, &names, dir),
        }
    }

    /// The real path of an open descriptor, read from `/proc/self/fd`.
    pub fn fd_path(fd: &OwnedFd) -> io::Result<PathBuf> {
        std::fs::read_link(format!("/proc/self/fd/{}", fd.as_raw_fd()))
    }

    /// The open flags: read only, no controlling terminal, never blocking on a
    /// FIFO, and no link followed in the last component.
    fn flags(dir: bool) -> i32 {
        let base =
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_NOCTTY;
        if dir { base | libc::O_DIRECTORY } else { base }
    }

    /// Splits `rel` into plain names. `.`, `..`, a root or a prefix is refused.
    fn plain_names(rel: &Path) -> io::Result<Vec<Vec<u8>>> {
        rel.components()
            .map(|part| match part {
                Component::Normal(name) => Ok(name.as_bytes().to_vec()),
                _ => Err(io::Error::from(io::ErrorKind::InvalidInput)),
            })
            .collect()
    }

    pub(in crate::runtime::tools) fn cstring(bytes: &[u8]) -> io::Result<CString> {
        CString::new(bytes).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
    }

    pub(in crate::runtime::tools) fn owned(fd: i32) -> io::Result<OwnedFd> {
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            // SAFETY: `fd` is a new descriptor this process owns.
            Ok(unsafe { OwnedFd::from_raw_fd(fd) })
        }
    }

    /// Opens the existing `name` of the folder `dirfd` for reading, with fixed
    /// flags: never created, never written, no link in the last place. The
    /// write module cannot choose flags, so it cannot open for writing here.
    pub(in crate::runtime::tools) fn open_existing_at(
        dirfd: RawFd,
        name: &CString,
    ) -> io::Result<OwnedFd> {
        open_raw(dirfd, name, flags(false))
    }

    fn open_raw(dirfd: RawFd, path: &CString, flags: i32) -> io::Result<OwnedFd> {
        // SAFETY: `path` is a valid NUL-terminated string for the whole call.
        owned(unsafe { libc::openat(dirfd, path.as_ptr(), flags) })
    }

    /// The kernel's `struct open_how`.
    #[repr(C)]
    struct OpenHow {
        flags: u64,
        mode: u64,
        resolve: u64,
    }

    const RESOLVE_NO_SYMLINKS: u64 = 0x04;
    const RESOLVE_BENEATH: u64 = 0x08;

    fn openat2(dirfd: RawFd, path: &CString, flags: i32) -> io::Result<OwnedFd> {
        let how = OpenHow {
            flags: flags as u64,
            mode: 0,
            resolve: RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS,
        };
        // SAFETY: `path` and `how` outlive the call and `size` is the real size.
        let fd = unsafe {
            libc::syscall(
                libc::SYS_openat2,
                dirfd,
                path.as_ptr(),
                &how as *const OpenHow,
                std::mem::size_of::<OpenHow>(),
            )
        };
        owned(i32::try_from(fd).unwrap_or(-1))
    }

    /// One `O_NOFOLLOW` open per component, each relative to the previous one.
    fn walk(base: OwnedFd, names: &[Vec<u8>], dir: bool) -> io::Result<OwnedFd> {
        let Some((last, parents)) = names.split_last() else {
            return open_raw(base.as_raw_fd(), &cstring(b".")?, flags(true));
        };
        let mut current = base;
        for name in parents {
            current = open_raw(current.as_raw_fd(), &cstring(name)?, flags(true))?;
        }
        open_raw(current.as_raw_fd(), &cstring(last)?, flags(dir))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::os::unix::fs::symlink;

        const BOTH: [Strategy; 2] = [Strategy::Openat2, Strategy::Walk];

        fn tree() -> (tempfile::TempDir, PathBuf) {
            let dir = tempfile::tempdir().unwrap();
            let home = dir.path().canonicalize().unwrap();
            std::fs::create_dir_all(home.join("a/b")).unwrap();
            std::fs::write(home.join("a/b/f.txt"), "x").unwrap();
            std::fs::create_dir_all(home.join("elsewhere")).unwrap();
            std::fs::write(home.join("elsewhere/g.txt"), "y").unwrap();
            (dir, home)
        }

        #[test]
        fn opens_a_file_and_a_dir_with_the_same_real_path_for_both_strategies() {
            let (_dir, home) = tree();
            for strategy in BOTH {
                let file = open_with(strategy, &home, Path::new("a/b/f.txt"), false).unwrap();
                assert_eq!(
                    fd_path(&file).unwrap(),
                    home.join("a/b/f.txt"),
                    "{strategy:?}"
                );
                let dir = open_with(strategy, &home, Path::new("a/b"), true).unwrap();
                assert_eq!(fd_path(&dir).unwrap(), home.join("a/b"), "{strategy:?}");
                let root = open_with(strategy, &home, Path::new(""), true).unwrap();
                assert_eq!(fd_path(&root).unwrap(), home, "{strategy:?}");
            }
        }

        #[test]
        fn a_symlink_as_the_last_component_is_refused() {
            let (_dir, home) = tree();
            symlink(home.join("a/b/f.txt"), home.join("link")).unwrap();
            symlink(home.join("a"), home.join("dirlink")).unwrap();
            for strategy in BOTH {
                assert!(
                    open_with(strategy, &home, Path::new("link"), false).is_err(),
                    "{strategy:?}"
                );
                assert!(
                    open_with(strategy, &home, Path::new("dirlink"), true).is_err(),
                    "{strategy:?}"
                );
            }
        }

        #[test]
        fn a_symlink_in_a_middle_component_is_refused() {
            let (_dir, home) = tree();
            symlink(home.join("elsewhere"), home.join("a/swap")).unwrap();
            for strategy in BOTH {
                assert!(
                    open_with(strategy, &home, Path::new("a/swap/g.txt"), false).is_err(),
                    "{strategy:?}"
                );
            }
        }

        #[test]
        fn dot_dot_and_absolute_parts_are_refused_before_any_syscall() {
            let (_dir, home) = tree();
            for strategy in BOTH {
                for rel in ["a/../elsewhere/g.txt", "/etc/passwd", "./a"] {
                    let err = open_with(strategy, &home, Path::new(rel), false).unwrap_err();
                    assert_eq!(
                        err.kind(),
                        io::ErrorKind::InvalidInput,
                        "{rel} {strategy:?}"
                    );
                }
            }
        }

        #[test]
        fn a_dir_flag_refuses_a_file_and_a_file_open_of_a_missing_name_fails() {
            let (_dir, home) = tree();
            for strategy in BOTH {
                assert!(open_with(strategy, &home, Path::new("a/b/f.txt"), true).is_err());
                assert!(open_with(strategy, &home, Path::new("a/none"), false).is_err());
            }
        }

        #[test]
        fn an_unavailable_openat2_falls_back_to_the_walk_but_a_real_refusal_does_not() {
            let (_dir, home) = tree();
            let rel = Path::new("a/b/f.txt");
            for errno in [libc::ENOSYS, libc::EPERM, libc::EINVAL, libc::E2BIG] {
                let first = Err(io::Error::from_raw_os_error(errno));
                let fd = with_fallback(first, &home, rel, false).unwrap();
                assert_eq!(fd_path(&fd).unwrap(), home.join(rel), "errno {errno}");
            }
            for errno in [libc::ELOOP, libc::EACCES, libc::ENOENT, libc::EXDEV] {
                let first = Err(io::Error::from_raw_os_error(errno));
                let err = with_fallback(first, &home, rel, false).unwrap_err();
                assert_eq!(err.raw_os_error(), Some(errno), "errno {errno}");
            }
        }

        #[test]
        fn the_path_of_a_deleted_file_no_longer_equals_the_checked_one() {
            let (_dir, home) = tree();
            let fd = open_beneath(&home, Path::new("a/b/f.txt"), false).unwrap();
            std::fs::remove_file(home.join("a/b/f.txt")).unwrap();
            assert_ne!(fd_path(&fd).unwrap(), home.join("a/b/f.txt"));
        }

        #[test]
        fn the_default_strategy_opens_like_the_explicit_ones() {
            let (_dir, home) = tree();
            let fd = open_beneath(&home, Path::new("a/b/f.txt"), false).unwrap();
            assert_eq!(fd_path(&fd).unwrap(), home.join("a/b/f.txt"));
        }
    }
}

/// Without Linux's `openat2` and `/proc/self/fd` nothing can be opened safely:
/// every call fails.
#[cfg(not(target_os = "linux"))]
mod other {
    use std::{
        io,
        os::fd::OwnedFd,
        path::{Path, PathBuf},
    };

    /// Always fails: see the module docs.
    pub fn open_beneath(_home: &Path, _rel: &Path, _dir: bool) -> io::Result<OwnedFd> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    /// Always fails: see the module docs.
    pub fn fd_path(_fd: &OwnedFd) -> io::Result<PathBuf> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}
