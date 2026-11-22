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
    dashboard::{AgentDay, HourCell, Range, Span, UsageData},
    provider::Provider,
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
    let chat = chat_with(CostView::with_data(usage(), Vec::new(), lines()));
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
        Vec::new(),
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

#[test]
fn the_text_report_renders_without_panic_at_any_width() {
    for width in 0..=12 {
        for height in [0, 1, 2, 5] {
            let chat = chat_with(CostView::with(lines()));
            draw(&chat, width, height);
        }
    }
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
        total: Some(input + output),
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
        // Tuesday 14:00 is the peak; the nine-day-old hour is outside the week.
        hours: vec![
            hour(0, 1, 14, 900),
            hour(0, 1, 15, 300),
            hour(2, 3, 9, 510),
            hour(2, 3, 10, 100),
            hour(0, 5, 22, 50),
            hour(9, 0, 8, 400),
        ],
        // Two workers of claude overlapped and one of ollama has no wall time.
        agents: vec![
            agent_day("claude", (2, 1_200, 300), Some(95_000)),
            agent_day("ollama", (1, 400, 80), None),
        ],
        spans: vec![
            Span {
                days_ago: 0,
                start: 100,
                end: 160,
            },
            Span {
                days_ago: 0,
                start: 130,
                end: 195,
            },
        ],
    }
}

fn agent_day(
    engine: &str,
    (workers, input, output): (u64, u64, u64),
    wall: Option<u64>,
) -> AgentDay {
    AgentDay {
        engine: engine.into(),
        workers,
        input: Some(input),
        output: Some(output),
        wall_ms: wall,
        timed: wall.map_or(0, |_| workers),
        ..AgentDay::default()
    }
}

fn hour(days_ago: u32, weekday: u8, hour: u8, tokens: u64) -> HourCell {
    HourCell {
        days_ago,
        weekday,
        hour,
        tokens,
    }
}

fn dashboard(width: u16, height: u16) -> String {
    let chat = chat_with(CostView::with_data(usage(), Vec::new(), lines()));
    text(&draw(&chat, width, height))
}

#[test]
fn snapshot_the_dashboard_in_a_wide_pane() {
    insta::assert_snapshot!(dashboard(100, 41));
}

#[test]
fn snapshot_the_dashboard_in_a_medium_pane() {
    insta::assert_snapshot!(dashboard(72, 46));
}

#[test]
fn snapshot_the_dashboard_in_a_narrow_pane() {
    insta::assert_snapshot!(dashboard(50, 55));
}

#[test]
fn snapshot_the_dashboard_with_no_usage() {
    let chat = chat_with(CostView::with_data(
        UsageData::default(),
        Vec::new(),
        lines(),
    ));
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
    let wide = dashboard(100, 15);
    assert!(share_a_line(&wide, ["Input", "Agent time"]), "{wide}");
    let medium = dashboard(72, 20);
    assert!(share_a_line(&medium, ["Input", "Output"]), "{medium}");
    assert!(!share_a_line(&medium, ["Input", "Tasks"]), "{medium}");
    let narrow = dashboard(50, 32);
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
    let mut chat = chat_with(CostView::with_data(usage(), Vec::new(), lines()));
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
    let mut chat = chat_with(CostView::with_data(usage(), Vec::new(), lines()));
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
    let shown = dashboard(100, 15);
    assert!(
        shown.ends_with("Esc close · 1-4 range · ↑↓ scroll"),
        "{shown}"
    );
}

#[test]
fn the_dashboard_scrolls_over_its_height() {
    let mut chat = chat_with(CostView::with_data(usage(), Vec::new(), lines()));
    // A column of cards is 21 rows: a 10-row pane hides the lower cards.
    assert!(!text(&draw(&chat, 50, 10)).contains("Agent time"));
    for _ in 0..13 {
        chat.cost_mut()
            .unwrap()
            .on_key(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }
    assert!(text(&draw(&chat, 50, 10)).contains("Agent time"));
}

#[test]
fn every_pane_size_draws_the_dashboard_without_panicking() {
    for data in [usage(), UsageData::default()] {
        let mut chat = chat_with(CostView::with_data(data, Vec::new(), lines()));
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

/// The week 11x the one before: every delta is four digits (`▲ 1000%`).
fn surge(times: u64) -> UsageData {
    let row = |days_ago, n: u64| UsageRow {
        days_ago,
        engine: "claude".into(),
        turns: 1,
        records: 1,
        input: Some(1_000 * n),
        output: Some(100 * n),
        chat_output: Some(100 * n),
        chat_outputs: 1,
        wall_ms: Some(60_000 * n),
        wall_timed: 1,
        ..UsageRow::default()
    };
    UsageData {
        rows: vec![row(0, times), row(8, 1)],
        tasks: vec![
            (0, "c1".into()),
            (0, "c2".into()),
            (0, "c3".into()),
            (8, "c4".into()),
        ],
        ..UsageData::default()
    }
}

fn surge_at(width: u16, times: u64) -> String {
    let chat = chat_with(CostView::with_data(surge(times), Vec::new(), Vec::new()));
    text(&draw(&chat, width, 40))
}

/// Every arrow is followed by its whole number and a `%`.
fn deltas_are_whole(shown: &str) -> bool {
    shown.match_indices("▲ ").all(|(at, arrow)| {
        let rest = &shown[at + arrow.len()..];
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        digits > 0 && rest[digits..].starts_with('%')
    })
}

#[test]
fn a_four_digit_delta_is_never_clipped_into_another_number() {
    for width in [80, 79, 60, 59, 40] {
        let shown = surge_at(width, 11);
        assert!(shown.contains("▲ 1000%"), "{width}: {shown}");
        assert!(deltas_are_whole(&shown), "{width}: {shown}");
    }
    // Under 40 columns there are no cards, so no delta at all.
    assert!(!surge_at(39, 11).contains('▲'));
}

#[test]
fn a_delta_that_does_not_fit_is_capped_not_cut() {
    // At 80 columns the Agent time card has no room for five digits.
    let tight = surge_at(80, 121);
    assert!(tight.contains("▲ 999%+"), "{tight}");
    for width in [80, 79, 60, 59, 40] {
        let shown = surge_at(width, 121);
        assert!(
            deltas_are_whole(&shown.replace("%+", "%")),
            "{width}: {shown}"
        );
    }
}

#[test]
fn the_keys_line_advertises_the_range_only_where_it_works() {
    let narrow = dashboard(39, 12);
    assert!(narrow.ends_with("Esc close · ↑↓ scroll"), "{narrow}");
    assert!(!narrow.contains("1-4"), "{narrow}");
    assert!(dashboard(40, 12).ends_with("1-4 range · ↑↓ scroll"));
}

#[test]
fn the_wide_tiers_do_not_repeat_the_text_report_under_the_cards() {
    for (width, height) in [(100, 41), (72, 46), (50, 55)] {
        let shown = dashboard(width, height);
        assert!(!shown.contains("this chat:"), "{width}: {shown}");
        assert!(!shown.contains("last 7 days:"), "{width}: {shown}");
        assert!(!shown.contains("by engine and model"), "{width}: {shown}");
        assert_eq!(shown.matches("AI usage (alpha)").count(), 1, "{shown}");
    }
}

#[test]
fn the_empty_state_does_not_repeat_the_report_under_it() {
    let chat = chat_with(CostView::with_data(
        UsageData::default(),
        Vec::new(),
        lines(),
    ));
    assert!(!text(&draw(&chat, 100, 20)).contains("this chat"));
}

fn press(chat: &mut ChatState, code: KeyCode) {
    chat.cost_mut()
        .unwrap()
        .on_key(&KeyEvent::new(code, KeyModifiers::NONE));
}

#[test]
fn a_range_switch_that_shortens_the_sheet_pulls_the_scroll_back() {
    // Only the month has rows: its sheet is tall, the empty week is short.
    let mut data = usage();
    data.rows.retain(|r| r.days_ago > 7);
    let mut chat = chat_with(CostView::with_data(data, Vec::new(), lines()));
    press(&mut chat, KeyCode::Char('2'));
    draw(&chat, 50, 10);
    for _ in 0..40 {
        press(&mut chat, KeyCode::Down);
    }
    assert!(!text(&draw(&chat, 50, 10)).contains("[30d]"));
    press(&mut chat, KeyCode::Char('1'));
    let shown = text(&draw(&chat, 50, 10));
    assert!(shown.contains("[7d] 30d 90d All"), "{shown}");
    assert!(shown.contains("no usage recorded yet"), "{shown}");
}

#[test]
fn a_resize_that_shortens_the_sheet_pulls_the_scroll_back() {
    let mut chat = chat_with(CostView::with_data(usage(), Vec::new(), lines()));
    draw(&chat, 50, 10);
    for _ in 0..60 {
        press(&mut chat, KeyCode::Down);
    }
    assert!(!text(&draw(&chat, 50, 10)).contains("[7d]"));
    let shown = text(&draw(&chat, 100, 41));
    assert!(shown.contains("[7d] 30d 90d All"), "{shown}");
}

#[test]
fn snapshot_a_card_with_an_approximate_time_and_millions() {
    let mut data = usage();
    data.rows[0].input = Some(1_234_567);
    // Two of the three turns of the day have no wall time: the sum is a
    // lower bound, marked with `~`.
    data.rows[0].wall_timed = 1;
    let chat = chat_with(CostView::with_data(data, Vec::new(), Vec::new()));
    let shown = text(&draw(&chat, 100, 9));
    assert!(shown.contains("1.2M") && shown.contains('~'), "{shown}");
    insta::assert_snapshot!(shown);
}

/// The rows of the chart: the legend, the six rows under it and the notes.
fn chart(width: u16) -> Vec<String> {
    chart_in(&dashboard(width, 70))
}

fn chart_in(shown: &str) -> Vec<String> {
    let rows: Vec<&str> = shown.lines().collect();
    let at = rows
        .iter()
        .position(|r| r.contains("in above"))
        .unwrap_or_else(|| panic!("no legend in\n{shown}"));
    rows[at..=at + 7].iter().map(|r| (*r).to_owned()).collect()
}

/// The glyph of the chart at `column`, `row` rows under the legend.
fn cell(chart: &[String], row: usize, column: usize) -> char {
    chart[row].chars().nth(column).unwrap_or(' ')
}

#[test]
fn the_chart_draws_input_above_the_axis_and_output_below() {
    let rows = chart(100);
    // Seven days, eight columns each (a bar of seven and a gap): today is the
    // last, 2 days ago the 5th.
    // Today's input (700) is the peak: a full column of 3 rows.
    for row in 1..=3 {
        assert_eq!(cell(&rows, row, 51), '█', "{rows:?}");
    }
    // Its output (200 of 700) is 2 of 6 half-rows: one full cell, below.
    assert_eq!(cell(&rows, 4, 51), '█', "{rows:?}");
    assert_eq!(cell(&rows, 5, 51), ' ', "{rows:?}");
    // Two days ago: input 410 of 700 is 15 of 24 steps: two rows and a part.
    assert_eq!(cell(&rows, 3, 35), '█');
    assert_eq!(cell(&rows, 2, 35), '▇');
    assert_eq!(cell(&rows, 1, 35), ' ');
    // Its output (100) is 1 half-row: a half block, nearest the axis.
    assert_eq!(cell(&rows, 4, 35), '▀');
    // A day with no row is blank in every row, never a known zero.
    for row in 1..=6 {
        assert_eq!(cell(&rows, row, 43), ' ', "{rows:?}");
    }
}

#[test]
fn the_chart_says_what_the_bars_are_without_colour() {
    let legend = &chart(100)[0];
    assert!(legend.starts_with("Tokens by day"), "{legend}");
    assert!(legend.contains("in above") && legend.contains("out below"));
    assert!(legend.ends_with("1 bar = 1 day"), "{legend}");
}

#[test]
fn a_long_range_states_the_bucket_size_in_the_legend() {
    let rows: Vec<UsageRow> = (0..90)
        .map(|d| UsageRow {
            days_ago: d,
            engine: "claude".into(),
            input: Some(10),
            output: Some(5),
            ..UsageRow::default()
        })
        .collect();
    let data = UsageData {
        rows,
        tasks: Vec::new(),
        ..UsageData::default()
    };
    let mut chat = chat_with(CostView::with_data(data, Vec::new(), lines()));
    chat.cost_mut()
        .unwrap()
        .on_key(&KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE));
    // 90 days in 50 columns: buckets of 2 days.
    let shown = text(&draw(&chat, 50, 40));
    assert!(shown.contains("1 bar = 2 days"), "{shown}");
    // 90 days in 100 columns: one bar per day.
    let wide = text(&draw(&chat, 100, 40));
    assert!(wide.contains("1 bar = 1 day"), "{wide}");
}

#[test]
fn a_known_zero_day_is_marked_and_an_empty_range_has_no_chart() {
    let data = UsageData {
        rows: vec![UsageRow {
            days_ago: 0,
            engine: "claude".into(),
            input: Some(0),
            output: Some(0),
            ..UsageRow::default()
        }],
        tasks: Vec::new(),
        ..UsageData::default()
    };
    let chat = chat_with(CostView::with_data(data, Vec::new(), lines()));
    let shown = text(&draw(&chat, 100, 40));
    // Today is the last of seven slots of eight columns, in the row above the axis.
    let rows = chart_in(&shown);
    assert_eq!(cell(&rows, 3, 51), '·', "{shown}");
    assert_eq!(cell(&rows, 3, 43), ' ', "{shown}");
    let empty = chat_with(CostView::with_data(
        UsageData::default(),
        Vec::new(),
        lines(),
    ));
    assert!(!text(&draw(&empty, 100, 40)).contains("in above"));
}

#[test]
fn the_chart_never_clips_its_legend_across_the_tier_boundaries() {
    for width in [40, 59, 60, 79, 80, 100] {
        let shown = dashboard(width, 60);
        let legend = shown
            .lines()
            .find(|l| l.contains(" out ") || l.contains("out below"))
            .unwrap_or_else(|| panic!("no legend at {width}\n{shown}"));
        assert!(
            legend.contains("1 bar") || legend.contains("d/bar"),
            "{legend}"
        );
    }
}

#[test]
fn snapshot_the_chart_buckets_90_days_at_the_smallest_dashboard_width() {
    let rows: Vec<UsageRow> = (0..90)
        .map(|d| UsageRow {
            days_ago: d,
            engine: "claude".into(),
            input: Some(100 + u64::from(d) * 10),
            output: Some(1_000 - u64::from(d) * 10),
            ..UsageRow::default()
        })
        .collect();
    let data = UsageData {
        rows,
        tasks: Vec::new(),
        ..UsageData::default()
    };
    let mut chat = chat_with(CostView::with_data(data, Vec::new(), Vec::new()));
    press(&mut chat, KeyCode::Char('3'));
    let shown = text(&draw(&chat, 40, 70));
    let rows: Vec<&str> = shown.lines().collect();
    let at = rows
        .iter()
        .position(|r| r.contains("in above") || r.contains("d/bar"));
    let at = at.unwrap_or_else(|| panic!("no legend in\n{shown}"));
    insta::assert_snapshot!(rows[at..=at + 7].join("\n"));
}

fn days_from_today(count: u32, set: impl Fn(&mut UsageRow)) -> UsageData {
    let rows = (0..count)
        .map(|d| {
            let mut r = UsageRow {
                days_ago: d,
                engine: "claude".into(),
                ..UsageRow::default()
            };
            set(&mut r);
            r
        })
        .collect();
    UsageData {
        rows,
        tasks: Vec::new(),
        ..UsageData::default()
    }
}

#[test]
fn a_day_with_only_its_input_known_and_zero_is_marked() {
    let data = days_from_today(1, |r| r.input = Some(0));
    let chat = chat_with(CostView::with_data(data, Vec::new(), lines()));
    let rows = chart_in(&text(&draw(&chat, 100, 40)));
    assert_eq!(cell(&rows, 3, 51), '·', "{rows:?}");
}

#[test]
fn a_partial_oldest_bar_is_said_under_the_chart() {
    let data = days_from_today(100, |r| r.input = Some(5));
    let mut chat = chat_with(CostView::with_data(data, Vec::new(), lines()));
    press(&mut chat, KeyCode::Char('4'));
    let rows = chart_in(&text(&draw(&chat, 40, 60)));
    assert!(rows[0].contains("1 bar = 3 days"), "{rows:?}");
    assert!(rows[7].contains("oldest bar 1 day"), "{rows:?}");
    // Nothing to say: the notes row is blank.
    let whole = chart_in(&dashboard(100, 40));
    assert_eq!(whole[7], "", "{whole:?}");
}

#[test]
fn a_series_cut_at_400_days_says_so_under_the_chart() {
    let mut data = days_from_today(2, |r| r.input = Some(5));
    data.rows[1].days_ago = 450;
    let mut chat = chat_with(CostView::with_data(data, Vec::new(), lines()));
    press(&mut chat, KeyCode::Char('4'));
    for width in [40, 59, 60, 100] {
        let rows = chart_in(&text(&draw(&chat, width, 60)));
        assert!(rows[7].contains("last 400 days"), "{width}: {rows:?}");
    }
}

/// The rows of the heatmap: the legend, the hour axis and the seven weekdays.
fn heat_in(shown: &str) -> Vec<String> {
    let rows: Vec<&str> = shown.lines().collect();
    let at = rows
        .iter()
        .position(|r| r.contains("low to high") || r.contains("no hourly usage"))
        .unwrap_or_else(|| panic!("no heat legend in\n{shown}"));
    rows[at..=at + 8].iter().map(|r| (*r).to_owned()).collect()
}

fn heat(width: u16) -> Vec<String> {
    heat_in(&dashboard(width, 80))
}

/// One busy hour: Tuesday 14:00 and nothing else.
fn one_hour() -> UsageData {
    UsageData {
        rows: vec![UsageRow {
            days_ago: 0,
            engine: "claude".into(),
            input: Some(5),
            ..UsageRow::default()
        }],
        hours: vec![hour(0, 1, 14, 5)],
        ..UsageData::default()
    }
}

fn heat_of(data: UsageData, width: u16) -> Vec<String> {
    let chat = chat_with(CostView::with_data(data, Vec::new(), lines()));
    heat_in(&text(&draw(&chat, width, 80)))
}

#[test]
fn snapshot_the_heatmap_in_a_wide_pane() {
    insta::assert_snapshot!(heat(100).join("\n"));
}

#[test]
fn snapshot_the_heatmap_in_a_narrow_pane() {
    insta::assert_snapshot!(heat(50).join("\n"));
}

#[test]
fn the_heatmap_starts_on_monday_and_names_every_weekday() {
    let rows = heat(100);
    let names: Vec<&str> = rows[2..].iter().map(|r| &r[..3]).collect();
    assert_eq!(names, ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]);
}

#[test]
fn the_peak_hour_is_the_darkest_glyph_and_empty_cells_are_blank() {
    let rows = heat(100);
    // Tuesday is row 3 under the legend and the axis; hour 14 starts at 4 + 28.
    assert_eq!(cell(&rows, 3, 32), '█', "{rows:?}");
    assert_eq!(cell(&rows, 3, 33), '█', "{rows:?}");
    assert_eq!(cell(&rows, 3, 34), '▒', "{rows:?}");
    // Monday has nothing this week; its row is the label and no glyph.
    assert_eq!(rows[2].trim_end(), "Mon", "{rows:?}");
}

#[test]
fn intensity_is_told_apart_by_glyph_alone() {
    let rows = heat(100);
    // Thursday 09:00 is 510 of 900 and 10:00 is 100 of 900.
    assert_eq!(cell(&rows, 5, 4 + 9 * 2), '▓', "{rows:?}");
    assert_eq!(cell(&rows, 5, 4 + 10 * 2), '░', "{rows:?}");
}

#[test]
fn a_single_busy_hour_is_one_cell_at_every_width() {
    for width in [40, 59, 60, 79, 80, 100] {
        let rows = heat_of(one_hour(), width);
        let shades: usize = rows[2..]
            .iter()
            .map(|r| r.chars().filter(|c| "░▒▓█".contains(*c)).count())
            .sum();
        // A cell is 2 columns of 24 hours, or 3 of 8 buckets.
        assert_eq!(shades, if width < 60 { 3 } else { 2 }, "{width}: {rows:?}");
        assert!(rows[3].contains('█'), "{width}: {rows:?}");
    }
}

#[test]
fn under_60_columns_the_hours_are_grouped_by_three() {
    // 14:00 is the bucket 12-14 (5th, index 4): 4 + 4 * 3.
    for width in [40, 59] {
        let rows = heat_of(one_hour(), width);
        assert_eq!(cell(&rows, 3, 16), '█', "{width}: {rows:?}");
        assert_eq!(rows[1].matches("00").count(), 1, "{rows:?}");
        assert!(rows[1].contains("21"), "{width}: {rows:?}");
    }
    for width in [60, 80] {
        let rows = heat_of(one_hour(), width);
        assert_eq!(cell(&rows, 3, 32), '█', "{width}: {rows:?}");
        assert!(rows[1].contains("18"), "{width}: {rows:?}");
    }
}

#[test]
fn the_heat_legend_is_never_clipped_across_the_tier_boundaries() {
    for width in [40, 59, 60, 79, 80, 100] {
        let rows = heat(width);
        assert!(
            rows[0].ends_with("░ ▒ ▓ █ low to high"),
            "{width}: {rows:?}"
        );
        assert!(rows[0].chars().count() <= usize::from(width), "{rows:?}");
    }
}

#[test]
fn under_40_columns_the_text_report_has_no_heatmap() {
    let shown = dashboard(39, 40);
    assert!(!shown.contains("low to high"), "{shown}");
}

#[test]
fn a_range_with_usage_and_no_hours_says_so_instead_of_an_empty_grid() {
    let mut data = one_hour();
    data.hours.clear();
    let rows = heat_of(data, 100);
    assert!(
        rows[0].contains("no hourly usage in this range"),
        "{rows:?}"
    );
    // An empty range has no dashboard blocks at all.
    let chat = chat_with(CostView::with_data(
        UsageData::default(),
        Vec::new(),
        lines(),
    ));
    assert!(!text(&draw(&chat, 100, 40)).contains("Tue"));
}

#[test]
fn a_known_zero_hour_is_marked_and_never_blank() {
    let mut data = one_hour();
    data.hours = vec![hour(0, 1, 14, 0)];
    let rows = heat_of(data, 100);
    assert_eq!(cell(&rows, 3, 32), '·', "{rows:?}");
}

#[test]
fn a_short_pane_shows_every_weekday_while_it_scrolls() {
    for height in [4, 6, 10] {
        let mut chat = chat_with(CostView::with_data(usage(), Vec::new(), lines()));
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..120 {
            let shown = text(&draw(&chat, 100, height));
            for day in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] {
                if shown.lines().any(|l| l.starts_with(day)) {
                    seen.insert(day);
                }
            }
            press(&mut chat, KeyCode::Down);
        }
        assert_eq!(seen.len(), 7, "{height}: {seen:?}");
    }
}

#[test]
fn narrow_cells_are_shaded_against_the_busiest_bucket_not_the_busiest_hour() {
    let mut data = one_hour();
    // Two busy hours add up in one bucket; a lighter bucket sits beside it.
    data.hours = vec![hour(0, 1, 12, 500), hour(0, 1, 13, 500), hour(0, 2, 9, 300)];
    let rows = heat_of(data, 50);
    let all: String = rows[2..].concat();
    // Only the busiest cell is the darkest glyph (one cell, 3 columns wide).
    assert_eq!(all.matches('█').count(), 3, "{rows:?}");
    assert!(rows[3].contains('█'), "{rows:?}");
    assert!(!rows[4].contains('█') && rows[4].contains('▒'), "{rows:?}");
}

/// One row of a pair: `tokens` is the whole total, split 4 to 1.
fn pair(engine: &str, model: Option<&str>, days_ago: u32, tokens: u64) -> UsageRow {
    UsageRow {
        days_ago,
        engine: engine.into(),
        model: model.map(str::to_owned),
        turns: 2,
        records: 2,
        input: Some(tokens * 4 / 5),
        output: Some(tokens / 5),
        total: Some(tokens),
        ..UsageRow::default()
    }
}

fn billed() -> Vec<(String, crate::core::ai::chat::usage_view::Billing)> {
    use crate::core::ai::chat::usage_view::Billing;
    vec![
        ("claude".into(), Billing::Subscription),
        ("codex".into(), Billing::PerToken),
        ("gemini".into(), Billing::Subscription),
    ]
}

/// Every provider once; claude is the biggest and has a dollar figure.
fn models_usage() -> UsageData {
    let mut claude = pair("claude", Some("sonnet"), 0, 3_000);
    claude.cost_micros = Some(500_000);
    claude.costed = 2;
    let mut codex = pair("codex", Some("gpt-5"), 1, 1_200);
    codex.cost_micros = Some(4_900);
    codex.costed = 2;
    UsageData {
        rows: vec![
            claude,
            pair("ollama", Some("llama3:8b"), 0, 5_000),
            codex,
            pair("gemini", None, 2, 900),
            pair("foo", Some("mixed"), 3, 400),
        ],
        ..UsageData::default()
    }
}

fn models_view(data: UsageData) -> ChatState {
    chat_with(CostView::with_data(data, billed(), Vec::new()))
}

/// The rows from the Models title to the last one with text.
fn models_block(chat: &ChatState, width: u16) -> String {
    let shown = text(&draw(chat, width, 90));
    let rows: Vec<&str> = shown.lines().collect();
    let from = rows
        .iter()
        .position(|r| r.starts_with("Models"))
        .expect("Models title");
    let to = rows.iter().rposition(|r| !r.is_empty()).unwrap_or(from);
    // The last row is the keys line.
    rows[from..to].join("\n")
}

#[test]
fn snapshot_the_models_and_the_provider_bar_at_100_columns() {
    insta::assert_snapshot!(models_block(&models_view(models_usage()), 100));
}

#[test]
fn snapshot_the_models_and_the_provider_bar_at_72_columns() {
    insta::assert_snapshot!(models_block(&models_view(models_usage()), 72));
}

#[test]
fn snapshot_the_models_and_the_provider_bar_at_50_columns() {
    insta::assert_snapshot!(models_block(&models_view(models_usage()), 50));
}

/// The first cell whose symbol is `glyph`: its foreground.
fn glyph_fg(buf: &Buffer, glyph: char) -> Option<ratatui::style::Color> {
    let area = buf.area;
    (0..area.height)
        .flat_map(|y| (0..area.width).map(move |x| (x, y)))
        .map(|at| &buf[at])
        .find(|c| c.symbol() == glyph.to_string())
        .map(|c| c.fg)
}

#[test]
fn a_provider_glyph_is_drawn_in_its_colour_with_the_label_beside_it() {
    let chat = models_view(models_usage());
    let buf = draw(&chat, 100, 90);
    let t = theme();
    for (p, color) in [
        (Provider::Local, t.ai_purple),
        (Provider::OpenAi, t.neutral),
        (Provider::Anthropic, t.warning_orange),
        (Provider::Google, t.info_blue),
        (Provider::Other, t.text_muted),
    ] {
        assert_eq!(glyph_fg(&buf, p.glyph()), Some(color), "{p:?}");
    }
    let shown = text(&buf);
    for p in Provider::ALL {
        let row = shown
            .lines()
            .find(|l| l.starts_with(p.glyph()))
            .unwrap_or_else(|| panic!("{p:?} row\n{shown}"));
        // A local or other row names its engine, the rest their provider.
        let name = match p {
            Provider::Local => "ollama",
            Provider::Other => "foo",
            _ => p.label(),
        };
        assert!(row.contains(name), "{row}");
    }
}

#[test]
fn the_bar_fills_the_width_and_every_segment_has_a_glyph_and_a_label() {
    let chat = models_view(models_usage());
    for width in [40u16, 59, 60, 79, 80, 100] {
        let shown = text(&draw(&chat, width, 90));
        let bar = shown
            .lines()
            .find(|l| {
                l.chars()
                    .all(|c| Provider::ALL.iter().any(|p| p.glyph() == c))
                    && !l.is_empty()
            })
            .unwrap_or_else(|| panic!("no bar at {width}\n{shown}"));
        assert_eq!(bar.chars().count(), usize::from(width), "{bar}");
        for p in Provider::ALL {
            assert!(bar.contains(p.glyph()), "{width} {p:?} {bar}");
            assert!(
                shown.contains(&format!("{} {} ", p.glyph(), p.label())),
                "{shown}"
            );
        }
    }
}

#[test]
fn the_tiers_add_the_dollars_at_60_and_the_turns_at_80() {
    let chat = models_view(models_usage());
    let head = |w| {
        let block = models_block(&chat, w);
        block.lines().nth(1).unwrap().to_owned()
    };
    for (width, turns, usd) in [
        (40, false, false),
        (59, false, false),
        (60, false, true),
        (79, false, true),
        (80, true, true),
    ] {
        let h = head(width);
        assert_eq!(
            (h.contains("turns"), h.contains('$')),
            (turns, usd),
            "{width}: {h}"
        );
        assert!(h.starts_with("provider"), "{h}");
    }
}

#[test]
fn the_models_follow_the_range_keys() {
    let mut data = models_usage();
    data.rows.push(pair("ollama", Some("old-model"), 20, 100));
    let mut chat = models_view(data);
    assert!(!models_block(&chat, 100).contains("old-model"));
    press(&mut chat, KeyCode::Char('2'));
    assert!(models_block(&chat, 100).contains("old-model"));
    press(&mut chat, KeyCode::Char('1'));
    assert!(!models_block(&chat, 100).contains("old-model"));
}

#[test]
fn many_models_show_a_count_and_the_table_never_pushes_the_bar_away() {
    let rows = (0..12)
        .map(|n| pair("ollama", Some(&format!("m{n:02}")), 0, 100 + n))
        .collect();
    let chat = models_view(UsageData {
        rows,
        ..UsageData::default()
    });
    let block = models_block(&chat, 100);
    assert!(block.contains("+4 more models"), "{block}");
    assert!(block.contains("Local 100%"), "{block}");
}

#[test]
fn zero_tokens_say_so_instead_of_drawing_a_bar() {
    let mut row = pair("ollama", Some("a"), 0, 0);
    row.total = None;
    let chat = models_view(UsageData {
        rows: vec![row],
        ..UsageData::default()
    });
    let block = models_block(&chat, 100);
    assert!(
        block.contains("no tokens reported in this range"),
        "{block}"
    );
    assert!(!block.contains("⬡⬡"), "{block}");
}

#[test]
fn an_empty_range_has_no_models_block() {
    let chat = models_view(UsageData::default());
    assert!(!text(&draw(&chat, 100, 30)).contains("Models"));
}

#[test]
fn long_and_wide_model_names_never_break_the_layout_or_panic() {
    let long = "m".repeat(70);
    let rows = vec![
        pair("claude", Some(&long), 0, 9_999_999),
        pair("ollama", Some("模型名称模型名称模型名称模型名称"), 0, 10),
    ];
    let mut chat = models_view(UsageData {
        rows,
        ..UsageData::default()
    });
    for width in 0..=120 {
        for height in [0, 5, 20, 90] {
            draw(&chat, width, height);
        }
        press(&mut chat, KeyCode::Char('4'));
    }
    assert!(models_block(&chat, 45).contains('…'));
}
