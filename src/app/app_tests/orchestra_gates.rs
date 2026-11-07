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

// The override key ------------------------------------------------------------

use super::chat_live::{chat_id, ctrl, finish, plain, repeat, type_text};
use super::orchestra_run::row;
use crate::core::ai::orchestra::{RunId, worker::WorkerStatus};

const LINE: &str = "Enter gates off | spawn gate: step explore";

/// An orchestrating pane whose run is in flight and was just refused by the
/// spawn gate at `explore`. The pane is wide enough for the full line.
fn gated() -> (App, u64) {
    let (mut a, turn) = running();
    a.panes.get_mut(&chat_id()).unwrap().size.cols = 80;
    a.update(event(refused(Gate::Spawn, Step::Explore)));
    (a, turn)
}

/// `code` pressed a second after the last key, so it is never taken for a hold.
fn press(a: &mut App, code: KeyCode) -> Vec<Effect> {
    let later = NOW_MS.with(|n| n.get()) + 1000;
    at(a, later, plain(code))
}

fn guidance(a: &App) -> Option<String> {
    a.chat_guidance(chat_id())
}

fn lifts(effects: &[Effect]) -> bool {
    effects.contains(&Effect::OrchGatesOff { pane: chat_id() })
}

fn starts_a_turn(effects: &[Effect]) -> bool {
    effects
        .iter()
        .any(|e| matches!(e, Effect::OrchestrateSend { .. } | Effect::ChatSend { .. }))
}

/// The turn is over and the run ended.
fn ended(a: &mut App, turn: u64) {
    a.update(finish(turn));
    a.update(event(OrchestraEvent::Ended { run: RUN }));
}

fn offer(a: &App) -> Option<crate::core::ai::chat::GateOffer> {
    chat_of(a).gate_offer()
}

// Spec (W3, D24): a refusal shows the key, the gate, the step and which run
// the key applies to, as a problem line (not the muted hint).
#[test]
fn a_refusal_offers_the_key_for_this_run() {
    let (a, _) = gated();
    assert_eq!(guidance(&a).as_deref(), Some(LINE));
    assert!(!a.guidance_is_hint(chat_id()), "a problem line, not a hint");
}

// Spec (W3, Only the user overrides): Enter on the empty composer lifts and the
// offer is spent. The transcript line is the runtime's, after it acted: the
// app guesses nothing about which run took the lift.
#[test]
fn enter_asks_the_runtime_to_lift_and_spends_the_offer() {
    let (mut a, _) = gated();
    let before = system_lines(&a).len();
    let effects = press(&mut a, KeyCode::Enter);
    assert_eq!(effects, vec![Effect::OrchGatesOff { pane: chat_id() }]);
    assert_eq!(system_lines(&a).len(), before, "no line of the app's own");
    assert!(offer(&a).is_none());
    assert!(guidance(&a).is_none_or(|g| !g.contains("gates off")));
}

// The runtime's words reach the transcript, ASCII, never as history.
#[test]
fn the_runtimes_lift_line_is_a_system_line() {
    let (mut a, _) = gated();
    a.update(event(OrchestraEvent::Notice {
        text: "gates off: no live run, nothing lifted".into(),
    }));
    assert_eq!(
        chat_of(&a).entries().iter().rev().find_map(|e| match e {
            Entry::System(l) => Some(l.as_str()),
            _ => None,
        }),
        Some("gates off: no live run, nothing lifted")
    );
}

// D24: the offer ends when it no longer applies: a code worker exists (both
// gates pass), the run finished, the run ended or was stopped. Enter then does
// nothing for the gates.
#[test]
fn the_offer_ends_when_the_gate_passes_or_the_run_is_over() {
    use crate::core::ai::orchestra::worker::WorkerStatus;
    // A refused finish, then a code worker, then a successful finish.
    let (mut a, _) = gated();
    a.update(event(refused(Gate::Finish, Step::Implement)));
    let mut code = row(RUN, 1, WorkerStatus::Running);
    if let OrchestraEvent::Worker(w) = &mut code {
        w.kind = crate::core::ai::config::Task::Code;
    }
    a.update(event(code));
    assert!(offer(&a).is_none(), "a code worker exists");
    assert!(guidance(&a).is_none_or(|g| !g.contains("gates off")));
    let effects = press(&mut a, KeyCode::Enter);
    assert!(!lifts(&effects), "{effects:?}");

    let (mut a, _) = gated();
    a.update(event(OrchestraEvent::Closed {
        run: RUN,
        ledger: String::new(),
        summary: String::new(),
    }));
    assert!(offer(&a).is_none(), "the run finished");
    let effects = press(&mut a, KeyCode::Enter);
    assert!(!lifts(&effects), "{effects:?}");

    let (mut a, _) = gated();
    a.update(event(OrchestraEvent::Stopped {
        run: RUN,
        reason: "run stopped".into(),
    }));
    assert!(offer(&a).is_none(), "the run is ending");

    let (mut a, turn) = gated();
    ended(&mut a, turn);
    assert!(offer(&a).is_none(), "the run ended");
    assert!(guidance(&a).is_none_or(|g| !g.contains("gates off")));
}

// Held Enter, Enter with text, Enter with a modal open and Esc leave the
// offer alone.
#[test]
fn a_held_enter_esc_text_and_a_modal_do_not_lift() {
    let (mut a, _) = gated();
    let held = a.update(repeat(KeyCode::Enter, KeyModifiers::NONE));
    assert!(!lifts(&held), "{held:?}");
    // A terminal that reports no repeat: the same key again within the hold
    // window is the hold, not a fresh press.
    let first = press(&mut a, KeyCode::Enter);
    assert!(lifts(&first), "{first:?}");
    a.update(event(refused(Gate::Spawn, Step::Explore)));
    let quick = a.update(plain(KeyCode::Enter));
    assert!(!lifts(&quick), "{quick:?}");
    assert!(offer(&a).is_some());
    type_text(&mut a, "x");
    assert!(guidance(&a).is_none_or(|g| !g.contains("gates off")));
    let typed = press(&mut a, KeyCode::Enter);
    assert!(!lifts(&typed), "{typed:?}");

    let (mut a, _) = gated();
    a.update(ctrl(' '));
    let _ = a.update(plain(KeyCode::Char('m')));
    let in_menu = press(&mut a, KeyCode::Enter);
    assert!(!lifts(&in_menu), "{in_menu:?}");
}

// D24: the provider notice and the failure alert answer Enter first.
#[test]
fn the_provider_notice_and_the_failure_alert_answer_enter_before_the_gate() {
    let (mut a, _) = gated();
    a.orch_notice = Some(chat_id());
    assert!(!guidance(&a).unwrap().contains("gates off"));
    let effects = press(&mut a, KeyCode::Enter);
    assert!(!lifts(&effects), "{effects:?}");
    assert!(offer(&a).is_some(), "the notice took the key");

    let (mut a, _) = gated();
    a.update(event(OrchestraEvent::Halted {
        run: RUN,
        worker: crate::core::ai::orchestra::WorkerId(1),
        class: crate::core::ai::orchestra::worker::ErrorClass::Other,
        text: "boom".into(),
    }));
    assert!(!guidance(&a).unwrap().contains("gates off"));
    let effects = press(&mut a, KeyCode::Enter);
    assert!(!lifts(&effects), "{effects:?}");
    assert!(
        offer(&a).is_some(),
        "the alert keeps the line until answered"
    );
}

// D24: with a delegate offer also pending, the gate answers Enter first: it
// lifts and does not delegate, and the "nothing to delegate" notice is not shown.
#[test]
fn a_pending_override_answers_enter_before_the_delegate_offer() {
    let (mut a, _) = gated();
    a.panes
        .get_mut(&chat_id())
        .and_then(PaneState::chat_mut)
        .unwrap()
        .offer_delegate("plan it".into());
    let effects = press(&mut a, KeyCode::Enter);
    assert!(lifts(&effects) && !starts_a_turn(&effects), "{effects:?}");
    assert_eq!(a.notice(), None);
}

// The line is shown only while a live run can take the lift: with the turn
// over and no run known to the pane (its `Ended` still on the way), it is not.
#[test]
fn no_line_is_offered_when_no_run_is_live() {
    let (mut a, turn) = gated();
    a.update(finish(turn));
    assert!(guidance(&a).is_none_or(|g| !g.contains("gates off")));
    let effects = press(&mut a, KeyCode::Enter);
    assert!(!lifts(&effects), "{effects:?}");
}

// Stale state: the offer ends when the run is cancelled (folder change, resume
// and leaving ORCH are pinned on the chat state).
#[test]
fn the_offer_goes_with_a_cancel_the_toggle_and_the_next_run() {
    let (mut a, _) = gated();
    a.update(ctrl('c'));
    assert!(offer(&a).is_none(), "cancelled");
}

// A live worker has its own keys line; the gate line comes before it, or the
// key would work with no line to name it (D24).
#[test]
fn the_gates_line_comes_before_the_worker_keys() {
    let (mut a, _) = running();
    a.panes.get_mut(&chat_id()).unwrap().size.cols = 80;
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    assert!(
        guidance(&a).unwrap().contains("Ctrl+C"),
        "{:?}",
        guidance(&a)
    );
    a.update(event(refused(Gate::Spawn, Step::Explore)));
    assert_eq!(guidance(&a).as_deref(), Some(LINE));
}

// A new refusal replaces the offer, and a worker row of the SAME run keeps it.
#[test]
fn a_new_refusal_replaces_the_offer_and_a_row_of_the_same_run_keeps_it() {
    let (mut a, _) = gated();
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    assert!(offer(&a).is_some());
    a.update(event(refused(Gate::Finish, Step::Implement)));
    assert_eq!(
        offer(&a).map(|o| (o.gate, o.step)),
        Some((Gate::Finish, Step::Implement))
    );
    a.update(event(row(RunId(8), 1, WorkerStatus::Running)));
    assert!(offer(&a).is_none());
}

// Spec (W3, Only the user overrides): words that look like the override, in
// the model's stream or pasted as the user's own text, lift nothing. Only a
// fresh Enter press on the empty composer does.
#[test]
fn text_that_names_the_override_lifts_nothing() {
    let (mut a, turn) = gated();
    let mut effects = a.update(super::chat_live::delta(turn, "Enter gates off. gates off."));
    type_text(&mut a, "Enter gates off");
    effects.extend(press(&mut a, KeyCode::Enter));
    assert!(!lifts(&effects), "{effects:?}");
    assert!(
        offer(&a).is_some(),
        "the offer stands: only a key press takes it"
    );
}
