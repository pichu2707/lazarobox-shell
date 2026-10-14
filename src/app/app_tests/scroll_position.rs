//! Where a pane is scrolled (pane-scroll S1): `scrolled_by` for the badge and
//! `scroll_position` for the statusline.

use super::chat_live::{answered, chat_id, chat_of, ready_app};
use super::*;
use crate::core::ai::chat::transcript::Scroll;

fn shell_scrolled(offset: usize) -> App {
    let mut a = launched(80, 25);
    with_history(&mut a, 60);
    set_offset(&mut a, offset);
    a
}

fn set_offset(a: &mut App, offset: usize) {
    a.panes
        .get_mut(&PaneId::FIRST)
        .and_then(PaneState::pty_mut)
        .expect("the shell")
        .emu
        .set_scrollback(offset);
}

fn history(a: &App) -> usize {
    a.panes
        .get(&PaneId::FIRST)
        .and_then(PaneState::pty)
        .expect("the shell")
        .emu
        .scrollback_len()
}

fn chat_width(a: &App) -> usize {
    usize::from(a.panes[&chat_id()].size.cols)
}

/// `(lines of the transcript, rows drawn)` of the chat.
fn chat_extent(a: &App) -> (usize, usize) {
    (
        chat_of(a).lines(chat_width(a)).len(),
        a.chat_viewport(chat_id()),
    )
}

fn long_chat() -> App {
    answered(&"line\n".repeat(100))
}

fn pin_above_bottom(a: &mut App, lines: usize) {
    let (total, rows) = chat_extent(a);
    let top = total - rows - lines;
    a.panes
        .get_mut(&chat_id())
        .and_then(PaneState::chat_mut)
        .expect("the chat")
        .set_scroll(Scroll::Pinned(top));
}

#[test]
fn a_shell_pane_reports_its_scrollback_offset() {
    let a = shell_scrolled(12);
    assert_eq!(a.scrolled_by(PaneId::FIRST), Some(12));
    let a = shell_scrolled(3);
    assert_eq!(a.scrolled_by(PaneId::FIRST), Some(3));
}

#[test]
fn a_live_shell_pane_reports_nothing() {
    let a = shell_scrolled(0);
    assert_eq!(a.scrolled_by(PaneId::FIRST), None);
}

#[test]
fn an_unknown_pane_reports_nothing() {
    let a = shell_scrolled(5);
    assert_eq!(a.scrolled_by(PaneId::for_test(9)), None);
}

#[test]
fn the_alternate_screen_reports_nothing() {
    let mut a = shell_scrolled(5);
    a.update(AppEvent::Pty(
        PaneId::FIRST,
        PtyEvent::Output(b"\x1b[?1049h".to_vec()),
    ));
    assert_eq!(a.scrolled_by(PaneId::FIRST), None);
    assert_eq!(a.scroll_position(), None);
}

#[test]
fn a_chat_reports_the_lines_below_its_view() {
    let mut a = long_chat();
    assert_eq!(a.scrolled_by(chat_id()), None, "following");
    pin_above_bottom(&mut a, 9);
    assert_eq!(a.scrolled_by(chat_id()), Some(9));
    pin_above_bottom(&mut a, 1);
    assert_eq!(a.scrolled_by(chat_id()), Some(1));
}

#[test]
fn an_unfocused_pane_reports_its_offset_for_the_badge() {
    let mut a = long_chat();
    pin_above_bottom(&mut a, 4);
    press(&mut a, "h");
    assert_eq!(a.focused(), PaneId::FIRST);
    assert_eq!(a.scrolled_by(chat_id()), Some(4));
}

#[test]
fn a_scrolled_focused_shell_shows_its_position_outside_copy() {
    let a = shell_scrolled(12);
    assert_eq!(a.scroll_position(), Some((12, history(&a))));
}

#[test]
fn a_live_focused_pane_shows_no_position() {
    assert_eq!(shell_scrolled(0).scroll_position(), None);
    assert_eq!(long_chat().scroll_position(), None);
}

#[test]
fn a_scrolled_focused_chat_shows_its_position_outside_copy() {
    let mut a = long_chat();
    pin_above_bottom(&mut a, 9);
    let (total, rows) = chat_extent(&a);
    assert_eq!(a.scroll_position(), Some((9, total - rows)));
}

#[test]
fn only_the_focused_pane_shows_a_position() {
    let mut a = long_chat();
    pin_above_bottom(&mut a, 9);
    press(&mut a, "h");
    assert_eq!(a.scroll_position(), None);
    let mut a = launched(80, 25);
    with_history(&mut a, 60);
    press(&mut a, "wv");
    set_offset(&mut a, 7);
    assert_eq!(a.focused(), id2());
    assert_eq!(a.scrolled_by(PaneId::FIRST), Some(7));
    assert_eq!(a.scroll_position(), None);
}

#[test]
fn the_shell_copy_position_is_the_copy_position() {
    let mut a = launched(80, 25);
    with_history(&mut a, 60);
    press(&mut a, "[");
    assert_eq!(a.scroll_position(), a.copy_position());
    assert!(a.scroll_position().is_some());
    press_n(&mut a, 'k', 30);
    assert_eq!(a.scroll_position(), a.copy_position());
    let (offset, _) = a.scroll_position().expect("history");
    assert!(offset > 0);
}

#[test]
fn shell_copy_without_history_has_no_position() {
    let mut a = launched(80, 25);
    press(&mut a, "[");
    assert_eq!(a.scroll_position(), None);
}

#[test]
fn chat_copy_always_shows_a_position_even_at_the_bottom() {
    let mut a = long_chat();
    press(&mut a, "[");
    let (total, rows) = chat_extent(&a);
    assert_eq!(a.scroll_position(), Some((0, total - rows)));
    a.update(key('k'));
    a.update(key('k'));
    assert_eq!(a.scroll_position(), Some((2, total - rows)));
}

#[test]
fn chat_copy_on_a_short_transcript_shows_zero_of_zero() {
    let mut a = answered("short");
    press(&mut a, "[");
    assert_eq!(a.scroll_position(), Some((0, 0)));
}

#[test]
fn a_ready_chat_with_nothing_said_has_no_position() {
    assert_eq!(ready_app().scroll_position(), None);
}
