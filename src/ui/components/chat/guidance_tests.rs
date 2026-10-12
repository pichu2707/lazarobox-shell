//! The guidance a chat pane shows when its engine is not ready.

use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

use super::*;
use crate::core::ai::chat::{SendOutcome, engine::Availability};

fn row(buf: &Buffer, y: u16) -> String {
    (0..buf.area.width)
        .map(|x| buf[(x, y)].symbol())
        .collect::<String>()
        .trim_end()
        .to_owned()
}

fn draw(chat: &ChatState, guidance: Option<&str>, width: u16, height: u16) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    ChatView::new(chat, &LazaroboxTheme::default())
        .guidance(guidance)
        .render(area, &mut buf);
    buf
}

fn rows(buf: &Buffer) -> Vec<String> {
    (0..buf.area.height).map(|y| row(buf, y)).collect()
}

#[test]
fn guidance_sits_just_above_the_composer_in_the_warning_color() {
    let chat = ChatState::new(None);
    let buf = draw(&chat, Some("No engine assigned to chat"), 40, 6);
    assert_eq!(row(&buf, 4), "No engine assigned to chat");
    assert_eq!(buf[(0, 4)].fg, LazaroboxTheme::default().warning_orange);
    assert_eq!(row(&buf, 5), "", "the composer row is untouched");
}

#[test]
fn long_guidance_wraps_at_the_pane_width() {
    let chat = ChatState::new(None);
    let buf = draw(
        &chat,
        Some("ollama: not running; start or install it"),
        20,
        8,
    );
    let shown = rows(&buf);
    assert_eq!(
        &shown[5..8],
        ["ollama: not running;", "start or install it", ""],
        "{shown:?}"
    );
}

#[test]
fn the_transcript_keeps_the_rows_above_the_guidance() {
    let mut chat = ChatState::new(None);
    let ready = Availability::Ready {
        engine: "ollama".into(),
        model: None,
    };
    let SendOutcome::Sent { .. } = chat.send("hello", &ready, "") else {
        panic!("send refused");
    };
    let shown = rows(&draw(&chat, Some("gate"), 30, 6));
    assert!(shown.contains(&"› hello".to_owned()), "{shown:?}");
    assert!(shown.contains(&"gate".to_owned()), "{shown:?}");
}

#[test]
fn no_guidance_draws_exactly_what_it_drew_before() {
    let chat = ChatState::new(None);
    assert_eq!(draw(&chat, None, 30, 6), {
        let area = Rect::new(0, 0, 30, 6);
        let mut buf = Buffer::empty(area);
        ChatView::new(&chat, &LazaroboxTheme::default()).render(area, &mut buf);
        buf
    });
}

#[test]
fn guidance_never_panics_at_any_size() {
    let chat = ChatState::new(None);
    for width in 0..=12 {
        for height in 0..=6 {
            draw(&chat, Some("a long line of guidance text"), width, height);
        }
    }
}
