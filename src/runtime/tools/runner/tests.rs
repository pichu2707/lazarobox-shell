//! The spawn core, driven with `/bin/sh` scripts: it sits below the
//! allow-list, so these are programs the allow-list would never let through.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use super::*;

fn script(text: &str) -> Run {
    Run {
        program: PathBuf::from("/bin/sh"),
        argv: vec![OsString::from("-c"), OsString::from(text)],
        cwd: std::env::temp_dir(),
        env: vec![(OsString::from("PATH"), OsString::from("/usr/bin:/bin"))],
        timeout: Duration::from_secs(30),
    }
}

async fn run(text: &str) -> Done {
    spawn_core(script(text)).await.expect("the shell starts")
}

#[tokio::test]
async fn stdout_and_stderr_share_one_pipe_and_keep_their_order() {
    let done = run("echo one; echo two >&2; echo three; echo four >&2").await;
    assert_eq!(done.capture.text(), "one\ntwo\nthree\nfour\n");
}

#[tokio::test]
async fn stdin_is_dev_null() {
    let done = run("if read line; then echo got; else echo eof; fi").await;
    assert_eq!(done.capture.text(), "eof\n");
}

#[tokio::test]
async fn the_child_leads_its_own_process_group() {
    let done = run("echo $$; ps -o pgid= -p $$").await;
    let text = done.capture.text();
    let mut lines = text.lines().map(str::trim);
    let (pid, pgid) = (lines.next().unwrap(), lines.next().unwrap());
    assert_eq!(pid, pgid);
    // SAFETY: getpgrp has no preconditions and cannot fail.
    assert_ne!(pgid, unsafe { libc::getpgrp() }.to_string());
}

#[tokio::test]
async fn the_environment_is_exactly_what_was_given() {
    let mut run = script("/usr/bin/env");
    run.env
        .push((OsString::from("ONLY"), OsString::from("this")));
    let done = spawn_core(run).await.unwrap();
    let mut seen: Vec<String> = done.capture.text().lines().map(str::to_owned).collect();
    seen.retain(|line| {
        !line.starts_with("PWD=") && !line.starts_with("_=") && !line.starts_with("SHLVL=")
    });
    seen.sort();
    assert_eq!(seen, ["ONLY=this", "PATH=/usr/bin:/bin"]);
}

#[tokio::test]
async fn it_runs_in_the_given_folder() {
    let dir = tempfile::tempdir().unwrap();
    let mut run = script("pwd -P");
    run.cwd = dir.path().canonicalize().unwrap();
    let done = spawn_core(run.clone()).await.unwrap();
    assert_eq!(done.capture.text().trim(), run.cwd.to_str().unwrap());
}

#[tokio::test]
async fn an_exit_code_and_a_signal_are_told_apart() {
    let done = run("exit 101").await;
    assert_eq!((done.ended.exit, done.ended.signal), (Some(101), None));
    assert!(!done.ended.timed_out);
    let done = run("kill -9 $$").await;
    assert_eq!((done.ended.exit, done.ended.signal), (None, Some(9)));
}

#[tokio::test]
async fn a_program_that_does_not_exist_is_an_error() {
    let mut run = script("true");
    run.program = PathBuf::from("/nonexistent/lbx-program");
    assert!(spawn_core(run).await.is_err());
}
