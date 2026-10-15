//! The binary refuses to nest before touching the terminal, and never blocks
//! the internal `__jail-exec` helper.
//!
//! If the guard ever regressed, the child would start the real TUI. So each
//! child gets a session of its own (no controlling tty to flip into raw mode),
//! no terminal-related variables, and a deadline after which it is killed.

#![cfg(unix)]

use std::{
    io::Read,
    os::unix::process::CommandExt,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use lazarobox_shell::core::ai::jail::NOT_ENFORCED_EXIT;

const DEADLINE: Duration = Duration::from_secs(5);

struct Run {
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: String,
}

fn run(args: &[&str]) -> Run {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_lazarobox-shell"));
    cmd.args(args)
        .env("LAZAROBOX", "1")
        .env_remove("TERM")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SAFETY: setsid is async-signal-safe and allowed between fork and exec.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = cmd.spawn().expect("spawn binary");
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll child") {
            break status;
        }
        if start.elapsed() > DEADLINE {
            let _ = child.kill();
            let _ = child.wait();
            panic!("binary did not exit within {DEADLINE:?}: the guard did not stop it");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut stdout = Vec::new();
    let mut stderr = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut stdout)
        .unwrap();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    Run {
        code: status.code(),
        stdout,
        stderr,
    }
}

#[test]
fn refuses_to_nest_with_one_line_on_stderr() {
    let out = run(&[]);
    assert_eq!(out.code, Some(1));
    assert_eq!(out.stderr.lines().count(), 1, "{}", out.stderr);
    assert!(out.stderr.contains("--force"));
    assert!(
        out.stdout.is_empty(),
        "no terminal escapes before the guard"
    );
}

#[test]
fn internal_subcommand_is_not_blocked() {
    // With no arguments the helper itself fails with its own exit code; that
    // proves it ran instead of the guard (which exits 1).
    let out = run(&["__jail-exec"]);
    assert_eq!(out.code, Some(NOT_ENFORCED_EXIT), "{}", out.stderr);
    assert!(!out.stderr.contains("already running"), "{}", out.stderr);
}
