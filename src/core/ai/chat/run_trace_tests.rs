//! The worker transcript of the run view (agent-orchestrator, Run view and
//! persistence): bounded, sanitized, apart from the chat entries.

use std::time::Duration;

use super::{
    engine::{CallId, ToolStatus},
    run_trace::{CUT_LINE, MAX_TRACED, NO_OUTPUT, Scroll, TRACE_CAP},
    state_tests::chat,
    transcript::LineKind,
    *,
};
use crate::core::ai::{
    config::Task,
    orchestra::{RunId, WorkerId, event::TraceItem, event::WorkerRow, worker::WorkerStatus},
};

const RUN: RunId = RunId(7);

fn row(id: u32, status: WorkerStatus) -> WorkerRow {
    WorkerRow {
        run: RUN,
        id: WorkerId(id),
        kind: Task::Review,
        engine: "codex".into(),
        model: Some("gpt-5".into()),
        status,
        elapsed: Some(Duration::from_secs(3)),
        usage: None,
        partial: false,
        overridden: false,
    }
}

/// A chat whose run has worker `id` running, with the view open on it.
fn viewing(id: u32) -> ChatState {
    let mut state = chat();
    state.worker_row(row(id, WorkerStatus::Running));
    state.set_orch_run(Some(RUN));
    assert!(state.open_run_view(WorkerId(id)));
    state.trace(RUN, WorkerId(id), TraceItem::Begin);
    state
}

fn text(piece: &str) -> TraceItem {
    TraceItem::Text(piece.to_owned())
}

fn rows(state: &ChatState) -> Vec<String> {
    (state.run_view_rows(80).iter())
        .map(|line| line.text.clone())
        .collect()
}

#[test]
fn text_pieces_join_into_wrapped_rows() {
    let mut state = viewing(1);
    state.trace(RUN, WorkerId(1), text("hel"));
    state.trace(RUN, WorkerId(1), text("lo\nworld"));
    assert_eq!(rows(&state), ["hello", "world"]);
}

#[test]
fn control_characters_of_a_worker_are_stripped() {
    let mut state = viewing(1);
    state.trace(RUN, WorkerId(1), text("a\u{1b}[31mred\u{7}\u{1b}[0m b"));
    state.trace(
        RUN,
        WorkerId(1),
        TraceItem::Call {
            id: CallId(1),
            name: "run\u{1b}[2J".into(),
            summary: "ls \u{1b}]0;title\u{7}-la".into(),
        },
    );
    let shown = rows(&state);
    assert_eq!(shown, ["ared b", "\u{bb} run ls -la"]);
    assert!(shown.iter().all(|row| !row.contains('\u{1b}')));
}

#[test]
fn a_tool_call_and_its_result_are_two_rows() {
    let mut state = viewing(1);
    let call = TraceItem::Call {
        id: CallId(1),
        name: "read_file".into(),
        summary: "a.rs".into(),
    };
    state.trace(RUN, WorkerId(1), call);
    state.trace(
        RUN,
        WorkerId(1),
        TraceItem::Result {
            id: CallId(1),
            status: ToolStatus::Ok,
            summary: "10 lines".into(),
        },
    );
    state.trace(
        RUN,
        WorkerId(1),
        TraceItem::Result {
            id: CallId(9),
            status: ToolStatus::Error,
            summary: "no such call".into(),
        },
    );
    let lines = state.run_view_rows(80);
    let shown: Vec<_> = lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(shown, ["\u{bb} read_file a.rs", "  ok: 10 lines"]);
    assert_eq!(lines[1].kind, LineKind::Tool);
}

#[test]
fn a_failed_tool_result_is_drawn_as_an_alert() {
    let mut state = viewing(1);
    state.trace(
        RUN,
        WorkerId(1),
        TraceItem::Call {
            id: CallId(1),
            name: "run".into(),
            summary: String::new(),
        },
    );
    state.trace(
        RUN,
        WorkerId(1),
        TraceItem::Result {
            id: CallId(1),
            status: ToolStatus::Denied,
            summary: String::new(),
        },
    );
    assert_eq!(state.run_view_rows(80)[1].kind, LineKind::ToolAlert);
}

#[test]
fn past_the_cap_the_oldest_goes_and_the_view_says_so() {
    let mut state = viewing(1);
    let line = format!("{}\n", "x".repeat(1023));
    for n in 0..300 {
        state.trace(RUN, WorkerId(1), text(&line));
        assert!(n < 300);
    }
    state.trace(RUN, WorkerId(1), text("last"));
    let shown = rows(&state);
    assert_eq!(shown[0], CUT_LINE);
    assert_eq!(shown.last().map(String::as_str), Some("last"));
    let kept: usize = shown.iter().map(String::len).sum();
    assert!(kept <= TRACE_CAP + 80, "kept {kept} bytes");
}

#[test]
fn one_huge_piece_is_cut_at_a_character_boundary() {
    let mut state = viewing(1);
    state.trace(RUN, WorkerId(1), text(&"\u{e9}".repeat(TRACE_CAP)));
    let shown = rows(&state);
    assert_eq!(shown[0], CUT_LINE);
    assert!(
        shown
            .iter()
            .skip(1)
            .all(|row| row.chars().all(|c| c == '\u{e9}'))
    );
}

#[test]
fn a_new_attempt_starts_a_clean_transcript() {
    let mut state = viewing(1);
    state.trace(RUN, WorkerId(1), text("first try"));
    state.trace(RUN, WorkerId(1), TraceItem::Begin);
    state.trace(RUN, WorkerId(1), text("second"));
    assert_eq!(rows(&state), ["second"]);
}

#[test]
fn only_the_run_of_the_pane_is_kept_and_only_so_many_workers() {
    let mut state = viewing(0);
    state.trace(RunId(8), WorkerId(0), text("another run"));
    assert!(!state.has_trace(RunId(8), WorkerId(0)));
    for id in 0..=u32::try_from(MAX_TRACED).unwrap() {
        state.trace(RUN, WorkerId(id), text("x"));
    }
    // The oldest worker is dropped, the newest kept.
    assert!(!state.has_trace(RUN, WorkerId(0)) || state.run_view().is_some());
    assert!(state.has_trace(RUN, WorkerId(u32::try_from(MAX_TRACED).unwrap())));
    assert!(!state.has_trace(RUN, WorkerId(1)));
}

#[test]
fn the_view_needs_a_run_and_a_transcript_goes_with_the_view_once_the_run_is_over() {
    let mut empty = chat();
    assert!(!empty.open_run_view(WorkerId(1)));
    assert!(empty.run_view().is_none());

    let mut state = viewing(1);
    state.trace(RUN, WorkerId(1), text("kept while open"));
    state.end_orch_run(RUN);
    state.set_orch_run(None);
    assert_eq!(rows(&state), ["kept while open"]);
    state.close_run_view();
    assert!(state.run_view().is_none());
    assert!(!state.has_trace(RUN, WorkerId(1)));
}

#[test]
fn the_head_names_the_worker_and_its_status() {
    let mut state = viewing(3);
    assert_eq!(
        state.run_view_head().as_deref(),
        Some("worker 3 \u{b7} review codex>gpt-5 running 3.0s")
    );
    state.worker_row(row(3, WorkerStatus::Done));
    assert!(state.run_view_head().unwrap().contains("done"));
}

#[test]
fn the_view_follows_the_end_until_scrolled_and_again_on_demand() {
    let mut state = viewing(1);
    assert_eq!(state.run_view_top(100, 10), 90);
    state.run_view_scroll(Scroll::Up(3), 100, 10);
    assert_eq!(state.run_view_top(100, 10), 87);
    // New output does not move a scrolled view.
    assert_eq!(state.run_view_top(120, 10), 87);
    state.run_view_scroll(Scroll::Top, 100, 10);
    assert_eq!(state.run_view_top(100, 10), 0);
    state.run_view_scroll(Scroll::Up(5), 100, 10);
    assert_eq!(state.run_view_top(100, 10), 0);
    state.run_view_scroll(Scroll::Down(95), 100, 10);
    // Reaching the end follows it again.
    assert_eq!(state.run_view_top(130, 10), 120);
    state.run_view_scroll(Scroll::Up(1), 100, 10);
    state.run_view_scroll(Scroll::Follow, 100, 10);
    assert_eq!(state.run_view_top(140, 10), 130);
    // A short transcript never scrolls.
    assert_eq!(state.run_view_top(4, 10), 0);
}

#[test]
fn a_worker_with_no_output_yet_says_so() {
    let state = viewing(1);
    assert_eq!(rows(&state), [NO_OUTPUT]);
}

#[test]
fn a_step_that_comes_after_its_run_ended_is_dropped() {
    let mut state = chat();
    state.trace(RUN, WorkerId(1), text("late"));
    assert!(!state.has_trace(RUN, WorkerId(1)));
}

/// The bytes of the text rows of the view.
fn kept(state: &ChatState) -> usize {
    rows(state).iter().map(String::len).sum()
}

#[test]
fn a_call_after_a_full_text_drops_only_the_excess() {
    let mut state = viewing(1);
    let line = format!("{}\n", "x".repeat(99));
    let lines = (TRACE_CAP - 100) / line.len();
    let text_bytes = line.len() * lines;
    state.trace(RUN, WorkerId(1), text(&line.repeat(lines)));
    let before = kept(&state);
    assert!(before > TRACE_CAP / 2, "{before} of {text_bytes}");
    state.trace(
        RUN,
        WorkerId(1),
        TraceItem::Call {
            id: CallId(1),
            name: "run".into(),
            summary: "y".repeat(400),
        },
    );
    state.trace(RUN, WorkerId(1), text(&line.repeat(3)));
    let shown = rows(&state);
    assert!(
        kept(&state) > TRACE_CAP / 2,
        "the text was wiped: {}",
        kept(&state)
    );
    assert!(shown.iter().any(|row| row.starts_with("\u{bb} run")));
    // The 99-character rows wrap at 80 columns: the last row is the 19 left.
    assert_eq!(shown.last().map(String::len), Some(19));
}

#[test]
fn a_trim_cuts_down_to_leave_room() {
    let mut state = viewing(1);
    let line = format!("{}\n", "x".repeat(1023));
    for _ in 0..(TRACE_CAP / line.len() + 2) {
        state.trace(RUN, WorkerId(1), text(&line));
    }
    let after = kept(&state);
    assert!(after <= TRACE_CAP / 8 * 7 + 1100, "kept {after}");
}

#[test]
fn a_worker_traced_again_after_an_eviction_shows_the_cut() {
    let mut state = viewing(1);
    // No `Begin` first: its start was dropped with an evicted trace.
    state.trace(RUN, WorkerId(2), text("tail"));
    state.close_run_view();
    state.worker_row(row(2, WorkerStatus::Running));
    assert!(state.open_run_view(WorkerId(2)));
    assert_eq!(rows(&state), [CUT_LINE, "tail"]);
}

#[test]
fn tool_rows_are_capped() {
    let mut state = viewing(1);
    state.trace(
        RUN,
        WorkerId(1),
        TraceItem::Call {
            id: CallId(1),
            name: "run".into(),
            summary: "y".repeat(5000),
        },
    );
    state.trace(
        RUN,
        WorkerId(1),
        TraceItem::Result {
            id: CallId(1),
            status: ToolStatus::Ok,
            summary: "z".repeat(5000),
        },
    );
    assert!(kept(&state) < 1200, "{}", kept(&state));
}

#[test]
fn the_wrapped_rows_are_kept_until_the_transcript_or_the_width_changes() {
    let mut state = viewing(1);
    state.trace(RUN, WorkerId(1), text(&"word ".repeat(2000)));
    let first = state.run_view_rows(80);
    assert!(std::sync::Arc::ptr_eq(&first, &state.run_view_rows(80)));
    assert!(!std::sync::Arc::ptr_eq(&first, &state.run_view_rows(40)));
    let again = state.run_view_rows(80);
    state.trace(RUN, WorkerId(1), text("more"));
    assert!(!std::sync::Arc::ptr_eq(&again, &state.run_view_rows(80)));
    assert!(state.run_view_rows(80).iter().any(|l| l.text == "more"));
}
