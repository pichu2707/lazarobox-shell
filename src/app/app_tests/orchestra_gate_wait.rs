//! A run that waits for the user's reply after a spawn gate refusal
//! (agent-orchestrator [W3], option B): the pane says so, the reply attaches
//! to the same run, and every ending is visible.

use super::chat_live::{ask, chat_id, ctrl, finish, plain};
use super::orchestra_run::{RUN, event, running};
use super::orchestra_send::toggle;
use super::*;
use crate::core::ai::{
    chat::Entry,
    orchestra::{
        event::OrchestraEvent,
        steps::{Gate, Step},
    },
};

const WAITS: &str = "run waits for your reply: step explore is not closed (Enter gates off)";

fn waiting() -> (App, u64) {
    let (mut a, turn) = running();
    a.update(event(OrchestraEvent::GateRefused {
        run: RUN,
        gate: Gate::Spawn,
        step: Step::Explore,
        reason: Gate::Spawn.refusal(Step::Explore),
    }));
    a.update(event(OrchestraEvent::Waits {
        run: RUN,
        step: Step::Explore,
    }));
    a.update(finish(turn));
    (a, turn)
}

fn systems(a: &App) -> Vec<String> {
    let lines = chat_of(a).entries().iter();
    lines
        .filter_map(|e| match e {
            Entry::System(line) => Some(line.clone()),
            _ => None,
        })
        .collect()
}

fn chat_of(a: &App) -> &crate::core::ai::chat::ChatState {
    super::chat_live::chat_of(a)
}

fn attach_of(effects: &[Effect]) -> Option<Option<crate::core::ai::orchestra::RunId>> {
    effects.iter().find_map(|e| match e {
        Effect::OrchestrateSend { orch, .. } => Some(orch.attach),
        _ => None,
    })
}

// The pane says the run waits, ASCII, and the offer of the key stays.
#[test]
fn the_pane_says_the_run_waits_and_keeps_the_gates_offer() {
    let (a, _) = waiting();
    let lines = systems(&a);
    assert!(lines.iter().any(|l| l == WAITS), "{lines:?}");
    assert!(WAITS.is_ascii());
    assert!(chat_of(&a).gate_offer().is_some());
    assert_eq!(chat_of(&a).waiting_run(), Some(RUN));
}

// The typed reply attaches to the waiting run, and the composer is cleared.
#[test]
fn the_reply_attaches_to_the_waiting_run() {
    let (mut a, _) = waiting();
    let effects = ask(&mut a, "go ahead");
    assert_eq!(attach_of(&effects), Some(Some(RUN)), "{effects:?}");
    assert!(chat_of(&a).composer().text().is_empty());
    assert_eq!(chat_of(&a).waiting_run(), None, "the wait is over");
}

// A message sent with no run waiting starts a new run, as before.
#[test]
fn a_message_with_nothing_waiting_starts_a_new_run() {
    let (mut a, turn) = running();
    a.update(finish(turn));
    a.update(event(OrchestraEvent::Ended { run: RUN }));
    let effects = ask(&mut a, "again");
    assert_eq!(attach_of(&effects), Some(None), "{effects:?}");
}

// The run ending says the wait is over, and a later message is a new run.
#[test]
fn the_run_ending_ends_the_wait_with_a_line() {
    let (mut a, _) = waiting();
    a.update(event(OrchestraEvent::Ended { run: RUN }));
    assert_eq!(chat_of(&a).waiting_run(), None);
    let lines = systems(&a);
    assert!(
        lines
            .iter()
            .any(|l| l == "run no longer waits for your reply: the run ended"),
        "{lines:?}"
    );
    let effects = ask(&mut a, "again");
    assert_eq!(attach_of(&effects), Some(None), "{effects:?}");
}

// Ctrl+C cancels the waiting run.
#[test]
fn cancel_ends_the_wait() {
    let (mut a, _) = waiting();
    let effects = a.update(ctrl('c'));
    assert!(
        effects.contains(&Effect::ChatCancel(chat_id())),
        "{effects:?}"
    );
    assert_eq!(chat_of(&a).waiting_run(), None);
}

// ORCH toggled off while the run waits: the run is cancelled, said, and the
// toggle is not refused with the "run active" hint.
#[test]
fn toggling_orch_off_ends_the_wait_and_cancels_the_run() {
    let (mut a, _) = waiting();
    let effects = toggle(&mut a);
    assert!(
        effects.contains(&Effect::ChatCancel(chat_id())),
        "{effects:?}"
    );
    assert!(!chat_of(&a).orch(), "ORCH is off");
    assert_eq!(chat_of(&a).waiting_run(), None);
    let lines = systems(&a);
    assert!(
        lines
            .iter()
            .any(|l| l == "run no longer waits for your reply: ORCH was toggled off"),
        "{lines:?}"
    );
}

// Enter on an empty composer still lifts the gates while the run waits.
#[test]
fn enter_still_lifts_while_the_run_waits() {
    let (mut a, _) = waiting();
    let later = NOW_MS.with(|n| n.get()) + 1000;
    let effects = at(&mut a, later, plain(KeyCode::Enter));
    assert!(
        effects.contains(&Effect::OrchGatesOff {
            pane: chat_id(),
            run: RUN,
        }),
        "{effects:?}"
    );
    assert_eq!(chat_of(&a).waiting_run(), Some(RUN), "still waiting");
}
