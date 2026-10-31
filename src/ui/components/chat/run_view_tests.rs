//! The full-pane transcript of a worker (agent-orchestrator S9, Run view and
//! persistence): header, rows, footer, and the approval prompt under it.

use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

use super::*;
use crate::core::ai::{
    chat::{
        approval::READ_KEYS,
        engine::{CallId, ToolStatus},
        run_trace::Scroll,
    },
    config::Task,
    orchestra::{
        RunId, WorkerId,
        event::{TraceItem, WorkerRow},
        worker::WorkerStatus,
    },
};

const RUN: RunId = RunId(7);

fn theme() -> LazaroboxTheme {
    LazaroboxTheme::default()
}

/// A chat with worker 1 of run 7 running and the view open on it.
fn viewing() -> ChatState {
    let mut chat = ChatState::new(None);
    chat.worker_row(WorkerRow {
        run: RUN,
        id: WorkerId(1),
        kind: Task::Review,
        engine: "codex".into(),
        model: Some("gpt-5".into()),
        status: WorkerStatus::Running,
        elapsed: None,
        usage: None,
        partial: false,
        overridden: false,
    });
    chat.set_orch_run(Some(RUN));
    assert!(chat.open_run_view(WorkerId(1)));
    chat.trace(RUN, WorkerId(1), TraceItem::Begin);
    chat
}

fn say(chat: &mut ChatState, text: &str) {
    chat.trace(RUN, WorkerId(1), TraceItem::Text(text.to_owned()));
}

fn draw(chat: &ChatState, approval: Option<&ApprovalPrompt>, w: u16, h: u16) -> String {
    let area = Rect::new(0, 0, w, h);
    let mut buf = Buffer::empty(area);
    ChatView::new(chat, &theme())
        .approval(approval)
        .render(area, &mut buf);
    (0..h)
        .map(|y| {
            let row: String = (0..w).map(|x| buf[(x, y)].symbol()).collect();
            row.trim_end().to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn prompt() -> ApprovalPrompt {
    ApprovalPrompt {
        target: "send a file's text to the engine".into(),
        command: "read ~/notes.txt".into(),
        keys: READ_KEYS,
        armed: true,
        why: None,
        notice: None,
    }
}

#[test]
fn the_view_takes_the_whole_pane_with_a_header_rows_and_a_footer() {
    let mut chat = viewing();
    say(&mut chat, "reading the parser\n");
    chat.trace(
        RUN,
        WorkerId(1),
        TraceItem::Call {
            id: CallId(1),
            name: "read_file".into(),
            summary: "src/parser.rs".into(),
        },
    );
    chat.trace(
        RUN,
        WorkerId(1),
        TraceItem::Result {
            id: CallId(1),
            status: ToolStatus::Ok,
            summary: "120 lines".into(),
        },
    );
    say(&mut chat, "found a bug in the tokenizer");
    insta::assert_snapshot!("run_view_following", draw(&chat, None, 50, 10));
}

#[test]
fn a_scrolled_view_says_so_in_the_footer() {
    let mut chat = viewing();
    let lines: String = (0..30).map(|n| format!("line {n}\n")).collect();
    say(&mut chat, &lines);
    let rows = 8;
    let total = chat.run_view_rows(50).len();
    chat.run_view_scroll(Scroll::Up(5), total, rows);
    insta::assert_snapshot!("run_view_scrolled", draw(&chat, None, 50, 10));
}

#[test]
fn an_approval_keeps_its_rows_under_the_view() {
    let mut chat = viewing();
    say(&mut chat, "needs a file");
    let shown = draw(&chat, Some(&prompt()), 50, 12);
    insta::assert_snapshot!("run_view_with_approval", shown);
    assert!(shown.contains("read ~/notes.txt"));
    assert!(shown.contains("needs a file"));
}

#[test]
fn the_composer_cursor_is_not_shown_in_the_view() {
    let chat = viewing();
    assert_eq!(
        ChatView::new(&chat, &theme()).cursor(Rect::new(0, 0, 50, 10)),
        None
    );
}

#[test]
fn a_tiny_pane_does_not_panic() {
    let mut chat = viewing();
    say(&mut chat, "text");
    for (w, h) in [(1, 1), (10, 1), (10, 2), (3, 3), (50, 3)] {
        let _ = draw(&chat, None, w, h);
        let _ = draw(&chat, Some(&prompt()), w, h);
    }
}
