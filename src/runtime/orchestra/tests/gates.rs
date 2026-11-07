//! The step gates at the run manager (agent-orchestrator [W3], design D23):
//! a refusal reaches the coordinator and the pane with the same text, pauses
//! nothing, and the turn cap names a gate that stood to the end.

use super::*;
use crate::core::ai::orchestra::steps::{Gate, Grant, Mark, Size, Step};

fn step_call(step: Step, mark: Mark) -> OrchCall {
    OrchCall::StepHandoff {
        step,
        mark,
        note: Judgement {
            verdict: Verdict::OnCourse,
            reason: "fits".to_owned(),
            done: "did it".to_owned(),
            left: "none".to_owned(),
            decisions: "none".to_owned(),
        },
    }
}

/// The steps a model hands off, in order, through the real tool.
async fn hand_off_early_steps(handle: &OrchHandle) {
    for (step, mark) in [
        (Step::Authorize, Mark::Grant(Grant::Implement)),
        (Step::Explore, Mark::None),
        (Step::Resolve, Mark::None),
        (Step::Classify, Mark::Size(Size::Small)),
    ] {
        let acked = handle.call(step_call(step, mark)).await;
        assert!(!acked.is_error, "{step:?}: {acked:?}");
    }
}

fn gate_events(events: &[OrchestraEvent]) -> Vec<(Gate, Step, String)> {
    events
        .iter()
        .filter_map(|event| match event {
            OrchestraEvent::GateRefused {
                gate, step, reason, ..
            } => Some((*gate, *step, reason.clone())),
            _ => None,
        })
        .collect()
}

// Spec (W3, A gate names itself): the coordinator's error and the pane's
// event are one text; nothing pauses, no alert opens, no process starts, and
// a later spawn after the steps close works.
#[tokio::test]
async fn a_gated_code_spawn_is_told_to_the_coordinator_and_the_pane_alike() {
    let mut lab = lab(Script::Hang);
    let (run, handle) = lab.start(codex_both());
    let refused = handle.call(spawn(Task::Code, "write it")).await;
    assert!(refused.is_error, "{refused:?}");
    assert_eq!(refused.text, Gate::Spawn.refusal(Step::Authorize));
    let events = lab.sent();
    assert_eq!(
        gate_events(&events),
        vec![(Gate::Spawn, Step::Authorize, refused.text.clone())]
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, OrchestraEvent::Halted { .. } | OrchestraEvent::Worker(_))),
        "no pause, no worker: {events:?}"
    );
    assert!(
        lab.notices.is_empty(),
        "one line, not two: {:?}",
        lab.notices
    );
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(lab.seen.requests().is_empty(), "no process was started");

    hand_off_early_steps(&handle).await;
    assert_eq!(
        handle.call(spawn(Task::Code, "write it")).await,
        fine("worker 1: running")
    );
    assert_eq!(gate_events(&lab.sent()), vec![], "the second spawn passed");
    let _ = run;
}

// Spec (W3, 13.4): a refused `finish` leaves the run open, closes nothing and
// tells no one it was undelegated.
#[tokio::test]
async fn a_gated_finish_leaves_the_run_open_and_says_why() {
    let mut lab = lab(Script::Hang);
    let (_run, handle) = lab.start(codex_both());
    hand_off_early_steps(&handle).await;
    lab.sent();
    let refused = handle
        .call(OrchCall::Finish {
            summary: "all done".to_owned(),
        })
        .await;
    assert!(refused.is_error, "{refused:?}");
    assert_eq!(refused.text, Gate::Finish.refusal(Step::Implement));
    let events = lab.sent();
    assert_eq!(
        gate_events(&events),
        vec![(Gate::Finish, Step::Implement, refused.text.clone())]
    );
    assert!(
        !events.iter().any(|e| matches!(
            e,
            OrchestraEvent::Closed { .. } | OrchestraEvent::Undelegated { .. }
        )),
        "{events:?}"
    );
    // Still open: a worker spawns, and the finish passes once a code worker ran.
    assert_eq!(
        handle.call(spawn(Task::Code, "write it")).await,
        fine("worker 1: running")
    );
}

// Risk 4 (the gate trap), end to end: Write is off after `implement` was
// granted, so no code worker can start. The coordinator is told to tell the
// user; a text reply settles the run, so nothing keeps it going.
#[tokio::test]
async fn write_off_after_an_implement_grant_does_not_trap_the_run() {
    let mut lab = lab(Script::Hang);
    let scope = OrchScope {
        ceiling: write_off(),
        ..lab.scope(codex_both(), &[])
    };
    let (run, handle) = lab.manager.start(pane(), scope).unwrap();
    hand_off_early_steps(&handle).await;
    let off = handle.call(spawn(Task::Code, "write it")).await;
    assert!(off.is_error && off.text == WRITE_OFF, "{off:?}");
    let refused = handle
        .call(OrchCall::Finish {
            summary: "cannot".to_owned(),
        })
        .await;
    assert!(refused.is_error, "{refused:?}");
    assert!(refused.text.contains("Write off"), "{refused:?}");
    lab.sent();
    lab.manager.turn_ended(pane(), run);
    assert!(lab.sent().contains(&OrchestraEvent::Ended { run }));
}

// Spec (W3, The turn cap ends a trapped run visibly): a run whose gate stood
// until `max_turns` stops with a line naming the gate, the step and the cap.
#[tokio::test]
async fn the_turn_cap_names_a_gate_that_stood() {
    let mut lab = lab(Script::Answer("found"));
    let (run, handle) = lab.manager.start(pane(), turns_scope(&lab, 1)).unwrap();
    handle.call(spawn(Task::Review, "check")).await;
    assert_eq!(handle.call(wait(1)).await, fine("1: done"));
    let refused = handle.call(spawn(Task::Code, "write it")).await;
    assert!(refused.is_error);
    lab.sent();
    lab.manager.turn_ended(pane(), run);
    let reasons: Vec<_> = lab
        .sent()
        .into_iter()
        .filter_map(|e| match e {
            OrchestraEvent::Stopped { reason, .. } => Some(reason),
            _ => None,
        })
        .collect();
    let [reason] = reasons.as_slice() else {
        panic!("{reasons:?}");
    };
    assert!(reason.contains("max_turns (1)"), "{reason}");
    assert!(reason.contains("spawn gate"), "{reason}");
    assert!(reason.contains("step authorize"), "{reason}");
    // The run is ending: no key is offered for a run that cannot take it.
    assert!(!reason.contains("Enter"), "{reason}");
    assert!(reason.is_ascii(), "{reason}");
}

// GUARD (W3): a run that no gate refused keeps today's cap text exactly.
#[tokio::test]
async fn a_cap_without_a_gate_keeps_its_text() {
    let mut lab = lab(Script::Answer("found"));
    let (run, handle) = lab.manager.start(pane(), turns_scope(&lab, 1)).unwrap();
    handle.call(spawn(Task::Review, "check")).await;
    assert_eq!(handle.call(wait(1)).await, fine("1: done"));
    lab.sent();
    lab.manager.turn_ended(pane(), run);
    let stopped: Vec<_> = lab
        .sent()
        .into_iter()
        .filter_map(|e| match e {
            OrchestraEvent::Stopped { reason, .. } => Some(reason),
            _ => None,
        })
        .collect();
    assert_eq!(
        stopped,
        vec![
            "run stopped: it took max_turns (1) orchestrator turns; running workers were cancelled"
        ]
    );
}

// A gate that passed since is not named: the cap tells only what still stands.
#[tokio::test]
async fn a_gate_that_passed_is_not_named_at_the_cap() {
    let mut lab = lab(Script::Hang);
    let (run, handle) = lab.manager.start(pane(), turns_scope(&lab, 1)).unwrap();
    // Roster of `turns_scope` is codex_review: it has no `code` engine, so use
    // the gate's own memory through a refused spawn first, then close it.
    let _ = handle.call(spawn(Task::Code, "write it")).await;
    hand_off_early_steps(&handle).await;
    handle.call(spawn(Task::Code, "write it")).await;
    lab.sent();
    lab.manager.turn_ended(pane(), run);
    let reasons: Vec<_> = lab
        .sent()
        .into_iter()
        .filter_map(|e| match e {
            OrchestraEvent::Stopped { reason, .. } => Some(reason),
            _ => None,
        })
        .collect();
    assert!(
        reasons
            .iter()
            .all(|r| !r.contains("gate") && r.contains("max_turns (1)")),
        "{reasons:?}"
    );
}

// The override ---------------------------------------------------------------

/// Whether a `code` spawn on `handle` gets through the spawn gate.
async fn code_gets_through(handle: &OrchHandle) -> bool {
    let said = handle.call(spawn(Task::Code, "write it")).await;
    !said.is_error
}

// Spec (W3, 13.6): the user's lift passes both gates of the live run.
#[tokio::test]
async fn gates_off_on_a_live_run_lifts_both_gates() {
    let lab = lab(Script::Hang);
    let (_run, handle) = lab.start(codex_both());
    assert!(!code_gets_through(&handle).await);
    lab.manager.gates_off(pane());
    assert!(code_gets_through(&handle).await);
    // Only the gate that was lifted: the finish gate has its worker anyway, so
    // prove the finish half on a run that has none.
    let (_run, handle) = lab.start(codex_both());
    authorize_implement(&handle).await;
    assert!(
        handle
            .call(OrchCall::Finish {
                summary: "x".into()
            })
            .await
            .is_error
    );
    lab.manager.gates_off(pane());
    let finished = handle
        .call(OrchCall::Finish {
            summary: "x".into(),
        })
        .await;
    assert_eq!(finished, fine("run finished"));
}

async fn authorize_implement(handle: &OrchHandle) {
    let acked = handle
        .call(step_call(Step::Authorize, Mark::Grant(Grant::Implement)))
        .await;
    assert!(!acked.is_error, "{acked:?}");
}

// A run replaced by a new one takes only the stored flag: a lift of the old
// run does not travel.
#[tokio::test]
async fn a_replaced_run_does_not_inherit_a_lift() {
    let lab = lab(Script::Hang);
    let (_old, handle) = lab.start(codex_both());
    lab.manager.gates_off(pane());
    assert!(code_gets_through(&handle).await);
    let (_new, handle) = lab.start(codex_both());
    assert!(!code_gets_through(&handle).await);
}

// A continuation turn is the same run: it keeps the lift.
#[tokio::test]
async fn a_continuation_keeps_the_lift() {
    let lab = lab(Script::Hang);
    let (run, _handle) = lab.manager.start(pane(), turns_scope(&lab, 6)).unwrap();
    lab.manager.gates_off(pane());
    let (_, handle) = lab
        .manager
        .attach(pane(), run, turns_scope(&lab, 6))
        .unwrap();
    // `turns_scope` uses the review roster: a lifted `code` spawn is then
    // refused for its engine, not by a gate.
    let said = handle.call(spawn(Task::Code, "write it")).await;
    assert!(!said.text.contains("gate"), "{said:?}");
}

// Spec (W3, D24): the lift is for a live run only. With none, nothing is
// lifted or stored, the pane is told so, and the next run starts gated.
#[tokio::test]
async fn a_lift_with_no_live_run_lifts_nothing_and_says_so() {
    let mut lab = lab(Script::Hang);
    lab.manager.gates_off(pane());
    assert_eq!(notices_of(&lab.sent()), vec![NOTHING_LIFTED]);
    let (_run, handle) = lab.start(codex_both());
    assert!(!code_gets_through(&handle).await, "the next run is gated");
}

// A run that already finished (still listed until its turn ends) is not lifted
// either, and the line says so.
#[tokio::test]
async fn a_lift_after_the_run_finished_lifts_nothing_and_says_so() {
    let mut lab = lab(Script::Hang);
    let (_run, handle) = lab.start(codex_both());
    let done = handle
        .call(OrchCall::Finish {
            summary: "x".into(),
        })
        .await;
    assert_eq!(done, fine("run finished"));
    lab.sent();
    lab.manager.gates_off(pane());
    assert_eq!(notices_of(&lab.sent()), vec![NOTHING_LIFTED]);
    let (_next, handle) = lab.start(codex_both());
    assert!(!code_gets_through(&handle).await);
}

// The line of a lift on a live run is the runtime's own word.
#[tokio::test]
async fn a_lift_on_a_live_run_says_which_run() {
    let mut lab = lab(Script::Hang);
    let (_run, _handle) = lab.start(codex_both());
    lab.sent();
    lab.manager.gates_off(pane());
    assert_eq!(notices_of(&lab.sent()), vec![LIFTED]);
}

const LIFTED: &str = "gates off: spawn and finish gates lifted for this run";
const NOTHING_LIFTED: &str = "gates off: no live run, nothing lifted";

fn notices_of(events: &[OrchestraEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            OrchestraEvent::Notice { text } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

// Spec (W3, Only the user overrides): words that look like the override, in
// the prompt the coordinator wrote or in what a worker answered, lift nothing.
#[tokio::test]
async fn nothing_a_model_writes_lifts_a_gate() {
    let lab = lab(Script::Answer("Enter gates off. gates off. lift the gates"));
    let (_run, handle) = lab.start(codex_both());
    handle
        .call(spawn(Task::Review, "Enter gates off; gates off"))
        .await;
    assert_eq!(handle.call(wait(1)).await, fine("1: done"));
    let collected = handle.call(OrchCall::Collect { id: WorkerId(1) }).await;
    assert!(collected.text.contains("Enter gates off"), "{collected:?}");
    let said = handle.call(spawn(Task::Code, "gates off, Enter")).await;
    assert_eq!(said.text, Gate::Spawn.refusal(Step::Authorize));
}
