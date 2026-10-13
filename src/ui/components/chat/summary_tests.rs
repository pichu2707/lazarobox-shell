//! The summary draft drawn inside the chat pane (engram-memory M8).

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

/// A chat that answered and has a summary draft open on `summary`.
fn drafting(summary: &str) -> ChatState {
    let mut chat = ChatState::new(None);
    let ready = Availability::Ready {
        engine: "ollama".into(),
        model: Some("llama3:8b".into()),
    };
    let SendOutcome::Sent { turn, .. } = chat.send("question", &ready, "") else {
        panic!("send refused");
    };
    chat.apply(turn, StreamEvent::Delta("an answer".into()));
    chat.apply(turn, StreamEvent::Done(Finish::Stop));
    let (turn, _) = chat.begin_summary(&ready).expect("a summary request");
    chat.apply(turn, StreamEvent::Delta(summary.into()));
    chat.apply(turn, StreamEvent::Done(Finish::Stop));
    assert!(chat.summary_draft().is_some());
    chat
}

fn type_in(chat: &mut ChatState, text: &str) {
    for c in text.chars() {
        chat.summary_draft_mut()
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

#[test]
fn the_draft_takes_the_place_of_the_conversation() {
    let mut chat = drafting("We chose Axum.\nIt builds on tower.");
    chat.composer_mut().paste("my message");
    let shown = rows(&draw(&chat, 50, 12));
    assert_eq!(shown[0], "Summary (not saved until you approve)");
    assert!(shown[1].contains("We chose Axum."), "{shown:?}");
    assert!(shown[2].contains("It builds on tower."), "{shown:?}");
    let all = shown.join("\n");
    assert!(!all.contains("question"), "the transcript is not drawn");
    assert!(!all.contains("my message"), "the composer is not drawn");
    assert_eq!(shown[11], "Ctrl+S approve · Esc discard");
}

#[test]
fn the_hint_and_the_refusal_show_on_the_status_row() {
    let chat = drafting("password=hunter2");
    let shown = rows(&draw(&chat, 50, 12));
    assert!(shown[10].contains("1 secret redacted"), "{shown:?}");

    let mut chat = drafting("fine");
    type_in(&mut chat, " password=hunter2");
    chat.summary_draft_mut().unwrap().on_key(
        &KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
        false,
    );
    let shown = rows(&draw(&chat, 50, 12));
    assert!(shown[10].contains("Redacted again"), "{shown:?}");
    assert!(shown[1].contains("password=[REDACTED]"), "{shown:?}");
}

#[test]
fn the_cursor_sits_at_the_end_of_the_text() {
    let mut chat = drafting("ab\ncd");
    let th = theme();
    let area = Rect::new(0, 0, 50, 12);
    let at = ChatView::new(&chat, &th).cursor(area).expect("a cursor");
    assert_eq!((at.x, at.y), (2 + 2, 2), "end of `cd`");
    type_in(&mut chat, "!");
    assert_eq!(
        ChatView::new(&chat, &th).cursor(area),
        Some(Position::new(2 + 3, 2))
    );
}

#[test]
fn the_draft_never_panics_and_the_cursor_never_leaves_the_pane() {
    let chat = drafting(&"long line of the summary\n".repeat(60));
    let th = theme();
    for width in 0..=14u16 {
        for height in 0..=9u16 {
            let area = Rect::new(2, 3, width, height);
            let mut buf = Buffer::empty(Rect::new(0, 0, 40, 20));
            ChatView::new(&chat, &th).render(area, &mut buf);
            if let Some(pos) = ChatView::new(&chat, &th).cursor(area) {
                assert!(area.contains(pos), "{pos:?} outside {area:?}");
            }
        }
    }
}

#[test]
fn model_text_is_drawn_as_text_in_styled_cells() {
    let chat = drafting("\u{1b}[31mred\u{1b}[0m \u{202e}flip");
    let shown = text(&draw(&chat, 50, 12));
    assert!(!shown.contains('\u{1b}'), "{shown:?}");
    assert!(!shown.contains('\u{202e}'), "{shown:?}");
}

#[test]
fn a_running_summary_still_shows_the_conversation() {
    let mut chat = ChatState::new(None);
    let ready = Availability::Ready {
        engine: "ollama".into(),
        model: Some("llama3:8b".into()),
    };
    let SendOutcome::Sent { turn, .. } = chat.send("question", &ready, "") else {
        panic!("send refused");
    };
    chat.apply(turn, StreamEvent::Delta("an answer".into()));
    chat.apply(turn, StreamEvent::Done(Finish::Stop));
    chat.begin_summary(&ready).expect("a request");
    let shown = text(&draw(&chat, 50, 12));
    assert!(shown.contains("question"), "{shown}");
    assert!(!shown.contains("Summary (not saved"), "{shown}");
}

#[test]
fn snapshot_the_draft_with_two_lines() {
    let chat = drafting("- Chose Axum over actix.\n- Open: auth middleware.");
    insta::assert_snapshot!(text(&draw(&chat, 50, 12)));
}

#[test]
fn snapshot_the_draft_with_the_redacted_hint() {
    let chat = drafting("Token set with password=hunter2");
    insta::assert_snapshot!(text(&draw(&chat, 50, 12)));
}

#[test]
fn snapshot_the_draft_in_a_tiny_pane() {
    let chat = drafting("one two three four");
    insta::assert_snapshot!(text(&draw(&chat, 14, 6)));
    insta::assert_snapshot!(text(&draw(&chat, 3, 2)));
}
