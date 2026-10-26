use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use super::*;
use crate::runtime::proc::process_state;

/// A fake `llama-server` in a temporary folder: a script, the file where it
/// records its arguments and the log the server writes.
struct Lab {
    dir: tempfile::TempDir,
}

impl Lab {
    fn new() -> Self {
        let lab = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        fs::write(lab.model(), b"GGUF").unwrap();
        lab
    }

    /// The model file the server is given; it exists.
    fn model(&self) -> PathBuf {
        self.dir.path().join("q.gguf")
    }

    fn record(&self) -> PathBuf {
        self.dir.path().join("record")
    }

    fn log(&self) -> PathBuf {
        self.dir.path().join("state/llama-server.log")
    }

    /// The `PATH` the server is looked up in: the lab's folder, which the test
    /// process's own `PATH` never holds, then the system tools the fakes use.
    fn path(&self) -> OsString {
        let mut path = self.dir.path().as_os_str().to_owned();
        path.push(":/usr/bin:/bin");
        path
    }

    /// The fake: it records `$@` as one line, then runs `body`.
    fn program(&self, body: &str) {
        let path = self.dir.path().join("llama-server");
        let text = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n{body}\n",
            self.record().display()
        );
        fs::write(&path, text).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn managed(&self, body: &str, probe: Probe, wait: Duration) -> Managed {
        self.program(body);
        Managed::with(
            OsString::from("llama-server"),
            self.path(),
            self.model(),
            39281,
            Some(self.log()),
            probe,
            wait,
        )
    }

    fn starts(&self) -> Vec<String> {
        fs::read_to_string(self.record())
            .map(|text| text.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    }
}

/// Answers 200 once `ready` exists, like a server that has loaded its model.
fn answers_once(ready: &Path) -> Probe {
    let ready = ready.to_owned();
    Arc::new(move |_| {
        let up = ready.exists();
        Box::pin(async move { up.then_some(200) })
    })
}

fn answers(status: Option<u16>) -> Probe {
    Arc::new(move |_| Box::pin(async move { status }))
}

const WAIT: Duration = Duration::from_secs(5);

fn gone(pid: u32) -> bool {
    process_state(pid as libc::pid_t).is_none()
}

// Spec: A managed llama-server starts on the first turn.
#[tokio::test]
async fn the_first_ensure_starts_the_server_once_and_later_ones_reuse_it() {
    let lab = Lab::new();
    let managed = lab.managed("exec sleep 30", answers_once(&lab.record()), WAIT);
    assert_eq!(managed.ensure().await, Ok(()));
    assert_eq!(managed.ensure().await, Ok(()));
    assert_eq!(
        lab.starts(),
        vec![format!(
            "--host 127.0.0.1 --port 39281 -m {}",
            lab.model().display()
        )]
    );
    managed.shutdown().await;
}

// Spec S4 (llamacpp-pick-model): a server replaced by another model stops
// and never starts again, not even for a turn that still holds it.
#[tokio::test]
async fn a_retired_server_stops_and_refuses_to_start_again() {
    let lab = Lab::new();
    let managed = lab.managed("exec sleep 30", answers_once(&lab.record()), WAIT);
    assert_eq!(managed.ensure().await, Ok(()));
    let pid = managed.pid().await.expect("it runs");
    managed.retire().await;
    assert_eq!(managed.pid().await, None);
    assert!(gone(pid));
    assert_eq!(managed.ensure().await, Err(REPLACED.to_owned()));
    assert_eq!(managed.pid().await, None);
    assert_eq!(lab.starts().len(), 1);
}

// Spec: Quitting stops the managed llama-server.
#[tokio::test]
async fn shutdown_stops_the_server_and_its_children() {
    let lab = Lab::new();
    let child = lab.dir.path().join("child");
    let body = format!("sleep 30 & echo $! > '{}'\nwait", child.display());
    let managed = lab.managed(&body, answers_once(&lab.record()), WAIT);
    assert_eq!(managed.ensure().await, Ok(()));
    let pid = managed.pid().await.expect("running");
    let grandchild: u32 = loop {
        if let Ok(text) = fs::read_to_string(&child)
            && let Ok(pid) = text.trim().parse()
        {
            break pid;
        }
        sleep(Duration::from_millis(20)).await;
    };
    managed.shutdown().await;
    assert!(gone(pid), "the server is left");
    for _ in 0..100 {
        if gone(grandchild) {
            break;
        }
        sleep(Duration::from_millis(20)).await;
    }
    assert!(gone(grandchild), "its child is left");
    assert_eq!(managed.pid().await, None);
}

// Spec: A port that already answers is not ours.
#[tokio::test]
async fn a_port_that_already_answers_is_refused_and_nothing_starts() {
    for status in [200, 404, 503] {
        let lab = Lab::new();
        let managed = lab.managed("exec sleep 30", answers(Some(status)), WAIT);
        assert_eq!(
            managed.ensure().await,
            Err("llama-server not started: another program answers on 127.0.0.1:39281".into())
        );
        assert_eq!(lab.starts(), Vec::<String>::new(), "{status}");
    }
}

// Spec: A failed start says why.
#[tokio::test]
async fn a_server_that_exits_says_so_with_the_last_line_of_its_log() {
    let lab = Lab::new();
    let managed = lab.managed(
        "echo 'loading' ; echo 'error: failed to open /m/q.gguf' >&2 ; exit 1",
        answers(None),
        WAIT,
    );
    assert_eq!(
        managed.ensure().await,
        Err("llama-server stopped: error: failed to open /m/q.gguf".into())
    );
    assert_eq!(managed.pid().await, None);
    let mode = fs::metadata(lab.log()).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}

/// The pid a fake wrote to `path`, once it is there.
async fn child_of(path: &Path) -> u32 {
    wait_for_pid(path).await
}

/// Waits up to 2 s for `pid` to be gone.
async fn ends(pid: u32) -> bool {
    for _ in 0..100 {
        if gone(pid) {
            return true;
        }
        sleep(Duration::from_millis(20)).await;
    }
    false
}

// Spec: nothing lazarobox started outlives it (verifier A2): a server that
// ends during its start leaves nothing running in its group.
#[tokio::test]
async fn a_server_that_exits_during_its_start_leaves_no_child() {
    let lab = Lab::new();
    let child = lab.dir.path().join("child");
    let body = format!("sleep 30 & echo $! > '{}'\nexit 1", child.display());
    let managed = lab.managed(&body, answers(None), WAIT);
    assert!(
        managed
            .ensure()
            .await
            .is_err_and(|e| e.starts_with("llama-server stopped"))
    );
    assert!(ends(child_of(&child).await).await, "its child is left");
}

// Spec: the same for a server that ended after it answered.
#[tokio::test]
async fn a_server_that_ended_leaves_no_child_once_the_next_ensure_sees_it() {
    let lab = Lab::new();
    let child = lab.dir.path().join("child");
    let body = format!("sleep 30 & echo $! > '{}'\nwait", child.display());
    let managed = lab.managed(&body, answers_once(&lab.record()), WAIT);
    assert_eq!(managed.ensure().await, Ok(()));
    let pid = managed.pid().await.expect("running");
    let grandchild = child_of(&child).await;
    // SAFETY: a plain signal to the fake this test started (not its group).
    unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
    for _ in 0..100 {
        if process_state(pid as libc::pid_t) == Some('Z') {
            break;
        }
        sleep(Duration::from_millis(20)).await;
    }
    assert!(
        !gone(grandchild),
        "the child outlives its leader until seen"
    );
    fs::remove_file(lab.record()).unwrap();
    fs::remove_file(&child).unwrap();
    assert_eq!(managed.ensure().await, Ok(()));
    assert!(ends(grandchild).await, "its child is left");
    managed.shutdown().await;
}

// Spec: The log is private to the user, also one left by an earlier start.
#[tokio::test]
async fn an_existing_log_is_made_private_and_emptied() {
    let lab = Lab::new();
    fs::create_dir_all(lab.log().parent().unwrap()).unwrap();
    fs::write(lab.log(), "old run\n").unwrap();
    fs::set_permissions(lab.log(), fs::Permissions::from_mode(0o644)).unwrap();
    let managed = lab.managed("exec sleep 30", answers_once(&lab.record()), WAIT);
    assert_eq!(managed.ensure().await, Ok(()));
    let mode = fs::metadata(lab.log()).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    assert_eq!(fs::read_to_string(lab.log()).unwrap(), "");
    managed.shutdown().await;
}

// Spec: A failed start says why (no answer).
#[tokio::test]
async fn a_server_that_never_answers_is_stopped_after_the_wait() {
    let lab = Lab::new();
    let pid_file = lab.dir.path().join("pid");
    let body = format!("echo $$ > '{}'\nexec sleep 30", pid_file.display());
    let managed = lab.managed(&body, answers(None), Duration::from_secs(1));
    assert_eq!(
        managed.ensure().await,
        Err("llama-server did not answer within 1 s".into())
    );
    let pid: u32 = fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(gone(pid), "the server that never answered is left");
}

// Spec: A managed llama-server starts on the first turn (it ended).
#[tokio::test]
async fn a_server_that_ended_is_started_again_by_the_next_ensure() {
    let lab = Lab::new();
    let managed = lab.managed("exec sleep 30", answers_once(&lab.record()), WAIT);
    assert_eq!(managed.ensure().await, Ok(()));
    let pid = managed.pid().await.expect("running");
    // SAFETY: a plain signal to the fake this test started.
    unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
    for _ in 0..100 {
        if process_state(pid as libc::pid_t) == Some('Z') {
            break;
        }
        sleep(Duration::from_millis(20)).await;
    }
    // The fake's record makes the probe answer, which would read as another
    // program on the port: a silent port lets the second start happen.
    fs::remove_file(lab.record()).unwrap();
    assert_eq!(managed.ensure().await, Ok(()));
    assert_eq!(lab.starts().len(), 1, "started again");
    assert_ne!(managed.pid().await, Some(pid));
    managed.shutdown().await;
}

// Spec S9: the extra arguments come after lazarobox's own, one each.
#[tokio::test]
async fn the_extra_args_follow_the_ones_lazarobox_sets() {
    let lab = Lab::new();
    let argv = lab.dir.path().join("argv");
    let body = format!(
        "printf '%s\\n' \"$@\" > '{}'\nexec sleep 30",
        argv.display()
    );
    let managed = lab
        .managed(&body, answers_once(&lab.record()), WAIT)
        .with_args(vec![
            "-ngl".into(),
            "99".into(),
            "--alias".into(),
            "my model".into(),
        ]);
    assert_eq!(managed.ensure().await, Ok(()));
    managed.shutdown().await;
    let model = lab.model().display().to_string();
    assert_eq!(
        fs::read_to_string(argv)
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        [
            "--host",
            "127.0.0.1",
            "--port",
            "39281",
            "-m",
            &model,
            "-ngl",
            "99",
            "--alias",
            "my model"
        ]
    );
}

// Spec S10: the model file is checked before each start.
#[tokio::test]
async fn a_missing_model_file_starts_nothing_and_says_so() {
    let lab = Lab::new();
    let managed = lab.managed("exec sleep 30", answers(None), WAIT);
    fs::remove_file(lab.model()).unwrap();
    assert_eq!(
        managed.ensure().await,
        Err(format!(
            "llama-server not started: model file not found: {}",
            lab.model().display()
        ))
    );
    assert_eq!(lab.starts(), Vec::<String>::new());
    // Back in place (a drive mounted later): the next turn starts it.
    fs::write(lab.model(), b"GGUF").unwrap();
    let managed = lab.managed("exec sleep 30", answers_once(&lab.record()), WAIT);
    assert_eq!(managed.ensure().await, Ok(()));
    managed.shutdown().await;
}

// Spec: A failed start says why (no binary).
#[tokio::test]
async fn a_missing_program_says_it_is_not_on_the_path() {
    let lab = Lab::new();
    let managed = Managed::with(
        OsString::from("llama-server"),
        lab.path(),
        PathBuf::from("/m/q.gguf"),
        39281,
        None,
        answers(None),
        WAIT,
    );
    assert_eq!(
        managed.ensure().await,
        Err("could not start llama-server: is it on the PATH?".into())
    );
}

// Spec: llama-server is found on the PATH lazarobox gives its tools (the login
// PATH), not on the one it was started with, and keeps it.
#[tokio::test]
async fn the_server_is_found_on_the_given_path_and_runs_with_it() {
    let lab = Lab::new();
    let seen = lab.dir.path().join("seen");
    let body = format!(
        "printf '%s' \"$PATH\" > '{}'\nexec sleep 30",
        seen.display()
    );
    let managed = lab.managed(&body, answers_once(&seen), WAIT);
    assert_eq!(managed.ensure().await, Ok(()));
    assert_eq!(
        fs::read_to_string(&seen).unwrap(),
        lab.path().to_str().unwrap()
    );
    managed.shutdown().await;
}

// Spec: The managed llama-server never outlives lazarobox. The kernel's
// parent-death signal fires when the thread that started it ends, which a
// thread of its own shows without killing the test process.
#[cfg(target_os = "linux")]
#[test]
fn a_server_dies_with_the_thread_that_started_it() {
    let lab = Lab::new();
    let pid_file = lab.dir.path().join("pid");
    let body = format!("echo $$ > '{}'\nexec sleep 30", pid_file.display());
    let managed = Arc::new(lab.managed(&body, answers_once(&lab.record()), WAIT));
    let starter = Arc::clone(&managed);
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        assert_eq!(runtime.block_on(starter.ensure()), Ok(()));
    })
    .join()
    .unwrap();
    let pid: libc::pid_t = fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    // Killed but not reaped (its parent is this process) reads as a zombie.
    let dead = || matches!(process_state(pid), None | Some('Z'));
    for _ in 0..100 {
        if dead() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(dead(), "the server outlived the thread that started it");
}

// Races found by the independent verifier (llamacpp-pick-model S4).

fn read_pid(path: &Path) -> Option<u32> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

async fn wait_for_pid(path: &Path) -> u32 {
    for _ in 0..100 {
        if let Some(pid) = read_pid(path) {
            return pid;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the fake never started");
}

// A turn's start that holds the lock while retire comes in: the server it
// started is the one retire stops, and nothing starts afterwards.
#[tokio::test]
async fn retire_during_an_in_flight_start_leaves_nothing_running() {
    let lab = Lab::new();
    let pidfile = lab.dir.path().join("pid");
    let ready = lab.dir.path().join("ready");
    let body = format!(
        "echo $$ > '{}'\nsleep 1\ntouch '{}'\nexec sleep 30",
        pidfile.display(),
        ready.display()
    );
    let managed = Arc::new(lab.managed(&body, answers_once(&ready), WAIT));
    let starting = tokio::spawn({
        let managed = Arc::clone(&managed);
        async move { managed.ensure().await }
    });
    let pid = wait_for_pid(&pidfile).await;
    managed.retire().await;
    let _ = starting.await.unwrap();
    assert_eq!(managed.pid().await, None);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(dead(pid), "the in-flight server was left running");
    assert_eq!(managed.ensure().await, Err(REPLACED.to_owned()));
    assert_eq!(lab.starts().len(), 1);
}

// Quit while the start a save asked for is in flight: the bounded shutdown
// cannot take the lock, the runtime then drops the task; nothing survives.
#[tokio::test]
async fn quit_while_a_start_is_in_flight_leaves_nothing_running() {
    let lab = Lab::new();
    let pidfile = lab.dir.path().join("pid");
    let ready = lab.dir.path().join("never");
    let body = format!("echo $$ > '{}'\nexec sleep 30", pidfile.display());
    let managed = Arc::new(lab.managed(&body, answers_once(&ready), Duration::from_secs(60)));
    let live = super::super::LiveServers::default();
    assert!(live.manage(Arc::clone(&managed)).is_none());
    let start = tokio::spawn({
        let managed = Arc::clone(&managed);
        async move { managed.ensure().await }
    });
    let pid = wait_for_pid(&pidfile).await;
    // Chats::shutdown does this, bounded.
    let current = live.managed().unwrap();
    let _ = tokio::time::timeout(Duration::from_millis(300), current.shutdown()).await;
    // The runtime drop cancels the in-flight task.
    start.abort();
    let _ = start.await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(dead(pid), "an in-flight start survived the quit");
}

// Quit right after a save swapped the server but before the replacing task
// retired the old one: dropping that task (runtime drop) still stops it.
#[tokio::test]
async fn quit_before_the_old_server_is_retired_leaves_nothing_running() {
    let lab = Lab::new();
    let pidfile = lab.dir.path().join("pid");
    let ready = lab.dir.path().join("ready");
    let body = format!(
        "echo $$ > '{}'\ntouch '{}'\nexec sleep 30",
        pidfile.display(),
        ready.display()
    );
    let old = Arc::new(lab.managed(&body, answers_once(&ready), WAIT));
    assert_eq!(old.ensure().await, Ok(()));
    let pid = old.pid().await.unwrap();
    let live = super::super::LiveServers::new(Default::default(), Some(Arc::clone(&old)));
    drop(old);
    let lab2 = Lab::new();
    let new = Arc::new(lab2.managed("exec sleep 30", answers(None), WAIT));
    let replaced = live.manage(Arc::clone(&new)).unwrap();
    // The task the manager spawns, never polled before the quit.
    let task = tokio::spawn(async move {
        replaced.retire().await;
        let _ = new.ensure().await;
    });
    task.abort();
    let _ = task.await;
    let _ = tokio::time::timeout(
        Duration::from_millis(300),
        live.managed().unwrap().shutdown(),
    )
    .await;
    drop(live);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(dead(pid), "the replaced server was left running untracked");
}

/// Gone or a zombie waiting to be reaped: either way no longer running.
fn dead(pid: u32) -> bool {
    matches!(process_state(pid as libc::pid_t), None | Some('Z'))
}
