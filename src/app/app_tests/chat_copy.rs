//! COPY on a chat pane (pane-scroll S1): entering, the key table, leaving, the
//! debounce of a held exit key and the resets.

use std::time::{Duration, Instant};

use super::chat_live::{
    answered, ask, chat_id, chat_of, code, composer, ctrl, delta, plain, ready_app, repeat,
    type_text,
};
use super::chat_summary::with_draft;
use super::*;
use crate::core::ai::chat::{
    engine::{Finish, StreamEvent},
    tag_editor::TagTarget,
    transcript::Scroll,
};

const REFUSED: &str = "nothing to scroll here";

/// A chat with 100+ transcript lines, the draft `hi` in the composer.
fn long() -> App {
    let mut a = answered(&"line\n".repeat(100));
    type_text(&mut a, "hi");
    a
}

fn in_chat_copy(a: &App) -> bool {
    matches!(a.input(), InputMode::ChatCopy(_))
}

fn width(a: &App) -> usize {
    a.tiling()
        .panes
        .iter()
        .find(|(id, _)| *id == chat_id())
        .map(|(_, rect)| usize::from(rect.width))
        .expect("the chat pane")
}

/// Lines below the viewport.
fn below(a: &App) -> usize {
    let chat = chat_of(a);
    let total = chat.lines(width(a)).len();
    let rows = a.chat_viewport(chat_id());
    total - rows - chat.scroll().top(total, rows)
}

fn rows(a: &App) -> usize {
    a.chat_viewport(chat_id())
}

fn shell_offset(a: &App) -> usize {
    a.panes
        .get(&PaneId::FIRST)
        .and_then(PaneState::pty)
        .expect("the shell")
        .emu
        .scrollback_offset()
}

fn enter(a: &mut App) {
    assert_eq!(press(a, "["), vec![]);
    assert!(in_chat_copy(a), "{:?}", a.input());
}

fn keys(a: &mut App, text: &str) {
    for c in text.chars() {
        a.update(key(c));
    }
}

thread_local! {
    static NOW_MS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static BASE: Instant = Instant::now();
}

fn mock_now() -> Instant {
    BASE.with(|b| *b + Duration::from_millis(NOW_MS.with(|n| n.get())))
}

fn at(a: &mut App, ms: u64, ev: AppEvent) -> Vec<Effect> {
    NOW_MS.with(|n| n.set(ms));
    a.update(ev)
}

// Entering ----------------------------------------------------------------------

#[test]
fn prefix_bracket_on_a_conversation_enters_chat_copy_quietly() {
    let mut a = long();
    let effects = press(&mut a, "[");
    assert_eq!(effects, vec![]);
    assert!(in_chat_copy(&a), "{:?}", a.input());
    assert_eq!(a.input().label(), "COPY");
    assert_eq!(a.notice(), None);
    assert_eq!(composer(&a), "hi");
    assert!(a.take_dirty());
}

#[test]
fn entering_copy_on_a_short_conversation_is_allowed() {
    let mut a = answered("short");
    enter(&mut a);
    keys(&mut a, "k");
    assert_eq!(chat_of(&a).scroll(), Scroll::Follow);
}

#[test]
fn the_old_refusal_notice_is_gone() {
    let mut a = launched(80, 25);
    a.open_chat(None);
    press(&mut a, "[");
    assert_ne!(
        a.notice(),
        Some("copy mode is not available in chat panes; Ctrl+Y copies the last answer")
    );
}

fn refused(mut a: App) {
    let effects = press(&mut a, "[");
    assert_eq!(effects, vec![]);
    assert_eq!(a.input(), InputMode::Terminal);
    assert_eq!(a.notice(), Some(REFUSED));
    a.update(plain(KeyCode::Right));
    assert_eq!(a.notice(), None, "the next key clears it");
}

#[test]
fn the_conversation_list_refuses_copy() {
    let mut a = answered("hello");
    a.focused_state_mut().chat_mut().unwrap().open_list();
    refused(a);
}

#[test]
fn the_tag_editor_refuses_copy() {
    let mut a = answered("hello");
    assert!(
        a.focused_state_mut()
            .chat_mut()
            .unwrap()
            .open_tag_editor(TagTarget::Pane)
    );
    refused(a);
}

#[test]
fn the_note_editor_refuses_copy() {
    let mut a = answered("hello");
    a.focused_state_mut()
        .chat_mut()
        .unwrap()
        .open_note_editor()
        .unwrap();
    refused(a);
}

#[test]
fn the_summary_draft_refuses_copy() {
    refused(with_draft("a summary"));
}

#[test]
fn guidance_with_no_transcript_refuses_copy() {
    let mut a = launched(80, 25);
    a.open_chat(None);
    assert!(a.chat_guidance(chat_id()).is_some(), "no engine: guidance");
    refused(a);
}

#[test]
fn copy_on_a_shell_pane_beside_a_chat_is_unchanged() {
    let mut a = ready_app();
    press(&mut a, "h");
    assert_eq!(a.focused(), PaneId::FIRST);
    press(&mut a, "[");
    assert!(matches!(a.input(), InputMode::Copy(_)), "{:?}", a.input());
}

// Keys --------------------------------------------------------------------------

#[test]
fn j_and_k_scroll_one_line() {
    let mut a = long();
    enter(&mut a);
    keys(&mut a, "kk");
    assert_eq!(below(&a), 2);
    keys(&mut a, "j");
    assert_eq!(below(&a), 1);
    assert!(in_chat_copy(&a));
    keys(&mut a, "j");
    assert_eq!(below(&a), 0);
    assert_eq!(chat_of(&a).scroll(), Scroll::Follow);
    assert!(in_chat_copy(&a), "reaching the bottom does not leave COPY");
}

#[test]
fn ctrl_u_and_ctrl_d_move_half_a_viewport() {
    let mut a = long();
    enter(&mut a);
    let half = rows(&a) / 2;
    a.update(ctrl('u'));
    assert_eq!(below(&a), half);
    a.update(ctrl('u'));
    assert_eq!(below(&a), 2 * half);
    a.update(ctrl('d'));
    assert_eq!(below(&a), half);
    a.update(ctrl('d'));
    assert_eq!(below(&a), 0);
}

#[test]
fn gg_goes_to_the_top_and_capital_g_to_the_bottom() {
    let mut a = long();
    enter(&mut a);
    keys(&mut a, "gg");
    assert_eq!(chat_of(&a).scroll(), Scroll::Pinned(0));
    a.update(code(KeyCode::Char('G'), KeyModifiers::SHIFT));
    assert_eq!(chat_of(&a).scroll(), Scroll::Follow);
    assert!(in_chat_copy(&a));
}

#[test]
fn a_lone_g_is_discarded() {
    let mut a = long();
    enter(&mut a);
    keys(&mut a, "gjg");
    assert_ne!(chat_of(&a).scroll(), Scroll::Pinned(0));
    keys(&mut a, "k");
    // `g`, `k`, then `g` again: still no `gg`.
    assert_ne!(chat_of(&a).scroll(), Scroll::Pinned(0));
}

#[test]
fn motions_clamp_at_both_ends() {
    let mut a = long();
    enter(&mut a);
    keys(&mut a, "gg");
    keys(&mut a, "k");
    a.update(ctrl('u'));
    assert_eq!(chat_of(&a).scroll(), Scroll::Pinned(0));
    keys(&mut a, "G");
    keys(&mut a, "j");
    a.update(ctrl('d'));
    assert_eq!(chat_of(&a).scroll(), Scroll::Follow);
}

#[test]
fn a_short_transcript_does_not_move() {
    let mut a = answered("short");
    enter(&mut a);
    keys(&mut a, "kgg");
    a.update(ctrl('u'));
    assert_eq!(chat_of(&a).scroll(), Scroll::Follow);
    assert!(in_chat_copy(&a));
}

#[test]
fn repeat_scrolls_for_j_but_not_for_the_exit_keys() {
    let mut a = long();
    enter(&mut a);
    a.update(repeat(KeyCode::Char('k'), KeyModifiers::NONE));
    a.update(repeat(KeyCode::Char('k'), KeyModifiers::NONE));
    assert_eq!(below(&a), 2);
    a.update(repeat(KeyCode::Char('j'), KeyModifiers::NONE));
    assert_eq!(below(&a), 1);
    a.update(repeat(KeyCode::Esc, KeyModifiers::NONE));
    a.update(repeat(KeyCode::Char('q'), KeyModifiers::NONE));
    assert!(in_chat_copy(&a));
}

#[test]
fn other_keys_are_swallowed_without_effects() {
    let mut a = long();
    enter(&mut a);
    let mut effects = Vec::new();
    for ev in [
        key('a'),
        key('x'),
        key('v'),
        code(KeyCode::Char('V'), KeyModifiers::SHIFT),
        key('y'),
        key('i'),
        key('h'),
        key('l'),
        key('0'),
        key('$'),
        plain(KeyCode::Enter),
        plain(KeyCode::Backspace),
        plain(KeyCode::Tab),
        ctrl('y'),
        ctrl('c'),
    ] {
        effects.extend(a.update(ev));
    }
    assert_eq!(effects, vec![]);
    assert!(in_chat_copy(&a));
    assert_eq!(composer(&a), "hi");
    assert_eq!(chat_of(&a).scroll(), Scroll::Follow);
}

#[test]
fn ctrl_space_inside_chat_copy_gives_prefix() {
    let mut a = long();
    enter(&mut a);
    assert_eq!(a.update(ctrl_space()), vec![]);
    assert_eq!(a.input(), InputMode::Prefix);
}

// Leaving -----------------------------------------------------------------------

#[test]
fn esc_and_q_leave_to_terminal_following() {
    for exit in [esc(), key('q')] {
        let mut a = long();
        enter(&mut a);
        keys(&mut a, "kkk");
        assert_ne!(chat_of(&a).scroll(), Scroll::Follow);
        assert_eq!(a.update(exit), vec![]);
        assert_eq!(a.input(), InputMode::Terminal);
        assert_eq!(chat_of(&a).scroll(), Scroll::Follow);
        assert_eq!(composer(&a), "hi");
    }
}

#[test]
fn a_held_exit_key_does_not_reach_the_composer() {
    let mut a = long().with_clock(mock_now);
    at(&mut a, 0, ctrl_space());
    at(&mut a, 0, key('['));
    assert!(in_chat_copy(&a));
    at(&mut a, 0, key('q'));
    assert_eq!(a.input(), InputMode::Terminal);
    for ms in [400, 433, 466] {
        assert_eq!(at(&mut a, ms, key('q')), vec![], "q at {ms}");
    }
    assert_eq!(
        at(&mut a, 500, repeat(KeyCode::Char('q'), KeyModifiers::NONE)),
        vec![]
    );
    assert_eq!(composer(&a), "hi");
    // A deliberate press after the window is typed.
    at(&mut a, 1500, key('q'));
    assert_eq!(composer(&a), "hiq");
}

#[test]
fn a_held_esc_does_not_cancel_or_edit_after_leaving() {
    let mut a = long().with_clock(mock_now);
    at(&mut a, 0, ctrl_space());
    at(&mut a, 0, key('['));
    at(&mut a, 0, esc());
    for ms in [300, 400] {
        assert_eq!(at(&mut a, ms, esc()), vec![], "esc at {ms}");
    }
    assert_eq!(composer(&a), "hi");
}

// While in COPY -----------------------------------------------------------------

#[test]
fn the_view_stays_on_the_same_content_while_a_stream_runs() {
    let mut a = ready_app();
    let turn = match ask(&mut a, "q").as_slice() {
        [Effect::ChatSend { turn, .. }] => *turn,
        other => panic!("expected one ChatSend, got {other:?}"),
    };
    let numbered: String = (0..60).map(|i| format!("row {i}\n")).collect();
    a.update(delta(turn, &numbered));
    enter(&mut a);
    keys(&mut a, "kkkkkkkkkk");
    let w = width(&a);
    let text_at_top = |a: &App| {
        let chat = chat_of(a);
        let lines = chat.lines(w);
        lines[chat.scroll().top(lines.len(), rows(a))].text.clone()
    };
    let pinned = text_at_top(&a);
    a.update(delta(turn, "fresh 1\nfresh 2\nfresh 3\nfresh 4\nfresh 5\n"));
    assert_eq!(text_at_top(&a), pinned, "the same content stays visible");
    assert_eq!(below(&a), 15, "the new lines grew below the view");
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Done(Finish::Stop),
    ));
    assert!(in_chat_copy(&a), "the stream ending does not leave COPY");
}

#[test]
fn chat_copy_leaves_the_shell_offsets_alone() {
    let mut a = long();
    enter(&mut a);
    keys(&mut a, "kkgg");
    assert_eq!(shell_offset(&a), 0);
}

// Resets ------------------------------------------------------------------------

/// Ctrl+Space ends chat COPY (it is PREFIX), so a focus or tab change after it
/// is an ordinary one: the view stays where COPY left it (R1).
#[test]
fn a_focus_change_after_ctrl_space_leaves_the_chat_pinned() {
    let mut a = long();
    enter(&mut a);
    keys(&mut a, "kkk");
    let pinned = chat_of(&a).scroll();
    assert!(matches!(pinned, Scroll::Pinned(_)));
    a.update(ctrl_space());
    assert_eq!(a.input(), InputMode::Prefix);
    keys(&mut a, "h");
    assert_eq!(a.focused(), PaneId::FIRST);
    assert_eq!(a.input(), InputMode::Terminal);
    assert_eq!(chat_of(&a).scroll(), pinned);
    assert_eq!(a.scrolled_by(chat_id()), Some(3), "the badge keeps showing");
}

#[test]
fn a_tab_change_after_ctrl_space_leaves_the_chat_pinned() {
    let mut a = long();
    a.new_tab();
    a.activate(0);
    assert_eq!(a.focused(), chat_id());
    enter(&mut a);
    keys(&mut a, "kkk");
    let pinned = chat_of(&a).scroll();
    a.update(ctrl_space());
    keys(&mut a, "gb");
    assert_eq!(a.active, 1);
    assert_eq!(a.input(), InputMode::Terminal);
    assert_eq!(chat_of(&a).scroll(), pinned);
    a.activate(0);
    assert_eq!(a.scrolled_by(chat_id()), Some(3));
}

#[test]
fn a_held_g_does_not_jump_to_the_top() {
    let mut a = long();
    enter(&mut a);
    a.update(repeat(KeyCode::Char('g'), KeyModifiers::NONE));
    a.update(repeat(KeyCode::Char('g'), KeyModifiers::NONE));
    assert_eq!(chat_of(&a).scroll(), Scroll::Follow);
    // A held `G` does not act either.
    keys(&mut a, "gg");
    assert_eq!(chat_of(&a).scroll(), Scroll::Pinned(0));
    a.update(repeat(KeyCode::Char('G'), KeyModifiers::SHIFT));
    assert_eq!(chat_of(&a).scroll(), Scroll::Pinned(0));
    a.update(code(KeyCode::Char('G'), KeyModifiers::SHIFT));
    assert_eq!(chat_of(&a).scroll(), Scroll::Follow);
}
