//! Opens a link in the browser (see [`crate::core::opener`]): the first
//! opener found on PATH runs with the link as its only argument, with no
//! shell, and is reaped on a thread of its own so the loop never waits.

use std::ffi::OsStr;
use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;

use crate::core::opener::{candidates, is_web_url};
use crate::runtime::desktop::which;

/// The program that opens links on this host.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Opener {
    program: PathBuf,
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
            .map(|program| Self { program })
    }

    /// Starts the opener on `url`. A link [`is_web_url`] refuses is never
    /// handed over.
    pub fn open(&self, url: &str) -> io::Result<()> {
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
        thread::spawn(move || {
            let _ = child.wait();
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
        Opener { program }
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
        fake(dir.path(), &out).open(url).unwrap();
        assert_eq!(wait_for(&out), format!("{url}\n"));
    }

    #[test]
    fn a_link_that_is_not_https_is_never_handed_over() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("args");
        let error = fake(dir.path(), &out).open("--help").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        thread::sleep(Duration::from_millis(50));
        assert!(!out.exists());
    }
}
