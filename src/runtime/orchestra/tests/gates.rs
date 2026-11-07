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
