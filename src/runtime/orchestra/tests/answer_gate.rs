//! The answer gate in the runtime: a turn that ended with text before the
//! steps are handed off is re-asked once, then the run waits for the user
//! (agent-orchestrator, "Gates on step state are visible and bounded").

use super::*;
use crate::core::ai::orchestra::run::TurnEnd;
use crate::core::ai::orchestra::steps::{Gate, Grant, Mark, Step, answer_line, answer_text};

const TEXT: TurnEnd = TurnEnd {
    answered: true,
    written: None,
};

fn refusals(events: &[OrchestraEvent]) -> Vec<(Gate, Step, String)> {
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

fn continuations(events: &[OrchestraEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            OrchestraEvent::Continue(c) => Some(c.text.clone()),
            _ => None,
        })
        .collect()
}

// A text end before `authorize` sends the gate line, then a continuation that
// opens with the re-ask and holds the ledger; it is not undelegated.
#[tokio::test]
async fn a_text_end_before_authorize_is_re_asked_with_the_ledger() {
    let mut lab = lab(Script::Hang);
    let (run, _handle) = lab.start(codex_both());
    lab.sent();
    lab.manager.turn_ended_as(pane(), run, TEXT);
    let events = lab.sent();
    assert_eq!(
        refusals(&events),
        vec![(
            Gate::Answer,
            Step::Authorize,
            answer_line(Some(Step::Authorize), None)
        )]
    );
    let [text] = continuations(&events).try_into().expect("one continuation");
    let reask = answer_text(Some(Step::Authorize), &[], None);
    // One user message: the app marker first, the re-ask, the fixed Goal
    // line, then the ledger as data. The goal is only inside the wrapper.
    assert!(text.starts_with(REASK_MARKER), "{text}");
    assert!(text.contains(&reask), "{text}");
    assert!(text.contains(REASK_GOAL_LINE), "{text}");
    let (head, data) = text
        .split_once("<untrusted-tool-output")
        .expect("the ledger is wrapped as data");
    assert!(!head.contains("build the parser"), "{text}");
    assert!(data.contains("build the parser"), "{text}");
    assert!(!text.contains("worker"), "no worker head: {text}");
    assert!(
        !events.iter().any(|e| matches!(
            e,
            OrchestraEvent::Undelegated { .. } | OrchestraEvent::Ended { .. }
        )),
        "{events:?}"
    );
}

// The same step is not re-asked twice: the run waits, naming the gate.
#[tokio::test]
async fn a_spent_re_ask_waits_naming_the_answer_gate() {
    let mut lab = lab(Script::Hang);
    let (run, _handle) = lab.start(codex_both());
    lab.manager.turn_ended_as(pane(), run, TEXT);
    lab.sent();
    lab.manager.turn_ended_as(pane(), run, TEXT);
    let events = lab.sent();
    assert!(continuations(&events).is_empty(), "{events:?}");
    let waits: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            OrchestraEvent::Waits {
                step, gate, closed, ..
            } => Some((*step, *gate, closed.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(waits, vec![(Step::Authorize, Gate::Answer, vec![])]);
}

// A turn that did not end in `Done` keeps today's path: no re-ask.
#[tokio::test]
async fn an_unanswered_end_is_not_re_asked() {
    let mut lab = lab(Script::Hang);
    let (run, _handle) = lab.start(codex_both());
    lab.sent();
    lab.manager.turn_ended_as(pane(), run, TurnEnd::default());
    let events = lab.sent();
    assert!(continuations(&events).is_empty(), "{events:?}");
    assert!(refusals(&events).is_empty(), "{events:?}");
}

// A written call is re-asked with its tool name, even after `read_only`.
#[tokio::test]
async fn a_written_call_is_re_asked_by_name_after_a_read_only_grant() {
    let mut lab = lab(Script::Hang);
    let (run, handle) = lab.start(codex_both());
    let acked = handle
        .call(step_call(Step::Authorize, Mark::Grant(Grant::ReadOnly)))
        .await;
    assert!(!acked.is_error, "{acked:?}");
    lab.sent();
    let end = TurnEnd {
        answered: true,
        written: Some("spawn_agent"),
    };
    lab.manager.turn_ended_as(pane(), run, end);
    let events = lab.sent();
    assert_eq!(
        refusals(&events)[0].2,
        answer_line(None, Some("spawn_agent"))
    );
    let [text] = continuations(&events).try_into().expect("one continuation");
    assert!(text.starts_with(REASK_MARKER), "{text}");
    assert!(
        text.contains(&answer_text(None, &[], Some("spawn_agent"))),
        "{text}"
    );
}

// An allowed read_only inline answer settles with no notice and no re-ask.
#[tokio::test]
async fn a_read_only_inline_answer_settles_with_no_notice() {
    let mut lab = lab(Script::Hang);
    let (run, handle) = lab.start(codex_both());
    let acked = handle
        .call(step_call(Step::Authorize, Mark::Grant(Grant::ReadOnly)))
        .await;
    assert!(!acked.is_error, "{acked:?}");
    lab.sent();
    lab.manager.turn_ended_as(pane(), run, TEXT);
    let events = lab.sent();
    assert!(
        events.contains(&OrchestraEvent::Ended { run }),
        "{events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, OrchestraEvent::Undelegated { .. })),
        "{events:?}"
    );
    assert!(continuations(&events).is_empty(), "{events:?}");
}

// With the gates lifted the end settles, and the notice is still told.
#[tokio::test]
async fn a_text_end_with_the_gates_lifted_settles_with_the_notice() {
    let mut lab = lab(Script::Hang);
    let (run, _handle) = lab.start(codex_both());
    lab.manager.gates_off(pane(), run);
    lab.sent();
    lab.manager.turn_ended_as(pane(), run, TEXT);
    let events = lab.sent();
    assert!(
        events.contains(&OrchestraEvent::Undelegated { run }),
        "{events:?}"
    );
    assert!(
        events.contains(&OrchestraEvent::Ended { run }),
        "{events:?}"
    );
}

#[test]
fn the_lift_note_says_what_is_not_gated_now() {
    assert!(
        LIFT_NOTE.contains("spawn, finish and turn ends are not gated now"),
        "{LIFT_NOTE}"
    );
    assert!(LIFT_NOTE.is_ascii());
}

// The lift is told in the transcript, naming the answer gate among the ones
// it lifted; the coordinator's next text-only turn then settles.
#[tokio::test]
async fn the_lift_notice_names_the_answer_gate() {
    let mut lab = lab(Script::Hang);
    let (run, _handle) = lab.start(codex_both());
    lab.manager.turn_ended_as(pane(), run, TEXT);
    lab.sent();
    lab.manager.gates_off(pane(), run);
    let notices: Vec<_> = lab
        .sent()
        .into_iter()
        .filter_map(|e| match e {
            OrchestraEvent::Notice { text } => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(
        notices,
        vec!["gates off: spawn, finish and answer gates lifted for this run"]
    );
}
