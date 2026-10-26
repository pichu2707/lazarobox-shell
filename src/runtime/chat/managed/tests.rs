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
        // Never the real `/slots`: a lazarobox of the user may run on the port.
        .with_slots(slots(None))
    }

    fn starts(&self) -> Vec<String> {
        fs::read_to_string(self.record())
            .map(|text| text.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    }
}

/// Ready once `ready` exists, like a server that has loaded its model; silent
/// before. Every port reads the same.
fn answers_once(ready: &Path) -> Probe {
    let ready = ready.to_owned();
    Arc::new(move |_| {
        let up = ready.exists();
        Box::pin(async move { if up { Seen::Ready(None) } else { Seen::Silent } })
    })
}

/// `seen` on every port.
fn answers(seen: Seen) -> Probe {
    Arc::new(move |_| {
        let seen = seen.clone();
        Box::pin(async move { seen })
    })
}

/// `answering` requests on every port.
fn slots(answering: Option<usize>) -> Slots {
    Arc::new(move |_| Box::pin(async move { answering }))
}

/// What `on` says each port shows, read when it is probed.
fn by_port(on: impl Fn(u16) -> Seen + Send + Sync + 'static) -> Probe {
    Arc::new(move |port| {
        let seen = on(port);
        Box::pin(async move { seen })
    })
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

// Spec S12 (managed-llama-server-followups): a llama-server of this user
// with the same model, from another lazarobox or started by hand, is used as
// it is: nothing starts, and it is never ours to stop.
#[tokio::test]
async fn a_running_server_with_the_same_model_is_used_and_nothing_starts() {
    let lab = Lab::new();
    let model = lab.model();
    let managed = Arc::new(lab.managed(
        "exec sleep 30",
        by_port(move |port| match port {
            39281 => Seen::Ready(Some(model.clone())),
            _ => Seen::Silent,
        }),
        WAIT,
    ));
    drop(managed.lease().await.expect("used"));
    assert_eq!(managed.port(), 39281);
    assert_eq!(lab.starts(), Vec::<String>::new());
    assert_eq!(managed.pid().await, None);
    let later = Instant::now() + IDLE * 2;
    assert!(!managed.stop_if_idle(IDLE, later).await, "not ours to stop");
}

// Spec S12: "si no es bloqueante para usar otros modelos": another model on
// the port gets ours started on the next free one.
#[tokio::test]
async fn another_model_on_the_port_starts_ours_on_the_next_free_one() {
    let lab = Lab::new();
    let record = lab.record();
    let managed = lab.managed(
        "exec sleep 30",
        by_port(move |port| match port {
            39281 => Seen::Ready(Some(PathBuf::from("/m/other.gguf"))),
            39282 if record.exists() => Seen::Ready(None),
            _ => Seen::Silent,
        }),
        WAIT,
    );
    assert_eq!(managed.ensure().await, Ok(()));
    assert_eq!(managed.port(), 39282);
    assert_eq!(
        lab.starts(),
        vec![format!(
            "--host 127.0.0.1 --port 39282 -m {}",
            lab.model().display()
        )]
    );
    managed.shutdown().await;
}

// Spec S12: detection probes the port the managed server runs on.
#[tokio::test]
async fn detection_reads_the_port_the_server_runs_on() {
    let lab = Lab::new();
    let record = lab.record();
    let managed = Arc::new(lab.managed(
        "exec sleep 30",
        by_port(move |port| match port {
            39281 => Seen::Ready(Some(PathBuf::from("/m/other.gguf"))),
            39282 if record.exists() => Seen::Ready(None),
            _ => Seen::Silent,
        }),
        WAIT,
    ));
    let ports = crate::core::ai::ServerPorts::default().with("llamacpp", 39281);
    let live = crate::runtime::chat::LiveServers::new(ports, Some(Arc::clone(&managed)));
    assert_eq!(live.ports().port("llamacpp"), Some(39281));
    assert_eq!(managed.ensure().await, Ok(()));
    assert_eq!(live.ports().port("llamacpp"), Some(39282));
    managed.shutdown().await;
}

// Spec S12: the same model is found past a port that is busy or free.
#[tokio::test]
async fn the_same_model_is_found_past_other_ports() {
    let lab = Lab::new();
    let model = lab.model();
    let managed = lab.managed(
        "exec sleep 30",
        by_port(move |port| match port {
            39281 => Seen::Busy,
            39283 => Seen::Ready(Some(model.clone())),
            _ => Seen::Silent,
        }),
        WAIT,
    );
    assert_eq!(managed.ensure().await, Ok(()));
    assert_eq!(managed.port(), 39283);
    assert_eq!(lab.starts(), Vec::<String>::new());
}

// Spec S12: a server lazarobox cannot tell is its model (another program, a
// server still loading, one of another user, another model) is never used,
// and with every port taken nothing starts.
#[tokio::test]
async fn with_every_port_taken_nothing_starts() {
    for seen in [
        Seen::Busy,
        Seen::Ready(None),
        Seen::Ready(Some(PathBuf::from("/m/other.gguf"))),
    ] {
        let lab = Lab::new();
        let managed = lab.managed("exec sleep 30", answers(seen.clone()), WAIT);
        assert_eq!(
            managed.ensure().await,
            Err("llama-server not started: no free port on 127.0.0.1 from 39281 to 39288".into()),
            "{seen:?}"
        );
        assert_eq!(lab.starts(), Vec::<String>::new(), "{seen:?}");
    }
}

// Spec S12: when the server it used is gone, the next turn starts its own.
#[tokio::test]
async fn a_used_server_that_went_away_is_replaced_by_ours() {
    let lab = Lab::new();
    let (model, shared, record) = (lab.model(), lab.dir.path().join("shared"), lab.record());
    fs::write(&shared, "").unwrap();
    let flag = shared.clone();
    let managed = lab.managed(
        "exec sleep 30",
        by_port(move |port| match port {
            39281 if flag.exists() => Seen::Ready(Some(model.clone())),
            39281 if record.exists() => Seen::Ready(None),
            _ => Seen::Silent,
        }),
        WAIT,
    );
    assert_eq!(managed.ensure().await, Ok(()));
    assert_eq!(managed.pid().await, None);
    fs::remove_file(&shared).unwrap();
    assert_eq!(managed.ensure().await, Ok(()));
    assert_eq!(managed.port(), 39281);
    assert_eq!(lab.starts().len(), 1);
    assert!(managed.pid().await.is_some(), "ours now");
    managed.shutdown().await;
}

// Spec S12: two lazarobox starting the same model at once: the one that
// loses the port uses the other's server.
#[tokio::test]
async fn a_start_that_lost_the_port_to_the_same_model_uses_that_server() {
    let lab = Lab::new();
    let (model, record) = (lab.model(), lab.record());
    let managed = lab.managed(
        "exit 1",
        by_port(move |port| match port {
            39281 if record.exists() => Seen::Ready(Some(model.clone())),
            _ => Seen::Silent,
        }),
        WAIT,
    );
    assert_eq!(managed.ensure().await, Ok(()));
    assert_eq!(managed.port(), 39281);
    assert_eq!(managed.pid().await, None);
    assert_eq!(lab.starts().len(), 1);
}

// Spec S12: the model is the same file, whatever path names it.
#[test]
fn the_same_file_through_a_link_is_the_same_model() {
    let lab = Lab::new();
    fs::write(lab.model(), b"GGUF").unwrap();
    let link = lab.dir.path().join("link.gguf");
    std::os::unix::fs::symlink(lab.model(), &link).unwrap();
    assert!(same_model(&lab.model(), &link));
    assert!(!same_model(&lab.model(), Path::new("/m/other.gguf")));
    assert!(same_model(
        Path::new("/m/none.gguf"),
        Path::new("/m/none.gguf")
    ));
}

// Spec S12: the model a llama-server serves is the `model_path` of `/props`.
#[test]
fn the_served_model_is_read_from_props() {
    assert_eq!(
        served_model(br#"{"model_path":"/m/q.gguf","model_alias":"mine"}"#),
        Some(PathBuf::from("/m/q.gguf"))
    );
    assert_eq!(served_model(br#"{"total_slots":1}"#), None);
    assert_eq!(served_model(b"not json"), None);
}

// Spec: A failed start says why.
#[tokio::test]
async fn a_server_that_exits_says_so_with_the_last_line_of_its_log() {
    let lab = Lab::new();
    let managed = lab.managed(
        "echo 'loading' ; echo 'error: failed to open /m/q.gguf' >&2 ; exit 1",
        answers(Seen::Silent),
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
    let managed = lab.managed(&body, answers(Seen::Silent), WAIT);
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
    let managed = lab.managed(&body, answers(Seen::Silent), Duration::from_secs(1));
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

const IDLE: Duration = Duration::from_secs(15 * 60);

/// A running managed server, shared like the runtime shares it.
async fn idle_lab() -> (Lab, Arc<Managed>, u32) {
    let lab = Lab::new();
    let managed = Arc::new(lab.managed("exec sleep 30", answers_once(&lab.record()), WAIT));
    drop(managed.lease().await.expect("starts"));
    let pid = managed.pid().await.expect("running");
    (lab, managed, pid)
}

// Spec S8: a server nobody used for the idle time stops; the next turn
// starts it again.
#[tokio::test]
async fn a_server_unused_for_the_idle_time_stops_and_the_next_turn_restarts_it() {
    let (lab, managed, pid) = idle_lab().await;
    let soon = std::time::Instant::now() + IDLE - Duration::from_secs(60);
    assert!(!managed.stop_if_idle(IDLE, soon).await, "stopped too early");
    assert_eq!(managed.pid().await, Some(pid));
    let later = std::time::Instant::now() + IDLE;
    assert!(managed.stop_if_idle(IDLE, later).await);
    assert_eq!(managed.pid().await, None);
    assert!(gone(pid), "the server is left");
    // Its record made the probe answer: silent again, like a stopped server.
    fs::remove_file(lab.record()).unwrap();
    let lease = managed.lease().await.expect("starts again");
    assert_ne!(managed.pid().await, Some(pid));
    drop(lease);
    managed.shutdown().await;
}

// Spec S8: a turn in progress (a long answer) is never cut by the idle stop,
// and the idle time counts from its end.
#[tokio::test]
async fn a_turn_in_progress_keeps_the_server_and_the_idle_time_counts_from_its_end() {
    let (_lab, managed, pid) = idle_lab().await;
    let lease = managed.lease().await.expect("running");
    let later = std::time::Instant::now() + IDLE;
    assert!(!managed.stop_if_idle(IDLE, later).await, "stopped mid-turn");
    assert_eq!(managed.pid().await, Some(pid));
    drop(lease);
    assert!(!managed.stop_if_idle(IDLE, std::time::Instant::now()).await);
    assert!(
        managed
            .stop_if_idle(IDLE, std::time::Instant::now() + IDLE)
            .await
    );
    assert!(gone(pid));
}

// Spec S8: the idle time the runtime checks with is the one set last (the
// menu changes it while running); none never stops the server.
#[tokio::test]
async fn the_live_idle_time_decides_and_none_never_stops() {
    let (_lab, managed, pid) = idle_lab().await;
    let live = super::super::LiveServers::new(Default::default(), Some(Arc::clone(&managed)));
    let later = std::time::Instant::now() + IDLE;
    assert!(!live.stop_if_idle(later).await, "no idle time: never");
    live.set_idle(Some(Duration::from_secs(30 * 60)));
    assert!(!live.stop_if_idle(later).await, "30 min not over");
    live.set_idle(Some(IDLE));
    assert!(live.stop_if_idle(later).await);
    assert!(gone(pid));
}

#[tokio::test]
async fn nothing_running_has_nothing_to_stop() {
    let lab = Lab::new();
    let managed = lab.managed("exec sleep 30", answers(Seen::Silent), WAIT);
    let later = std::time::Instant::now() + IDLE;
    assert!(!managed.stop_if_idle(IDLE, later).await);
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
    let managed = lab.managed("exec sleep 30", answers(Seen::Silent), WAIT);
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
        answers(Seen::Silent),
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
    let new = Arc::new(lab2.managed("exec sleep 30", answers(Seen::Silent), WAIT));
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

/// A started server whose `/slots` says it answers `answering` requests.
async fn shared_lab(answering: Option<usize>) -> (Lab, Arc<Managed>) {
    let lab = Lab::new();
    let managed = Arc::new(
        lab.managed("exec sleep 30", answers_once(&lab.record()), WAIT)
            .with_slots(slots(answering)),
    );
    drop(managed.lease().await.expect("starts"));
    (lab, managed)
}

// Spec S13 (managed-llama-server-followups): "avisarle cuando vaya a cerrar":
// our server answering more requests than our turns is answering another
// lazarobox.
#[tokio::test]
async fn our_server_answering_beyond_our_turns_answers_another_lazarobox() {
    let (_lab, managed) = shared_lab(Some(1)).await;
    assert!(managed.answering_others().await);
    // Our own turn takes that slot.
    let lease = managed.lease().await.expect("runs");
    assert!(!managed.answering_others().await);
    drop(lease);
    managed.shutdown().await;
}

#[tokio::test]
async fn a_server_that_answers_nothing_or_does_not_say_answers_nobody_else() {
    for answering in [Some(0), None] {
        let (_lab, managed) = shared_lab(answering).await;
        assert!(!managed.answering_others().await, "{answering:?}");
        managed.shutdown().await;
    }
}

// Spec S13: a server we only use is not ours to warn about.
#[tokio::test]
async fn a_server_we_only_use_is_never_reported() {
    let lab = Lab::new();
    let model = lab.model();
    let managed = lab
        .managed(
            "exec sleep 30",
            by_port(move |_| Seen::Ready(Some(model.clone()))),
            WAIT,
        )
        .with_slots(slots(Some(3)));
    assert_eq!(managed.ensure().await, Ok(()));
    assert!(!managed.answering_others().await);
}

// Spec S13, derived from S8 ("a reply in progress is never cut"): the idle
// stop waits while another lazarobox is answered, and the idle time counts
// again from then.
#[tokio::test]
async fn the_idle_stop_waits_while_another_lazarobox_is_answered() {
    let lab = Lab::new();
    let answering = Arc::new(AtomicUsize::new(1));
    let probe: Slots = {
        let answering = Arc::clone(&answering);
        Arc::new(move |_| {
            let now = answering.load(Ordering::SeqCst);
            Box::pin(async move { Some(now) })
        })
    };
    let managed = Arc::new(
        lab.managed("exec sleep 30", answers_once(&lab.record()), WAIT)
            .with_slots(probe),
    );
    drop(managed.lease().await.expect("starts"));
    let pid = managed.pid().await.expect("running");
    assert!(!managed.stop_if_idle(IDLE, Instant::now() + IDLE * 2).await);
    assert_eq!(managed.pid().await, Some(pid));
    answering.store(0, Ordering::SeqCst);
    assert!(!managed.stop_if_idle(IDLE, Instant::now() + IDLE / 2).await);
    assert!(managed.stop_if_idle(IDLE, Instant::now() + IDLE * 2).await);
    assert!(ends(pid).await, "the server is left");
}

// Spec S13: the slots that are processing, from `/slots`.
#[test]
fn processing_slots_are_counted() {
    let body = br#"[{"id":0,"is_processing":true},{"id":1,"is_processing":false},{"id":2}]"#;
    assert_eq!(processing(body), Some(1));
    assert_eq!(processing(b"{}"), None);
}
