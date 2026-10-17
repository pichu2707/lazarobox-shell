//! `version` must hand the child a closed stdin. This lives in its own test
//! binary because it swaps file descriptor 0 for the whole process, which
//! would race with any other test running in the same process.

#![cfg(target_os = "linux")]

use std::{fs, os::unix::fs::PermissionsExt};

use lazarobox_shell::runtime::ai_detect::version;

/// Puts the read end of a pipe on fd 0 and restores the original on drop,
/// even if the test panics.
struct StdinPipe {
    saved: libc::c_int,
    fds: [libc::c_int; 2],
}

impl StdinPipe {
    fn install() -> Self {
        let mut fds = [0; 2];
        // SAFETY: plain syscalls on descriptors owned by this guard.
        unsafe {
            assert_eq!(libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC), 0);
            let saved = libc::fcntl(0, libc::F_DUPFD_CLOEXEC, 3);
            assert!(saved >= 3);
            // dup2 clears CLOEXEC on the target, so fd 0 is inheritable.
            assert_eq!(libc::dup2(fds[0], 0), 0);
            Self { saved, fds }
        }
    }
}

impl Drop for StdinPipe {
    fn drop(&mut self) {
        // SAFETY: restores fd 0 and closes only descriptors this guard owns.
        unsafe {
            libc::dup2(self.saved, 0);
            libc::close(self.saved);
            libc::close(self.fds[0]);
            libc::close(self.fds[1]);
        }
    }
}

#[tokio::test]
async fn version_closes_stdin() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("gemini");
    let body = "#!/bin/sh\nt=$(readlink /proc/self/fd/0)\nif [ \"$t\" = /dev/null ]; then echo null; else echo \"stdin=$t\"; fi\n";
    fs::write(&script, body).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let guard = StdinPipe::install();
    let answer = version(&script, std::ffi::OsStr::new("/usr/bin:/bin")).await;
    drop(guard);
    assert_eq!(answer, Some(b"null\n".to_vec()));
}
