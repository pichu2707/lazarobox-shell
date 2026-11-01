//! The ORCH mode of a chat pane (agent-orchestrator D2): the Ctrl+O toggle,
//! its debounce and the global gate, and the ORCH badge on screen.

use super::chat_live::{
    answered, ask, chat_id, chat_of, composer, ctrl, delta, ready_app, repeat, type_text,
};
use super::orchestra_send::{orch_app, toggle};
use super::*;
use crate::app::orchestra::OrchHint;
use crate::core::ai::chat::{Entry, ReasoningState, engine::StreamEvent};
use crate::core::clipboard::osc52_only;
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

/// The whole screen as the app draws it, one trimmed row per line.
pub(super) fn screen(a: &App) -> String {
    screen_at(a, 80, 25)
}

/// [`screen`] on a terminal of `cols` x `rows`.
pub(super) fn screen_at(a: &App, cols: u16, rows: u16) -> String {
    use ratatui::{Terminal, backend::TestBackend};
    let theme = crate::ui::theme::LazaroboxTheme::default();
    let mut terminal = Terminal::new(TestBackend::new(cols, rows)).unwrap();
    terminal
        .draw(|frame| crate::ui::render(frame, a, &theme))
        .unwrap();
    let buf = terminal.backend().buffer().clone();
    (0..buf.area.height)
        .map(|y| {
            let row: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
            row.trim_end().to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// chat-pane / Badge follows both switches: gate off (with the pane's mode
// on), gate on with ORCH off, then both on.
#[test]
fn the_orch_badge_follows_the_gate_and_the_pane_mode() {
    let mut a = orch_app();
    toggle(&mut a);
    assert!(orch(&a, chat_id()));
    a.ai.orchestrator.enabled = false;
    assert!(!screen(&a).contains("ORCH"), "gate off:\n{}", screen(&a));
    a.ai.orchestrator.enabled = true;
    toggle(&mut a);
    assert!(!orch(&a, chat_id()));
    assert!(!screen(&a).contains("ORCH"), "ORCH off:\n{}", screen(&a));
    toggle(&mut a);
    let shown = screen(&a);
    assert!(shown.lines().next().unwrap().ends_with(" ORCH"), "{shown}");
    insta::assert_snapshot!("orch_badge_on_screen", shown);
}

// modal-input / Ctrl+O toggles: on, then off; a second later is a new press.
#[test]
fn ctrl_o_turns_the_orch_mode_on_then_off() {
    let mut a = gate_on();
    assert!(!orch(&a, chat_id()), "a new pane starts with ORCH off");
    assert_eq!(a.orch_hint(chat_id()), Some(OrchHint::Toggle));
    // The first ORCH of a start asks the store for interrupted runs (S9).
    let recover = Effect::Store(StoreCmd::RecoverRuns { pane: chat_id() });
    assert_eq!(at(&mut a, 0, ctrl('o')), vec![recover]);
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
    assert_eq!(a.notice(), Some(osc52_only(false).as_str()));
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

// chat-pane / Toggle blocked during a run: a run is active from its
// orchestrator turn on; one that sent nothing past its turn is over with it
// (a run that outlives its turn: `orchestra_run.rs`).
#[test]
fn the_toggle_is_blocked_while_a_run_is_active() {
    let mut a = orch_app();
    toggle(&mut a);
    let turn = match ask(&mut a, "plan it").as_slice() {
        [.., Effect::OrchestrateSend { turn, .. }] => *turn,
        other => panic!("an orchestrator turn: {other:?}"),
    };
    assert_eq!(a.orch_hint(chat_id()), Some(OrchHint::CancelRunFirst));
    assert_eq!(toggle(&mut a), vec![]);
    assert!(orch(&a, chat_id()), "the mode stays on");
    a.update(super::chat_live::finish(turn));
    assert_eq!(a.orch_hint(chat_id()), Some(OrchHint::Toggle));
    toggle(&mut a);
    assert!(
        !orch(&a, chat_id()),
        "with the run over the key works again"
    );
}

// modal-input / ORCH toggle key: the key is shown in the chat footer hint
// while the gate is on, and only then.
#[test]
fn the_footer_hint_names_ctrl_o_only_with_the_gate_on() {
    let mut a = orch_app();
    let hint = |a: &App| a.chat_guidance(chat_id());
    assert_eq!(hint(&a).as_deref(), Some("Ctrl+O: orchestrate this pane"));
    toggle(&mut a);
    assert_eq!(hint(&a).as_deref(), Some("Ctrl+O: stop orchestrating"));
    assert!(screen(&a).contains("Ctrl+O: stop"), "{}", screen(&a));
    a.ai.orchestrator.enabled = false;
    assert_eq!(hint(&a), None);
    assert!(!screen(&a).contains("Ctrl+O"), "gate off:\n{}", screen(&a));
}

// chat-pane / Toggle blocked during a run: the footer hint says to cancel
// the run first, and so does a notice when the key is pressed.
#[test]
fn the_footer_hint_says_to_cancel_the_run_first() {
    let mut a = orch_app();
    toggle(&mut a);
    ask(&mut a, "plan it");
    // The provider notice comes first; the hint is what is left under it.
    a.orch_notice = None;
    let hint = a.chat_guidance(chat_id()).expect("a hint");
    assert!(hint.contains("cancel the run first"), "{hint}");
    toggle(&mut a);
    assert!(
        a.notice()
            .is_some_and(|n| n.contains("cancel the run first")),
        "{:?}",
        a.notice()
    );
}
