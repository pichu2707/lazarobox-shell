//! The macOS probe: does `/usr/bin/sandbox-exec` really confine a program?
//! It runs three controls through the real binary and reports the first
//! thing that looks wrong. It needs only `std`, so its logic is also tested
//! on Linux against fake `sandbox-exec` scripts.

use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::core::ai::jail::SeatbeltFault;

/// Absolute on purpose: a `PATH` lookup could pick up a look-alike.
#[cfg(target_os = "macos")]
pub const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// How long one test program may run before the probe gives up.
#[cfg(target_os = "macos")]
pub const TIMEOUT: Duration = Duration::from_secs(5);

const POLL: Duration = Duration::from_millis(10);

const TOUCH: &str = "/usr/bin/touch";
const ECHO: &str = "/bin/echo";

/// The probe's profile: it may read and map code, and `EXE` may run and
/// write under `RW`. The paths only ever arrive as `-D` values.
const PROFILE: &str = "(version 1)(deny default)(allow file-read*)(allow file-map-executable)\
(allow sysctl-read)(allow process-exec (literal (param \"EXE\")))\
(allow file-write* (subpath (param \"RW\")))";

/// The probe's scratch folder, removed when it goes out of scope.
struct Scratch(PathBuf);

impl Scratch {
    fn new(root: &Path) -> Result<Self, SeatbeltFault> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir = root.join(format!("lazarobox-probe-{}-{nanos}", std::process::id()));
        let make = |path: &Path| fs::DirBuilder::new().mode(0o700).create(path);
        make(&dir).map_err(|_| SeatbeltFault::Setup)?;
        let mut scratch = Self(dir);
        // Seatbelt matches real paths (`/private/var/...`), not symlinks.
        scratch.0 = fs::canonicalize(&scratch.0).map_err(|_| SeatbeltFault::Setup)?;
        for part in ["rw", "denied"] {
            make(&scratch.0.join(part)).map_err(|_| SeatbeltFault::Setup)?;
        }
        Ok(scratch)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// What a finished test program did.
struct Ran {
    success: bool,
    stdout: Vec<u8>,
}

/// Runs `sandbox-exec -p PROFILE -D EXE=.. -D RW=.. -- program arg`, with a
/// scrubbed environment, no stdin and no stderr.
fn run(
    sandbox_exec: &Path,
    rw: &Path,
    program: &str,
    arg: &Path,
    timeout: Duration,
) -> Result<Ran, SeatbeltFault> {
    let mut exe = OsString::from("EXE=");
    exe.push(TOUCH);
    let mut rw_param = OsString::from("RW=");
    rw_param.push(rw);
    let mut child = Command::new(sandbox_exec)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .args(["-p", PROFILE, "-D"])
        .arg(exe)
        .arg("-D")
        .arg(rw_param)
        .args(["--", program])
        .arg(arg)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| SeatbeltFault::Refused)?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => std::thread::sleep(POLL),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(SeatbeltFault::Timeout);
            }
        }
    };
    let mut stdout = Vec::new();
    if let Some(mut pipe) = child.stdout.take() {
        let _ = pipe.read_to_end(&mut stdout);
    }
    Ok(Ran {
        success: status.success(),
        stdout,
    })
}

/// Three controls through `sandbox_exec`: a write inside the granted folder
/// must work, a write outside it and an exec of anything else must not.
pub fn probe_with(
    sandbox_exec: &Path,
    temp_root: &Path,
    timeout: Duration,
) -> Result<(), SeatbeltFault> {
    if !sandbox_exec.is_file() {
        return Err(SeatbeltFault::Missing);
    }
    let scratch = Scratch::new(temp_root)?;
    let (rw, denied) = (scratch.0.join("rw"), scratch.0.join("denied"));
    let (ok, no) = (rw.join("ok"), denied.join("no"));

    let allowed = run(sandbox_exec, &rw, TOUCH, &ok, timeout)?;
    if !allowed.success || !ok.exists() {
        return Err(SeatbeltFault::Refused);
    }
    let write = run(sandbox_exec, &rw, TOUCH, &no, timeout)?;
    if write.success || no.exists() {
        return Err(SeatbeltFault::Leaked);
    }
    let exec = run(sandbox_exec, &rw, ECHO, Path::new("x"), timeout)?;
    if exec.success || !exec.stdout.is_empty() {
        return Err(SeatbeltFault::Leaked);
    }
    Ok(())
}

/// The probe against the real `/usr/bin/sandbox-exec`.
#[cfg(target_os = "macos")]
pub fn probe() -> Result<(), SeatbeltFault> {
    probe_with(Path::new(SANDBOX_EXEC), &std::env::temp_dir(), TIMEOUT)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use super::*;
    use crate::core::ai::jail::SeatbeltFault;

    /// Enforces what the probe's profile grants: only `touch`, only under RW.
    /// `$ALLOW_ECHO` makes it leak the exec control.
    const ENFORCING: &str = r#"#!/bin/sh
rw=""; exe=""
while [ $# -gt 0 ] && [ "$1" != "--" ]; do
  if [ "$1" = "-D" ]; then
    case "$2" in RW=*) rw="${2#RW=}";; EXE=*) exe="${2#EXE=}";; esac
    shift
  fi
  shift
done
[ "$1" = "--" ] || exit 64
shift
prog="$1"; shift
[ -n "$ALLOW_ECHO" ] && [ "$prog" = /bin/echo ] && exec "$prog" "$@"
[ "$prog" = "$exe" ] || exit 71
case "$1" in "$rw"/*) exec "$prog" "$@";; esac
exit 1
"#;

    /// Ignores the sandbox: runs whatever follows `--`.
    const LEAKY: &str =
        "#!/bin/sh\nwhile [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"\n";

    const SLOW: &str = "#!/bin/sh\nexec sleep 30\n";

    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn probe_in(sandbox_exec: &Path, root: &Path) -> Result<(), SeatbeltFault> {
        probe_with(sandbox_exec, root, Duration::from_secs(5))
    }

    fn is_empty(dir: &Path) -> bool {
        fs::read_dir(dir).unwrap().next().is_none()
    }

    #[test]
    fn a_sandbox_that_confines_passes_and_leaves_no_folder() {
        let tools = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let fake = script(tools.path(), "sandbox-exec", ENFORCING);
        assert_eq!(probe_in(&fake, root.path()), Ok(()));
        assert!(is_empty(root.path()));
    }

    #[test]
    fn a_missing_binary_is_reported_before_anything_is_made() {
        let root = tempfile::tempdir().unwrap();
        let gone = root.path().join("no-such-sandbox-exec");
        assert_eq!(probe_in(&gone, root.path()), Err(SeatbeltFault::Missing));
    }

    #[test]
    fn a_binary_that_refuses_the_test_program_is_refused() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            probe_in(Path::new("/bin/false"), root.path()),
            Err(SeatbeltFault::Refused)
        );
        assert!(is_empty(root.path()));
    }

    #[test]
    fn a_sandbox_that_lets_the_write_through_leaks() {
        let tools = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let fake = script(tools.path(), "sandbox-exec", LEAKY);
        assert_eq!(probe_in(&fake, root.path()), Err(SeatbeltFault::Leaked));
        assert!(is_empty(root.path()));
    }

    #[test]
    fn a_sandbox_that_lets_the_exec_through_leaks() {
        let tools = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let fake = script(
            tools.path(),
            "sandbox-exec",
            &ENFORCING.replace("rw=\"\"", "rw=\"\"; ALLOW_ECHO=1"),
        );
        assert_eq!(probe_in(&fake, root.path()), Err(SeatbeltFault::Leaked));
    }

    #[test]
    fn a_hung_sandbox_times_out_and_is_killed() {
        let tools = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let fake = script(tools.path(), "sandbox-exec", SLOW);
        let started = Instant::now();
        let got = probe_with(&fake, root.path(), Duration::from_millis(300));
        assert_eq!(got, Err(SeatbeltFault::Timeout));
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(is_empty(root.path()));
    }

    #[test]
    fn an_unusable_temp_root_is_a_setup_fault() {
        let tools = tempfile::tempdir().unwrap();
        let fake = script(tools.path(), "sandbox-exec", ENFORCING);
        let nowhere = tools.path().join("no/such/root");
        assert_eq!(probe_in(&fake, &nowhere), Err(SeatbeltFault::Setup));
    }
}
