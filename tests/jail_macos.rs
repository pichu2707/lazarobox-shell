//! macOS only: `/usr/bin/sandbox-exec` must really confine a program on this
//! machine, through the real helper (`__jail-exec`, which renders the profile
//! and execs `sandbox-exec`). Set `LAZAROBOX_REQUIRE_JAIL=1` to fail instead
//! of skipping, so a CI job that is meant to have Seatbelt cannot go green by
//! testing nothing.
//!
//! The fake CLIs are `/bin/bash -c` scripts that stick to builtins, so the
//! only program a test has to grant for execution is bash itself.

#![cfg(target_os = "macos")]

use std::{
    ffi::{OsStr, OsString},
    fs,
    io::{BufRead, BufReader},
    net::TcpListener,
    os::unix::{fs::PermissionsExt, net::UnixListener},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::OnceLock,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use lazarobox_shell::{
    core::ai::{
        chat::engine::ChatError,
        jail::{JailArgs, JailSpec, SANDBOX_EXEC_EXIT, SANDBOX_EXEC_SYNTAX_EXIT, exit_error},
    },
    runtime::jail::seatbelt_probe_diagnosed,
};

const BASH: &str = "/bin/bash";
const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

fn required() -> bool {
    std::env::var_os("LAZAROBOX_REQUIRE_JAIL").is_some_and(|v| v == "1")
}

#[test]
fn sandbox_exec_confines_the_probe_programs() {
    match seatbelt_probe_diagnosed() {
        Ok(()) => {}
        // The detail is CI output only; the app never shows the raw stderr.
        Err((fault, detail)) if required() => {
            panic!("LAZAROBOX_REQUIRE_JAIL=1 but the probe failed: {fault:?} ({detail:?})")
        }
        Err((fault, detail)) => {
            eprintln!("skipped: the macOS probe failed ({fault:?}, {detail:?})")
        }
    }
}

/// Whether the scenarios can run here: the probe passes, or this run is not
/// required to have a jail. The probe runs once per test binary.
fn jail_available() -> bool {
    static PROBE: OnceLock<Result<(), String>> = OnceLock::new();
    match PROBE.get_or_init(|| {
        seatbelt_probe_diagnosed().map_err(|(fault, detail)| format!("{fault:?}, {detail:?}"))
    }) {
        Ok(()) => true,
        Err(why) => {
            assert!(
                !required(),
                "LAZAROBOX_REQUIRE_JAIL=1 but this system cannot jail ({why})"
            );
            eprintln!("skipped: no Seatbelt jail on this system ({why})");
            false
        }
    }
}

fn canon(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    fs::canonicalize(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A canonical temp dir (Seatbelt matches real paths, and `/var` is a link)
/// with a `ro` and an `rw` folder, plus an `outside` one that no rule names.
struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
    ro: PathBuf,
    rw: PathBuf,
    outside: PathBuf,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let root = canon(dir.path());
    let [ro, rw, outside] = ["ro", "rw", "outside"].map(|name| {
        let path = root.join(name);
        fs::create_dir(&path).unwrap();
        path
    });
    fs::write(ro.join("note"), "readable\n").unwrap();
    fs::write(outside.join("secret"), "hunter2\n").unwrap();
    World {
        _dir: dir,
        root,
        ro,
        rw,
        outside,
    }
}

/// A job that runs `script` with `/bin/bash -c`; `args` become `$1`, `$2`...
fn bash_job(script: &str, args: &[&Path], tweak: impl FnOnce(&mut JailSpec)) -> JailArgs {
    let program = canon(BASH);
    let mut spec = JailSpec {
        rx: vec![program.clone()],
        ..JailSpec::default()
    };
    tweak(&mut spec);
    let mut argv: Vec<OsString> = vec!["-c".into(), script.into(), "fake-cli".into()];
    argv.extend(args.iter().map(|a| a.as_os_str().to_owned()));
    JailArgs {
        min_abi: None,
        spec,
        program,
        argv,
    }
}

/// The helper binary, with a marker in its environment for the passthrough test.
fn helper(args: &[OsString]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_lazarobox-shell"));
    command
        .args(args)
        .env("LC_ALL", "C")
        .env("LAZAROBOX_TEST_MARK", "marker-value")
        .stdin(Stdio::null());
    command
}

fn run(job: &JailArgs) -> Output {
    helper(&job.render()).output().expect("the helper starts")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Everything a failed assertion needs to be read from the CI log alone.
fn report(out: &Output) -> String {
    format!(
        "status {:?}\nstdout: {}\nstderr: {}",
        out.status.code(),
        stdout(out),
        stderr(out)
    )
}

/// The EPERM a Seatbelt denial returns. "Permission denied" (EACCES, mode
/// bits) is deliberately not accepted: it would pass without any sandbox.
fn denied(out: &Output) -> bool {
    stderr(out).contains("Operation not permitted")
}

/// Status 71 plus the `sandbox-exec: ` first line: what the app turns into a
/// jail error.
fn assert_jail_error(out: &Output) {
    assert!(
        matches!(
            exit_error(out.status.code(), &stderr(out)),
            Some(ChatError::Jail(_))
        ),
        "{}",
        report(out)
    );
}

/// Calls `accept` until it yields or `limit` passes.
fn within<T>(limit: Duration, mut accept: impl FnMut() -> std::io::Result<T>) -> Option<T> {
    let started = Instant::now();
    while started.elapsed() < limit {
        match accept() {
            Ok(got) => return Some(got),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20))
            }
            Err(_) => return None,
        }
    }
    None
}

/// A loopback listener that reports whether anything connected within `limit`.
fn tcp_listener(limit: Duration) -> (u16, JoinHandle<bool>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    (
        port,
        thread::spawn(move || within(limit, || listener.accept()).is_some()),
    )
}

/// A unix listener that returns the first line a client sends, if any.
fn unix_listener(path: &Path, limit: Duration) -> JoinHandle<Option<String>> {
    let listener = UnixListener::bind(path).unwrap();
    listener.set_nonblocking(true).unwrap();
    thread::spawn(move || {
        let (stream, _) = within(limit, || listener.accept())?;
        stream.set_nonblocking(false).ok()?;
        stream.set_read_timeout(Some(limit)).ok()?;
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).ok()?;
        Some(line)
    })
}

/// A node on this machine, resolved to its real file; `None` skips the
/// scenario unless the run requires a jail.
fn node() -> Option<(PathBuf, PathBuf)> {
    let path = std::env::var_os("PATH")?;
    let found = std::env::split_paths(&path)
        .map(|dir| dir.join("node"))
        .find(|candidate| candidate.is_file())?;
    let dir = found.parent()?.to_path_buf();
    Some((canon(found), dir))
}

/// Homebrew's node reads its OpenSSL config at start, and Homebrew `etc/` is
/// closed in the profile on purpose (it holds service settings), so a node
/// layout has to grant this one file as an `ro_files` literal.
fn node_ro_files() -> Vec<PathBuf> {
    [
        "/opt/homebrew/etc/openssl@3/openssl.cnf",
        "/usr/local/etc/openssl@3/openssl.cnf",
    ]
    .into_iter()
    .filter(|p| Path::new(p).is_file())
    .map(canon)
    .collect()
}

fn node_or_skip() -> Option<(PathBuf, PathBuf)> {
    let found = node();
    if let Some((node, _)) = &found {
        eprintln!(
            "node under test: {} (ro_files {:?})",
            node.display(),
            node_ro_files()
        );
    }
    if found.is_none() {
        assert!(!required(), "LAZAROBOX_REQUIRE_JAIL=1 but there is no node");
        eprintln!("skipped: no node on PATH");
    }
    found
}

// --- 3b.1: the fake CLI, one behaviour per test ---------------------------

#[test]
fn a_file_in_ro_is_read_and_one_outside_is_denied() {
    if !jail_available() {
        return;
    }
    let w = world();
    let script = r#"read -r a < "$1/note" && echo "inside=$a"
read -r b < "$2/secret" && echo "outside=$b"
exit 0"#;
    let out = run(&bash_job(script, &[&w.ro, &w.outside], |s| {
        s.ro.push(w.ro.clone())
    }));
    assert!(out.status.success(), "{}", report(&out));
    assert!(stdout(&out).contains("inside=readable"), "{}", report(&out));
    assert!(!stdout(&out).contains("hunter2"), "{}", report(&out));
    assert!(denied(&out), "{}", report(&out));
}

#[test]
fn a_write_works_in_rw_and_is_denied_outside_it() {
    if !jail_available() {
        return;
    }
    let w = world();
    let script = r#"echo in > "$1/f"; echo "inside=$?"
echo out > "$2/f"; echo "outside=$?""#;
    let out = run(&bash_job(script, &[&w.rw, &w.outside], |s| {
        s.rw.push(w.rw.clone())
    }));
    let text = stdout(&out);
    assert!(text.contains("inside=0"), "{}", report(&out));
    assert!(text.contains("outside=1"), "{}", report(&out));
    assert!(denied(&out), "{}", report(&out));
    assert_eq!(fs::read_to_string(w.rw.join("f")).unwrap(), "in\n");
    assert!(!w.outside.join("f").exists());
}

#[test]
fn an_exec_beyond_rx_is_denied_and_one_inside_it_runs() {
    if !jail_available() {
        return;
    }
    let w = world();
    let script = r#"/usr/bin/true; echo "granted=$?"
/usr/bin/touch "$1/pwned"; echo "other=$?""#;
    let out = run(&bash_job(script, &[&w.rw], |s| {
        s.rw.push(w.rw.clone());
        s.rx.push(canon("/usr/bin/true"));
    }));
    let text = stdout(&out);
    assert!(text.contains("granted=0"), "{}", report(&out));
    assert!(!text.contains("other=0"), "{}", report(&out));
    let first = stderr(&out).lines().next().unwrap_or_default().to_owned();
    assert!(
        first.contains("Operation not permitted"),
        "{}",
        report(&out)
    );
    assert!(!w.rw.join("pwned").exists(), "touch ran: {}", report(&out));
}

#[test]
fn a_tcp_connect_goes_through_dev_tcp() {
    if !jail_available() {
        return;
    }
    let (port, accepted) = tcp_listener(Duration::from_secs(10));
    let script = r#"exec 3<>/dev/tcp/127.0.0.1/"$1" && echo connected"#;
    let out = run(&bash_job(script, &[Path::new(&port.to_string())], |_| {}));
    assert!(stdout(&out).contains("connected"), "{}", report(&out));
    assert!(accepted.join().unwrap(), "{}", report(&out));
}

/// A loopback port nothing listens on: bound and released just now.
fn fresh_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Kills the child and everything it started (its own process group).
fn kill_group(child: &mut Child) {
    // SAFETY: `kill` only sends a signal; the group id is the child's pid,
    // because the child was spawned with `process_group(0)`.
    unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
    let _ = child.wait();
}

#[test]
fn nc_cannot_bind_a_port() {
    use std::os::unix::process::CommandExt;

    if !jail_available() {
        return;
    }
    let nc = canon("/usr/bin/nc");

    // Control 1: the same nc, under the same profile, does connect.
    let (live, accepted) = tcp_listener(Duration::from_secs(10));
    let connect = r#"/usr/bin/nc -z 127.0.0.1 "$1"; echo "connect=$?""#;
    let out = run(&bash_job(connect, &[Path::new(&live.to_string())], |s| {
        s.rx.push(nc.clone())
    }));
    assert!(stdout(&out).contains("connect=0"), "{}", report(&out));
    assert!(accepted.join().unwrap(), "{}", report(&out));

    // Control 2: WITHOUT a sandbox, this nc does listen on a fresh port.
    let open = fresh_port();
    let mut bare = Command::new("/usr/bin/nc")
        .args(["-l", "127.0.0.1", &open.to_string()])
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    // A refused connect only means nc is not up yet: retry until the limit.
    let started = Instant::now();
    let listening = loop {
        if std::net::TcpStream::connect(("127.0.0.1", open)).is_ok() {
            break true;
        }
        if started.elapsed() > Duration::from_secs(10) {
            break false;
        }
        thread::sleep(Duration::from_millis(50));
    };
    kill_group(&mut bare);
    assert!(listening, "an unsandboxed nc -l did not listen");

    // The jailed bind, on another fresh port. A child that stays up means the
    // bind was allowed; the whole process group is killed then, so no nc keeps
    // the pipes open.
    let denied_port = fresh_port();
    let job = bash_job(
        r#"/usr/bin/nc -l 127.0.0.1 "$1" < /dev/null; echo "listen=$?""#,
        &[Path::new(&denied_port.to_string())],
        |s| s.rx.push(nc.clone()),
    );
    let mut child = helper(&job.render())
        .process_group(0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let ended = within(Duration::from_secs(10), || {
        child.try_wait().and_then(|status| {
            status.ok_or_else(|| std::io::Error::from(std::io::ErrorKind::WouldBlock))
        })
    });
    if ended.is_none() {
        kill_group(&mut child);
    }
    let out = child.wait_with_output().unwrap();
    assert!(ended.is_some(), "nc kept listening: {}", report(&out));
    assert!(denied(&out), "{}", report(&out));
    let text = stdout(&out);
    let code = text.lines().find_map(|l| l.strip_prefix("listen="));
    assert!(
        code.is_some_and(|c| c != "0"),
        "nc did not report a failure: {}",
        report(&out)
    );
}

#[test]
fn a_unix_socket_is_denied_unless_it_is_an_rw_file() {
    if !jail_available() {
        return;
    }
    let w = world();
    let nc = canon("/usr/bin/nc");
    let socket = w.rw.join("shim.sock");
    let script = r#"echo hello | /usr/bin/nc -U "$1"; echo "nc=$?""#;

    // Without a grant for the socket path: the connect is refused.
    let seen = unix_listener(&socket, Duration::from_secs(3));
    let out = run(&bash_job(script, &[&socket], |s| s.rx.push(nc.clone())));
    assert_eq!(seen.join().unwrap(), None, "{}", report(&out));
    assert!(!stdout(&out).contains("nc=0"), "{}", report(&out));
    fs::remove_file(&socket).unwrap();

    // The same socket as an `rw_files` literal: the shim's own socket works.
    let seen = unix_listener(&socket, Duration::from_secs(10));
    let out = run(&bash_job(script, &[&socket], |s| {
        s.rx.push(nc.clone());
        s.rw_files.push(socket.clone())
    }));
    assert_eq!(
        seen.join().unwrap().as_deref(),
        Some("hello\n"),
        "{}",
        report(&out)
    );
}

// --- 3b.2: the helper path and its edges ----------------------------------

#[test]
fn a_profile_that_does_not_compile_never_runs_the_program() {
    if !jail_available() {
        return;
    }
    let w = world();
    let marker = w.rw.join("ran");
    let out = Command::new(SANDBOX_EXEC)
        .args(["-p", "(version 1) (deny default) (allow", "--"])
        .args(["/usr/bin/touch"])
        .arg(&marker)
        .output()
        .unwrap();
    assert!(!marker.exists(), "touch ran: {}", report(&out));
    assert_eq!(
        out.status.code(),
        Some(SANDBOX_EXEC_SYNTAX_EXIT),
        "{}",
        report(&out)
    );
    assert_jail_error(&out);
}

#[test]
fn a_program_beyond_rx_is_a_jail_error_through_the_helper() {
    if !jail_available() {
        return;
    }
    let w = world();
    let program = canon("/usr/bin/touch");
    let marker = w.rw.join("ran");
    let job = JailArgs {
        min_abi: None,
        spec: JailSpec {
            rw: vec![w.rw.clone()],
            ..JailSpec::default()
        },
        program,
        argv: vec![marker.clone().into_os_string()],
    };
    let out = run(&job);
    assert!(!marker.exists(), "touch ran: {}", report(&out));
    assert_eq!(
        out.status.code(),
        Some(SANDBOX_EXEC_EXIT),
        "{}",
        report(&out)
    );
    assert_jail_error(&out);
}

#[test]
fn a_nested_sandbox_is_refused_as_a_jail_error() {
    if !jail_available() {
        return;
    }
    let w = world();
    let script = r#"/usr/bin/sandbox-exec -p '(version 1)(allow default)' /usr/bin/touch "$1/ran""#;
    let out = run(&bash_job(script, &[&w.rw], |s| {
        s.rw.push(w.rw.clone());
        s.rx.push(canon(SANDBOX_EXEC));
        s.rx.push(canon("/usr/bin/touch"));
    }));
    assert!(!w.rw.join("ran").exists(), "touch ran: {}", report(&out));
    assert!(stderr(&out).contains("sandbox_apply"), "{}", report(&out));
    assert_jail_error(&out);
}

/// A job that may write in `granted` only: a write there works, and one into
/// `prefix` (a path that merely starts like it) is denied.
fn assert_granted_exactly(w: &World, granted: &Path, prefix: &Path) {
    fs::create_dir(granted).unwrap();
    fs::create_dir(prefix).unwrap();
    let script = r#"echo in > "$1/f"; echo "granted=$?"
echo out > "$2/f"; echo "prefix=$?""#;
    let out = run(&bash_job(script, &[granted, prefix], |s| {
        s.rw.push(granted.to_path_buf())
    }));
    assert!(stdout(&out).contains("granted=0"), "{}", report(&out));
    assert!(stdout(&out).contains("prefix=1"), "{}", report(&out));
    assert!(granted.join("f").exists() && !prefix.join("f").exists());
    assert!(!w.outside.join("f").exists());
}

/// A rw folder named with every character that matters to the profile text
/// or to `KEY=value`: it is granted exactly, and its prefix is not.
#[test]
fn a_path_with_special_characters_is_granted_exactly() {
    if !jail_available() {
        return;
    }
    let w = world();
    assert_granted_exactly(&w, &w.root.join("a=b \"q\") (x\nz"), &w.root.join("a"));
}

/// `sandbox-exec` must split a `-D` at the first `=` only.
#[test]
fn an_equals_sign_in_a_value_is_kept() {
    if !jail_available() {
        return;
    }
    let w = world();
    assert_granted_exactly(&w, &w.root.join("k=v=w"), &w.root.join("k"));
}

/// Writes an executable script with the given first line.
fn write_executable(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn a_vendor_script_inside_an_rx_root_runs() {
    if !jail_available() {
        return;
    }
    let w = world();
    let vendor = w.root.join("pkg/vendor");
    fs::create_dir_all(&vendor).unwrap();
    write_executable(&vendor.join("tool"), "#!/bin/bash\necho vendor-ran\n");
    let root = w.root.join("pkg");
    let out = run(&bash_job(r#""$1/vendor/tool""#, &[&root], |s| {
        s.rx.push(root.clone())
    }));
    assert!(out.status.success(), "{}", report(&out));
    assert_eq!(stdout(&out), "vendor-ran\n", "{}", report(&out));
}

#[test]
fn the_environment_passes_through_the_helper() {
    if !jail_available() {
        return;
    }
    let out = run(&bash_job(
        r#"echo "mark=$LAZAROBOX_TEST_MARK""#,
        &[],
        |_| {},
    ));
    assert_eq!(stdout(&out), "mark=marker-value\n", "{}", report(&out));
}

#[test]
fn node_starts_under_the_profile() {
    if !jail_available() {
        return;
    }
    let Some((node, _)) = node_or_skip() else {
        return;
    };
    let job = JailArgs {
        min_abi: None,
        spec: JailSpec {
            rx: vec![node.clone()],
            ro_files: node_ro_files(),
            ..JailSpec::default()
        },
        program: node,
        argv: ["-e", "console.log(6 * 7)"].map(OsString::from).to_vec(),
    };
    let out = run(&job);
    assert_eq!(stdout(&out), "42\n", "{}", report(&out));
}

// --- 3b.4: an npm-style shim ----------------------------------------------

/// An npm shim is `#!/usr/bin/env node`: `env` finds node on `PATH`.
fn run_shim(with_env_in_rx: bool) -> Option<(Output, World)> {
    let (node, node_dir) = node_or_skip()?;
    let w = world();
    let shim = w.root.join("cli");
    write_executable(&shim, "#!/usr/bin/env node\nconsole.log('shim-ok');\n");
    let mut spec = JailSpec {
        rx: vec![shim.clone(), node],
        ro_files: node_ro_files(),
        ..JailSpec::default()
    };
    if with_env_in_rx {
        spec.rx.push(canon("/usr/bin/env"));
    }
    let job = JailArgs {
        min_abi: None,
        spec,
        program: shim,
        argv: Vec::new(),
    };
    let mut path = OsString::from(node_dir);
    path.push(OsStr::new(":/usr/bin:/bin"));
    let out = helper(&job.render()).env("PATH", path).output().unwrap();
    Some((out, w))
}

#[test]
fn an_npm_shim_starts_when_env_is_in_rx() {
    if !jail_available() {
        return;
    }
    let Some((out, _w)) = run_shim(true) else {
        return;
    };
    assert_eq!(stdout(&out), "shim-ok\n", "{}", report(&out));
}

/// `env` is a program like any other: the shebang does not exempt it, and no
/// `process-exec-interpreter` rule is needed once it is in `rx`.
#[test]
fn an_npm_shim_is_refused_when_env_is_not_in_rx() {
    if !jail_available() {
        return;
    }
    let Some((out, _w)) = run_shim(false) else {
        return;
    };
    assert!(!stdout(&out).contains("shim-ok"), "{}", report(&out));
    assert_eq!(
        out.status.code(),
        Some(SANDBOX_EXEC_EXIT),
        "{}",
        report(&out)
    );
    assert_jail_error(&out);
}
