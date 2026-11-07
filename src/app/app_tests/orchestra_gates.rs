//! A step gate refusal as the user sees it (agent-orchestrator [W3]): one
//! transcript line naming the gate and the step, ASCII, never history.

use super::chat_live::chat_of;
use super::orchestra_run::{RUN, event, running};
use super::*;
use crate::core::ai::{
    chat::Entry,
    orchestra::{
        event::OrchestraEvent,
        steps::{Gate, Step},
    },
};

fn refused(gate: Gate, step: Step) -> OrchestraEvent {
    OrchestraEvent::GateRefused {
        run: RUN,
        gate,
        step,
        reason: gate.refusal(step),
    }
}

/// The system lines that speak of a gate (the ORCH-on lines are not).
fn system_lines(a: &App) -> Vec<String> {
    chat_of(a)
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            Entry::System(line) if line.contains("gate") => Some(line.clone()),
            _ => None,
        })
        .collect()
}

// Spec (W3, A gate names itself): the user's notice names the gate and the
// step; it is a system line, so the model never reads it as history.
#[test]
fn a_gate_refusal_is_a_system_line_naming_the_gate_and_the_step() {
    let (mut a, _) = running();
    a.update(event(refused(Gate::Spawn, Step::Explore)));
    a.update(event(refused(Gate::Finish, Step::Implement)));
    let lines = system_lines(&a);
    assert_eq!(
        lines,
        vec![
            Gate::Spawn.refusal(Step::Explore),
            Gate::Finish.refusal(Step::Implement)
        ]
    );
    assert!(lines.iter().all(|l| l.is_ascii()));
    assert!(lines[0].contains("step explore") && lines[1].contains("step implement"));
}

// The text is ASCII even when it carried more (a reason is lazarobox's own
// today, but the line is the one place the rule is kept).
#[test]
fn a_gate_line_is_ascii_whatever_the_reason_holds() {
    let (mut a, _) = running();
    a.update(event(OrchestraEvent::GateRefused {
        run: RUN,
        gate: Gate::Spawn,
        step: Step::Authorize,
        reason: "spawn gate: step authorize caf\u{e9}".to_owned(),
    }));
    let lines = system_lines(&a);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].is_ascii(), "{lines:?}");
    assert!(lines[0].contains("step authorize caf?"), "{lines:?}");
}
