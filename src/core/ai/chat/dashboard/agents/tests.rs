use unicode_width::UnicodeWidthStr;

use super::*;
use crate::core::ai::chat::{dashboard::Dashboard, usage_view::UsageRow};

fn span(days_ago: u32, start: i64, end: i64) -> Span {
    Span {
        days_ago,
        start,
        end,
    }
}

fn day(days_ago: u32, engine: &str, set: impl FnOnce(&mut AgentDay)) -> AgentDay {
    let mut d = AgentDay {
        days_ago,
        engine: engine.to_owned(),
        workers: 1,
        input: Some(100),
        output: Some(50),
        wall_ms: Some(2_000),
        timed: 1,
        ..AgentDay::default()
    };
    set(&mut d);
    d
}

fn with_spans(spans: Vec<Span>) -> UsageData {
    UsageData {
        spans,
        ..UsageData::default()
    }
}

fn with_days(agents: Vec<AgentDay>) -> UsageData {
    UsageData {
        agents,
        ..UsageData::default()
    }
}

#[test]
fn the_overlap_counts_how_many_runs_overlap() {
    let data = with_spans(vec![span(0, 0, 10), span(0, 5, 15), span(0, 6, 8)]);
    assert_eq!(overlap(&data, Range::All), Some(3));
}

#[test]
fn a_run_that_ends_when_another_starts_does_not_overlap_it() {
    let data = with_spans(vec![span(0, 0, 10), span(0, 10, 20)]);
    assert_eq!(overlap(&data, Range::All), Some(1));
}

#[test]
fn a_run_of_zero_seconds_still_counts_as_one() {
    assert_eq!(
        overlap(&with_spans(vec![span(0, 7, 7)]), Range::All),
        Some(1)
    );
}

#[test]
fn no_spans_have_no_overlap_and_the_range_leaves_older_ones_out() {
    assert_eq!(overlap(&UsageData::default(), Range::All), None);
    let data = with_spans(vec![span(0, 0, 10), span(9, 0, 10), span(9, 2, 8)]);
    assert_eq!(overlap(&data, Range::Week), Some(1));
    assert_eq!(overlap(&data, Range::Month), Some(3));
}

#[test]
fn the_agent_time_card_says_how_many_run_at_once_or_counts_the_turns() {
    let rows = vec![UsageRow {
        workers: 2,
        ..UsageRow::default()
    }];
    let mut data = UsageData {
        rows,
        ..with_spans(vec![span(0, 0, 10), span(0, 5, 15)])
    };
    let card = Dashboard::build(&data, Range::Week, &Vec::new()).kpis[3]
        .sub
        .clone();
    assert_eq!(card, "up to 2 at once");
    data.spans.clear();
    let card = Dashboard::build(&data, Range::Week, &Vec::new()).kpis[3]
        .sub
        .clone();
    assert_eq!(card, "2 agent turns");
}

#[test]
fn an_engine_row_adds_its_days_and_leaves_other_ranges_out() {
    let data = with_days(vec![
        day(0, "claude", |_| {}),
        day(3, "claude", |d| d.input = Some(10)),
        day(40, "claude", |_| {}),
        day(0, "codex", |_| {}),
    ]);
    let rows = agents(&data, Range::Week);
    let claude = rows.iter().find(|r| r.engine == "claude").unwrap();
    assert_eq!((claude.input, claude.output), (Some(110), Some(100)));
    assert_eq!(claude.wall_ms, Some(4_000));
    assert_eq!(rows.len(), 2);
}

#[test]
fn rows_are_ordered_by_tokens_then_engine() {
    let data = with_days(vec![
        day(0, "codex", |d| d.input = Some(1)),
        day(0, "claude", |_| {}),
        day(0, "ollama", |d| d.input = Some(1)),
    ]);
    let names: Vec<_> = agents(&data, Range::All)
        .into_iter()
        .map(|r| r.engine)
        .collect();
    assert_eq!(names, ["claude", "codex", "ollama"]);
}

fn shown(data: &UsageData, width: usize) -> String {
    let t = agent_table(&agents(data, Range::All), width);
    std::iter::once(t.head)
        .chain(t.rows.into_iter().map(|(_, l)| l))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_columns_are_input_output_and_time_with_no_dollars_or_turns() {
    let text = shown(&with_days(vec![day(0, "claude", |_| {})]), 100);
    let head = text.lines().next().unwrap();
    let at = |s: &str| head.find(s).unwrap_or_else(|| panic!("{head}"));
    assert!(at("engine") < at("input") && at("input") < at("output") && at("output") < at("time"));
    assert!(!text.contains('$') && !head.contains("turns"), "{text}");
    assert!(text.contains("✻ claude") && text.contains("2s"), "{text}");
}

#[test]
fn old_rows_say_n_a_for_time_and_keep_their_tokens() {
    let old = day(0, "claude", |d| {
        d.wall_ms = None;
        d.timed = 0;
    });
    let text = shown(&with_days(vec![old]), 100);
    let row = text.lines().nth(1).unwrap();
    assert!(
        row.contains("100") && row.contains("50") && row.ends_with("n/a"),
        "{text}"
    );
}

#[test]
fn a_partly_timed_row_is_approximate() {
    let mixed = day(0, "claude", |d| {
        d.workers = 3;
        d.timed = 1;
    });
    let text = shown(&with_days(vec![mixed]), 100);
    assert!(text.lines().nth(1).unwrap().ends_with("~2s"), "{text}");
}

#[test]
fn a_figure_no_worker_reported_is_n_a_and_a_cut_one_is_approximate() {
    let rows = with_days(vec![
        day(0, "claude", |d| d.input = None),
        day(0, "codex", |d| d.partial = true),
    ]);
    let text = shown(&rows, 100);
    let line = |name: &str| text.lines().find(|l| l.contains(name)).unwrap().to_owned();
    assert!(line("claude").contains("n/a"), "{text}");
    assert!(line("codex").contains("~100"), "{text}");
}

#[test]
fn every_row_starts_with_its_provider_glyph_and_the_engine_name() {
    let data = with_days(vec![
        day(0, "ollama", |_| {}),
        day(0, "claude", |_| {}),
        day(0, "foo", |_| {}),
    ]);
    let text = shown(&data, 100);
    assert!(
        text.contains("⬡ ollama") && text.contains("✻ claude"),
        "{text}"
    );
    assert!(text.contains("⬦ foo"), "{text}");
}

#[test]
fn a_narrow_table_cuts_the_engine_and_never_overflows() {
    let data = with_days(vec![day(0, "a-very-long-engine-name", |_| {})]);
    for width in [40, 50, 100] {
        let t = agent_table(&agents(&data, Range::All), width);
        for line in std::iter::once(&t.head).chain(t.rows.iter().map(|(_, l)| l)) {
            assert!(line.width() <= width, "{width}: {line}");
        }
    }
}

#[test]
fn no_workers_show_a_line_and_no_table() {
    assert_eq!(agents_rows(&[]), 3);
    let rows = agents(&with_days(vec![day(0, "claude", |_| {})]), Range::All);
    assert_eq!(agents_rows(&rows), 2 + 1 + rows.len() as u16);
    assert_eq!(NO_AGENTS, "no agent runs in this range");
}

#[test]
fn the_dashboard_height_grows_with_the_agents_block() {
    let used = UsageRow {
        engine: "claude".to_owned(),
        input: Some(1),
        output: Some(1),
        ..UsageRow::default()
    };
    let mut data = UsageData {
        rows: vec![used],
        ..UsageData::default()
    };
    let without = Dashboard::build(&data, Range::Week, &Vec::new());
    assert_eq!(without.agents_rows(), 3);
    data.agents = vec![day(0, "claude", |_| {})];
    let with = Dashboard::build(&data, Range::Week, &Vec::new());
    assert_eq!(with.height(100), without.height(100) + 1);
}
