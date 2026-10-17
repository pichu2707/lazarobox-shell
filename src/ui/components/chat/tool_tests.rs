//! Tool rows drawn in the transcript (spec chat-pane "Tool rows").

use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

use super::*;
use crate::core::ai::chat::{
    SendOutcome,
    engine::{Availability, CallId, Finish, StreamEvent, ToolStatus},
};

fn theme() -> LazaroboxTheme {
    LazaroboxTheme::default()
}

fn call(id: u64, name: &str, summary: &str) -> StreamEvent {
    StreamEvent::ToolCall {
        id: CallId(id),
        name: name.into(),
        summary: summary.into(),
        approval: None,
    }
}

fn result(id: u64, status: ToolStatus, summary: &str) -> StreamEvent {
    StreamEvent::ToolResult {
        id: CallId(id),
        status,
        summary: summary.into(),
    }
}

/// A chat whose turn is still open, so more events can follow.
fn open_turn() -> (ChatState, u64) {
    let mut chat = ChatState::new(None);
    let ready = Availability::Ready {
        engine: "claude".into(),
        model: None,
    };
    let SendOutcome::Sent { turn, .. } = chat.send("where is the course?", &ready, "") else {
        panic!("send refused");
    };
    (chat, turn)
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

/// The first cell of the first row that starts with `needle`.
fn row_style(buf: &Buffer, needle: &str) -> ratatui::style::Style {
    let y = (0..buf.area.height)
        .find(|&y| {
            let row: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
            row.trim_start().starts_with(needle)
        })
        .unwrap_or_else(|| panic!("no row starts with {needle:?}"));
    buf[(0, y)].style()
}

fn with_pair(status: ToolStatus, summary: &str) -> ChatState {
    let (mut chat, turn) = open_turn();
    chat.apply(turn, call(1, "find_dir", "curso-rust"));
    chat.apply(turn, result(1, status, summary));
    chat.apply(turn, StreamEvent::Delta("It is under dev.".into()));
    chat.apply(turn, StreamEvent::Done(Finish::Stop));
    chat
}

#[test]
fn snapshot_a_call_and_its_result_between_the_question_and_the_answer() {
    let chat = with_pair(ToolStatus::Ok, "1 result");
    let buf = draw(&chat, 40, 9);
    insta::assert_snapshot!(text(&buf));
    assert_eq!(row_style(&buf, "» find_dir").fg, Some(theme().info_blue));
    assert_eq!(row_style(&buf, "ok: 1 result").fg, Some(theme().info_blue));
}

#[test]
fn snapshot_a_denied_run_reads_denied_in_the_alert_style() {
    let (mut chat, turn) = open_turn();
    chat.apply(turn, call(1, "run_in_pane", "rm -rf ~"));
    chat.apply(turn, result(1, ToolStatus::Denied, "not approved"));
    let buf = draw(&chat, 40, 8);
    insta::assert_snapshot!(text(&buf));
    assert_eq!(row_style(&buf, "denied").fg, Some(theme().warning_orange));
}

#[test]
fn snapshot_a_call_still_running_has_no_result_row_yet() {
    let (mut chat, turn) = open_turn();
    chat.apply(turn, call(1, "read_file", "~/notes.txt"));
    insta::assert_snapshot!(text(&draw(&chat, 40, 7)));
}

#[test]
fn snapshot_row_text_is_shown_literally_without_code_styling() {
    let (mut chat, turn) = open_turn();
    chat.apply(turn, call(1, "read_file", "```\n# x"));
    chat.apply(turn, result(1, ToolStatus::Error, "``` # y"));
    let buf = draw(&chat, 40, 7);
    insta::assert_snapshot!(text(&buf));
    // The last row is the composer, which has its own panel colour.
    for y in 0..buf.area.height - 1 {
        for x in 0..buf.area.width {
            assert_ne!(buf[(x, y)].bg, theme().bg_panel, "code style at {x},{y}");
        }
    }
}

#[test]
fn snapshot_an_answer_that_imitates_a_row_keeps_the_answer_style() {
    let (mut chat, turn) = open_turn();
    chat.apply(
        turn,
        StreamEvent::Delta("» find_dir curso-rust\n  ok: 1 result".into()),
    );
    chat.apply(turn, StreamEvent::Done(Finish::Stop));
    let buf = draw(&chat, 40, 7);
    insta::assert_snapshot!(text(&buf));
    assert_ne!(row_style(&buf, "» find_dir").fg, Some(theme().info_blue));
}

#[test]
fn a_tiny_pane_with_rows_never_panics() {
    let chat = with_pair(ToolStatus::Ok, "1 result");
    for width in 0..=10 {
        for height in 0..=2 {
            draw(&chat, width, height);
        }
    }
}
