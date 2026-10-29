//! The mouse wheel over a pane (pane-scroll S2): the hit-test, the modes that
//! ignore it, the routing per pane kind, and the return to the live screen.

use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

use super::chat_live::{answered, chat_id, chat_of, ready_app, type_text};
use super::chat_summary::with_draft;
use super::*;
use crate::core::{
    ai::chat::{tag_editor::TagTarget, transcript::Scroll},
    layout::Rect,
};

const SGR_ON: &[u8] = b"\x1b[?1000h\x1b[?1006h";

// Helpers ------------------------------------------------------------------------

fn mouse_event(
    kind: MouseEventKind,
    (column, row): (u16, u16),
    modifiers: KeyModifiers,
) -> AppEvent {
    AppEvent::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers,
    })
}

fn wheel(a: &mut App, kind: MouseEventKind, at: (u16, u16)) -> Vec<Effect> {
    a.update(mouse_event(kind, at, KeyModifiers::NONE))
}

fn up(a: &mut App, at: (u16, u16)) -> Vec<Effect> {
    wheel(a, MouseEventKind::ScrollUp, at)
}

fn down(a: &mut App, at: (u16, u16)) -> Vec<Effect> {
    wheel(a, MouseEventKind::ScrollDown, at)
}

fn up_on(a: &mut App, id: PaneId) -> Vec<Effect> {
    let at = inside(a, id);
    up(a, at)
}

fn down_on(a: &mut App, id: PaneId) -> Vec<Effect> {
    let at = inside(a, id);
    down(a, at)
}

fn enabled(mut a: App) -> App {
    a.config.mouse = true;
    a
}

fn rect_of(a: &App, id: PaneId) -> Rect {
    a.tiling()
        .panes
        .iter()
        .find(|(pane, _)| *pane == id)
        .map(|(_, rect)| *rect)
        .expect("the pane is laid out")
}

/// A cell in the middle of pane `id`.
fn inside(a: &App, id: PaneId) -> (u16, u16) {
    let r = rect_of(a, id);
    (r.x + r.width / 2, r.y + r.height / 2)
}

fn output(a: &mut App, id: PaneId, bytes: &[u8]) {
    a.update(AppEvent::Pty(id, PtyEvent::Output(bytes.to_vec())));
}

fn lines(a: &mut App, id: PaneId, count: usize) {
    for i in 0..count {
        output(a, id, format!("line{i}\r\n").as_bytes());
    }
}

fn offset(a: &App, id: PaneId) -> usize {
    a.pane(id).expect("a shell pane").scrollback_offset()
}

/// One pane with 60 lines of output, mouse on.
fn single() -> App {
    let mut a = enabled(launched(80, 25));
    lines(&mut a, PaneId::FIRST, 60);
    a
}

/// Panes 1 (left, focused) and 2 (right), both with history, mouse on.
fn side_by_side() -> App {
    let mut a = enabled(launched(80, 25));
    press(&mut a, "wv");
    lines(&mut a, PaneId::FIRST, 60);
    lines(&mut a, id2(), 60);
    press(&mut a, "h");
    assert_eq!(a.focused(), PaneId::FIRST);
    a.take_dirty();
    a
}

fn history(a: &App, id: PaneId) -> usize {
    a.pane(id).expect("a shell pane").scrollback_len()
}

// Hit-test ------------------------------------------------------------------------

#[test]
fn the_wheel_scrolls_the_pane_under_the_pointer_without_moving_focus() {
    let mut a = side_by_side();
    let effects = up_on(&mut a, id2());
    assert_eq!(effects, vec![]);
    assert_eq!(offset(&a, id2()), 3);
    assert_eq!(offset(&a, PaneId::FIRST), 0);
    assert_eq!(a.focused(), PaneId::FIRST);
    assert_eq!(a.input(), InputMode::Terminal);
    assert!(a.take_dirty());
}

#[test]
fn the_wheel_scrolls_the_focused_pane_too() {
    let mut a = side_by_side();
    up_on(&mut a, PaneId::FIRST);
    assert_eq!(offset(&a, PaneId::FIRST), 3);
    assert_eq!(offset(&a, id2()), 0);
}

#[test]
fn a_separator_column_is_ignored() {
    let mut a = side_by_side();
    let row = inside(&a, PaneId::FIRST).1;
    let tiling = a.tiling();
    let gap: Vec<u16> = (0..80)
        .filter(|x| crate::core::layout::pane_at(&tiling, *x, row).is_none())
        .collect();
    assert!(!gap.is_empty(), "two panes leave a separator column");
    for x in gap {
        assert_eq!(up(&mut a, (x, row)), vec![]);
    }
    assert_eq!(offset(&a, PaneId::FIRST), 0);
    assert_eq!(offset(&a, id2()), 0);
    assert!(!a.take_dirty());
}

#[test]
fn rows_outside_the_body_and_columns_beyond_it_are_ignored() {
    let mut a = enabled(launched(80, 25));
    lines(&mut a, PaneId::FIRST, 60);
    press(&mut a, "tn");
    lines(&mut a, id2(), 60);
    a.take_dirty();
    let body = a.screen().body;
    let outside: Vec<u16> = (0..25)
        .filter(|y| !(body.y..body.y + body.height).contains(y))
        .collect();
    assert!(outside.len() >= 2, "the tab bar and the statusline");
    for y in outside {
        assert_eq!(up(&mut a, (10, y)), vec![]);
    }
    assert_eq!(up(&mut a, (200, body.y)), vec![]);
    assert_eq!(offset(&a, id2()), 0);
    assert_eq!(offset(&a, PaneId::FIRST), 0, "the other tab is never hit");
    assert!(!a.take_dirty());
}

#[test]
fn a_zoomed_pane_is_the_only_target() {
    let mut a = side_by_side();
    press(&mut a, "wz");
    assert!(a.zoomed());
    // Where the right pane would be: the zoomed left pane answers.
    let far_right = (78, inside(&a, PaneId::FIRST).1);
    up(&mut a, far_right);
    assert_eq!(offset(&a, PaneId::FIRST), 3);
    assert_eq!(offset(&a, id2()), 0);
}

#[test]
fn after_a_split_the_new_pane_owns_its_cells() {
    let mut a = side_by_side();
    press(&mut a, "l");
    press(&mut a, "wh");
    let below = PaneId::for_test(3);
    lines(&mut a, below, 60);
    up_on(&mut a, below);
    assert_eq!(offset(&a, below), 3);
    assert_eq!(offset(&a, id2()), 0, "the pane that was split");
    assert_eq!(offset(&a, PaneId::FIRST), 0);
}

// Events and modes that ignore the wheel -------------------------------------------

#[test]
fn only_vertical_wheel_events_are_handled() {
    let mut a = side_by_side();
    let at = inside(&a, PaneId::FIRST);
    let kinds = [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
        MouseEventKind::Drag(MouseButton::Left),
        MouseEventKind::Moved,
        MouseEventKind::ScrollLeft,
        MouseEventKind::ScrollRight,
    ];
    for kind in kinds {
        assert_eq!(wheel(&mut a, kind, at), vec![], "{kind:?}");
    }
    assert_eq!(offset(&a, PaneId::FIRST), 0);
    assert_eq!(a.input(), InputMode::Terminal);
    assert!(!a.take_dirty());
}

#[test]
fn modifiers_do_not_change_the_step() {
    let mut a = side_by_side();
    let at = inside(&a, PaneId::FIRST);
    a.update(mouse_event(
        MouseEventKind::ScrollUp,
        at,
        KeyModifiers::CONTROL,
    ));
    assert_eq!(offset(&a, PaneId::FIRST), 3);
}

#[test]
fn the_wheel_is_ignored_when_the_mouse_is_off() {
    let mut a = side_by_side();
    a.config.mouse = false;
    assert_eq!(up_on(&mut a, id2()), vec![]);
    assert_eq!(offset(&a, id2()), 0);
    assert!(!a.take_dirty());
}

#[test]
fn the_wheel_is_ignored_outside_terminal_and_copy() {
    type Setup = fn(&mut App);
    let setups: [(&str, Setup); 6] = [
        ("prefix", |a| {
            press(a, "");
        }),
        ("group", |a| {
            press(a, "w");
        }),
        ("resize", |a| {
            press(a, "wr");
        }),
        ("menu", |a| {
            press(a, "m");
        }),
        ("quit confirmation", |a| {
            press(a, "q");
        }),
        ("close confirmation", |a| {
            press(a, "wq");
        }),
    ];
    for (name, setup) in setups {
        let mut a = side_by_side();
        setup(&mut a);
        a.notice = Some("keep me".into());
        a.take_dirty();
        let mode = a.input();
        for pane in [PaneId::FIRST, id2()] {
            assert_eq!(up_on(&mut a, pane), vec![], "{name}");
            assert_eq!(down_on(&mut a, pane), vec![], "{name}");
            assert_eq!(offset(&a, pane), 0, "{name}");
        }
        assert_eq!(a.input(), mode, "{name}");
        assert_eq!(a.notice(), Some("keep me"), "{name}");
        assert!(!a.take_dirty(), "{name}");
    }
}

// Main screen scrollback -----------------------------------------------------------

#[test]
fn the_main_screen_scrolls_back_by_three_and_forward_by_three() {
    let mut a = single();
    let at = inside(&a, PaneId::FIRST);
    assert_eq!(up(&mut a, at), vec![], "no bytes reach the PTY");
    assert_eq!(offset(&a, PaneId::FIRST), 3);
    up(&mut a, at);
    assert_eq!(offset(&a, PaneId::FIRST), 6);
    down(&mut a, at);
    assert_eq!(offset(&a, PaneId::FIRST), 3);
    assert_eq!(a.input(), InputMode::Terminal, "COPY is not entered");
}

#[test]
fn wheel_down_clamps_to_live() {
    let mut a = single();
    let at = inside(&a, PaneId::FIRST);
    a.scroll_shell_for_test(PaneId::FIRST, 2);
    down(&mut a, at);
    assert_eq!(offset(&a, PaneId::FIRST), 0);
    assert_eq!(down(&mut a, at), vec![]);
    assert_eq!(offset(&a, PaneId::FIRST), 0);
}

#[test]
fn wheel_up_clamps_to_the_history() {
    let mut a = single();
    let at = inside(&a, PaneId::FIRST);
    let total = history(&a, PaneId::FIRST);
    a.scroll_shell_for_test(PaneId::FIRST, total - 1);
    up(&mut a, at);
    assert_eq!(offset(&a, PaneId::FIRST), total);
    up(&mut a, at);
    assert_eq!(offset(&a, PaneId::FIRST), total);
}

#[test]
fn a_pane_without_history_stays_live() {
    let mut a = enabled(launched(80, 25));
    assert_eq!(up_on(&mut a, PaneId::FIRST), vec![]);
    assert_eq!(offset(&a, PaneId::FIRST), 0);
}

#[test]
fn a_scrolled_view_stays_anchored_under_new_output() {
    let mut a = single();
    for _ in 0..3 {
        up_on(&mut a, PaneId::FIRST);
    }
    assert_eq!(offset(&a, PaneId::FIRST), 9);
    lines(&mut a, PaneId::FIRST, 5);
    assert_eq!(offset(&a, PaneId::FIRST), 14);
}

// Forwarding to a child in mouse mode ----------------------------------------------

#[test]
fn a_child_in_mouse_mode_gets_an_sgr_wheel_in_its_own_pty_only() {
    let mut a = side_by_side();
    output(&mut a, id2(), SGR_ON);
    let r = rect_of(&a, id2());
    let effects = up(&mut a, (r.x + 4, r.y + 3));
    assert_eq!(
        effects,
        vec![Effect::WritePty(id2(), b"\x1b[<64;5;4M".to_vec())]
    );
    assert_eq!(offset(&a, id2()), 0, "the mouse mode wins over scrollback");
    assert_eq!(a.focused(), PaneId::FIRST);
    let effects = down(&mut a, (r.x, r.y));
    assert_eq!(
        effects,
        vec![Effect::WritePty(id2(), b"\x1b[<65;1;1M".to_vec())]
    );
}

#[test]
fn the_modifier_bits_travel_to_the_child() {
    let mut a = side_by_side();
    output(&mut a, PaneId::FIRST, SGR_ON);
    let r = rect_of(&a, PaneId::FIRST);
    let effects = a.update(mouse_event(
        MouseEventKind::ScrollUp,
        (r.x, r.y),
        KeyModifiers::CONTROL,
    ));
    assert_eq!(
        effects,
        vec![Effect::WritePty(PaneId::FIRST, b"\x1b[<80;1;1M".to_vec())]
    );
}

#[test]
fn a_pane_scrolled_back_still_forwards_to_a_child_in_mouse_mode() {
    let mut a = single();
    a.scroll_shell_for_test(PaneId::FIRST, 10);
    output(&mut a, PaneId::FIRST, SGR_ON);
    let at = inside(&a, PaneId::FIRST);
    let effects = up(&mut a, at);
    assert_eq!(effects.len(), 1);
    assert!(matches!(effects[0], Effect::WritePty(..)));
    assert!(offset(&a, PaneId::FIRST) >= 10, "the view is not touched");
}

#[test]
fn the_alternate_screen_gets_three_arrow_keys() {
    let mut a = single();
    output(&mut a, PaneId::FIRST, b"\x1b[?1049h");
    let at = inside(&a, PaneId::FIRST);
    assert_eq!(
        down(&mut a, at),
        vec![Effect::WritePty(
            PaneId::FIRST,
            b"\x1b[B\x1b[B\x1b[B".to_vec()
        )]
    );
    output(&mut a, PaneId::FIRST, b"\x1b[?1h");
    assert_eq!(
        up(&mut a, at),
        vec![Effect::WritePty(
            PaneId::FIRST,
            b"\x1bOA\x1bOA\x1bOA".to_vec()
        )]
    );
    output(&mut a, PaneId::FIRST, b"\x1b[?1l");
    assert_eq!(
        up(&mut a, at),
        vec![Effect::WritePty(
            PaneId::FIRST,
            b"\x1b[A\x1b[A\x1b[A".to_vec()
        )]
    );
    assert_eq!(offset(&a, PaneId::FIRST), 0);
}

// COPY -----------------------------------------------------------------------------

fn copy_offset(a: &App) -> usize {
    match a.input() {
        InputMode::Copy(state) => state.offset,
        other => panic!("expected COPY, got {other:?}"),
    }
}

#[test]
fn the_wheel_scrolls_the_copy_viewport_and_keeps_copy() {
    let mut a = single();
    press(&mut a, "[");
    let at = inside(&a, PaneId::FIRST);
    assert_eq!(up(&mut a, at), vec![]);
    assert_eq!(copy_offset(&a), 3);
    assert_eq!(offset(&a, PaneId::FIRST), 3);
    down(&mut a, at);
    assert_eq!(copy_offset(&a), 0);
    assert_eq!(offset(&a, PaneId::FIRST), 0);
}

#[test]
fn the_copy_cursor_stays_on_a_visible_row() {
    let mut a = single();
    press(&mut a, "[");
    let at = inside(&a, PaneId::FIRST);
    for _ in 0..5 {
        up(&mut a, at);
        assert!(a.copy_cursor().is_some(), "offset {}", copy_offset(&a));
    }
}

#[test]
fn the_wheel_keeps_the_selection() {
    let mut a = single();
    press(&mut a, "[");
    a.update(key('v'));
    let at = inside(&a, PaneId::FIRST);
    down(&mut a, at);
    up(&mut a, at);
    assert_eq!(a.copy_selection_kind(), Some(SelectionKind::Char));
    assert!(matches!(a.input(), InputMode::Copy(_)));
}

#[test]
fn the_wheel_clamps_in_copy() {
    let mut a = single();
    press(&mut a, "[");
    let at = inside(&a, PaneId::FIRST);
    let total = history(&a, PaneId::FIRST);
    for _ in 0..total {
        up(&mut a, at);
    }
    assert_eq!(copy_offset(&a), total);
    up(&mut a, at);
    assert_eq!(copy_offset(&a), total);
    assert!(matches!(a.input(), InputMode::Copy(_)));
}

#[test]
fn a_pane_beside_the_copy_pane_follows_its_own_rules() {
    let mut a = side_by_side();
    press(&mut a, "[");
    let left = inside(&a, PaneId::FIRST);
    for _ in 0..3 {
        up(&mut a, left);
    }
    let before = a.input();
    assert_eq!(copy_offset(&a), 9);
    assert_eq!(up_on(&mut a, id2()), vec![]);
    assert_eq!(offset(&a, id2()), 3);
    assert_eq!(offset(&a, PaneId::FIRST), 9);
    assert_eq!(a.input(), before, "the COPY state is untouched");
}

#[test]
fn a_mouse_mode_child_beside_the_copy_pane_gets_the_wheel() {
    let mut a = side_by_side();
    output(&mut a, id2(), SGR_ON);
    press(&mut a, "[");
    let before = a.input();
    let r = rect_of(&a, id2());
    assert_eq!(
        up(&mut a, (r.x, r.y)),
        vec![Effect::WritePty(id2(), b"\x1b[<64;1;1M".to_vec())]
    );
    assert_eq!(a.input(), before);
}

#[test]
fn the_wheel_on_an_alternate_screen_in_copy_does_nothing() {
    let mut a = single();
    output(&mut a, PaneId::FIRST, b"\x1b[?1049h");
    press(&mut a, "[");
    let at = inside(&a, PaneId::FIRST);
    assert_eq!(up(&mut a, at), vec![]);
    assert_eq!(down(&mut a, at), vec![]);
    assert_eq!(copy_offset(&a), 0);
    assert_eq!(offset(&a, PaneId::FIRST), 0);
}

// Chat -----------------------------------------------------------------------------

fn long_chat() -> App {
    enabled(answered(&"line\n".repeat(100)))
}

fn width(a: &App) -> usize {
    usize::from(rect_of(a, chat_id()).width)
}

/// Lines below the viewport.
fn below(a: &App) -> usize {
    let chat = chat_of(a);
    let total = chat.lines(width(a)).len();
    let rows = a.chat_viewport(chat_id());
    total - rows - chat.scroll().top(total, rows)
}

#[test]
fn the_wheel_scrolls_a_chat_transcript_by_three_lines() {
    let mut a = long_chat();
    assert_eq!(below(&a), 0);
    assert_eq!(up_on(&mut a, chat_id()), vec![]);
    assert_eq!(below(&a), 3);
    up_on(&mut a, chat_id());
    assert_eq!(below(&a), 6);
    down_on(&mut a, chat_id());
    assert_eq!(below(&a), 3);
    assert_eq!(a.input(), InputMode::Terminal, "COPY is not entered");
    assert_eq!(a.focused(), chat_id());
}

#[test]
fn an_unfocused_chat_scrolls_and_focus_stays() {
    let mut a = long_chat();
    press(&mut a, "h");
    assert_eq!(a.focused(), PaneId::FIRST);
    up_on(&mut a, chat_id());
    assert_eq!(below(&a), 3);
    assert_eq!(a.focused(), PaneId::FIRST);
    assert_eq!(offset(&a, PaneId::FIRST), 0);
}

#[test]
fn wheel_down_to_the_bottom_follows_again() {
    let mut a = long_chat();
    up_on(&mut a, chat_id());
    down_on(&mut a, chat_id());
    assert_eq!(chat_of(&a).scroll(), Scroll::Follow);
    down_on(&mut a, chat_id());
    assert_eq!(chat_of(&a).scroll(), Scroll::Follow);
}

#[test]
fn a_short_transcript_does_not_scroll() {
    let mut a = enabled(answered("short"));
    up_on(&mut a, chat_id());
    assert_eq!(chat_of(&a).scroll(), Scroll::Follow);
}

#[test]
fn the_wheel_in_chat_copy_moves_the_transcript_and_keeps_copy() {
    let mut a = long_chat();
    press(&mut a, "[");
    assert!(matches!(a.input(), InputMode::ChatCopy(_)));
    up_on(&mut a, chat_id());
    assert_eq!(below(&a), 3);
    assert!(matches!(a.input(), InputMode::ChatCopy(_)));
}

#[test]
fn the_wheel_leaves_the_composer_alone() {
    let mut a = long_chat();
    type_text(&mut a, "hi");
    up_on(&mut a, chat_id());
    assert_eq!(chat_of(&a).composer().text(), "hi");
}

fn pin(a: &mut App) {
    a.focused_state_mut()
        .chat_mut()
        .expect("the chat")
        .set_scroll(Scroll::Pinned(1));
}

fn chat_ignores_the_wheel(mut a: App) {
    a.take_dirty();
    assert_eq!(up_on(&mut a, chat_id()), vec![]);
    assert_eq!(down_on(&mut a, chat_id()), vec![]);
    assert_eq!(chat_of(&a).scroll(), Scroll::Pinned(1));
    assert!(!a.take_dirty());
}

#[test]
fn the_conversation_list_ignores_the_wheel() {
    let mut a = long_chat();
    pin(&mut a);
    a.focused_state_mut().chat_mut().unwrap().open_list();
    chat_ignores_the_wheel(a);
}

#[test]
fn the_tag_editor_ignores_the_wheel() {
    let mut a = long_chat();
    pin(&mut a);
    assert!(
        a.focused_state_mut()
            .chat_mut()
            .unwrap()
            .open_tag_editor(TagTarget::Pane)
    );
    chat_ignores_the_wheel(a);
}

#[test]
fn the_note_editor_ignores_the_wheel() {
    let mut a = long_chat();
    pin(&mut a);
    a.focused_state_mut()
        .chat_mut()
        .unwrap()
        .open_note_editor()
        .unwrap();
    chat_ignores_the_wheel(a);
}

#[test]
fn the_summary_draft_ignores_the_wheel() {
    let mut a = enabled(with_draft("a summary"));
    pin(&mut a);
    chat_ignores_the_wheel(a);
}

#[test]
fn guidance_with_no_transcript_ignores_the_wheel() {
    let mut a = enabled(launched(80, 25));
    a.open_chat(None);
    assert!(a.chat_guidance(chat_id()).is_some());
    a.take_dirty();
    assert_eq!(up_on(&mut a, chat_id()), vec![]);
    assert_eq!(chat_of(&a).scroll(), Scroll::Follow);
    assert!(!a.take_dirty());
}

// Back to live and the resets -------------------------------------------------------

#[test]
fn an_encoded_key_returns_the_focused_pane_to_live() {
    let mut a = single();
    for _ in 0..4 {
        up_on(&mut a, PaneId::FIRST);
    }
    assert_eq!(offset(&a, PaneId::FIRST), 12);
    let effects = a.update(key('l'));
    assert_eq!(
        effects,
        vec![Effect::WritePty(PaneId::FIRST, b"l".to_vec())]
    );
    assert_eq!(offset(&a, PaneId::FIRST), 0);
}

#[test]
fn a_paste_returns_the_focused_pane_to_live() {
    let mut a = single();
    for _ in 0..4 {
        up_on(&mut a, PaneId::FIRST);
    }
    a.update(AppEvent::Paste("hi".into()));
    assert_eq!(offset(&a, PaneId::FIRST), 0);
}

#[test]
fn a_swallowed_key_does_not_return_to_live() {
    let mut a = single();
    for _ in 0..4 {
        up_on(&mut a, PaneId::FIRST);
    }
    a.update(ctrl_space());
    a.update(esc());
    assert_eq!(a.input(), InputMode::Terminal);
    assert_eq!(offset(&a, PaneId::FIRST), 12);
}

#[test]
fn a_key_for_one_pane_does_not_touch_another_panes_view() {
    let mut a = side_by_side();
    up_on(&mut a, id2());
    a.update(key('x'));
    assert_eq!(offset(&a, id2()), 3, "only the focused pane goes live");
}

#[test]
fn leaving_the_focused_pane_resets_its_view() {
    let mut a = side_by_side();
    up_on(&mut a, PaneId::FIRST);
    press(&mut a, "l");
    assert_eq!(offset(&a, PaneId::FIRST), 0);
}

#[test]
fn an_unfocused_pane_keeps_its_view_until_it_is_focused_and_left() {
    let mut a = enabled(launched(80, 25));
    press(&mut a, "wv");
    press(&mut a, "wh");
    let (top_right, bottom_right) = (id2(), PaneId::for_test(3));
    assert_eq!(a.focused(), bottom_right);
    for id in [PaneId::FIRST, top_right, bottom_right] {
        lines(&mut a, id, 60);
    }
    up_on(&mut a, PaneId::FIRST);
    up_on(&mut a, PaneId::FIRST);
    assert_eq!(offset(&a, PaneId::FIRST), 6);
    press(&mut a, "k");
    assert_eq!(a.focused(), top_right);
    assert_eq!(offset(&a, PaneId::FIRST), 6, "focus moved between others");
    up_on(&mut a, PaneId::FIRST);
    assert_eq!(offset(&a, PaneId::FIRST), 9, "continues from 6");
    press(&mut a, "h");
    assert_eq!(a.focused(), PaneId::FIRST);
    assert_eq!(offset(&a, PaneId::FIRST), 9, "focusing does not reset it");
    press(&mut a, "l");
    assert_eq!(offset(&a, PaneId::FIRST), 0, "focused and left");
}

#[test]
fn switching_tabs_resets_every_pane_of_the_tab_left() {
    let mut a = side_by_side();
    up_on(&mut a, id2());
    up_on(&mut a, PaneId::FIRST);
    assert_eq!((offset(&a, id2()), offset(&a, PaneId::FIRST)), (3, 3));
    press(&mut a, "tn");
    assert_eq!(a.active_tab(), 1);
    assert_eq!(offset(&a, id2()), 0, "the unfocused pane of the tab left");
    assert_eq!(offset(&a, PaneId::FIRST), 0);
}

#[test]
fn a_chat_keeps_its_scroll_across_focus_and_tab_changes() {
    let mut a = long_chat();
    up_on(&mut a, chat_id());
    up_on(&mut a, chat_id());
    assert_eq!(below(&a), 6);
    let pinned = chat_of(&a).scroll();
    assert!(matches!(pinned, Scroll::Pinned(_)));
    press(&mut a, "h");
    press(&mut a, "l");
    assert_eq!(below(&a), 6);
    // The tab bar appears with a second tab and takes a row, so the view is
    // compared by the pinned position, not by the distance to the bottom.
    press(&mut a, "tn");
    press(&mut a, "b1");
    assert_eq!(a.active_tab(), 0);
    assert_eq!(chat_of(&a).scroll(), pinned);
}

#[test]
fn ready_app_has_the_chat_on_the_right() {
    // Guards the helpers above: the shell is pane 1 and the chat pane 2.
    let a = ready_app();
    assert_eq!(chat_id(), id2());
    assert!(rect_of(&a, chat_id()).x > rect_of(&a, PaneId::FIRST).x);
}

// Entering COPY on a wheel-scrolled pane -------------------------------------------

#[test]
fn copy_entered_on_a_wheel_scrolled_pane_keeps_the_view() {
    let mut a = single();
    for _ in 0..4 {
        up_on(&mut a, PaneId::FIRST);
    }
    assert_eq!(offset(&a, PaneId::FIRST), 12);
    press(&mut a, "[");
    assert_eq!(copy_offset(&a), 12);
    assert_eq!(offset(&a, PaneId::FIRST), 12);
    let (row, _) = a.copy_cursor().expect("the cursor is on screen");
    assert_eq!(row, 23, "the last visible row");
    assert_eq!(a.copy_position(), Some((12, history(&a, PaneId::FIRST))));
    assert_eq!(a.scrolled_by(PaneId::FIRST), Some(12));
    assert_eq!(a.scroll_position(), Some((12, history(&a, PaneId::FIRST))));
}

#[test]
fn the_first_key_in_copy_moves_the_cursor_without_jumping_the_view() {
    let mut a = single();
    for _ in 0..4 {
        up_on(&mut a, PaneId::FIRST);
    }
    press(&mut a, "[");
    a.update(key('k'));
    assert_eq!(copy_offset(&a), 12);
    assert_eq!(offset(&a, PaneId::FIRST), 12);
    assert_eq!(a.copy_cursor().map(|c| c.0), Some(22));
    a.update(key('j'));
    a.update(key('j'));
    assert_eq!(copy_offset(&a), 11, "only the edge scrolls");
}

#[test]
fn copy_entered_at_the_bottom_still_uses_the_live_cursor() {
    let mut a = single();
    press(&mut a, "[");
    assert_eq!(copy_offset(&a), 0);
    assert!(a.copy_cursor().is_some());
}

#[test]
fn leaving_copy_after_a_wheel_scrolled_entry_goes_live() {
    let mut a = single();
    up_on(&mut a, PaneId::FIRST);
    press(&mut a, "[");
    a.update(key('q'));
    assert_eq!(a.input(), InputMode::Terminal);
    assert_eq!(offset(&a, PaneId::FIRST), 0);
}

// X10 Press mode ------------------------------------------------------------------

#[test]
fn x10_press_mode_gets_no_wheel_and_scrolls_the_view() {
    let mut a = single();
    output(&mut a, PaneId::FIRST, b"\x1b[?9h\x1b[?1006h");
    assert_eq!(up_on(&mut a, PaneId::FIRST), vec![]);
    assert_eq!(offset(&a, PaneId::FIRST), 3);
}

#[test]
fn x10_press_mode_on_the_alternate_screen_gets_arrows() {
    let mut a = single();
    output(&mut a, PaneId::FIRST, b"\x1b[?1049h\x1b[?9h");
    assert_eq!(
        down_on(&mut a, PaneId::FIRST),
        vec![Effect::WritePty(
            PaneId::FIRST,
            b"\x1b[B\x1b[B\x1b[B".to_vec()
        )]
    );
}

// The literal prefix ---------------------------------------------------------------

#[test]
fn the_literal_prefix_returns_the_focused_pane_to_live() {
    let mut a = single();
    for _ in 0..4 {
        up_on(&mut a, PaneId::FIRST);
    }
    a.update(ctrl_space());
    let effects = a.update(ctrl_space());
    assert_eq!(effects.len(), 1);
    assert!(matches!(effects[0], Effect::WritePty(..)));
    assert_eq!(offset(&a, PaneId::FIRST), 0);
}

// Redraws only on a change -----------------------------------------------------------

#[test]
fn a_clamped_shell_tick_does_not_mark_the_screen_dirty() {
    let mut a = single();
    a.take_dirty();
    down_on(&mut a, PaneId::FIRST);
    assert!(!a.take_dirty(), "already live");
    let total = history(&a, PaneId::FIRST);
    a.scroll_shell_for_test(PaneId::FIRST, total);
    a.take_dirty();
    up_on(&mut a, PaneId::FIRST);
    assert!(!a.take_dirty(), "already at the top");
    down_on(&mut a, PaneId::FIRST);
    assert!(a.take_dirty(), "a real move redraws");
}

#[test]
fn a_clamped_copy_tick_does_not_mark_the_screen_dirty() {
    let mut a = single();
    press(&mut a, "[");
    a.take_dirty();
    down_on(&mut a, PaneId::FIRST);
    assert!(!a.take_dirty(), "COPY already at the bottom");
    up_on(&mut a, PaneId::FIRST);
    assert!(a.take_dirty());
}

#[test]
fn a_clamped_chat_tick_does_not_mark_the_screen_dirty() {
    let mut a = enabled(answered("short"));
    a.take_dirty();
    up_on(&mut a, chat_id());
    down_on(&mut a, chat_id());
    assert!(!a.take_dirty(), "nothing to scroll");
    let mut a = long_chat();
    a.take_dirty();
    down_on(&mut a, chat_id());
    assert!(!a.take_dirty(), "already at the bottom");
    up_on(&mut a, chat_id());
    assert!(a.take_dirty());
}

#[test]
fn the_wheel_in_chat_copy_moves_the_view_and_clamps_the_cursor() {
    use crate::core::mouse::WHEEL_STEP;
    let mut a = enabled(answered(&"line\n".repeat(100)));
    a.update(ctrl_space());
    a.update(key('['));
    assert!(matches!(a.input(), InputMode::ChatCopy(_)));
    let cursor = |a: &App| a.chat_copy_marks().expect("chat COPY").cursor.line;
    let last = cursor(&a);
    up_on(&mut a, chat_id());
    assert_eq!(a.scrolled_by(chat_id()), Some(WHEEL_STEP));
    assert_eq!(cursor(&a), last - WHEEL_STEP, "on the new bottom row");
    assert!(matches!(a.input(), InputMode::ChatCopy(_)));
    down_on(&mut a, chat_id());
    assert_eq!(chat_of(&a).scroll(), Scroll::Follow);
    assert_eq!(cursor(&a), last - WHEEL_STEP, "still visible: it stays");
}
