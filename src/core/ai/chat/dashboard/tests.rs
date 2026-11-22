use super::*;

fn row(days_ago: u32, set: impl FnOnce(&mut UsageRow)) -> UsageRow {
    let mut r = UsageRow {
        days_ago,
        engine: "claude".to_owned(),
        ..UsageRow::default()
    };
    set(&mut r);
    r
}

fn input(days_ago: u32, n: u64) -> UsageRow {
    row(days_ago, |r| r.input = Some(n))
}

fn build(rows: Vec<UsageRow>, range: Range) -> Dashboard {
    Dashboard::build(
        &UsageData {
            rows,
            tasks: Vec::new(),
        },
        range,
    )
}

#[test]
fn the_default_range_is_the_week() {
    assert_eq!(Range::default(), Range::Week);
    assert_eq!(Range::Week.days(), Some(7));
    assert_eq!(Range::Month.days(), Some(30));
    assert_eq!(Range::Quarter.days(), Some(90));
    assert_eq!(Range::All.days(), None);
}

#[test]
fn tasks_count_conversations_and_runs_once_and_turns_count_chat_rows() {
    // Chat A x5 and chat B x2 are 7 turns; one run of 3 workers is no turn.
    let data = UsageData {
        rows: vec![row(0, |r| {
            r.turns = 7;
            r.records = 10;
            r.workers = 3;
        })],
        tasks: vec![(0, "c1".into()), (0, "c2".into()), (0, "r9".into())],
    };
    let d = Dashboard::build(&data, Range::Week);
    assert_eq!(d.kpis[2].label, "Tasks");
    assert_eq!(d.kpis[2].value, "3");
    assert_eq!(d.kpis[2].sub, "7 turns");
}

#[test]
fn a_task_on_two_days_counts_once_and_one_outside_the_range_not_at_all() {
    let data = UsageData {
        rows: vec![input(0, 1)],
        tasks: vec![(0, "c1".into()), (3, "c1".into()), (8, "c2".into())],
    };
    assert_eq!(Dashboard::build(&data, Range::Week).kpis[2].value, "1");
    assert_eq!(Dashboard::build(&data, Range::Month).kpis[2].value, "2");
}

#[test]
fn rows_of_deleted_chats_add_tokens_but_no_task() {
    let d = build(vec![row(0, |r| r.input = Some(1_500))], Range::Week);
    assert_eq!(d.kpis[0].value, "1.5k");
    assert_eq!(d.kpis[2].value, "0");
}

#[test]
fn the_input_card_shows_the_cache_share_or_says_there_is_none() {
    let d = build(
        vec![row(0, |r| {
            r.input = Some(100);
            r.cache_hit = Some(63);
            r.cache_base = Some(100);
        })],
        Range::Week,
    );
    assert_eq!(d.kpis[0].label, "Input");
    assert_eq!(d.kpis[0].sub, "cache 63%");
    assert_eq!(
        build(vec![input(0, 5)], Range::Week).kpis[0].sub,
        "cache n/a"
    );
}

#[test]
fn the_output_average_divides_chat_output_by_chat_turns_not_records() {
    // 1,200 output overall, 1,000 of it from 4 chat turns; 2 more records.
    let d = build(
        vec![row(0, |r| {
            r.output = Some(1_200);
            r.chat_output = Some(1_000);
            r.turns = 4;
            r.records = 6;
        })],
        Range::Week,
    );
    assert_eq!(d.kpis[1].label, "Output");
    assert_eq!(d.kpis[1].value, "1.2k");
    assert_eq!(d.kpis[1].sub, "avg 250/turn");
    let none = build(vec![row(0, |r| r.output = Some(9))], Range::Week);
    assert_eq!(none.kpis[1].sub, "avg n/a");
}

#[test]
fn the_range_leaves_older_rows_out() {
    let rows = vec![input(0, 100), input(6, 100), input(7, 900)];
    assert_eq!(build(rows.clone(), Range::Week).kpis[0].value, "200");
    assert_eq!(build(rows, Range::Month).kpis[0].value, "1.1k");
}

#[test]
fn a_week_and_a_month_compare_with_the_previous_period() {
    let up = build(vec![input(0, 200), input(7, 100)], Range::Week);
    assert_eq!(up.kpis[0].delta, Delta::Up(100));
    assert_eq!(up.kpis[0].delta.text(), "▲ 100%");
    let down = build(vec![input(1, 100), input(9, 200)], Range::Week);
    assert_eq!(down.kpis[0].delta.text(), "▼ 50%");
    let flat = build(vec![input(1, 100), input(9, 100)], Range::Week);
    assert_eq!(flat.kpis[0].delta.text(), "= 0%");
    let month = build(vec![input(0, 150), input(40, 100)], Range::Month);
    assert_eq!(month.kpis[0].delta.text(), "▲ 50%");
}

#[test]
fn a_delta_is_not_available_for_long_ranges_or_an_empty_previous_period() {
    let rows = vec![input(0, 200), input(7, 100), input(100, 100)];
    for range in [Range::Quarter, Range::All] {
        assert_eq!(build(rows.clone(), range).kpis[0].delta.text(), "n/a");
    }
    let none = build(vec![input(0, 200)], Range::Week);
    assert_eq!(none.kpis[0].delta.text(), "n/a");
    let zero = build(vec![input(0, 200), input(8, 0)], Range::Week);
    assert_eq!(zero.kpis[0].delta, Delta::NotAvailable);
}

#[test]
fn agent_time_is_n_a_for_old_rows_and_approximate_when_mixed() {
    let old = build(vec![row(0, |r| r.workers = 3)], Range::Week);
    assert_eq!(old.kpis[3].label, "Agent time");
    assert_eq!(old.kpis[3].value, "n/a");
    let mixed = build(
        vec![row(0, |r| {
            r.workers = 3;
            r.worker_timed = 2;
            r.worker_wall_ms = Some(60_000);
        })],
        Range::Week,
    );
    assert_eq!(mixed.kpis[3].value, "~1m");
    let full = build(
        vec![row(0, |r| {
            r.workers = 2;
            r.worker_timed = 2;
            r.worker_wall_ms = Some(125_000);
        })],
        Range::Week,
    );
    assert_eq!(full.kpis[3].value, "2m");
}

#[test]
fn the_agent_time_delta_needs_wall_time_in_the_previous_period() {
    let timed = |days_ago, wall, workers, timed| {
        row(days_ago, |r| {
            r.workers = workers;
            r.worker_timed = timed;
            r.worker_wall_ms = Some(wall);
        })
    };
    let ok = build(vec![timed(0, 200, 1, 1), timed(8, 100, 1, 1)], Range::Week);
    assert_eq!(ok.kpis[3].delta.text(), "▲ 100%");
    let gap = build(vec![timed(0, 200, 1, 1), timed(8, 100, 2, 1)], Range::Week);
    assert_eq!(gap.kpis[3].delta.text(), "n/a");
}

#[test]
fn a_range_without_usage_is_empty() {
    assert!(build(Vec::new(), Range::Week).empty);
    assert!(build(vec![input(10, 1)], Range::Week).empty);
    assert!(!build(vec![input(10, 1)], Range::Month).empty);
}

#[test]
fn every_card_carries_its_metric_by_day_oldest_first() {
    let d = build(vec![input(0, 30), input(2, 10)], Range::Week);
    assert_eq!(d.kpis[0].spark, [0, 0, 0, 0, 10, 0, 30]);
    assert_eq!(d.kpis[0].spark.len(), 7);
}

#[test]
fn bucket_sums_adjacent_days_to_fit_the_columns() {
    let v = [1, 2, 3, 4, 5];
    assert_eq!(bucket(&v, 5), v);
    assert_eq!(bucket(&v, 9), v);
    assert_eq!(bucket(&v, 3), [3, 7, 5]);
    assert_eq!(bucket(&v, 1), [15]);
    assert!(bucket(&v, 0).is_empty());
    assert!(bucket(&[], 4).is_empty());
}
