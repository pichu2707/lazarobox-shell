//! The `/cost` overlay drawn inside the chat pane (ai-cost 1c).

use ratatui::{
    buffer::Buffer,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
    layout::Rect,
    widgets::Widget,
};

use super::*;
use crate::core::ai::chat::{
    cost_view::CostView,
    dashboard::{Range, UsageData},
    usage_view::UsageRow,
};

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
fn snapshot_the_cost_overlay_with_data_under_40_columns() {
    let chat = chat_with(CostView::with_data(usage(), lines()));
    insta::assert_snapshot!(text(&draw(&chat, 36, 12)));
}

#[test]
fn snapshot_the_cost_overlay_while_reading() {
    let chat = chat_with(CostView::reading());
    insta::assert_snapshot!(text(&draw(&chat, 40, 5)));
}

#[test]
fn snapshot_a_narrow_pane_wraps_and_keeps_the_label_and_partial() {
    let mut rows = lines();
    rows[5] = "  claude › claude-sonnet-4-5-20250929-with-a-long-name: in 2 · out 4 · $0.0049 list-price equivalent · partial".into();
    let chat = chat_with(CostView::with(rows));
    let shown = text(&draw(&chat, 30, 14));
    assert!(shown.contains("list-price"), "{shown}");
    assert!(shown.contains("partial"), "{shown}");
    insta::assert_snapshot!(shown);
}

#[test]
fn snapshot_no_data_and_partial() {
    let chat = chat_with(CostView::with_data(
        UsageData::default(),
        [
            "AI usage (alpha)",
            "today: no data · 1 turns · partial",
            "last 7 days: no data · 1 turns · partial",
            "by engine and model (last 7 days):",
            "  ollama › llama3:8b: in no data · out 20 · partial",
            "this chat: no data",
        ]
        .map(str::to_owned)
        .to_vec(),
    ));
    insta::assert_snapshot!(text(&draw(&chat, 36, 9)));
}

#[test]
fn the_ctrl_c_note_replaces_the_keys_line() {
    let mut view = CostView::with(lines());
    view.on_key(&KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    let shown = text(&draw(&chat_with(view), 72, 10));
    assert!(shown.ends_with("Ctrl+C then cancels the answer"), "{shown}");
}

#[test]
fn scrolling_follows_the_wrapped_rows() {
    let mut chat = chat_with(CostView::with(lines()));
    // The first draw measures; the keys then scroll the wrapped rows.
    draw(&chat, 20, 5);
    for _ in 0..20 {
        chat.cost_mut()
            .unwrap()
            .on_key(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }
    let shown = text(&draw(&chat, 20, 5));
    assert!(shown.contains("this chat"), "the end is reachable: {shown}");
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
    let mut chat = chat_with(CostView::with(lines()));
    draw(&chat, 72, 4);
    for _ in 0..3 {
        chat.cost_mut()
            .unwrap()
            .on_key(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }
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

/// The text lines plus the charts of real rows, built by the fold itself.
fn with_charts() -> Vec<String> {
    use crate::core::ai::chat::usage_view::{Session, UsageRow, report_lines};
    let day = |days_ago, engine: &str, total: Option<u64>| UsageRow {
        days_ago,
        engine: engine.into(),
        model: Some("m".into()),
        in_chat: false,
        turns: 1,
        records: 1,
        costed: 0,
        input: total.map(|_| 1),
        output: total.map(|_| 1),
        cache_read: None,
        cache_hit: None,
        cache_base: None,
        cache_gap: 0,
        prefill_ms: None,
        prefill_tokens: None,
        prefill_gap: 0,
        cost_micros: None,
        total,
        partial: false,
        workers: 0,
        worker_total: None,
        worker_partial: false,
        ..UsageRow::default()
    };
    let rows = [
        day(3, "claude", Some(0)),
        day(2, "claude", Some(100)),
        day(1, "claude", Some(200)),
        day(0, "claude", Some(400)),
        day(0, "ollama", Some(100)),
        day(0, "codex", None),
    ];
    report_lines(&rows, &Session::default(), &|_| None)
}

#[test]
fn snapshot_the_charts_under_the_text_lines() {
    let chat = chat_with(CostView::with_data(usage(), with_charts()));
    insta::assert_snapshot!(text(&draw(&chat, 36, 22)));
}

#[test]
fn snapshot_the_charts_in_a_narrow_pane() {
    let chat = chat_with(CostView::with(with_charts()));
    insta::assert_snapshot!(text(&draw(&chat, 24, 30)));
}

#[test]
fn charts_render_without_panic_at_any_width() {
    for width in 0..=12 {
        for height in [0, 1, 2, 5] {
            let chat = chat_with(CostView::with(with_charts()));
            draw(&chat, width, height);
        }
    }
}

#[test]
fn the_chart_lines_are_not_taken_for_this_chat() {
    let view = CostView::with(with_charts());
    let rows = view.body_rows(60);
    let charts = rows
        .iter()
        .position(|(row, _)| row.starts_with("per day"))
        .expect("charts shown");
    assert!(rows[charts..].iter().all(|(_, mine)| !mine));
}

#[test]
fn snapshot_a_narrow_pane_wraps_a_long_cache_line_and_keeps_partial() {
    use crate::core::ai::chat::usage_view::{Session, UsageRow, report_lines};
    let row = |engine: &str, hit, gap| UsageRow {
        engine: engine.into(),
        model: Some("m".into()),
        turns: 1,
        records: 1,
        input: Some(10),
        output: Some(5),
        cache_read: Some(hit),
        cache_hit: Some(hit),
        cache_base: Some(100),
        cache_gap: gap,
        prefill_ms: None,
        prefill_tokens: None,
        prefill_gap: 0,
        total: Some(15),
        ..UsageRow::default()
    };
    let rows = [
        row("claude", 91, 0),
        row("codex", 79, 1),
        row("gemini", 40, 0),
        row("llamacpp", 5, 0),
        row("ollama-cloud", 12, 0),
    ];
    let billing = |_: &str| None;
    let mut lines = report_lines(&rows, &Session::default(), &billing);
    lines.retain(|l| l.starts_with("cache") || l.starts_with("AI usage"));
    let chat = chat_with(CostView::with(lines));
    let shown = text(&draw(&chat, 30, 14));
    assert!(shown.contains("partial"), "{shown}");
    insta::assert_snapshot!(shown);
}

#[test]
fn snapshot_a_narrow_pane_wraps_a_long_prefill_line_and_keeps_partial() {
    use crate::core::ai::chat::usage_view::{Session, UsageRow, report_lines};
    let row = |engine: &str, ms, gap| UsageRow {
        engine: engine.into(),
        model: Some("m".into()),
        turns: 1,
        records: 1,
        input: Some(10),
        output: Some(5),
        prefill_ms: Some(ms),
        prefill_tokens: Some(1_000),
        prefill_gap: gap,
        total: Some(15),
        ..UsageRow::default()
    };
    let rows = [
        row("llamacpp", 3, 0),
        row("lmstudio", 12, 1),
        row("ollama", 1, 0),
        row("vllm", 8, 0),
    ];
    let billing = |_: &str| None;
    let mut lines = report_lines(&rows, &Session::default(), &billing);
    lines.retain(|l| l.starts_with("prefill") || l.starts_with("AI usage"));
    let chat = chat_with(CostView::with(lines));
    let shown = text(&draw(&chat, 30, 14));
    assert!(shown.contains("partial"), "{shown}");
    insta::assert_snapshot!(shown);
}

/// Two chats and a run in the week, one of the chats also the week before.
fn usage() -> UsageData {
    let row = |days_ago, input, output, wall| UsageRow {
        days_ago,
        engine: "claude".into(),
        model: Some("sonnet".into()),
        turns: 3,
        records: 3,
        input: Some(input),
        output: Some(output),
        chat_output: Some(output),
        chat_outputs: 3,
        cache_hit: Some(63),
        cache_base: Some(100),
        wall_ms: Some(wall),
        wall_timed: 3,
        ..UsageRow::default()
    };
    UsageData {
        rows: vec![
            row(0, 700, 200, 60_000),
            row(2, 410, 100, 120_000),
            row(9, 300, 90, 30_000),
        ],
        tasks: vec![(0, "c1".into()), (2, "c2".into()), (2, "r7".into())],
    }
}

fn dashboard(width: u16, height: u16) -> String {
    let chat = chat_with(CostView::with_data(usage(), lines()));
    text(&draw(&chat, width, height))
}

#[test]
fn snapshot_the_dashboard_in_a_wide_pane() {
    insta::assert_snapshot!(dashboard(100, 10));
}

#[test]
fn snapshot_the_dashboard_in_a_medium_pane() {
    insta::assert_snapshot!(dashboard(72, 15));
}

#[test]
fn snapshot_the_dashboard_in_a_narrow_pane() {
    insta::assert_snapshot!(dashboard(50, 24));
}

#[test]
fn snapshot_the_dashboard_with_no_usage() {
    let chat = chat_with(CostView::with_data(UsageData::default(), lines()));
    insta::assert_snapshot!(text(&draw(&chat, 100, 8)));
}

/// Whether some line of the sheet holds every label.
fn share_a_line(shown: &str, labels: [&str; 2]) -> bool {
    shown
        .lines()
        .any(|line| labels.iter().all(|label| line.contains(label)))
}

#[test]
fn the_tiers_draw_cards_in_a_row_a_grid_and_a_column() {
    let wide = dashboard(100, 10);
    assert!(share_a_line(&wide, ["Input", "Agent time"]), "{wide}");
    let medium = dashboard(72, 15);
    assert!(share_a_line(&medium, ["Input", "Output"]), "{medium}");
    assert!(!share_a_line(&medium, ["Input", "Tasks"]), "{medium}");
    let narrow = dashboard(50, 24);
    assert!(!share_a_line(&narrow, ["Input", "Output"]), "{narrow}");
    assert!(narrow.contains("Agent time"), "{narrow}");
}

#[test]
fn under_40_columns_the_text_report_replaces_the_cards() {
    let shown = dashboard(39, 12);
    assert!(shown.starts_with("AI usage (alpha)"), "{shown}");
    assert!(!shown.contains("Input"), "{shown}");
    assert!(shown.contains("today: 6 tokens"), "{shown}");
    assert!(dashboard(40, 12).contains("Input"));
}

#[test]
fn the_active_range_is_bracketed_so_colour_is_not_needed() {
    let mut chat = chat_with(CostView::with_data(usage(), lines()));
    assert!(text(&draw(&chat, 100, 10)).contains("[7d] 30d 90d All"));
    chat.cost_mut()
        .unwrap()
        .on_key(&KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
    assert!(text(&draw(&chat, 100, 10)).contains("7d [30d] 90d All"));
    assert_eq!(
        chat.cost().unwrap().dashboard().unwrap().range,
        Range::Month
    );
}

#[test]
fn a_range_key_in_the_text_tier_shows_the_note_on_the_keys_line() {
    let mut chat = chat_with(CostView::with_data(usage(), lines()));
    draw(&chat, 36, 12);
    chat.cost_mut()
        .unwrap()
        .on_key(&KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE));
    let shown = text(&draw(&chat, 36, 12));
    assert!(
        shown.ends_with("widen the pane to change the range"),
        "{shown}"
    );
}

#[test]
fn the_keys_line_lists_the_range_keys() {
    let shown = dashboard(100, 10);
    assert!(
        shown.ends_with("Esc close · 1-4 range · ↑↓ scroll"),
        "{shown}"
    );
}

#[test]
fn the_dashboard_scrolls_over_its_height() {
    let mut chat = chat_with(CostView::with_data(usage(), lines()));
    // A column of cards is 21 rows: a 10-row pane hides the lower cards.
    assert!(!text(&draw(&chat, 50, 10)).contains("Agent time"));
    for _ in 0..30 {
        chat.cost_mut()
            .unwrap()
            .on_key(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }
    assert!(text(&draw(&chat, 50, 10)).contains("Agent time"));
}

#[test]
fn every_pane_size_draws_the_dashboard_without_panicking() {
    for data in [usage(), UsageData::default()] {
        let mut chat = chat_with(CostView::with_data(data, lines()));
        for width in 0..=120 {
            for height in 0..=40 {
                draw(&chat, width, height);
            }
            chat.cost_mut()
                .unwrap()
                .on_key(&KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE));
        }
    }
}
