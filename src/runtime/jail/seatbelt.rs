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

use super::Failure;
use crate::core::ai::jail::sbpl;
use crate::core::ai::jail::{
    EXEC_FAILED_REASON, JailOff, JailSpec, MIN_ABI_REASON, PROFILE_REASON, SeatbeltFault,
    parse_args,
};

/// Absolute on purpose: a `PATH` lookup could pick up a look-alike.
#[cfg(target_os = "macos")]
pub const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// How long one test program may run before the probe gives up.
#[cfg(target_os = "macos")]
pub const TIMEOUT: Duration = Duration::from_secs(5);

const POLL: Duration = Duration::from_millis(10);

const TOUCH: &str = "/usr/bin/touch";
const ECHO: &str = "/bin/echo";

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

/// The `sandbox-exec` command the helper replaces itself with:
/// `sandbox-exec -p <profile> -D KEY=path ... -- program argv`. The profile
/// is rendered here, in the helper, from the rules on its own command line.
/// Nothing else is set on it, so the environment and the directory pass
/// through exactly as the app prepared them. Every failure is a
/// `NotEnforced` with a fixed text: the program has not run.
pub fn command(sandbox_exec: &Path, args: &[OsString]) -> Result<Command, Failure> {
    let job = parse_args(args).map_err(|e| Failure::NotEnforced(e.0.to_owned()))?;
    if job.min_abi.is_some() {
        return Err(Failure::NotEnforced(MIN_ABI_REASON.to_owned()));
    }
    if !sandbox_exec.is_file() {
        let missing = JailOff::NoSeatbelt(SeatbeltFault::Missing);
        return Err(Failure::NotEnforced(missing.text()));
    }
    let profile =
        sbpl::render(&job.spec).map_err(|_| Failure::NotEnforced(PROFILE_REASON.to_owned()))?;
    let mut command = Command::new(sandbox_exec);
    command
        .args(sbpl::sandbox_args(&profile))
        .arg("--")
        .arg(&job.program)
        .args(&job.argv);
    Ok(command)
}

/// What the helper reports when the `exec` of `sandbox-exec` itself fails (it
/// vanished, or is not executable). The jail is NOT in place then, so this is
/// a jail error (exit 125), never the 127 of a program that did not run.
#[cfg(any(target_os = "macos", test))]
pub fn not_started() -> Failure {
    Failure::NotEnforced(EXEC_FAILED_REASON.to_owned())
}

/// Which control failed and how: for test and CI output only. The stderr is
/// sandbox-exec's own first line and is never shown to users of the app.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Diag {
    /// "positive write", "negative write" or "negative exec".
    pub control: &'static str,
    /// The exit status of the control (`None` when a signal ended it).
    pub status: Option<i32>,
    /// The first stderr line, cut to 200 characters.
    pub stderr: String,
}

/// What one control did.
struct Outcome {
    success: bool,
    status: Option<i32>,
    stderr: String,
}

impl Outcome {
    fn diag(&self, control: &'static str) -> Diag {
        Diag {
            control,
            status: self.status,
            stderr: self.stderr.clone(),
        }
    }
}

/// The probe's minimal spec: `program` may run, and `rw` is writable. Every
/// path is resolved first, because Seatbelt matches the real vnode path
/// (`/var/folders/..` is `/private/var/folders/..`), so a link never matches.
/// `None` when a path does not exist.
fn probe_spec(rw: &Path, program: &Path) -> Option<JailSpec> {
    Some(JailSpec {
        rx: vec![fs::canonicalize(program).ok()?],
        rw: vec![fs::canonicalize(rw).ok()?],
        ..JailSpec::default()
    })
}

/// Runs `sandbox-exec <sandbox_args> -- program arg`, with a scrubbed
/// environment and no stdin or stdout, so nothing a test program leaves
/// running can hold the probe up. The stderr goes to `err_file` (outside the
/// granted folder), from which its first line is read back.
fn run(
    sandbox_exec: &Path,
    profile: &[OsString],
    program: &str,
    arg: &Path,
    err_file: &Path,
    timeout: Duration,
) -> Result<Outcome, SeatbeltFault> {
    let stderr = fs::File::create(err_file).map_err(|_| SeatbeltFault::Setup)?;
    let mut child = Command::new(sandbox_exec)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .args(profile)
        .args(["--", program])
        .arg(arg)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
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
    let said = fs::read(err_file).unwrap_or_default();
    let first = String::from_utf8_lossy(&said)
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(200)
        .collect();
    Ok(Outcome {
        success: status.success(),
        status: status.code(),
        stderr: first,
    })
}

/// Three controls through `sandbox_exec`: a write inside the granted folder
/// must work, a write outside it and an exec of anything else must not.
pub fn probe_with(
    sandbox_exec: &Path,
    temp_root: &Path,
    timeout: Duration,
) -> Result<(), SeatbeltFault> {
    probe_diagnosed_with(sandbox_exec, temp_root, timeout).map_err(|(fault, _)| fault)
}

/// [`probe_with`], and which control failed when one does.
pub fn probe_diagnosed_with(
    sandbox_exec: &Path,
    temp_root: &Path,
    timeout: Duration,
) -> Result<(), (SeatbeltFault, Diag)> {
    let bare = |fault| {
        (
            fault,
            Diag {
                control: "setup",
                status: None,
                stderr: String::new(),
            },
        )
    };
    if !sandbox_exec.is_file() {
        return Err(bare(SeatbeltFault::Missing));
    }
    let scratch = Scratch::new(temp_root).map_err(bare)?;
    let (rw, denied) = (scratch.0.join("rw"), scratch.0.join("denied"));
    let (ok, no) = (rw.join("ok"), denied.join("no"));
    let err_file = scratch.0.join("stderr");
    // The real renderer, from a minimal spec: a macOS release that rejects
    // one of our operations fails the probe instead of every turn.
    let spec = probe_spec(&rw, Path::new(TOUCH)).ok_or(bare(SeatbeltFault::Setup))?;
    let profile = sbpl::render(&spec).map_err(|_| bare(SeatbeltFault::Setup))?;
    let profile = sbpl::sandbox_args(&profile);
    let go = |program: &str, arg: &Path| {
        run(sandbox_exec, &profile, program, arg, &err_file, timeout).map_err(bare)
    };

    let allowed = go(TOUCH, &ok)?;
    if !allowed.success || !ok.exists() {
        return Err((SeatbeltFault::Refused, allowed.diag("positive write")));
    }
    let write = go(TOUCH, &no)?;
    if write.success || no.exists() {
        return Err((SeatbeltFault::Leaked, write.diag("negative write")));
    }
    let exec = go(ECHO, Path::new("x"))?;
    if exec.success {
        return Err((SeatbeltFault::Leaked, exec.diag("negative exec")));
    }
    Ok(())
}

/// The probe against the real `/usr/bin/sandbox-exec`.
#[cfg(target_os = "macos")]
pub fn probe() -> Result<(), SeatbeltFault> {
    probe_with(Path::new(SANDBOX_EXEC), &std::env::temp_dir(), TIMEOUT)
}

/// [`probe`], with the failing control for CI output.
#[cfg(target_os = "macos")]
pub fn probe_diagnosed() -> Result<(), (SeatbeltFault, Diag)> {
    probe_diagnosed_with(Path::new(SANDBOX_EXEC), &std::env::temp_dir(), TIMEOUT)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::fs;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use super::*;
    use crate::core::ai::jail::{JailOff, SeatbeltFault};

    /// Enforces what the probe's profile grants: only `touch` (RX_0), only under RW_0.
    /// `$ALLOW_ECHO` makes it leak the exec control.
    const ENFORCING: &str = r#"#!/bin/sh
rw=""; exe=""
while [ $# -gt 0 ] && [ "$1" != "--" ]; do
  if [ "$1" = "-D" ]; then
    case "$2" in RW_0=*) rw="${2#RW_0=}";; RX_0=*) exe="${2#RX_0=}";; esac
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

    /// Records its whole argv, one element per line, then behaves as ENFORCING.
    fn recording(log: &Path) -> String {
        ENFORCING.replace(
            "rw=\"\"; exe=\"\"",
            &format!(
                "printf '%s\\n' \"$@\" >> {}\nrw=\"\"; exe=\"\"",
                log.display()
            ),
        )
    }

    #[test]
    fn the_probe_runs_the_real_renderer_profile_not_a_constant() {
        let tools = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let log = tools.path().join("argv.log");
        let fake = script(tools.path(), "sandbox-exec", &recording(&log));
        assert_eq!(probe_in(&fake, root.path()), Ok(()));
        let seen = fs::read_to_string(&log).unwrap();
        // Rendered text: the shared head, then the rx and rw lines by key.
        assert!(seen.contains("(deny default)"), "{seen}");
        assert!(seen.contains("(allow process-fork)"), "{seen}");
        assert!(
            seen.contains(
                "(allow file-read* file-map-executable process-exec (subpath (param \"RX_0\")))"
            ),
            "{seen}"
        );
        assert!(
            seen.contains("(allow file-read* file-write* (subpath (param \"RW_0\")))"),
            "{seen}"
        );
        assert!(seen.contains("RX_0=/usr/bin/touch\n"), "{seen}");
        assert!(seen.contains("/rw\n"), "{seen}");
        // The old constant profile used keys EXE and RW.
        assert!(!seen.contains("EXE="), "{seen}");
    }

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    /// The reason of a `NotEnforced` failure.
    fn refused(got: Result<std::process::Command, Failure>) -> String {
        match got {
            Err(Failure::NotEnforced(why)) => why,
            other => panic!("expected NotEnforced, got {other:?}"),
        }
    }

    fn some_exec() -> tempfile::NamedTempFile {
        tempfile::NamedTempFile::new().unwrap()
    }

    #[test]
    fn the_command_is_sandbox_exec_with_the_rendered_profile_then_the_program() {
        let exe = some_exec();
        let argv = args(&[
            "--rx",
            "/opt/cli",
            "--rw",
            "/work",
            "--",
            "/opt/cli/bin",
            "-p",
            "a b",
        ]);
        let cmd = command(exe.path(), &argv).unwrap();
        assert_eq!(cmd.get_program(), exe.path().as_os_str());

        let spec = parse_args(&argv).unwrap();
        let profile = sbpl::render(&spec.spec).unwrap();
        let mut want = sbpl::sandbox_args(&profile);
        want.extend(args(&["--", "/opt/cli/bin", "-p", "a b"]));
        let got: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(
            got,
            want.iter().map(OsString::as_os_str).collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_command_leaves_the_environment_and_the_directory_alone() {
        let exe = some_exec();
        let cmd = command(exe.path(), &args(&["--", "/bin/x"])).unwrap();
        assert_eq!(cmd.get_envs().count(), 0);
        assert_eq!(cmd.get_current_dir(), None);
    }

    #[test]
    fn the_spawned_command_inherits_the_environment() {
        let tools = tempfile::tempdir().unwrap();
        let out = tools.path().join("env.out");
        // Stands in for sandbox-exec: it reports the environment it got.
        let body = format!(
            "#!/bin/sh\nprintf '%s\\n%s\\n' \"${{PATH-unset}}\" \"${{HOME-unset}}\" > {}\n",
            out.display()
        );
        let fake = script(tools.path(), "sandbox-exec", &body);
        let status = command(&fake, &args(&["--", "/bin/x"]))
            .unwrap()
            .status()
            .unwrap();
        assert!(status.success());
        let want = |name: &str| std::env::var(name).unwrap_or_else(|_| "unset".to_owned());
        let seen = fs::read_to_string(&out).unwrap();
        assert_eq!(seen, format!("{}\n{}\n", want("PATH"), want("HOME")));
    }

    #[test]
    fn paths_and_arguments_that_are_not_utf8_pass_byte_for_byte() {
        let exe = some_exec();
        let bad = OsStr::from_bytes(b"\xff");
        let rw = OsStr::from_bytes(b"/work/\xff");
        let argv = vec![
            OsString::from("--rw"),
            rw.to_owned(),
            OsString::from("--"),
            OsString::from("/bin/x"),
            bad.to_owned(),
        ];
        let cmd = command(exe.path(), &argv).unwrap();
        let got: Vec<&OsStr> = cmd.get_args().collect();
        // The program argument, last and verbatim.
        assert_eq!(got.last().copied(), Some(bad));
        // The path travels as its own `-D RW_0=<bytes>` value.
        let mut param = OsString::from("RW_0=");
        param.push(rw);
        assert!(got.contains(&param.as_os_str()));
    }

    #[test]
    fn a_sandbox_exec_that_cannot_start_is_a_fixed_not_enforced_text() {
        let why = match not_started() {
            Failure::NotEnforced(why) => why,
            other => panic!("expected NotEnforced, got {other:?}"),
        };
        assert_eq!(why, EXEC_FAILED_REASON);
    }

    /// Fails every control with a status and a two-line stderr.
    const NOISY_REFUSAL: &str =
        "#!/bin/sh\necho 'sandbox-exec: nope' >&2\necho second >&2\nexit 3\n";

    fn diagnosed(body: &str) -> (SeatbeltFault, Diag) {
        let tools = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let fake = script(tools.path(), "sandbox-exec", body);
        probe_diagnosed_with(&fake, root.path(), Duration::from_secs(5)).unwrap_err()
    }

    #[test]
    fn a_refusal_says_which_control_failed_with_its_status_and_first_stderr_line() {
        let (fault, diag) = diagnosed(NOISY_REFUSAL);
        assert_eq!(fault, SeatbeltFault::Refused);
        assert_eq!(diag.control, "positive write");
        assert_eq!(diag.status, Some(3));
        assert_eq!(diag.stderr, "sandbox-exec: nope");
    }

    #[test]
    fn a_leak_names_the_negative_control_that_got_through() {
        let (fault, diag) = diagnosed(LEAKY);
        assert_eq!(fault, SeatbeltFault::Leaked);
        assert_eq!(diag.control, "negative write");
        let allow_echo = ENFORCING.replace("rw=\"\"", "rw=\"\"; ALLOW_ECHO=1");
        let (fault, diag) = diagnosed(&allow_echo);
        assert_eq!(fault, SeatbeltFault::Leaked);
        assert_eq!(diag.control, "negative exec");
        assert_eq!(diag.status, Some(0));
    }

    #[test]
    fn the_probe_spec_holds_canonical_paths_even_when_given_a_symlink() {
        let base = tempfile::tempdir().unwrap();
        let real = fs::canonicalize(base.path()).unwrap().join("real");
        fs::create_dir(&real).unwrap();
        let link = real.parent().unwrap().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let spec = probe_spec(&link, Path::new("/bin/sh")).unwrap();
        assert_eq!(spec.rw, vec![real]);
        // The program resolves too (`/bin/sh` may itself be a link).
        assert_eq!(spec.rx, vec![fs::canonicalize("/bin/sh").unwrap()]);
        assert_eq!(probe_spec(&link.join("gone"), Path::new("/bin/sh")), None);
    }

    #[test]
    fn the_program_argv_passes_verbatim_even_when_it_looks_like_a_flag() {
        let exe = some_exec();
        let argv = args(&["--", "/bin/x", "--", "-D", "KEY=v", "-p"]);
        let cmd = command(exe.path(), &argv).unwrap();
        let got: Vec<&OsStr> = cmd.get_args().collect();
        let tail = ["--", "/bin/x", "--", "-D", "KEY=v", "-p"].map(OsStr::new);
        assert_eq!(&got[got.len() - tail.len()..], &tail);
    }

    #[test]
    fn min_abi_is_a_linux_only_option_and_is_not_enforced_here() {
        let exe = some_exec();
        let why = refused(command(
            exe.path(),
            &args(&["--min-abi", "3", "--", "/bin/x"]),
        ));
        assert_eq!(why, MIN_ABI_REASON);
    }

    #[test]
    fn a_missing_sandbox_exec_is_not_enforced_with_the_fixed_text() {
        let gone = Path::new("/no/such/sandbox-exec");
        let why = refused(command(gone, &args(&["--", "/bin/x"])));
        assert_eq!(why, JailOff::NoSeatbelt(SeatbeltFault::Missing).text());
    }

    #[test]
    fn a_bad_helper_line_or_an_unrenderable_path_is_not_enforced() {
        let exe = some_exec();
        let why = refused(command(exe.path(), &args(&["--rx", "rel", "--", "/bin/x"])));
        assert_eq!(why, "rule paths must be absolute");
        let why = refused(command(
            exe.path(),
            &args(&["--rx", "/a/../b", "--", "/bin/x"]),
        ));
        assert_eq!(why, PROFILE_REASON);
        let why = refused(command(exe.path(), &args(&["--rx", "/", "--", "/bin/x"])));
        assert_eq!(why, PROFILE_REASON);
    }
}
