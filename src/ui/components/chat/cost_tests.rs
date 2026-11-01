//! The `/cost` overlay drawn inside the chat pane (ai-cost 1c).

use ratatui::{
    buffer::Buffer,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
    layout::Rect,
    widgets::Widget,
};

use super::*;
use crate::core::ai::chat::usage_view::CostView;

fn theme() -> LazaroboxTheme {
    LazaroboxTheme::default()
}

fn lines() -> Vec<String> {
    [
        "AI usage (alpha)",
        "today: 6 tokens · 2 turns",
        "last 7 days: 126 tokens · 3 turns",
        "by engine and model (last 7 days):",
        "  ollama › llama3:8b: in 100 · out 20",
        "  claude › sonnet: in 2 · out 4 · $0.0049 list-price equivalent",
        "this chat: 6 tokens · 2 turns",
    ]
    .map(str::to_owned)
    .to_vec()
}

fn chat_with(view: CostView) -> ChatState {
    let mut chat = ChatState::new(None);
    chat.open_cost(view);
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
fn snapshot_the_cost_overlay_with_data() {
    let chat = chat_with(CostView::with(lines()));
    insta::assert_snapshot!(text(&draw(&chat, 72, 10)));
}

#[test]
fn snapshot_the_cost_overlay_while_reading() {
    let chat = chat_with(CostView::reading());
    insta::assert_snapshot!(text(&draw(&chat, 40, 5)));
}

#[test]
fn the_overlay_replaces_the_conversation_and_hides_the_cursor() {
    let chat = chat_with(CostView::with(lines()));
    assert!(!chat.conversation_shown());
    let area = Rect::new(0, 0, 60, 10);
    assert_eq!(ChatView::new(&chat, &theme()).cursor(area), None);
}

#[test]
fn a_view_taller_than_the_pane_scrolls_and_never_panics() {
    let mut view = CostView::with(lines());
    let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
    for _ in 0..3 {
        view.on_key(&down);
    }
    let chat = chat_with(view);
    let shown = text(&draw(&chat, 72, 4));
    assert!(shown.starts_with("AI usage (alpha)"), "{shown}");
    assert!(shown.contains("llama3:8b"), "{shown}");
    assert!(!shown.contains("today:"), "{shown}");
}

#[test]
fn any_pane_size_draws_without_panicking() {
    let chat = chat_with(CostView::with(lines()));
    for width in 0..=24 {
        for height in 0..=6 {
            draw(&chat, width, height);
        }
    }
}
