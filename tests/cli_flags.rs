//! The top-level flags answer before any TTY, raw mode or nesting check.

use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lazarobox-shell"))
        .args(args)
        .env("LAZAROBOX", "1")
        .output()
        .expect("spawn lazarobox-shell")
}

#[test]
fn version_prints_the_package_version() {
    for flag in ["--version", "-V"] {
        let out = run(&[flag]);
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            concat!("lazarobox-shell ", env!("CARGO_PKG_VERSION"), "\n")
        );
    }
}

#[test]
fn help_lists_the_public_usage_only() {
    for flag in ["--help", "-h"] {
        let out = run(&[flag]);
        assert_eq!(out.status.code(), Some(0));
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("--dir <folder>") && text.contains("install-desktop"));
        assert!(!text.contains("__"));
    }
}

#[test]
fn unknown_input_exits_2_with_one_stderr_line() {
    for (arg, needle) in [
        ("--bogus", "unknown option '--bogus'"),
        ("bogus", "unknown command 'bogus'"),
    ] {
        let out = run(&[arg]);
        assert_eq!(out.status.code(), Some(2));
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains(needle) && err.trim_end().lines().count() == 1);
        assert!(out.stdout.is_empty());
    }
}

#[test]
fn odd_and_misplaced_arguments_exit_2() {
    for args in [
        &["--"][..],
        &["-"],
        &[""],
        &["--force", "install-desktop"],
        &["--dir", "x", "__watch"],
        &["--dir", "--bogus"],
    ] {
        assert_eq!(run(args).status.code(), Some(2), "{args:?}");
    }
    let err = run(&["--force", "__watch"]).stderr;
    assert!(String::from_utf8_lossy(&err).contains("must be the first argument"));
}

#[test]
fn version_wins_over_what_follows_and_ignores_force() {
    for args in [
        &["--force", "--version"][..],
        &["--version", "--bogus"],
        &["--dir", "--version"],
    ] {
        let out = run(args);
        assert_eq!(out.status.code(), Some(0), "{args:?}");
        assert!(String::from_utf8_lossy(&out.stdout).starts_with("lazarobox-shell "));
    }
}

#[test]
fn the_internal_shim_is_not_rejected() {
    assert_ne!(run(&["__mcp-serve"]).status.code(), Some(2));
}
