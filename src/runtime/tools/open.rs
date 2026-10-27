//! Opening a path below HOME without following a single symlink.
//!
//! The path policy judges names; this module is what stops a link swapped in
//! between the judgment and the open. Linux and macOS: elsewhere every open
//! fails and the tools report "could not read" (there is no way to read the real
//! path of a descriptor back to re-check it).
//!
//! Linux prefers `openat2` and reads the real path from `/proc/self/fd`. macOS
//! has neither: it always walks one `O_NOFOLLOW` open per component and reads
//! the real path with `fcntl(F_GETPATH)`. A refusal there (`EPERM` from TCC, a
//! data vault) is an error like any other, never a reason to try another way.
//!
//! Mounts: `RESOLVE_NO_XDEV` is deliberately NOT used. A bind or network mount
//! below HOME (a projects disk, a synced folder) is legitimate, so crossing a
//! mount point is allowed; what is refused is any symlink, and any real path
//! that the policy judges again on the opened descriptor.

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub use other::*;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use unix::*;

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix {
    use std::{
        ffi::CString,
        io,
        os::{
            fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
            unix::ffi::OsStrExt,
        },
        path::{Component, Path},
    };

    #[cfg(target_os = "linux")]
    pub use super::linux_sys::fd_path;
    #[cfg(target_os = "linux")]
    use super::linux_sys::{base_flags, openat2};
    #[cfg(target_os = "macos")]
    use super::macos_sys::base_flags;
    #[cfg(target_os = "macos")]
    pub use super::macos_sys::fd_path;

    /// How a path is opened. `Openat2` (Linux) is preferred; `Walk` is the
    /// fallback for a kernel (or a seccomp filter) without `openat2`, and the only
    /// way on macOS.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum Strategy {
        #[cfg(target_os = "linux")]
        Openat2,
        Walk,
    }

    /// Opens `rel` (a relative path of plain names) below `home`, never following
    /// a symlink in any component. `dir` demands a directory.
    ///
    /// Tries `openat2` with `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS` and falls back
    /// to one `O_NOFOLLOW` open per component when the kernel lacks it.
    #[cfg(target_os = "linux")]
    pub fn open_beneath(home: &Path, rel: &Path, dir: bool) -> io::Result<OwnedFd> {
        with_fallback(open_with(Strategy::Openat2, home, rel, dir), home, rel, dir)
    }

    /// Opens `rel` below `home` with one `O_NOFOLLOW` open per component. macOS
    /// has no `openat2`, and no fallback: an error (`EPERM` from TCC included) is
    /// the answer.
    #[cfg(target_os = "macos")]
    pub fn open_beneath(home: &Path, rel: &Path, dir: bool) -> io::Result<OwnedFd> {
        open_with(Strategy::Walk, home, rel, dir)
    }

    /// Keeps the `openat2` outcome unless it says the call itself is unavailable.
    #[cfg(target_os = "linux")]
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
        let base = open_raw(
            libc::AT_FDCWD,
            &cstring(home.as_os_str().as_bytes())?,
            base_flags(),
        )?;
        match strategy {
            #[cfg(target_os = "linux")]
            Strategy::Openat2 => {
                let joined = names.join(&b"/"[..]);
                let path = cstring(if joined.is_empty() { b"." } else { &joined })?;
                openat2(base.as_raw_fd(), &path, flags(dir))
            }
            Strategy::Walk => walk(base, &names, dir),
        }
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
    // Only the write module calls it, and it writes on macOS from the next
    // slice on.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
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
        use std::{os::unix::fs::symlink, path::PathBuf};

        #[cfg(target_os = "linux")]
        const BOTH: [Strategy; 2] = [Strategy::Openat2, Strategy::Walk];
        #[cfg(target_os = "macos")]
        const BOTH: [Strategy; 1] = [Strategy::Walk];

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

        #[cfg(target_os = "linux")]
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

        // Probe (a). Linux reads back `<path> (deleted)`. macOS `F_GETPATH` gives
        // the old path, so its `fd_path` fails: the path names no file any more.
        #[test]
        fn the_path_of_a_deleted_file_no_longer_equals_the_checked_one() {
            let (_dir, home) = tree();
            let fd = open_beneath(&home, Path::new("a/b/f.txt"), false).unwrap();
            std::fs::remove_file(home.join("a/b/f.txt")).unwrap();
            let after = fd_path(&fd);
            assert!(
                after.as_ref().ok() != Some(&home.join("a/b/f.txt")),
                "probe (a) UNSAFE: an unlinked fd still reports its old path: {after:?}"
            );
            eprintln!("probe (a) fd_path after unlink: {after:?}");
        }

        // Probe (b): a renamed folder must not keep reporting the checked path.
        #[test]
        fn the_path_of_a_renamed_folder_no_longer_equals_the_checked_one() {
            let (_dir, home) = tree();
            let fd = open_beneath(&home, Path::new("a/b"), true).unwrap();
            std::fs::rename(home.join("a/b"), home.join("a/moved")).unwrap();
            let after = fd_path(&fd);
            assert!(
                after.as_ref().ok() != Some(&home.join("a/b")),
                "probe (b) UNSAFE: a renamed folder still reports its old path: {after:?}"
            );
            eprintln!("probe (b) fd_path after rename: {after:?}");
        }

        // Probe (c): on a case-insensitive volume `readme.md` opens `README.md`.
        // `fd_path` must give the on-disk spelling, so `opened == real` refuses it.
        // The only test that makes names differing by case, and only to ask.
        #[test]
        fn a_name_with_the_wrong_case_never_reports_the_requested_spelling() {
            let (_dir, home) = tree();
            std::fs::write(home.join("README.md"), "x").unwrap();
            let Ok(fd) = open_beneath(&home, Path::new("readme.md"), false) else {
                // GitHub's macOS runners are case-insensitive APFS: a green run
                // there must mean this was tested, not skipped.
                assert!(
                    cfg!(not(target_os = "macos")) || std::env::var_os("CI").is_none(),
                    "probe (c) NOT TESTED: the CI volume is case-sensitive, `readme.md` does not open"
                );
                eprintln!(
                    "probe (c) SKIPPED (not CI): case-sensitive volume, `readme.md` does not open"
                );
                return;
            };
            let after = fd_path(&fd);
            assert_eq!(
                after.as_ref().ok(),
                Some(&home.join("README.md")),
                "probe (c) UNSAFE: the alias reports something other than the on-disk spelling: {after:?}"
            );
            eprintln!("probe (c) case-insensitive volume, fd_path: {after:?}");
        }

        // Probe (d): F_GETPATH must give the firmlink spelling (`/Users/x`), never
        // `/System/Volumes/Data/...`, or every normal HOME would be refused.
        #[cfg(target_os = "macos")]
        #[test]
        fn a_real_home_reports_its_canonical_path_not_the_data_volume() {
            let canonical = |p: &str| Path::new(p).canonicalize().unwrap();
            let mut cases = vec![(canonical("/"), "Users")];
            if let Some(home) = std::env::var_os("HOME") {
                cases.push((canonical("/"), ""));
                let home = Path::new(&home).canonicalize().unwrap();
                cases.push((home, ""));
            }
            for (base, rel) in cases {
                let fd = open_beneath(&base, Path::new(rel), true).unwrap();
                let got = fd_path(&fd);
                assert_eq!(
                    got.as_ref().ok(),
                    Some(&base.join(rel)),
                    "probe (d) firmlink: base {base:?} rel {rel:?} gave {got:?}"
                );
            }
        }

        // Probe (e): a real path longer than MAXPATHLEN opens (the walk is per
        // component) but `F_GETPATH` cannot hold it. It must fail or be exact,
        // never a truncated path that could equal another one.
        #[cfg(target_os = "macos")]
        #[test]
        fn a_real_path_longer_than_maxpathlen_fails_closed() {
            let (_dir, home) = tree();
            let mut rel = PathBuf::new();
            let mut at = open_with(Strategy::Walk, &home, Path::new(""), true).unwrap();
            let name = "d".repeat(60);
            while home.join(&rel).as_os_str().len() <= libc::MAXPATHLEN as usize + 64 {
                let c = cstring(name.as_bytes()).unwrap();
                // SAFETY: `c` is NUL-terminated and `at` is an open folder.
                assert_eq!(
                    unsafe { libc::mkdirat(at.as_raw_fd(), c.as_ptr(), 0o700) },
                    0
                );
                at = open_raw(at.as_raw_fd(), &c, flags(true)).unwrap();
                rel.push(&name);
            }
            let fd = open_beneath(&home, &rel, true).unwrap();
            let got = fd_path(&fd);
            assert!(
                !got.as_ref().is_ok_and(|p| *p != home.join(&rel)),
                "probe (e) UNSAFE: a path over MAXPATHLEN came back altered: {got:?}"
            );
            eprintln!("probe (e) fd_path over MAXPATHLEN is err: {}", got.is_err());
        }

        #[test]
        fn the_default_strategy_opens_like_the_explicit_ones() {
            let (_dir, home) = tree();
            let fd = open_beneath(&home, Path::new("a/b/f.txt"), false).unwrap();
            assert_eq!(fd_path(&fd).unwrap(), home.join("a/b/f.txt"));
        }
    }
}

/// The Linux seams: `openat2`, `/proc/self/fd` and an `O_PATH` base.
#[cfg(target_os = "linux")]
mod linux_sys {
    use std::{
        ffi::CString,
        io,
        os::fd::{AsRawFd, OwnedFd, RawFd},
        path::PathBuf,
    };

    /// The real path of an open descriptor, read from `/proc/self/fd`.
    pub fn fd_path(fd: &OwnedFd) -> io::Result<PathBuf> {
        std::fs::read_link(format!("/proc/self/fd/{}", fd.as_raw_fd()))
    }

    /// The flags of the base folder: `O_PATH`, so it is only a reference.
    pub fn base_flags() -> i32 {
        libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW
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

    pub fn openat2(dirfd: RawFd, path: &CString, flags: i32) -> io::Result<OwnedFd> {
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
        super::unix::owned(i32::try_from(fd).unwrap_or(-1))
    }
}

/// The macOS seams: `F_GETPATH` and a plain read-only base folder (there is no
/// `O_PATH`). Written without a macOS compiler at hand: keep it small.
#[cfg(target_os = "macos")]
mod macos_sys {
    use std::{
        ffi::{CStr, OsStr},
        io,
        os::{
            fd::{AsRawFd, OwnedFd},
            unix::{ffi::OsStrExt, fs::MetadataExt},
        },
        path::PathBuf,
    };

    /// The real path of an open descriptor, from `fcntl(F_GETPATH)`. Never
    /// `F_GETPATH_NOFIRMLINK`: the policy judges the firmlink spelling (`/Users`).
    /// Any failure is an error, so the caller refuses.
    ///
    /// `F_GETPATH` keeps answering the old path of a deleted file (probe (a)),
    /// so the path counts only while it still names this very file: the same
    /// device and inode, read without following a link.
    pub fn fd_path(fd: &OwnedFd) -> io::Result<PathBuf> {
        let path = getpath(fd)?;
        let named = std::fs::symlink_metadata(&path)?;
        let opened = std::fs::File::from(fd.try_clone()?).metadata()?;
        if (opened.dev(), opened.ino()) != (named.dev(), named.ino()) {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        Ok(path)
    }

    fn getpath(fd: &OwnedFd) -> io::Result<PathBuf> {
        let mut buf = [0u8; libc::MAXPATHLEN as usize];
        // SAFETY: `F_GETPATH` writes at most MAXPATHLEN bytes into `buf`.
        if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETPATH, buf.as_mut_ptr()) } < 0 {
            return Err(io::Error::last_os_error());
        }
        let path = CStr::from_bytes_until_nul(&buf)
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
        Ok(PathBuf::from(OsStr::from_bytes(path.to_bytes())))
    }

    /// The flags of the base folder: no `O_PATH` on macOS, so a read only open.
    pub fn base_flags() -> i32 {
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC
    }
}

/// Without a way to read the real path of a descriptor nothing can be opened
/// safely: every call fails.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
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
