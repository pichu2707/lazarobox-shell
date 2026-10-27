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

// --- the jailed run: scrub, private folders, integrity, ring ---

use std::os::unix::fs::PermissionsExt;

use crate::core::ai::jail::{NetBind, Rlimits};
use crate::core::ai::tools::allow::Program;
use crate::runtime::tools::integrity::Kind;

/// A stand-in for `lazarobox __jail-exec`: it prints the options it was given
/// (one `arg:` line each), then runs the program after `--`.
const HELPER: &str = "#!/bin/sh\n\
    while [ \"$1\" != \"--\" ]; do echo \"arg:$1\"; shift; done\n\
    shift\n\
    exec \"$@\"\n";

struct Fixture {
    dir: tempfile::TempDir,
    helper: PathBuf,
    root: PathBuf,
    tmp_parent: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let helper = base.join("helper");
    std::fs::write(&helper, HELPER).unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let root = base.join("project");
    std::fs::create_dir_all(root.join(".git/hooks")).unwrap();
    std::fs::write(root.join(".git/config"), "[core]\n").unwrap();
    Fixture {
        tmp_parent: base.join("tmp"),
        dir,
        helper,
        root,
    }
}

impl Fixture {
    fn job(&self, tool: Program, script: &str) -> Job {
        Job {
            helper: self.helper.clone(),
            spec: crate::core::ai::jail::JailSpec {
                rwx: vec![self.root.clone()],
                ..Default::default()
            },
            tool,
            program: PathBuf::from("/bin/sh"),
            argv: vec![OsString::from("-c"), OsString::from(script)],
            root: self.root.clone(),
            cwd: self.root.clone(),
            scratch: Scratch::unique(&self.tmp_parent, "run").unwrap(),
            timeout: Duration::from_secs(30),
        }
    }
}

/// The parent's environment: a secret, the session token, and the usual.
fn parent(name: &str) -> Option<OsString> {
    match name {
        "PATH" => Some("/usr/bin:/bin".into()),
        "HOME" => Some("/home/someone".into()),
        "SECRET_TOKEN" => Some("hunter2".into()),
        "LAZAROBOX_SESSION_TOKEN" => Some("abc123".into()),
        "AWS_SECRET_ACCESS_KEY" => Some("aws".into()),
        _ => None,
    }
}

fn lines(done: &Done) -> Vec<String> {
    done.capture.text().lines().map(str::to_owned).collect()
}

#[tokio::test]
async fn no_secret_of_the_parent_reaches_the_child() {
    let fx = fixture();
    let done = run_jailed(fx.job(Program::Cargo, "/usr/bin/env"), &parent)
        .await
        .unwrap()
        .done;
    let text = done.capture.text();
    for secret in [
        "SECRET_TOKEN",
        "LAZAROBOX_SESSION_TOKEN",
        "hunter2",
        "abc123",
        "AWS_",
    ] {
        assert!(!text.contains(secret), "{secret} leaked:\n{text}");
    }
    assert!(text.contains("PATH=/usr/bin:/bin"));
    assert!(text.contains("NO_COLOR=1"));
}

#[tokio::test]
async fn the_private_tmpdir_is_the_childs_and_is_gone_after_the_run() {
    let fx = fixture();
    let job = fx.job(Program::Cargo, "echo $TMPDIR; touch $TMPDIR/left-behind");
    let tmp = job.scratch.path().to_path_buf();
    let done = run_jailed(job, &parent).await.unwrap().done;
    assert_eq!(lines(&done).last().unwrap(), tmp.to_str().unwrap());
    assert!(tmp.starts_with(&fx.tmp_parent));
    assert!(!tmp.exists(), "the private folder outlived the run");
    assert!(fx.tmp_parent.exists());
}

#[tokio::test]
async fn a_dropped_run_removes_its_private_folder_too() {
    let fx = fixture();
    let job = fx.job(Program::Cargo, "touch $TMPDIR/up; sleep 300 & wait");
    let tmp = job.scratch.path().to_path_buf();
    let up = tmp.join("up");
    let ready = async {
        while !up.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    tokio::select! {
        _ = run_jailed(job, &parent) => panic!("it ends only when killed"),
        _ = ready => {}
    }
    assert!(!tmp.exists());
}

#[tokio::test]
async fn only_go_gets_an_executable_tmp_under_the_root_and_it_is_removed_after() {
    let fx = fixture();
    let go = fx.job(
        Program::Go,
        "echo $GOTMPDIR; test -d \"$GOTMPDIR\" && echo there",
    );
    let done = run_jailed(go, &parent).await.unwrap().done;
    let seen = lines(&done);
    let gotmp = PathBuf::from(&seen[seen.len() - 2]);
    assert!(gotmp.starts_with(&fx.root), "{gotmp:?}");
    assert_eq!(seen.last().unwrap(), "there");
    assert!(!gotmp.exists());
    assert_eq!(
        std::fs::read_dir(&fx.root).unwrap().count(),
        1,
        "only .git is left"
    );

    let cargo = fx.job(Program::Cargo, "echo \"[$GOTMPDIR]\"");
    let done = run_jailed(cargo, &parent).await.unwrap().done;
    assert_eq!(lines(&done).last().unwrap(), "[]");
}

#[tokio::test]
async fn the_helper_gets_the_toolchain_profile_and_then_the_program() {
    let fx = fixture();
    let done = run_jailed(fx.job(Program::Cargo, "true"), &parent)
        .await
        .unwrap()
        .done;
    let args = lines(&done);
    let root = fx.root.to_str().unwrap();
    assert_eq!(
        args,
        [
            "arg:__jail-exec".to_owned(),
            "arg:--rwx".to_owned(),
            format!("arg:{root}"),
            "arg:--net-bind".to_owned(),
            "arg:open".to_owned(),
            "arg:--rlimits".to_owned(),
            "arg:toolchain".to_owned(),
        ]
    );
}

#[test]
fn the_toolchain_profile_is_set_on_the_spec_whatever_the_caller_left() {
    let fx = fixture();
    let job = fx.job(Program::Cargo, "true");
    assert_eq!(
        (job.spec.net_bind, job.spec.rlimits),
        (NetBind::Closed, Rlimits::None)
    );
    let spec = profile(job.spec);
    assert_eq!(
        (spec.net_bind, spec.rlimits),
        (NetBind::Open, Rlimits::Toolchain)
    );
}

#[tokio::test]
async fn a_long_output_is_cut_to_head_and_tail_through_the_whole_run() {
    let fx = fixture();
    let script = "head -c 204800 /dev/zero | tr '\\0' x; echo; echo the-end";
    let done = run_jailed(fx.job(Program::Cargo, script), &parent)
        .await
        .unwrap()
        .done;
    assert!(done.capture.truncated());
    assert!(done.capture.total() > 200_000);
    let text = done.capture.text();
    assert!(text.len() <= crate::core::ai::tools::label::OUTPUT_CAP);
    assert!(text.contains("bytes omitted"));
    // The helper's own lines are the head; the end of the run is the tail.
    assert!(text.starts_with("arg:__jail-exec\n"));
    assert!(text.ends_with("the-end\n"));
}

#[tokio::test]
async fn a_hook_written_by_the_run_is_reported_and_nothing_else_is() {
    let fx = fixture();
    let script = "echo '#!/bin/sh' > .git/hooks/pre-commit; echo x > src.txt";
    let ran = run_jailed(fx.job(Program::Cargo, script), &parent)
        .await
        .unwrap();
    let found: Vec<_> = ran
        .changes
        .iter()
        .map(|c| (c.path.to_string_lossy().into_owned(), c.kind))
        .collect();
    assert_eq!(found, [(".git/hooks/pre-commit".to_owned(), Kind::Added)]);
    let quiet = run_jailed(fx.job(Program::Cargo, "echo y > other.txt"), &parent)
        .await
        .unwrap();
    assert_eq!(quiet.changes, []);
}

#[tokio::test]
async fn a_failing_run_is_still_checked_and_a_timeout_too() {
    let fx = fixture();
    let mut job = fx.job(
        Program::Cargo,
        "echo '[core]' >> .git/config; sleep 300 & wait",
    );
    job.timeout = Duration::from_millis(400);
    let ran = run_jailed(job, &parent).await.unwrap();
    assert!(ran.done.ended.timed_out);
    assert_eq!(ran.changes.len(), 1);
    assert_eq!(ran.changes[0].kind, Kind::Changed);
    let _ = &fx.dir;
}
