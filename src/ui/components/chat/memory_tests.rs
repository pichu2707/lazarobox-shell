//! The memory line drawn at the top of the transcript (engram-memory 10.4).

use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

use super::*;
use crate::core::ai::{
    chat::{
        SendOutcome,
        engine::{Availability, Finish, StreamEvent},
    },
    memory::context::ContextBlock,
};

fn theme() -> LazaroboxTheme {
    LazaroboxTheme::default()
}

fn block(notes: usize, data: &str) -> ContextBlock {
    let text =
        format!("<project-memory source=\"engram\" project=\"p\">\n{data}\n</project-memory>");
    ContextBlock {
        bytes: text.len(),
        notes,
        text,
    }
}

/// A chat that answered `hello` to `hi`, holding a memory line.
fn chat_with(notes: usize, data: &str) -> ChatState {
    let mut chat = ChatState::new(None);
    let ready = Availability::Ready {
        engine: "ollama".into(),
        model: Some("llama3:8b".into()),
    };
    let SendOutcome::Sent { turn, .. } = chat.send("hi", &ready, "") else {
        panic!("send refused");
    };
    chat.apply(turn, StreamEvent::Delta("hello".into()));
    chat.set_usage_config(crate::core::ai::config::UsageConfig::without_line());
    chat.apply(turn, StreamEvent::Done(Finish::Stop));
    chat.inject_memory(block(notes, data));
    chat
}

fn draw(chat: &ChatState, width: u16, height: u16) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    ChatView::new(chat, &theme()).render(area, &mut buf);
    buf
}

fn text(buf: &Buffer) -> String {
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn snapshot_the_folded_line_above_the_messages() {
    let chat = chat_with(3, "- [decision] **Use Axum**");
    let buf = draw(&chat, 40, 8);
    insta::assert_snapshot!(text(&buf));
    assert_eq!(buf[(0, 0)].symbol(), "▸");
    assert_eq!(buf[(0, 0)].style().fg, Some(theme().ai_purple));
}

#[test]
fn snapshot_the_unfolded_line_shows_the_injected_text_dimmed() {
    let mut chat = chat_with(3, "- decision: use Axum");
    assert!(chat.toggle_memory());
    let buf = draw(&chat, 60, 10);
    insta::assert_snapshot!(text(&buf));
    assert_eq!(buf[(0, 0)].symbol(), "▾");
    assert_eq!(buf[(2, 1)].style().fg, Some(theme().text_muted));
    assert!(text(&buf).contains("┊ - decision: use Axum"));
}

#[test]
fn snapshot_one_note_is_singular() {
    let chat = chat_with(1, "- a");
    insta::assert_snapshot!(text(&draw(&chat, 40, 6)));
}

#[test]
fn without_a_line_the_pane_is_drawn_as_before() {
    let mut chat = chat_with(3, "- a");
    chat.inject_memory(block(0, "- sessions only"));
    let shown = text(&draw(&chat, 40, 6));
    assert!(!shown.contains("memory"), "{shown}");
    assert!(shown.contains("› hi"), "{shown}");
}

#[test]
fn a_long_note_wraps_behind_the_gutter() {
    let mut chat = chat_with(1, &"word ".repeat(30));
    chat.toggle_memory();
    let shown = text(&draw(&chat, 30, 14));
    let wrapped: Vec<&str> = shown.lines().filter(|l| l.starts_with("┊ word")).collect();
    assert!(wrapped.len() >= 3, "{shown}");
}

#[test]
fn snapshot_a_tiny_pane_draws_the_line_without_panicking() {
    let mut chat = chat_with(3, "- a");
    chat.set_scroll(crate::core::ai::chat::transcript::Scroll::Pinned(0));
    for expanded in [false, true] {
        if expanded {
            chat.toggle_memory();
        }
        for (w, h) in [(1, 1), (2, 1), (3, 2), (10, 2), (14, 4)] {
            draw(&chat, w, h);
        }
    }
    insta::assert_snapshot!(text(&draw(&chat, 14, 4)));
}
