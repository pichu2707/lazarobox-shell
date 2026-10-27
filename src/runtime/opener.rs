//! Opens a link in the browser (see [`crate::core::opener`]): the first
//! opener found on PATH runs with the link as its only argument, with no
//! shell, and is reaped on a thread of its own so the loop never waits.

use std::ffi::OsStr;
use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::core::opener::{candidates, is_web_url};
use crate::runtime::desktop::which;

/// How long after its start an opener's error still means the link did
/// not open. `xdg-open` that finds no browser fails at once; one that runs
/// the browser in the foreground exits with it, much later.
const LAUNCH: Duration = Duration::from_secs(5);

/// The program that opens links on this host.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Opener {
    program: PathBuf,
    /// See [`LAUNCH`].
    launch: Duration,
}

impl Opener {
    /// The opener of this host, if one is installed on `path`.
    pub fn find(path: &OsStr) -> Option<Self> {
        let wsl = super::feedback::host_facts().wsl;
        Self::find_in(std::env::consts::OS, wsl, &|cmd| which(cmd, Some(path)))
    }

    /// The first of the host's [`candidates`] that `lookup` resolves.
    fn find_in(os: &str, wsl: bool, lookup: &dyn Fn(&str) -> Option<PathBuf>) -> Option<Self> {
        candidates(os, wsl)
            .iter()
            .find_map(|cmd| lookup(cmd))
            .map(|program| Self {
                program,
                launch: LAUNCH,
            })
    }

    /// Starts the opener on `url`. A link [`is_web_url`] refuses is never
    /// handed over. `failed` runs, on the reaping thread, when the opener
    /// exits with an error within [`LAUNCH`]: it found no browser (an SSH
    /// session, a desktop with no default). A later exit is the browser's.
    /// `explorer.exe` exits 1 even when it opened the link, so its exit says
    /// nothing.
    pub fn open(&self, url: &str, failed: impl FnOnce() + Send + 'static) -> io::Result<()> {
        if !is_web_url(url) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a web link",
            ));
        }
        let mut child = Command::new(&self.program)
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let trust_exit = self.program.file_name() != Some(OsStr::new("explorer.exe"));
        let started = Instant::now();
        let launch = self.launch;
        thread::spawn(move || {
            if let Ok(status) = child.wait()
                && !status.success()
                && trust_exit
                && started.elapsed() < launch
            {
                failed();
            }
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::time::{Duration, Instant};

    use super::*;

    fn found(names: &[&str]) -> impl Fn(&str) -> Option<PathBuf> {
        move |cmd| {
            names
                .contains(&cmd)
                .then(|| PathBuf::from(format!("/usr/bin/{cmd}")))
        }
    }

    #[test]
    fn the_first_installed_candidate_is_the_opener() {
        let names = ["explorer.exe", "xdg-open"];
        let opener = Opener::find_in("linux", true, &found(&names)).expect("opener");
        assert_eq!(opener.program, Path::new("/usr/bin/explorer.exe"));
        let opener = Opener::find_in("linux", false, &found(&names)).expect("opener");
        assert_eq!(opener.program, Path::new("/usr/bin/xdg-open"));
        assert_eq!(Opener::find_in("macos", false, &found(&names)), None);
    }

    /// A fake opener that writes its arguments, one per line, to `out`.
    fn fake(dir: &Path, out: &Path) -> Opener {
        let program = dir.join("fake-open");
        fs::write(
            &program,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}.tmp'\nmv '{0}.tmp' '{0}'\n",
                out.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        Opener {
            program,
            launch: LAUNCH,
        }
    }

    fn wait_for(path: &Path) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Ok(text) = fs::read_to_string(path) {
                return text;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("the opener never ran");
    }

    #[test]
    fn the_link_is_the_only_argument_even_with_shell_characters() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("args");
        let url = "https://github.com/o/r/issues/new?title=a%20b&body=$(id)`x`;y";
        fake(dir.path(), &out).open(url, || {}).unwrap();
        assert_eq!(wait_for(&out), format!("{url}\n"));
    }

    #[test]
    fn a_link_that_is_not_https_is_never_handed_over() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("args");
        let error = fake(dir.path(), &out).open("--help", || {}).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        thread::sleep(Duration::from_millis(50));
        assert!(!out.exists());
    }

    /// A fake opener named `name` that exits with `code`.
    fn exiting(dir: &Path, name: &str, code: i32) -> Opener {
        let program = dir.join(name);
        fs::write(&program, format!("#!/bin/sh\nexit {code}\n")).unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        Opener {
            program,
            launch: LAUNCH,
        }
    }

    fn failed_after(opener: &Opener) -> bool {
        let (tx, rx) = std::sync::mpsc::channel();
        opener
            .open("https://example.com/", move || {
                let _ = tx.send(());
            })
            .unwrap();
        rx.recv_timeout(Duration::from_secs(2)).is_ok()
    }

    // Review finding (late exit): an opener that runs the browser in the
    // foreground exits when the browser does, maybe with an error, long
    // after the link opened: only a failure at launch is reported.
    #[test]
    fn a_failure_after_the_launch_is_not_reported() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("xdg-open");
        fs::write(&program, "#!/bin/sh\nsleep 0.5\nexit 1\n").unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        let opener = Opener {
            program,
            launch: Duration::from_millis(200),
        };
        assert!(!failed_after(&opener));
    }

    // Review finding (opener failure): an opener that exits with an error
    // is reported; `explorer.exe`, which exits 1 even when it opened the
    // link, is not.
    #[test]
    fn an_opener_that_fails_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        assert!(failed_after(&exiting(dir.path(), "xdg-open", 3)));
        assert!(!failed_after(&exiting(dir.path(), "explorer.exe", 1)));
    }
}
