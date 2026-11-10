//! The worker clock (agent-orchestrator "Waiting on the user stops the worker
//! clock", design D5, owner decision 13): a worker's deadline stops while one
//! of its own asks waits for the user, and only its own; the run wall clock
//! keeps counting (owner, 2026-11-10).
//!
//! These drive real sockets, so real short deadlines and never a paused
//! clock; every wait is bounded so a clock that never resumes fails fast.

use super::*;

/// A worker deadline short enough to be passed by the waits below.
const DEADLINE: Duration = Duration::from_millis(1500);

/// The deadline of a writer, whose apply after the answer counts.
#[cfg(target_os = "linux")]
const WRITER_DEADLINE: Duration = Duration::from_millis(2500);

/// The longest any of these tests may take: a hang fails here.
const BOUND: Duration = Duration::from_secs(20);

fn session_of(run: RunId, worker: u32) -> SessionKey {
    SessionKey::Worker(
        pane(),
        WorkerKey {
            run,
            worker: WorkerId(worker),
        },
    )
}

/// The worker's own count of asks waiting for the user (`Route.own`).
fn own_count(lab: &Lab, run: RunId, worker: u32) -> usize {
    lab.manager
        .on_run(pane(), run, |live| {
            live.routes[&WorkerId(worker)].own.count()
        })
        .unwrap()
}

fn status(lab: &Lab, worker: u32) -> Option<WorkerStatus> {
    lock(&lab.manager.runs)[&pane()]
        .run
        .status(WorkerId(worker))
}

/// A run of review workers on codex, `parallel` at once, each with
/// `deadline`, whose asks wait in `waits` under turn 7 on a channel the test
/// owns.
fn reviewers(
    lab: &Lab,
    waits: &Arc<Waits>,
    parallel: usize,
    deadline: Duration,
    wall: Duration,
) -> (RunId, OrchHandle, mpsc::Receiver<StreamEvent>) {
    let pending = Pending::new();
    let (tx, rx) = mpsc::channel(TURN_CHANNEL);
    let scope = OrchScope {
        approver: waits.approver(pane(), 7, pending.clone()),
        pending,
        events: tx,
        deadline,
        limits: RunLimits {
            parallel,
            wall,
            ..RunLimits::default()
        },
        ..lab.scope(codex_review(), &[])
    };
    let (run, handle) = lab.manager.start(pane(), scope).unwrap();
    (run, handle, rx)
}

/// A lab whose HOME holds the note a reviewer's read asks for.
fn reading_lab() -> Lab {
    let lab = lab_with(Script::Hang, tooled);
    std::fs::write(lab.home.path().join("notes.txt"), "TOP-SECRET").unwrap();
    lab
}

/// Waits until worker `worker` ends, or `BOUND` passes.
async fn ended(lab: &Lab, worker: u32) -> WorkerStatus {
    let until = Instant::now() + BOUND;
    while Instant::now() < until {
        match status(lab, worker) {
            Some(WorkerStatus::Queued | WorkerStatus::Running) | None => {}
            Some(other) => return other,
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("worker {worker} did not end within {BOUND:?}");
}

// Spec "Work time still counts", GUARD (15.3): the clock resumes after the
// answer. A worker that asks once, is answered after 1 s and then works on
// past its 1.5 s deadline still ends `timed_out`, and no sooner than 2.5 s:
// the wait was not counted, the work was. A clock that never resumes after a
// wait hangs here and fails at `BOUND`.
#[tokio::test]
async fn a_worker_that_works_past_its_deadline_after_an_answer_times_out() {
    let lab = reading_lab();
    let waits = Waits::new();
    let (run, handle, mut turn) = reviewers(&lab, &waits, 1, DEADLINE, Duration::MAX);
    let started = Instant::now();
    handle.call(spawn(Task::Review, "check")).await;
    let mut client = worker_client(&lab.seen).await;
    let id = raised(&mut client, &mut turn, 1).await;
    tokio::time::sleep(Duration::from_millis(1000)).await;
    assert!(waits.decide(session_of(run, 1), 7, id, Decision::Denied("no")));
    assert_eq!(ended(&lab, 1).await, WorkerStatus::TimedOut);
    let took = started.elapsed();
    assert!(took >= Duration::from_millis(2400), "{took:?}");
}

// D5, 15.4: two workers ask at once; the app shows one and queues the other
// (`approval.rs`). Both clocks stop at the ask, before any answer: past the
// 1.5 s deadline both still run. Each count is the worker's own: worker 2's
// stays at zero while only worker 1 waits.
#[tokio::test]
async fn a_queued_ask_stops_its_workers_clock_and_only_its_own() {
    let lab = reading_lab();
    let waits = Waits::new();
    let (run, handle, mut turn) = reviewers(&lab, &waits, 2, DEADLINE, Duration::MAX);
    handle.call(spawn(Task::Review, "one")).await;
    handle.call(spawn(Task::Review, "two")).await;
    let (mut one, mut two) = (worker_client(&lab.seen).await, {
        let request = lab.seen.nth(1).await;
        let Tools::On {
            mcp: Some(wire), ..
        } = &request.tools
        else {
            panic!("{:?}", request.tools);
        };
        ready(wire).await
    });
    let first = raised(&mut one, &mut turn, 1).await;
    assert_eq!(own_count(&lab, run, 1), 1);
    assert_eq!(own_count(&lab, run, 2), 0, "a sibling's ask is not its own");
    let second = raised(&mut two, &mut turn, 1).await;
    assert_eq!(own_count(&lab, run, 2), 1, "queued, and already counted");
    tokio::time::sleep(DEADLINE + Duration::from_millis(500)).await;
    assert_eq!(status(&lab, 1), Some(WorkerStatus::Running));
    assert_eq!(status(&lab, 2), Some(WorkerStatus::Running));
    assert!(waits.decide(session_of(run, 1), 7, first, Decision::Denied("no")));
    assert!(waits.decide(session_of(run, 2), 7, second, Decision::Denied("no")));
    // The answer reaches each call on its own task.
    let until = Instant::now() + BOUND;
    while own_count(&lab, run, 1) + own_count(&lab, run, 2) > 0 {
        assert!(Instant::now() < until, "the counts stay up");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// D5, 15.4 (route moved): a worker that outlived its orchestrator turn asks
// under the next one (`retarget`); its own clock stops exactly as under the
// turn that spawned it.
#[tokio::test]
async fn an_ask_after_the_route_moved_stops_the_workers_clock() {
    let lab = reading_lab();
    let waits = Waits::new();
    let (run, handle, _old) = reviewers(&lab, &waits, 1, DEADLINE, Duration::MAX);
    handle.call(spawn(Task::Review, "check")).await;
    let mut client = worker_client(&lab.seen).await;
    let (scope, mut next, _) = turn_scope(&lab, &waits, 8);
    lab.manager.attach(pane(), run, scope).unwrap();
    let id = raised(&mut client, &mut next, 1).await;
    assert_eq!(own_count(&lab, run, 1), 1);
    tokio::time::sleep(DEADLINE + Duration::from_millis(500)).await;
    assert_eq!(status(&lab, 1), Some(WorkerStatus::Running));
    assert!(waits.decide(session_of(run, 1), 8, id, Decision::Denied("no")));
}

// Spec "The run wall clock still counts" (D5, owner item 4), GUARD 15.6: a
// run whose wall clock passes while its worker waits for the user ends
// `Expired` as today; the wait does not hold it.
#[tokio::test]
async fn the_wall_clock_ends_the_run_while_a_worker_waits() {
    let mut lab = reading_lab();
    let waits = Waits::new();
    let wall = Duration::from_millis(800);
    let (run, handle, mut turn) = reviewers(&lab, &waits, 1, TEN_MINUTES, wall);
    handle.call(spawn(Task::Review, "check")).await;
    let mut client = worker_client(&lab.seen).await;
    let id = raised(&mut client, &mut turn, 1).await;
    assert_eq!(own_count(&lab, run, 1), 1, "the worker waits");
    let expired = timeout(BOUND, async {
        loop {
            if let OrchestraEvent::Expired { run: of, after } = lab.untimed().await {
                return (of, after);
            }
        }
    })
    .await
    .expect("the run expires");
    assert_eq!(expired, (run, wall));
    assert!(
        !waits.decide(session_of(run, 1), 7, id, Decision::Sent),
        "the ask ended with the run"
    );
}

// Spec "A long review does not time out the worker" and "Review time does
// not count against the deadline" (15.2): a `code` worker with a 2.5 s
// deadline whose batch waits 4 s for the user, then is approved, is not
// `timed_out` and its turn goes on to its answer. The deadline is longer
// than the others' because applying the batch is work time and its fsyncs
// took about 1.2 s here.
// Needs the write primitives (`openat2`, `renameat2`): Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_long_review_does_not_time_out_the_worker() {
    let mut lab = writing_lab(Script::Held("done"));
    let waits = Waits::new();
    let (run, handle) = lab.start_writing_run(&waits, WRITER_DEADLINE);
    handle.call(spawn(Task::Code, "write a")).await;
    let mut client = worker_client(&lab.seen).await;
    let call = tool_call(
        1,
        "write_file",
        json!({"path": "a.txt", "content": "one\n"}),
    );
    exchange(&mut client, call).await;
    let id = propose_and_ask(&mut lab, &mut client).await;
    assert_eq!(own_count(&lab, run, 1), 1);
    tokio::time::sleep(WRITER_DEADLINE + Duration::from_millis(1500)).await;
    assert_eq!(
        status(&lab, 1),
        Some(WorkerStatus::Running),
        "not timed out"
    );
    assert!(waits.decide(session_of(run, 1), 7, id, Decision::Sent));
    let mut reply = String::new();
    let read = timeout(Duration::from_secs(5), client.0.read_line(&mut reply));
    read.await.unwrap().unwrap();
    assert!(lab.app().join("a.txt").exists(), "{reply}");
    lab.seen.release.notify_one();
    let waited = timeout(BOUND, handle.call(wait(1))).await.unwrap();
    assert_eq!(waited, fine("1: done"), "{:?}", lab.sent());
}

/// Sends `propose_batch` and returns the id of the batch ask it raised.
#[cfg(target_os = "linux")]
async fn propose_and_ask(lab: &mut Lab, client: &mut Client) -> CallId {
    let propose = format!("{}\n", tool_call(2, "propose_batch", json!({})));
    client.1.write_all(propose.as_bytes()).await.unwrap();
    loop {
        let event = timeout(Duration::from_secs(5), lab.turn.1.recv()).await;
        if let StreamEvent::ToolCall {
            id,
            approval: Some(ask),
            ..
        } = event.unwrap().unwrap()
        {
            assert!(matches!(*ask, ApprovalAsk::Batch(_)), "{ask:?}");
            return id;
        }
    }
}

// D5, 15.7: the clock pauses, but a CLI still cuts one MCP call at its own
// cap (900 s, `tools/wiring.rs`). A real CLI then hands the model a tool
// error and goes on; this worker's engine (the fake) ends its turn failing
// once the call is cut: its row shows `failed`, the run halts with the text,
// and its staged batch is an orphan with its badge, still reviewable;
// nothing is written.
// Needs the write primitives (`openat2`, `renameat2`): Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_worker_cut_at_the_cli_cap_leaves_its_batch_as_an_orphan() {
    use crate::core::ai::chat::review::Badge;
    const CUT: &str = "MCP tool propose_batch timed out";
    let mut lab = writing_lab(Script::HeldThenFail(CUT));
    let handle = lab.start_writing(&Waits::new(), TEN_MINUTES);
    handle.call(spawn(Task::Code, "write a")).await;
    let mut client = worker_client(&lab.seen).await;
    let call = tool_call(
        1,
        "write_file",
        json!({"path": "a.txt", "content": "one\n"}),
    );
    exchange(&mut client, call).await;
    propose_and_ask(&mut lab, &mut client).await;
    lab.seen.release.notify_one();
    let waited = timeout(BOUND, handle.call(wait(1))).await.unwrap();
    assert_eq!(waited, fine("1: awaiting_user"));
    let events = lab.sent();
    let statuses: Vec<_> = events.iter().filter_map(status_of).collect();
    assert_eq!(statuses.last(), Some(&WorkerStatus::Failed), "{events:?}");
    let halted = events.iter().find_map(|e| match e {
        OrchestraEvent::Halted { text, .. } => Some(text.as_str()),
        _ => None,
    });
    assert_eq!(halted, Some(CUT), "{events:?}");
    let host = lab.host();
    let badges = host.badges(|_| false).unwrap();
    assert_eq!(badges.get(&pane()), Some(&Badge::Orphan(1)));
    let (_, review) = host
        .review_batch(pane(), false)
        .expect("an orphan to review");
    assert_eq!(review.counts(), "1 file, +1 -0");
    assert!(!lab.app().join("a.txt").exists());
}
