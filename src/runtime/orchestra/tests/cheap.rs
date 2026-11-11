//! The mandatory review of a cheap author (agent-orchestrator "Mandatory
//! review for a cheap author, optional for a strong one" [W7], design D20,
//! D21, D6): a local author's staged change is reviewed by the Review row
//! with no consent ask, its clock stops for the review, and a reviewer that
//! cannot run stops the run visibly.

use super::*;
#[cfg(target_os = "linux")]
use crate::core::ai::chat::review::BatchReview;

/// `code` on Ollama (a cheap author), `review` on `review`, `orchestrate` on
/// claude; claude, codex and Ollama are found.
fn local_code(review: (&str, &str)) -> Roster {
    let assigned = [
        (Task::Orchestrate, "claude", "opus"),
        (Task::Code, "ollama", "llama3:8b"),
        (Task::Review, review.0, review.1),
    ];
    let engines = vec![
        found("claude", &[]),
        found("codex", &[]),
        found("ollama", &["llama3:8b"]),
    ];
    roster(&assigned, engines, JAIL)
}

// Spec "Menu change after spawn" (17.3): the run manager tells a cheap
// author by the spec it was spawned with. A menu change later does not make
// it strong; an unknown worker or another run is never cheap.
#[tokio::test]
async fn the_run_manager_tells_a_cheap_author_by_its_spec_as_spawned() {
    let lab = lab(Script::Hang);
    let limits = RunLimits {
        parallel: 2,
        ..RunLimits::default()
    };
    let scope = OrchScope {
        limits,
        ..lab.scope(local_code(("codex", "default")), &[])
    };
    let (run, handle) = lab.begin(scope).unwrap();
    handle.call(spawn(Task::Code, "write a")).await;
    handle.call(spawn(Task::Review, "look")).await;
    assert!(handle.cheap(run, WorkerId(1)));
    assert!(!handle.cheap(run, WorkerId(2)));
    lab.manager
        .on_run(pane(), run, |live| {
            live.run.set_roster(codex_both());
        })
        .unwrap();
    assert!(handle.cheap(run, WorkerId(1)), "the menu now is not read");
    assert!(!handle.cheap(run, WorkerId(9)));
    assert!(!handle.cheap(RunId(run.0 + 1), WorkerId(1)));
}

// Spec "Cheap author, no consent" (17.2, D21): a mandatory review runs its
// lens workers with no consent recorded as the user's, and its receipt and
// batch-review line say `mandatory`.
#[tokio::test]
async fn a_mandatory_review_runs_and_its_receipt_says_mandatory() {
    let (mut lab, run, handle) = reviewed(quiet, Answered::Mandatory, RunLimits::default()).await;
    let lenses = lab.seen.requests().iter().filter(|r| is_lens(r)).count();
    assert_eq!(lenses, 2);
    let kept = lab.manager.reviews(pane(), run);
    assert_eq!(kept[0].outcome, Some(Ended::Clean));
    let made = receipts(&mut lab);
    assert_eq!(made.len(), 1, "{made:?}");
    assert!(
        made[0].done.contains("consent: mandatory"),
        "{}",
        made[0].done
    );
    let line = handle
        .receipt_line(run, WorkerId(1), &lock_change())
        .expect("a line");
    assert!(line.contains("consent mandatory"), "{line}");
    assert!(line.contains("clean"), "{line}");
}

// The cheap author end to end: an Ollama `code` worker that writes and
// proposes through its tool session. These drive real tool calls and the
// write primitives, so real short deadlines, never a paused clock, and
// Linux only.

/// A worker deadline the review below outlasts. Not shorter: applying the
/// batch after the user's `y` is the author's work time, and its fsyncs take
/// over a second on a loaded machine.
#[cfg(target_os = "linux")]
const DEADLINE: Duration = Duration::from_millis(3000);

/// How long each lens of the review takes: within the deadline, which a
/// lens worker has too. The two lenses run one after the other (one worker
/// at once; the parked author does not count), so the review outlasts the
/// author's deadline.
#[cfg(target_os = "linux")]
const LENS: Duration = Duration::from_millis(2000);

/// The longest any of these tests may take: a hang fails here.
#[cfg(target_os = "linux")]
const BOUND: Duration = Duration::from_secs(20);

#[cfg(target_os = "linux")]
fn session_of(run: RunId, worker: u32) -> SessionKey {
    SessionKey::Worker(
        pane(),
        WorkerKey {
            run,
            worker: WorkerId(worker),
        },
    )
}

#[cfg(target_os = "linux")]
fn status(lab: &Lab, worker: u32) -> Option<WorkerStatus> {
    lock(&lab.manager.runs)[&pane()]
        .run
        .status(WorkerId(worker))
}

/// Waits until worker `worker` ends, or `BOUND` passes.
#[cfg(target_os = "linux")]
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

/// A writing lab whose run has a local `code` author, each worker with
/// `deadline`, the Review row on `review`, one worker at once and its asks
/// in `waits` under turn 7.
#[cfg(target_os = "linux")]
fn local_run(
    lens: Duration,
    review: (&str, &str),
    deadline: Duration,
    waits: &Arc<Waits>,
) -> (Lab, RunId, OrchHandle) {
    let lab = writing_lab(Script::LocalAuthor { lens });
    let pending = Pending::new();
    let scope = OrchScope {
        origin: lab.app(),
        approver: waits.approver(pane(), 7, pending.clone()),
        pending,
        deadline,
        ..lab.scope(local_code(review), &["~/dev/app"])
    };
    let (run, handle) = lab.begin(scope).unwrap();
    (lab, run, handle)
}

/// The asks the worker's tool calls raise on the turn, up to the batch
/// review: its id and review; `None` when none came within `BOUND`. A
/// consent ask fails the test.
#[cfg(target_os = "linux")]
async fn batch_ask(lab: &mut Lab) -> Option<(CallId, Arc<BatchReview>)> {
    loop {
        let event = timeout(BOUND, lab.turn.1.recv()).await.ok()?;
        if let Some(StreamEvent::ToolCall {
            id,
            approval: Some(ask),
            ..
        }) = event
        {
            match *ask {
                ApprovalAsk::Batch(review) => return Some((id, review)),
                other => panic!("only the batch review asks: {other:?}"),
            }
        }
    }
}

// Spec "Cheap author, no consent" (17.2, D21) and "A model review does not
// time out its author" (17.4, D6, owner item 3): an Ollama author with a
// 3 s deadline stages a `passive` change and proposes it. A review runs on
// the Review row (claude) with no consent ask and lasts 4 s (two lenses of
// 2 s, one at a time); then the batch
// review waits for the user, and shows the receipt (`mandatory`). The author
// is not `timed_out`: neither the review nor the wait counted. Only the menu's
// engines ran.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_cheap_authors_review_runs_unasked_and_does_not_time_it_out() {
    let waits = Waits::new();
    let (mut lab, run, handle) = local_run(LENS, ("claude", "opus"), DEADLINE, &waits);
    let started = Instant::now();
    handle.call(spawn(Task::Code, "write a")).await;
    let Some((id, review)) = batch_ask(&mut lab).await else {
        panic!("no batch review; worker 1 is {:?}", status(&lab, 1));
    };
    assert!(started.elapsed() >= 2 * LENS, "{:?}", started.elapsed());
    let receipt = review.receipt().expect("the receipt line");
    assert!(receipt.contains("consent mandatory"), "{receipt}");
    assert!(receipt.contains("clean"), "{receipt}");
    tokio::time::sleep(Duration::from_millis(1000)).await;
    assert_eq!(
        status(&lab, 1),
        Some(WorkerStatus::Running),
        "not timed out"
    );
    assert!(waits.decide(session_of(run, 1), 7, id, Decision::Sent));
    assert_eq!(ended(&lab, 1).await, WorkerStatus::Done);
    assert!(lab.app().join("a.txt").exists());
    let proposed = lab.seen.proposed.lock().unwrap().clone();
    assert!(!proposed[0].is_error, "{proposed:?}");
    let requests = lab.seen.requests();
    let lenses: Vec<_> = requests.iter().filter(|r| is_lens(r)).collect();
    assert_eq!(lenses.len(), 2);
    assert!(
        lenses
            .iter()
            .all(|r| r.engine == "claude" && r.model.as_deref() == Some("opus"))
    );
    let mut engines = lab.seen.engines();
    engines.dedup();
    engines.sort();
    engines.dedup();
    assert_eq!(engines, ["claude", "ollama"]);
}

// Spec "A model review does not time out its author" (17.4): the review's
// own lens deadlines still apply. A lens that outlasts its 3 s deadline
// ends `timed_out` and the run halts as for any failure; its timeout does
// not end the author, whose own clock stopped for the review (the deadline
// of a worker is its own turn's, never a sibling's wall-clock sweep). The
// batch still reaches the user, not reviewed, and the author finishes.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_lens_past_its_deadline_times_out_and_not_its_author() {
    let waits = Waits::new();
    let slow = Duration::from_secs(10);
    let (mut lab, run, handle) = local_run(slow, ("claude", "opus"), DEADLINE, &waits);
    handle.call(spawn(Task::Code, "write a")).await;
    let Some((id, review)) = batch_ask(&mut lab).await else {
        panic!("no batch review; worker 1 is {:?}", status(&lab, 1));
    };
    assert_eq!(status(&lab, 2), Some(WorkerStatus::TimedOut), "the lens");
    assert_eq!(status(&lab, 1), Some(WorkerStatus::Running), "the author");
    let receipt = review.receipt().expect("the receipt line");
    assert!(receipt.contains("consent mandatory"), "{receipt}");
    assert!(!receipt.contains("clean"), "{receipt}");
    assert!(waits.decide(session_of(run, 1), 7, id, Decision::Sent));
    assert_eq!(ended(&lab, 1).await, WorkerStatus::Done);
}

// Spec "Reviewer unavailable stops visibly" and "No silent escalation"
// (17.7): a cheap author's change with the Review row on an engine that is
// not installed. The lens worker ends `refused`, the run halts with the
// failure alert, the batch reaches the user not reviewed, and the receipt
// and the transcript say why. No other engine is called: the fake runner
// sees the author's alone.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn an_unavailable_reviewer_stops_the_run_visibly_and_nothing_escalates() {
    let waits = Waits::new();
    let (mut lab, run, handle) = local_run(LENS, ("gemini", "pro"), DEADLINE, &waits);
    handle.call(spawn(Task::Code, "write a")).await;
    let Some((id, review)) = batch_ask(&mut lab).await else {
        panic!("no batch review; worker 1 is {:?}", status(&lab, 1));
    };
    assert_eq!(status(&lab, 2), Some(WorkerStatus::Refused), "the lens");
    let receipt = review.receipt().expect("the receipt line");
    assert!(receipt.contains("consent mandatory"), "{receipt}");
    assert!(
        receipt.contains("not reviewed (lens risk (worker 2) was refused"),
        "{receipt}"
    );
    let events = lab.sent();
    let halted = events.iter().find_map(|e| match e {
        OrchestraEvent::Halted { worker, class, .. } => Some((*worker, *class)),
        _ => None,
    });
    assert_eq!(halted, Some((WorkerId(2), ErrorClass::Setup)), "{events:?}");
    let said: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            OrchestraEvent::Review { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        said.iter()
            .any(|l| l.contains("could not start: lens risk (worker 2) was refused")),
        "{said:?}"
    );
    let made: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            OrchestraEvent::Handoff { handoff, .. }
                if matches!(handoff.unit, Unit::Receipt(..)) =>
            {
                Some(handoff.done.clone())
            }
            _ => None,
        })
        .collect();
    assert!(
        made.iter()
            .any(|done| done.contains("outcome: not reviewed (lens risk (worker 2) was refused")),
        "{made:?}"
    );
    assert!(waits.decide(session_of(run, 1), 7, id, Decision::Sent));
    assert_eq!(ended(&lab, 1).await, WorkerStatus::Done);
    let mut engines = lab.seen.engines();
    engines.dedup();
    assert_eq!(engines, ["ollama"], "no other engine ran");
}

// D21 GUARD (17.7): a mandatory review that cannot start for another reason
// (no room under `max_workers`) says so in the transcript and in its
// receipt, and the batch goes on unreviewed.
#[tokio::test]
async fn a_mandatory_review_that_cannot_start_says_why_and_the_batch_goes_on() {
    let limits = RunLimits {
        workers: 3,
        ..RunLimits::default()
    };
    let (mut lab, run, handle) = reviewed(quiet, Answered::Mandatory, limits).await;
    let kept = lab.manager.reviews(pane(), run);
    assert_eq!(kept[0].outcome, None);
    let events = lab.sent();
    let said: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            OrchestraEvent::Review { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        said.iter()
            .any(|l| l.contains("could not start: max_workers is 3")),
        "{said:?}"
    );
    let line = handle
        .receipt_line(run, WorkerId(1), &lock_change())
        .expect("a line");
    assert!(line.contains("not reviewed (max_workers is 3"), "{line}");
}
