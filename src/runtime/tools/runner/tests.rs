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

/// No such process, or only a zombie nobody has collected yet.
fn gone(pid: libc::pid_t) -> bool {
    matches!(crate::runtime::proc::process_state(pid), None | Some('Z'))
}

/// Polls until `pid` is gone; the group was signalled, so this is quick.
async fn wait_gone(pid: libc::pid_t) -> bool {
    for _ in 0..2000 {
        if gone(pid) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    false
}

fn first_pid(done: &Done) -> libc::pid_t {
    done.capture
        .text()
        .lines()
        .next()
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

#[tokio::test]
async fn a_timeout_kills_the_group_with_its_grandchild_and_says_so() {
    let mut run = script("sleep 300 & echo $!; wait");
    run.timeout = Duration::from_millis(300);
    let done = spawn_core(run).await.unwrap();
    assert!(done.ended.timed_out);
    assert_eq!((done.ended.exit, done.ended.signal), (None, Some(9)));
    assert!(done.duration < Duration::from_secs(10));
    // What it printed before the deadline is kept.
    assert!(wait_gone(first_pid(&done)).await, "the grandchild lives");
}

#[tokio::test]
async fn a_run_that_ends_in_time_is_not_a_timeout() {
    let mut run = script("echo fast");
    run.timeout = Duration::from_secs(20);
    let done = spawn_core(run).await.unwrap();
    assert!(!done.ended.timed_out);
    assert_eq!(done.ended.exit, Some(0));
}

#[tokio::test]
async fn a_leader_that_exits_takes_the_grandchildren_it_left_with_it() {
    let done = run("sleep 300 >/dev/null 2>&1 & echo $!; exit 0").await;
    assert_eq!(done.ended.exit, Some(0));
    assert!(!done.ended.timed_out);
    assert!(wait_gone(first_pid(&done)).await, "the grandchild lives");
}

#[tokio::test]
async fn dropping_the_future_kills_the_group() {
    let dir = tempfile::tempdir().unwrap();
    let pids = dir.path().join("pids");
    let text = format!(
        "sleep 300 & echo $! > '{p}.tmp'; echo $$ >> '{p}.tmp'; mv '{p}.tmp' '{p}'; wait",
        p = pids.display()
    );
    let ready = async {
        while !pids.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    tokio::select! {
        _ = spawn_core(script(&text)) => panic!("the script ends only when killed"),
        _ = ready => {}
    }
    // The future is gone now: both processes must follow it.
    let read = std::fs::read_to_string(&pids).unwrap();
    for pid in read.lines().map(|l| l.parse::<libc::pid_t>().unwrap()) {
        assert!(wait_gone(pid).await, "{pid} lives");
    }
}

#[tokio::test]
async fn what_the_leader_wrote_last_is_read_after_it_exits() {
    // Far over the 64 KiB of a pipe: the end is still in it when the leader goes.
    let done = run("head -c 300000 /dev/zero | tr '\\0' x").await;
    assert_eq!(done.capture.total(), 300_000);
    assert!(done.capture.truncated());
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_grandchild_that_left_the_group_cannot_hold_the_run() {
    let started = std::time::Instant::now();
    // The leader waits until the escapee has really left its group.
    let done = run("setsid sleep 300 & p=$!; echo $p; \
         while [ \"$(ps -o pgid= -p $p | tr -d ' ')\" = \"$$\" ]; do sleep 0.05; done; echo out")
    .await;
    let pid = first_pid(&done);
    // Reached the end although the pipe never closed: the drain is bounded.
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(done.capture.text().ends_with("out\n"));
    // A documented limit: it survives. Clean it up.
    assert!(
        !gone(pid),
        "setsid left the group, the kill cannot reach it"
    );
    // SAFETY: a signal to the sleep this test started.
    unsafe { libc::kill(pid, libc::SIGKILL) };
}

#[tokio::test]
async fn the_drain_reads_what_is_left_and_stops_at_its_bound_when_the_pipe_stays_open() {
    use std::io::Write;
    let (reader, mut writer) = std::io::pipe().unwrap();
    writer.write_all(b"left over").unwrap();
    let file = std::fs::File::from(std::os::fd::OwnedFd::from(reader));
    let mut pipe = Receiver::from_file(file).unwrap();
    let mut capture = Capture::new();
    let started = std::time::Instant::now();
    drain(&mut pipe, &mut capture, &mut [0u8; 64]).await;
    // The writer is still open: only the bound ended it, and not early.
    assert_eq!(capture.text(), "left over");
    assert!(started.elapsed() >= Duration::from_millis(400));
    assert!(started.elapsed() < Duration::from_secs(10));
    drop(writer);
}
