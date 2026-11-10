//! A run whose turn ended with a spawn gate refusal standing waits for the
//! user's reply instead of ending (agent-orchestrator [W3], option B).

use super::*;
use crate::core::ai::orchestra::steps::Step;

async fn code_gets_through(handle: &OrchHandle) -> bool {
    !handle.call(spawn(Task::Code, "write it")).await.is_error
}

fn waits_of(events: &[OrchestraEvent]) -> Vec<(RunId, Step)> {
    events
        .iter()
        .filter_map(|event| match event {
            OrchestraEvent::Waits { run, step } => Some((*run, *step)),
            _ => None,
        })
        .collect()
}

// A spawn refusal standing at the end of the turn keeps the run open, said
// once; nothing ends it and the app is not told it was undelegated.
#[tokio::test]
async fn a_turn_that_ends_with_a_spawn_refusal_standing_keeps_the_run_waiting() {
    let mut lab = lab(Script::Hang);
    let (run, handle) = lab.start(codex_both());
    assert!(!code_gets_through(&handle).await);
    lab.sent();
    lab.manager.turn_ended(pane(), run);
    let events = lab.sent();
    assert_eq!(waits_of(&events), vec![(run, Step::Authorize)]);
    assert!(
        !events.iter().any(|e| matches!(
            e,
            OrchestraEvent::Ended { .. } | OrchestraEvent::Undelegated { .. }
        )),
        "{events:?}"
    );
    // The run is still there to take the reply.
    assert!(
        lab.manager
            .attach(pane(), run, lab.scope(codex_both(), &[]))
            .is_ok()
    );
}

// A reply that settles with nothing pending and no new refusal ends normally.
#[tokio::test]
async fn a_reply_turn_with_no_new_refusal_ends_the_run() {
    let mut lab = lab(Script::Hang);
    let (run, handle) = lab.start(codex_both());
    assert!(!code_gets_through(&handle).await);
    lab.manager.turn_ended(pane(), run);
    lab.sent();
    let _reply = lab
        .manager
        .attach(pane(), run, lab.scope(codex_both(), &[]))
        .unwrap();
    lab.manager.turn_ended(pane(), run);
    let events = lab.sent();
    assert!(waits_of(&events).is_empty(), "{events:?}");
    assert!(
        events.contains(&OrchestraEvent::Ended { run }),
        "{events:?}"
    );
}

// A reply turn that is refused again waits again.
#[tokio::test]
async fn a_reply_turn_that_is_refused_again_waits_again() {
    let mut lab = lab(Script::Hang);
    let (run, handle) = lab.start(codex_both());
    assert!(!code_gets_through(&handle).await);
    lab.manager.turn_ended(pane(), run);
    let (_, handle) = lab
        .manager
        .attach(pane(), run, lab.scope(codex_both(), &[]))
        .unwrap();
    lab.sent();
    assert!(!code_gets_through(&handle).await);
    lab.manager.turn_ended(pane(), run);
    assert_eq!(waits_of(&lab.sent()), vec![(run, Step::Authorize)]);
}

// The lift made while the run waits is kept by the reply turn, and the
// coordinator is told in plain words (once).
#[tokio::test]
async fn a_lift_made_while_waiting_reaches_the_reply_turn_and_the_coordinator() {
    let lab = lab(Script::Hang);
    let (run, handle) = lab.start(codex_both());
    assert!(!code_gets_through(&handle).await);
    lab.manager.turn_ended(pane(), run);
    lab.manager.gates_off(pane(), run);
    let note = lab.manager.reply_note(pane(), run);
    assert!(
        note.is_some_and(|n| n.contains("lifted the step gates") && n.is_ascii()),
        "{note:?}"
    );
    let (_, handle) = lab
        .manager
        .attach(pane(), run, lab.scope(codex_both(), &[]))
        .unwrap();
    assert!(code_gets_through(&handle).await);
    assert_eq!(lab.manager.reply_note(pane(), run), None, "told once");
}

// No wait, or no lift: no line.
#[tokio::test]
async fn the_reply_note_needs_a_wait_and_a_lift() {
    let lab = lab(Script::Hang);
    let (run, _handle) = lab.start(codex_both());
    lab.manager.gates_off(pane(), run);
    assert_eq!(lab.manager.reply_note(pane(), run), None, "not waiting");
    let (run, handle) = lab.start(codex_both());
    assert!(!code_gets_through(&handle).await);
    lab.manager.turn_ended(pane(), run);
    assert_eq!(lab.manager.reply_note(pane(), run), None, "not lifted");
}

// The cancel and the turn cap still end a waiting run.
#[tokio::test]
async fn a_waiting_run_ends_on_cancel_and_on_the_turn_cap() {
    let mut lab = lab(Script::Hang);
    let (run, handle) = lab.start(codex_both());
    assert!(!code_gets_through(&handle).await);
    lab.manager.turn_ended(pane(), run);
    lab.sent();
    lab.manager.cancel(pane());
    assert!(lab.sent().contains(&OrchestraEvent::Ended { run }));

    let mut scope = lab.scope(codex_both(), &[]);
    scope.limits = RunLimits {
        turns: 1,
        ..RunLimits::default()
    };
    let (run, handle) = lab.manager.start(pane(), scope).unwrap();
    assert!(!code_gets_through(&handle).await);
    lab.sent();
    lab.manager.turn_ended(pane(), run);
    let events = lab.sent();
    assert!(waits_of(&events).is_empty(), "{events:?}");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, OrchestraEvent::Stopped { .. })),
        "{events:?}"
    );
}

// A reply to a run that ended while it waited is refused, not started.
#[tokio::test]
async fn a_reply_after_the_waiting_run_ended_is_refused() {
    let lab = lab(Script::Hang);
    let (run, handle) = lab.start(codex_both());
    assert!(!code_gets_through(&handle).await);
    lab.manager.turn_ended(pane(), run);
    lab.manager.cancel(pane());
    assert!(
        lab.manager
            .attach(pane(), run, lab.scope(codex_both(), &[]))
            .is_err()
    );
}

// The lift line is given once: after the reply the lifted run no longer waits.
#[tokio::test]
async fn the_lift_note_is_given_once_and_the_lifted_run_settles() {
    let mut lab = lab(Script::Hang);
    let (run, handle) = lab.start(codex_both());
    assert!(!code_gets_through(&handle).await);
    lab.manager.turn_ended(pane(), run);
    lab.manager.gates_off(pane(), run);
    assert!(lab.manager.reply_note(pane(), run).is_some());
    let _ = lab
        .manager
        .attach(pane(), run, lab.scope(codex_both(), &[]))
        .unwrap();
    lab.sent();
    lab.manager.turn_ended(pane(), run);
    let events = lab.sent();
    assert!(waits_of(&events).is_empty(), "{events:?}");
    assert!(
        events.contains(&OrchestraEvent::Ended { run }),
        "{events:?}"
    );
    assert_eq!(lab.manager.reply_note(pane(), run), None);
}

// A later, unrelated run never waits and cannot take the old run's reply.
#[tokio::test]
async fn a_later_run_does_not_inherit_the_wait() {
    let mut lab = lab(Script::Hang);
    let (old, handle) = lab.start(codex_both());
    assert!(!code_gets_through(&handle).await);
    lab.manager.turn_ended(pane(), old);
    lab.manager.gates_off(pane(), old);
    let (new, _handle) = lab.start(codex_both());
    assert_eq!(lab.manager.reply_note(pane(), new), None);
    assert_eq!(lab.manager.reply_note(pane(), old), None);
    assert!(
        lab.manager
            .attach(pane(), old, lab.scope(codex_both(), &[]))
            .is_err()
    );
    lab.sent();
    lab.manager.turn_ended(pane(), new);
    let events = lab.sent();
    assert!(waits_of(&events).is_empty(), "{events:?}");
    assert!(
        events.contains(&OrchestraEvent::Ended { run: new }),
        "{events:?}"
    );
}
