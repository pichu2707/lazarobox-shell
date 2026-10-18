//! The macOS probe: does `/usr/bin/sandbox-exec` really confine a program?
//! It runs three controls through the real binary and reports the first
//! thing that looks wrong. It needs only `std`, so its logic is also tested
//! on Linux against fake `sandbox-exec` scripts.

use std::ffi::OsString;
use std::fs;
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

/// Runs `sandbox-exec -p PROFILE -D EXE=.. -D RW=.. -- program arg`, with a
/// scrubbed environment and no stdio, so nothing a test program leaves
/// running can hold the probe up. Whether it exited 0 is all it reports.
fn run(
    sandbox_exec: &Path,
    rw: &Path,
    program: &str,
    arg: &Path,
    timeout: Duration,
) -> Result<bool, SeatbeltFault> {
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
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => SeatbeltFault::Missing,
            _ => SeatbeltFault::Setup,
        })?;
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
    Ok(status.success())
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
    if !allowed || !ok.exists() {
        return Err(SeatbeltFault::Refused);
    }
    let write = run(sandbox_exec, &rw, TOUCH, &no, timeout)?;
    if write || no.exists() {
        return Err(SeatbeltFault::Leaked);
    }
    let exec = run(sandbox_exec, &rw, ECHO, Path::new("x"), timeout)?;
    if exec {
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

    /// Writes an executable script. A parallel test may fork while the file
    /// is still open for writing, which makes the first exec fail with
    /// ETXTBSY; so the script is started once here, retrying, before the
    /// probe (whose spawn must stay fail-closed) ever sees it.
    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        for _ in 0..500 {
            match std::process::Command::new(&path).spawn() {
                Ok(mut child) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return path;
                }
                Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => panic!("cannot start the fake sandbox-exec: {e}"),
            }
        }
        panic!("the fake sandbox-exec stayed busy");
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
        // A fake of our own: `false` lives in `/usr/bin` on macOS, `/bin` elsewhere.
        let bin = tempfile::tempdir().unwrap();
        let fake = script(bin.path(), "sandbox-exec", "#!/bin/sh\nexit 1\n");
        let root = tempfile::tempdir().unwrap();
        assert_eq!(probe_in(&fake, root.path()), Err(SeatbeltFault::Refused));
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

    #[test]
    fn a_file_that_cannot_be_started_is_a_setup_fault_not_a_refusal() {
        let tools = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let plain = tools.path().join("sandbox-exec");
        fs::write(&plain, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&plain, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(probe_in(&plain, root.path()), Err(SeatbeltFault::Setup));
        assert!(is_empty(root.path()));
    }
}
