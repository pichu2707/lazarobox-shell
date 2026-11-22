//! Test helper: write an executable script that can be run at once.
//!
//! On Linux `execve` fails with ETXTBSY while any process holds the file open
//! for writing. Tests run on many threads, and a thread that forks while the
//! script's write fd is open hands a copy of it to the child, which keeps it
//! until the child's own `exec` closes it. A rename does not help (the inode
//! is the same), so the helper closes its fd and then waits until no copy of
//! it is left anywhere.

use std::{fs, io::Write, os::unix::fs::OpenOptionsExt, path::Path, thread};

/// Writes `text` to `path` as an executable (`mode`) and returns once no
/// process holds the file open for writing, so running it cannot hit ETXTBSY.
pub(crate) fn write_executable_mode(path: &Path, text: impl AsRef<[u8]>, mode: u32) {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(mode)
        .open(path)
        .unwrap();
    file.write_all(text.as_ref()).unwrap();
    drop(file);
    // `mode` is filtered by the umask at creation, and kept as is when the
    // file already existed.
    fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(mode)).unwrap();
    wait_until_unshared(path);
}

/// `write_executable_mode` with mode 0o755.
pub(crate) fn write_executable(path: &Path, text: impl AsRef<[u8]>) {
    write_executable_mode(path, text, 0o755);
}

/// Waits until no process has `path` open for writing. A read lease cannot be
/// taken while one does, even through a copy of the fd held by a forked child
/// that has not reached `exec` yet.
#[cfg(target_os = "linux")]
fn wait_until_unshared(path: &Path) {
    use std::os::fd::AsRawFd;
    let Ok(file) = fs::File::open(path) else {
        return; // not readable (mode 0): nothing is going to run it
    };
    wait_while_shared(path, || {
        // SAFETY: `fcntl` on an fd this function owns.
        let taken = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETLEASE, libc::F_RDLCK) };
        if taken == 0 {
            // SAFETY: same fd; releases the lease just taken.
            unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETLEASE, libc::F_UNLCK) };
            return Ok(());
        }
        Err(std::io::Error::last_os_error().raw_os_error().unwrap_or(0))
    });
}

/// Retries `lease` while it fails with EAGAIN (a writer is still there) or
/// EINTR. Any other errno is permanent (EINVAL on a filesystem without
/// leases, EACCES, EPERM...): waiting cannot help, so it returns at once.
#[cfg(target_os = "linux")]
fn wait_while_shared(path: &Path, mut lease: impl FnMut() -> Result<(), i32>) {
    use std::time::{Duration, Instant};
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match lease() {
            Ok(()) => return,
            Err(errno) if errno == libc::EAGAIN || errno == libc::EINTR => {}
            Err(_) => return,
        }
        assert!(
            Instant::now() < deadline,
            "{} stayed open for writing",
            path.display()
        );
        thread::sleep(Duration::from_millis(1));
    }
}

#[cfg(not(target_os = "linux"))]
fn wait_until_unshared(_path: &Path) {}

#[cfg(test)]
mod tests {
    use std::{
        process::Command,
        sync::atomic::{AtomicBool, Ordering},
    };

    use super::*;

    /// Runs `script` and returns the spawn error, if any.
    fn run(path: &Path) -> std::io::Result<()> {
        Command::new(path).status().map(|_| ())
    }

    /// Forks all the time on `forkers` threads while this thread writes and
    /// runs scripts; counts the ETXTBSY failures with `write`.
    fn busy_failures(write: impl Fn(&Path, &str) + Sync, rounds: usize) -> usize {
        let dir = tempfile::tempdir().unwrap();
        let stop = AtomicBool::new(false);
        let mut failures = 0;
        thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    while !stop.load(Ordering::Relaxed) {
                        let _ = Command::new("/bin/true").status();
                    }
                });
            }
            for round in 0..rounds {
                let path = dir.path().join(format!("s{round}"));
                write(&path, "#!/bin/sh\nexit 0\n");
                if let Err(error) = run(&path)
                    && error.raw_os_error() == Some(libc::ETXTBSY)
                {
                    failures += 1;
                }
            }
            stop.store(true, Ordering::Relaxed);
        });
        failures
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn only_eagain_and_eintr_keep_the_wait_going() {
        for errno in [libc::EINVAL, libc::EACCES, libc::EPERM, libc::ENOTSUP] {
            let mut calls = 0;
            wait_while_shared(Path::new("x"), || {
                calls += 1;
                Err(errno)
            });
            assert_eq!(calls, 1, "errno {errno} must not be retried");
        }
        let mut calls = 0;
        wait_while_shared(Path::new("x"), || {
            calls += 1;
            match calls {
                1 => Err(libc::EAGAIN),
                2 => Err(libc::EINTR),
                _ => Ok(()),
            }
        });
        assert_eq!(calls, 3);
    }

    fn naive(path: &Path, text: &str) {
        fs::write(path, text).unwrap();
        fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    }

    #[test]
    fn the_helper_never_hits_etxtbsy_under_concurrent_forks() {
        assert_eq!(busy_failures(|p, t| write_executable(p, t), 300), 0);
    }

    /// Documents the race the helper removes; not an assertion that it fires.
    #[test]
    #[ignore = "reproduces the race; run with --ignored --nocapture"]
    fn a_plain_write_hits_etxtbsy_under_concurrent_forks() {
        let failures = busy_failures(naive, 2000);
        eprintln!("plain write: {failures} ETXTBSY in 2000 runs");
        assert!(failures > 0);
    }
}
