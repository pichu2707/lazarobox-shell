//! The tag editor drawn inside the chat pane (ai-chat C9).

use ratatui::{
    buffer::Buffer,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
    layout::{Position, Rect},
    widgets::Widget,
};

use super::*;
use crate::core::ai::chat::{store::ConvRow, tag_editor::TagTarget, tags::Tags};

fn theme() -> LazaroboxTheme {
    LazaroboxTheme::default()
}

/// A chat tagged `proj, sql, etl` with its editor open.
fn editing() -> ChatState {
    let mut chat = ChatState::new(None);
    chat.set_tags(&Tags::parse("proj, sql, etl"));
    chat.composer_mut().paste("a draft in the composer");
    assert!(chat.open_tag_editor(TagTarget::Pane));
    chat
}

fn type_in(chat: &mut ChatState, text: &str) {
    for c in text.chars() {
        chat.tag_editor_mut()
            .unwrap()
            .on_key(&KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE), false);
    }
}

fn draw(chat: &ChatState, width: u16, height: u16) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    ChatView::new(chat, &theme()).render(area, &mut buf);
    buf
}

fn rows(buf: &Buffer) -> Vec<String> {
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

fn text(buf: &Buffer) -> String {
    rows(buf).join("\n")
}

fn row(id: i64, tag: &str) -> ConvRow {
    ConvRow {
        id,
        updated_at: 1_700_000_000,
        profile: "programmer".into(),
        tag: Some(tag.into()),
        extras: vec![],
        first: "How do I read a file?".into(),
        notes: 0,
    }
}

#[test]
fn the_editor_takes_the_place_of_the_conversation() {
    let shown = text(&draw(&editing(), 60, 8));
    assert!(shown.contains("proj, sql, etl"), "{shown}");
    assert!(!shown.contains("a draft"), "the composer is not drawn");
}

#[test]
fn the_editor_takes_the_place_of_the_list() {
    let mut chat = ChatState::new(None);
    chat.open_list();
    chat.list_mut().unwrap().set_rows(vec![row(1, "proj")]);
    assert!(chat.open_tag_editor_for(TagTarget::Row(1), &Tags::parse("proj")));
    let shown = text(&draw(&chat, 60, 8));
    assert!(!shown.contains("How do I read"), "{shown}");
    chat.close_tag_editor();
    assert!(text(&draw(&chat, 60, 8)).contains("How do I read"));
}

#[test]
fn closing_the_editor_draws_the_conversation_again() {
    let mut chat = editing();
    chat.close_tag_editor();
    assert!(text(&draw(&chat, 60, 8)).contains("a draft in the composer"));
}

#[test]
fn the_extra_count_follows_what_is_typed() {
    let mut chat = editing();
    assert!(text(&draw(&chat, 60, 8)).contains("2/8"), "two extras");
    type_in(&mut chat, ", a, A, b");
    assert!(
        text(&draw(&chat, 60, 8)).contains("4/8"),
        "repeats do not count"
    );
}

#[test]
fn the_keys_are_the_last_row() {
    let shown = rows(&draw(&editing(), 60, 8));
    assert_eq!(
        shown.last().map(String::as_str),
        Some("↵ save · Esc discard")
    );
}

#[test]
fn the_cursor_follows_the_end_of_the_line() {
    let mut chat = editing();
    let area = Rect::new(0, 0, 50, 8);
    let th = theme();
    let cursor = |chat: &ChatState| ChatView::new(chat, &th).cursor(area);
    let at = cursor(&chat).expect("the line shows the cursor");
    assert_eq!(at.y, 1);
    type_in(&mut chat, "!");
    assert_eq!(cursor(&chat), Some(Position::new(at.x + 1, 1)));
}

#[test]
fn a_long_line_scrolls_so_the_cursor_stays_in_the_pane() {
    let mut chat = editing();
    type_in(&mut chat, &"x".repeat(200));
    let area = Rect::new(2, 3, 30, 8);
    let pos = ChatView::new(&chat, &theme())
        .cursor(area)
        .expect("a cursor");
    assert!(area.contains(pos), "{pos:?}");
    let shown = text(&draw(&chat, 30, 8));
    assert!(shown.contains("xxxx"), "the end of the line shows: {shown}");
}

#[test]
fn the_editor_never_panics_at_any_size() {
    let mut chat = editing();
    type_in(&mut chat, &"t".repeat(100));
    let th = theme();
    for width in 0..=12u16 {
        for height in 0..=8u16 {
            let area = Rect::new(0, 0, width, height);
            let mut buf = Buffer::empty(area);
            let view = ChatView::new(&chat, &th);
            view.cursor(area);
            view.render(area, &mut buf);
        }
    }
    assert!(text(&draw(&chat, 200, 60)).contains("Edit tags"));
}

#[test]
fn a_cursor_never_leaves_the_pane() {
    let mut chat = editing();
    type_in(&mut chat, &"x".repeat(500));
    let th = theme();
    for (width, height) in [(1u16, 1u16), (2, 2), (3, 3), (10, 4), (30, 7), (50, 16)] {
        let area = Rect::new(2, 3, width, height);
        if let Some(pos) = ChatView::new(&chat, &th).cursor(area) {
            assert!(area.contains(pos), "{pos:?} outside {area:?}");
        }
    }
}

#[test]
fn the_line_is_drawn_as_text_in_styled_cells() {
    let mut chat = editing();
    chat.tag_editor_mut()
        .unwrap()
        .paste("\u{1b}[31mred\u{1b}[0m");
    let buf = draw(&chat, 60, 8);
    for y in 0..8 {
        for x in 0..60 {
            let symbol = buf[(x, y)].symbol();
            assert!(!symbol.contains('\u{1b}'), "an escape reached a cell");
        }
    }
}

#[test]
fn snapshot_the_editor_with_three_tags() {
    insta::assert_snapshot!(text(&draw(&editing(), 50, 8)));
}

#[test]
fn snapshot_the_editor_with_no_tags() {
    let mut chat = ChatState::new(None);
    assert!(chat.open_tag_editor(TagTarget::Pane));
    insta::assert_snapshot!(text(&draw(&chat, 50, 8)));
}

#[test]
fn snapshot_the_editor_with_a_line_longer_than_the_pane() {
    let mut chat = editing();
    type_in(
        &mut chat,
        ", kubernetes, observability, postgres, terraform",
    );
    insta::assert_snapshot!(text(&draw(&chat, 30, 8)));
}

#[test]
fn snapshot_the_editor_in_a_tiny_pane() {
    let chat = editing();
    insta::assert_snapshot!(text(&draw(&chat, 24, 4)));
    insta::assert_snapshot!(text(&draw(&chat, 16, 3)));
    insta::assert_snapshot!(text(&draw(&chat, 12, 2)));
    insta::assert_snapshot!(text(&draw(&chat, 8, 1)));
    insta::assert_snapshot!(text(&draw(&chat, 2, 2)));
}
