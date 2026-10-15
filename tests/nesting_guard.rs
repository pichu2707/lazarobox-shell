//! The binary refuses to nest before touching the terminal, and never blocks
//! the internal `__jail-exec` helper.

use std::process::{Command, Stdio};

fn bin() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_lazarobox-shell"));
    cmd.stdin(Stdio::null());
    cmd
}

#[test]
fn refuses_to_nest_with_one_line_on_stderr() {
    let out = bin().env("LAZAROBOX", "1").output().expect("run binary");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert!(stderr.contains("--force"));
    assert!(
        out.stdout.is_empty(),
        "no terminal escapes before the guard"
    );
}

#[test]
fn internal_subcommand_is_not_blocked() {
    let out = bin()
        .env("LAZAROBOX", "1")
        .arg("__jail-exec")
        .output()
        .expect("run binary");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("already running"), "{stderr}");
}
