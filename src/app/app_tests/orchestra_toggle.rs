//! The ORCH mode of a chat pane (agent-orchestrator D2): the Ctrl+O toggle,
//! its debounce and the global gate. State asserts only; the badge is drawn
//! in S1e-2.

use super::chat_live::{
    answered, ask, chat_id, chat_of, composer, ctrl, delta, ready_app, repeat, type_text,
};
use super::*;
use crate::app::orchestra::OrchHint;
use crate::core::ai::chat::{Entry, ReasoningState, engine::StreamEvent};
use crate::core::copy::osc52;

/// `a` with the global orchestrator gate (`[ai.orchestrator] enabled`) set.
fn gated(mut a: App, on: bool) -> App {
    a.ai.orchestrator.enabled = on;
    a
}

/// A ready chat (pane 2) with the gate on, on the test clock.
fn gate_on() -> App {
    gated(ready_app(), true).with_clock(mock_now)
}

fn orch(a: &App, id: PaneId) -> bool {
    a.pane_view(id)
        .and_then(|view| match view {
            PaneView::Chat(chat) => Some(chat.orch()),
            _ => None,
        })
        .expect("a chat pane")
}

fn no_pty_write(effects: &[Effect]) -> bool {
    effects.iter().all(|e| !matches!(e, Effect::WritePty(..)))
}

// modal-input / Ctrl+O toggles: on, then off; a second later is a new press.
#[test]
fn ctrl_o_turns_the_orch_mode_on_then_off() {
    let mut a = gate_on();
    assert!(!orch(&a, chat_id()), "a new pane starts with ORCH off");
    assert_eq!(a.orch_hint(chat_id()), Some(OrchHint::Toggle));
    assert_eq!(at(&mut a, 0, ctrl('o')), vec![]);
    assert!(orch(&a, chat_id()));
    assert!(a.orchestrating(chat_id()));
    assert_eq!(at(&mut a, 1000, ctrl('o')), vec![]);
    assert!(!orch(&a, chat_id()));
    assert!(!a.orchestrating(chat_id()));
}

// modal-input / Held key toggles once.
#[test]
fn a_held_ctrl_o_toggles_once() {
    let mut a = gate_on();
    at(&mut a, 0, repeat(KeyCode::Char('o'), KeyModifiers::CONTROL));
    assert!(!orch(&a, chat_id()), "a Repeat does not toggle");
    // A terminal without event types repeats as Presses close together.
    at(&mut a, 1000, ctrl('o'));
    at(&mut a, 1100, ctrl('o'));
    at(
        &mut a,
        1200,
        repeat(KeyCode::Char('o'), KeyModifiers::CONTROL),
    );
    assert!(orch(&a, chat_id()), "the hold toggled once");
}

// chat-pane / Toggle is per pane.
#[test]
fn the_toggle_changes_only_the_focused_pane() {
    let mut a = gate_on();
    press(&mut a, "c");
    let b = a.focused();
    assert_ne!(b, chat_id());
    at(&mut a, 0, ctrl('o'));
    assert!(a.orchestrating(b));
    assert!(!a.orchestrating(chat_id()));
    assert!(!orch(&a, chat_id()));
}

// chat-pane / Toggle does nothing with the gate off.
#[test]
fn with_the_gate_off_ctrl_o_does_nothing_and_reaches_no_pty() {
    let mut a = gated(ready_app(), false).with_clock(mock_now);
    type_text(&mut a, "draft");
    let effects = at(&mut a, 0, ctrl('o'));
    assert_eq!(effects, vec![]);
    assert!(no_pty_write(&effects));
    assert!(!orch(&a, chat_id()));
    assert!(!a.orchestrating(chat_id()));
    // What the badge and the footer hint are drawn from stays as it was.
    assert_eq!(a.orch_hint(chat_id()), None, "no hint with the gate off");
    assert_eq!(composer(&a), "draft");
}

// Existing keys unchanged (modal-input), with the gate on and ORCH off: one
// test per key the change shares the table and the debounce with.

#[test]
fn ctrl_c_still_cancels_a_running_stream() {
    let mut a = gate_on();
    let [Effect::ChatSend { turn, .. }] = ask(&mut a, "hi")[..] else {
        panic!("one send");
    };
    a.update(delta(turn, "Hel"));
    assert_eq!(a.update(ctrl('c')), vec![Effect::ChatCancel(chat_id())]);
    assert_eq!(chat_of(&a).phase(), Phase::Idle);
    assert!(!orch(&a, chat_id()));
}

#[test]
fn ctrl_y_still_copies_the_last_answer() {
    let mut a = gated(answered("hello"), true);
    let effects = a.update(ctrl('y'));
    assert_eq!(effects, vec![Effect::SetClipboard(osc52("hello").unwrap())]);
    assert_eq!(a.notice(), Some("Copied 5 characters"));
    assert!(!orch(&a, chat_id()));
}

#[test]
fn ctrl_b_still_copies_the_last_code_block() {
    let mut a = gated(answered("see\n```\nls -la\n```\n"), true);
    let effects = a.update(ctrl('b'));
    assert_eq!(
        effects,
        vec![Effect::SetClipboard(osc52("ls -la").unwrap())]
    );
    assert!(!orch(&a, chat_id()));
}

#[test]
fn ctrl_r_still_toggles_the_reasoning() {
    let mut a = gate_on();
    let [Effect::ChatSend { turn, .. }] = ask(&mut a, "q")[..] else {
        panic!("one send");
    };
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::ThinkingDelta("Let me check".into()),
    ));
    a.update(delta(turn, "Hi"));
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Done(crate::core::ai::chat::engine::Finish::Stop),
    ));
    assert_eq!(at(&mut a, 0, ctrl('r')), vec![]);
    let states: Vec<_> = chat_of(&a)
        .entries()
        .iter()
        .filter_map(|e| match e {
            Entry::Reasoning(r) => Some(r.state()),
            _ => None,
        })
        .collect();
    assert_eq!(states, [ReasoningState::Expanded]);
    assert!(!orch(&a, chat_id()));
}

#[test]
fn ctrl_j_still_adds_a_newline() {
    let mut a = gate_on();
    type_text(&mut a, "hi");
    assert_eq!(a.update(ctrl('j')), vec![]);
    assert_eq!(composer(&a), "hi\n");
    assert!(!orch(&a, chat_id()));
}

#[test]
fn ctrl_u_still_clears_the_composer() {
    let mut a = gate_on();
    type_text(&mut a, "draft");
    assert_eq!(a.update(ctrl('u')), vec![]);
    assert_eq!(composer(&a), "");
    assert!(!orch(&a, chat_id()));
}
