//! The note editor drawn inside the chat pane (engram-memory M7b).

use ratatui::{
    buffer::Buffer,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
    layout::{Position, Rect},
    widgets::Widget,
};

use super::*;
use crate::core::ai::chat::{
    SendOutcome,
    engine::{Availability, Finish, StreamEvent},
};

fn theme() -> LazaroboxTheme {
    LazaroboxTheme::default()
}

/// A chat that answered `answer` and has its note editor open.
fn editing(answer: &str) -> ChatState {
    let mut chat = ChatState::new(None);
    let ready = Availability::Ready {
        engine: "ollama".into(),
        model: Some("llama3:8b".into()),
    };
    let SendOutcome::Sent { turn, .. } = chat.send("question", &ready, "") else {
        panic!("send refused");
    };
    chat.apply(turn, StreamEvent::Delta(answer.into()));
    chat.apply(turn, StreamEvent::Done(Finish::Stop));
    chat.open_note_editor().expect("an answer to save");
    chat
}

fn key(chat: &mut ChatState, code: KeyCode) {
    chat.note_editor_mut()
        .unwrap()
        .on_key(&KeyEvent::new(code, KeyModifiers::NONE), false);
}

fn type_in(chat: &mut ChatState, text: &str) {
    for c in text.chars() {
        key(chat, KeyCode::Char(c));
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

#[test]
fn the_editor_takes_the_place_of_the_conversation() {
    let mut chat = editing("an answer");
    chat.composer_mut().paste("my draft");
    let shown = text(&draw(&chat, 50, 16));
    assert!(shown.contains("Category"), "{shown}");
    assert!(shown.contains("Title"), "{shown}");
    assert!(shown.contains("Content"), "{shown}");
    assert!(shown.contains("an answer"), "the content is the answer");
    assert!(!shown.contains("question"), "the transcript is not drawn");
    assert!(!shown.contains("my draft"), "the composer is not drawn");
}

#[test]
fn closing_the_editor_draws_the_conversation_again() {
    let mut chat = editing("an answer");
    chat.close_note_editor();
    let shown = text(&draw(&chat, 50, 16));
    assert!(shown.contains("question"), "{shown}");
    assert!(!shown.contains("Category"), "{shown}");
}

#[test]
fn the_kind_title_and_content_are_drawn() {
    let mut chat = editing("first\nsecond");
    type_in(&mut chat, "Use Axum");
    let shown = rows(&draw(&chat, 50, 16));
    assert!(shown[0].contains("decision"), "{shown:?}");
    assert!(shown[1].contains("Use Axum"), "{shown:?}");
    assert!(shown[3].contains("first"), "{shown:?}");
    assert!(shown[4].contains("second"), "{shown:?}");
}

#[test]
fn the_focused_field_is_marked() {
    let mut chat = editing("body");
    let marked = |chat: &ChatState| -> Vec<usize> {
        rows(&draw(chat, 50, 16))
            .iter()
            .enumerate()
            .filter(|(_, row)| row.starts_with('▸'))
            .map(|(y, _)| y)
            .collect()
    };
    assert_eq!(marked(&chat), [1], "the title is focused first");
    key(&mut chat, KeyCode::Tab);
    assert_eq!(marked(&chat), [2], "the content label");
    key(&mut chat, KeyCode::Tab);
    assert_eq!(marked(&chat), [0], "the kind");
}

#[test]
fn the_hint_and_the_notice_share_one_line_and_the_notice_wins() {
    let mut chat = editing("password=hunter2");
    let shown = text(&draw(&chat, 60, 16));
    assert!(shown.contains("1 secret redacted"), "{shown}");
    chat.note_editor_mut()
        .unwrap()
        .on_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), false);
    let shown = text(&draw(&chat, 60, 16));
    assert!(shown.contains("Title is required"), "{shown}");
    assert!(!shown.contains("1 secret redacted"), "{shown}");
}

#[test]
fn the_key_hint_is_the_last_row() {
    let chat = editing("body");
    let shown = rows(&draw(&chat, 60, 16));
    assert!(shown[15].contains("Ctrl+S"), "{shown:?}");
    assert!(shown[15].contains("Esc"), "{shown:?}");
}

#[test]
fn while_saving_the_keys_line_still_says_how_to_close() {
    let mut chat = editing("body");
    type_in(&mut chat, "T");
    chat.note_editor_mut().unwrap().on_key(
        &KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
        false,
    );
    assert!(chat.note_editor().unwrap().is_saving());
    let shown = rows(&draw(&chat, 60, 16));
    assert!(shown[15].contains("saving… · Esc close"), "{shown:?}");
}

#[test]
fn the_cursor_follows_the_focused_field() {
    let mut chat = editing("ab\ncd");
    type_in(&mut chat, "Hi");
    let area = Rect::new(0, 0, 50, 16);
    let th = theme();
    let cursor = |chat: &ChatState| ChatView::new(chat, &th).cursor(area);
    let title = cursor(&chat).expect("the title shows the cursor");
    assert_eq!(title.y, 1);
    let after = title.x;
    type_in(&mut chat, "!");
    assert_eq!(cursor(&chat), Some(Position::new(after + 1, 1)));
    key(&mut chat, KeyCode::Tab);
    let content = cursor(&chat).expect("the content shows the cursor");
    assert_eq!(
        (content.x, content.y),
        (2 + 2, 4),
        "end of `cd` in the region"
    );
    key(&mut chat, KeyCode::Tab);
    assert_eq!(cursor(&chat), None, "the kind has no text cursor");
}

#[test]
fn the_editor_never_panics_at_any_size() {
    let mut chat = editing(&"long line of the answer\n".repeat(40));
    type_in(&mut chat, &"t".repeat(100));
    let th = theme();
    for _ in 0..3 {
        for width in 0..=12u16 {
            for height in 0..=8u16 {
                let area = Rect::new(0, 0, width, height);
                let mut buf = Buffer::empty(area);
                let view = ChatView::new(&chat, &th);
                view.cursor(area);
                view.render(area, &mut buf);
            }
        }
        key(&mut chat, KeyCode::Tab);
    }
    let big = draw(&chat, 200, 60);
    assert!(text(&big).contains("Category"));
}

#[test]
fn a_cursor_never_leaves_the_pane() {
    let mut chat = editing(&"x".repeat(5000));
    key(&mut chat, KeyCode::Tab);
    let th = theme();
    for (width, height) in [(1u16, 1u16), (3, 3), (10, 4), (30, 7), (50, 16)] {
        let area = Rect::new(2, 3, width, height);
        if let Some(pos) = ChatView::new(&chat, &th).cursor(area) {
            assert!(area.contains(pos), "{pos:?} outside {area:?}");
        }
    }
}

#[test]
fn model_text_is_drawn_as_text_in_styled_cells() {
    let chat = editing("\u{1b}[31mred\u{1b}[0m \u{202e}flip");
    let shown = text(&draw(&chat, 50, 16));
    assert!(!shown.contains('\u{1b}'), "{shown:?}");
    assert!(!shown.contains('\u{202e}'), "{shown:?}");
}

// 9.8: snapshots ---------------------------------------------------------------

#[test]
fn snapshot_the_editor_with_a_title_and_two_lines() {
    let mut chat = editing("Axum has the best ergonomics.\nIt builds on tower.");
    type_in(&mut chat, "Use Axum");
    insta::assert_snapshot!(text(&draw(&chat, 50, 16)));
}

#[test]
fn snapshot_the_editor_with_the_redacted_hint() {
    let chat = editing("call it with password=hunter2 and sk-abcdefghijklmnopqrstuvwx");
    insta::assert_snapshot!(text(&draw(&chat, 50, 16)));
}

#[test]
fn snapshot_the_editor_after_a_refused_save() {
    let mut chat = editing("body");
    key(&mut chat, KeyCode::Enter);
    insta::assert_snapshot!(text(&draw(&chat, 50, 16)));
}

#[test]
fn snapshot_the_editor_in_a_tiny_pane() {
    let mut chat = editing("body of the note");
    type_in(&mut chat, "T");
    insta::assert_snapshot!(text(&draw(&chat, 14, 6)));
    insta::assert_snapshot!(text(&draw(&chat, 3, 2)));
}
