//! The ORCH badge of an orchestrating chat and its worker rows (chat-pane,
//! "Per-pane ORCH mode and badge", "Worker rows").

use std::time::Duration;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier},
    widgets::Widget,
};

use super::*;
use crate::core::ai::{
    chat::{
        SendOutcome,
        engine::{Availability, Finish, StreamEvent},
        review::Badge,
    },
    config::{ApprovalMode, Task},
    orchestra::{RunId, WorkerId, event::WorkerRow, worker::WorkerStatus},
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

fn draw(chat: &ChatState, orchestrating: bool, width: u16, height: u16) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    ChatView::new(chat, &theme())
        .orchestrating(orchestrating)
        .render(area, &mut buf);
    buf
}

fn row(buf: &Buffer, y: u16) -> String {
    (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
}

fn shot_of(buf: &Buffer) -> String {
    (0..buf.area.height)
        .map(|y| row(buf, y).trim_end().to_owned())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The cells of `text` on the top row, starting at `x`, are a badge on `bg`.
fn assert_badge_at(buf: &Buffer, x: u16, text: &str, bg: Color) {
    let len = u16::try_from(text.len()).unwrap();
    let shown: String = (x..x + len).map(|x| buf[(x, 0)].symbol()).collect();
    assert_eq!(shown, text, "{:?}", row(buf, 0));
    for x in x..x + len {
        let cell = &buf[(x, 0)];
        assert_eq!(cell.fg, theme().bg_base, "fg at {x}");
        assert_eq!(cell.bg, bg, "bg at {x}");
        assert!(cell.modifier.contains(Modifier::BOLD), "bold at {x}");
    }
}

fn no_orch(buf: &Buffer) -> bool {
    (0..buf.area.height).all(|y| {
        !row(buf, y).contains("ORCH")
            && (0..buf.area.width).all(|x| buf[(x, y)].bg != theme().ai_purple)
    })
}

// chat-pane / Badge follows both switches: the view draws the badge only for
// a pane the app says orchestrates (gate and mode both on).
#[test]
fn the_orch_badge_is_drawn_only_while_the_pane_orchestrates() {
    let chat = answered();
    assert!(no_orch(&draw(&chat, false, 30, 5)));
    let on = draw(&chat, true, 30, 5);
    assert_badge_at(&on, 24, " ORCH ", theme().ai_purple);
    insta::assert_snapshot!("orch_badge_30x5", shot_of(&on));
}

#[test]
fn the_orch_badge_sits_left_of_the_mode_badge() {
    let mut chat = answered();
    chat.set_run_hint(true);
    chat.set_approval_mode(ApprovalMode::Auto);
    let buf = draw(&chat, true, 30, 5);
    assert_badge_at(&buf, 24, " AUTO ", theme().warning_orange);
    assert_badge_at(&buf, 18, " ORCH ", theme().ai_purple);
    insta::assert_snapshot!("orch_and_auto_badges_30x5", shot_of(&buf));
}

#[test]
fn the_orch_badge_shows_in_the_list_view_too() {
    let mut list = answered();
    list.open_list();
    let buf = draw(&list, true, 40, 8);
    assert_badge_at(&buf, 34, " ORCH ", theme().ai_purple);
}

#[test]
fn on_a_narrow_pane_the_orch_badge_is_one_letter_then_gone() {
    let mut chat = answered();
    chat.set_run_hint(true);
    chat.set_approval_mode(ApprovalMode::Run);
    // ` RUN ` and ` ORCH ` need 11 columns.
    let eleven = draw(&chat, true, 11, 4);
    assert_eq!(row(&eleven, 0), " ORCH  RUN ");
    let eight = draw(&chat, true, 8, 4);
    assert!(row(&eight, 0).ends_with(" RUN "), "{:?}", row(&eight, 0));
    assert_eq!(eight[(2, 0)].symbol(), "O");
    assert_eq!(eight[(2, 0)].bg, theme().ai_purple);
    let five = draw(&chat, true, 5, 4);
    assert_eq!(row(&five, 0), " RUN ", "the mode badge comes first");
    for width in 1..=12 {
        for height in 1..=2 {
            draw(&chat, true, width, height);
        }
    }
}

#[test]
fn the_batch_badge_leaves_room_for_the_orch_badge() {
    let mut chat = answered();
    chat.set_run_hint(true);
    chat.set_approval_mode(ApprovalMode::Auto);
    chat.set_batch_badge(Some(Badge::Orphan(2)));
    let buf = draw(&chat, true, 30, 5);
    let top = row(&buf, 0);
    assert!(top.starts_with(" orphan 2 "), "{top:?}");
    assert!(top.ends_with(" ORCH  AUTO "), "{top:?}");
}

// Worker rows (chat-pane, Row rendered) -----------------------------------

fn worker(id: u32, kind: Task, status: WorkerStatus, elapsed: Option<u64>) -> WorkerRow {
    WorkerRow {
        run: RunId(1),
        id: WorkerId(id),
        kind,
        engine: "claude".into(),
        model: Some("opus".into()),
        status,
        elapsed: elapsed.map(Duration::from_secs),
    }
}

const STATUSES: [WorkerStatus; 7] = [
    WorkerStatus::Queued,
    WorkerStatus::Running,
    WorkerStatus::Done,
    WorkerStatus::Failed,
    WorkerStatus::Cancelled,
    WorkerStatus::TimedOut,
    WorkerStatus::Refused,
];

/// One worker per status; those that ended ran 12 s.
fn with_workers() -> ChatState {
    let mut chat = answered();
    for (n, status) in STATUSES.into_iter().enumerate() {
        let ended = !matches!(status, WorkerStatus::Queued | WorkerStatus::Running);
        let id = u32::try_from(n).unwrap() + 1;
        chat.worker_row(worker(id, Task::Review, status, ended.then_some(12)));
    }
    chat
}

/// The row of the screen that ends with `text`.
fn find(buf: &Buffer, text: &str) -> u16 {
    (0..buf.area.height)
        .find(|&y| row(buf, y).trim_end().ends_with(text))
        .unwrap_or_else(|| panic!("no row ends with {text:?}:\n{}", shot_of(buf)))
}

#[test]
fn a_running_worker_row_shows_kind_engine_and_status() {
    let mut chat = answered();
    chat.worker_row(worker(1, Task::Review, WorkerStatus::Running, None));
    let buf = draw(&chat, true, 40, 6);
    let y = find(&buf, "review claude>opus running");
    assert_eq!(buf[(4, y)].fg, theme().info_blue);
}

#[test]
fn a_worker_row_per_status_is_drawn_in_the_tool_colour_or_the_alert_colour() {
    let buf = draw(&with_workers(), true, 40, 18);
    for (status, alert) in [
        ("queued", false),
        ("running", false),
        ("done 12.0s", false),
        ("failed 12.0s", true),
        ("cancelled 12.0s", false),
        ("timed_out 12.0s", true),
        ("refused 12.0s", true),
    ] {
        let y = find(&buf, &format!("review claude>opus {status}"));
        let colour = if alert {
            theme().warning_orange
        } else {
            theme().info_blue
        };
        assert_eq!(buf[(4, y)].fg, colour, "{status}");
    }
    insta::assert_snapshot!("worker_rows_per_status_40x18", shot_of(&buf));
}
