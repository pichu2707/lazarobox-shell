//! The `AUTO` badge of a chat in auto approval mode (spec chat-pane,
//! "Approval mode badge").

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    widgets::Widget,
};

use super::*;
use crate::core::ai::{
    chat::{
        SendOutcome,
        engine::{Availability, Finish, StreamEvent},
    },
    config::ApprovalMode,
};

fn theme() -> LazaroboxTheme {
    LazaroboxTheme::default()
}

fn answered() -> ChatState {
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
    chat
}

/// A chat with tools on and in `mode`.
fn chat_in(mode: ApprovalMode) -> ChatState {
    let mut chat = answered();
    chat.set_run_hint(true);
    chat.set_approval_mode(mode);
    chat
}

fn draw(chat: &ChatState, width: u16, height: u16) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    ChatView::new(chat, &theme()).render(area, &mut buf);
    buf
}

fn row(buf: &Buffer, y: u16) -> String {
    (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
}

fn badge_style() -> Style {
    let theme = theme();
    Style::default()
        .fg(theme.bg_base)
        .bg(theme.error_red)
        .add_modifier(Modifier::BOLD)
}

fn assert_badge(buf: &Buffer) {
    let width = buf.area.width;
    assert!(row(buf, 0).ends_with(" AUTO "), "{:?}", row(buf, 0));
    for x in width - 6..width {
        let cell = &buf[(x, 0)];
        assert_eq!(cell.fg, badge_style().fg.unwrap(), "fg at {x}");
        assert_eq!(cell.bg, badge_style().bg.unwrap(), "bg at {x}");
        assert!(cell.modifier.contains(Modifier::BOLD), "bold at {x}");
    }
}

#[test]
fn auto_draws_the_badge_top_right_of_the_conversation() {
    let buf = draw(&chat_in(ApprovalMode::Auto), 30, 6);
    assert_badge(&buf);
}

#[test]
fn the_badge_shows_in_the_list_and_editor_views_too() {
    let mut list = chat_in(ApprovalMode::Auto);
    list.open_list();
    assert_badge(&draw(&list, 40, 8));

    let mut editor = chat_in(ApprovalMode::Auto);
    editor.open_note_editor().expect("an answer to save");
    assert_badge(&draw(&editor, 40, 10));
}

#[test]
fn below_six_columns_the_badge_is_one_letter() {
    let buf = draw(&chat_in(ApprovalMode::Auto), 5, 4);
    assert_eq!(buf[(4, 0)].symbol(), "A");
    assert_eq!(buf[(4, 0)].bg, theme().error_red);
    assert!(!row(&buf, 0).contains("AUTO"));
}

#[test]
fn ask_mode_and_tools_off_draw_no_marker() {
    let ask = draw(&chat_in(ApprovalMode::Ask), 30, 6);
    let mut off = chat_in(ApprovalMode::Auto);
    off.set_run_hint(false);
    let off = draw(&off, 30, 6);
    for buf in [ask, off] {
        for y in 0..buf.area.height {
            assert!(!row(&buf, y).contains("AUTO"), "{:?}", row(&buf, y));
            assert!((0..buf.area.width).all(|x| buf[(x, y)].bg != theme().error_red));
        }
    }
}

#[test]
fn tiny_areas_do_not_panic() {
    let chat = chat_in(ApprovalMode::Auto);
    for width in 1..=10 {
        for height in 1..=2 {
            draw(&chat, width, height);
        }
    }
}

#[test]
fn the_badge_snapshot() {
    let buf = draw(&chat_in(ApprovalMode::Auto), 30, 5);
    let shot = (0..buf.area.height)
        .map(|y| row(&buf, y).trim_end().to_owned())
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!("auto_badge_30x5", shot);
}
