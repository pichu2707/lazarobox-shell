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
    // No cache data: the sub-line is left out, not `n/a`.
    assert_eq!(build(vec![input(0, 5)], Range::Week).kpis[0].sub, "");
}

#[test]
fn the_cache_share_is_the_text_reports_not_read_over_input_plus_read() {
    // 100 fresh input, 50 read, 50 written (claude): the report's share is
    // 50 / 200 = 25%; read / (input + read) would say 33%.
    let d = build(
        vec![row(0, |r| {
            r.input = Some(100);
            r.cache_read = Some(50);
            r.cache_hit = Some(50);
            r.cache_base = Some(200);
        })],
        Range::Week,
    );
    assert_eq!(d.kpis[0].sub, "cache 25%");
}

#[test]
fn the_output_average_divides_chat_output_by_chat_turns_not_records() {
    // 1,200 output overall, 1,000 of it from 4 chat turns; 2 more records.
    let d = build(
        vec![row(0, |r| {
            r.output = Some(1_200);
            r.chat_output = Some(1_000);
            r.chat_outputs = 4;
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
fn the_output_average_leaves_out_chat_turns_that_reported_no_output() {
    // 4 chat turns, 2 of them with an output: 1,000 / 2, not 1,000 / 4.
    let d = build(
        vec![row(0, |r| {
            r.output = Some(1_000);
            r.chat_output = Some(1_000);
            r.chat_outputs = 2;
            r.turns = 4;
        })],
        Range::Week,
    );
    assert_eq!(d.kpis[1].sub, "avg 500/turn");
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

/// A row of `turns` chat and `workers` worker records, `timed` with wall time.
fn timed(days_ago: u32, (turns, workers, timed): (u64, u64, u64), wall: u64) -> UsageRow {
    row(days_ago, |r| {
        r.turns = turns;
        r.workers = workers;
        r.wall_timed = timed;
        r.wall_ms = Some(wall);
    })
}

#[test]
fn agent_time_is_n_a_for_old_rows_and_approximate_when_mixed() {
    let old = build(vec![row(0, |r| r.workers = 3)], Range::Week);
    assert_eq!(old.kpis[3].label, "Agent time");
    assert_eq!(old.kpis[3].value, "n/a");
    let mixed = build(vec![timed(0, (1, 2, 2), 60_000)], Range::Week);
    assert_eq!(mixed.kpis[3].value, "~1m");
    let full = build(vec![timed(0, (1, 1, 2), 125_000)], Range::Week);
    assert_eq!(full.kpis[3].value, "2m");
    // A chat turn counts as agent time too: parallel agents add up.
    let chats = build(vec![timed(0, (3, 0, 3), 90_000)], Range::Week);
    assert_eq!(chats.kpis[3].value, "1m");
    assert_eq!(chats.kpis[3].sub, "3 agent turns");
}

#[test]
fn the_agent_time_delta_needs_both_periods_complete() {
    let ok = build(
        vec![timed(0, (1, 1, 2), 200), timed(8, (1, 1, 2), 100)],
        Range::Week,
    );
    assert_eq!(ok.kpis[3].delta.text(), "▲ 100%");
    let prev_gap = build(
        vec![timed(0, (1, 1, 2), 200), timed(8, (1, 2, 2), 100)],
        Range::Week,
    );
    assert_eq!(prev_gap.kpis[3].delta.text(), "n/a");
    let now_gap = build(
        vec![timed(0, (1, 2, 2), 200), timed(8, (1, 1, 2), 100)],
        Range::Week,
    );
    assert_eq!(now_gap.kpis[3].delta.text(), "n/a");
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
    // Counted back from today: the newest bucket is whole.
    assert_eq!(bucket(&v, 3), [1, 5, 9]);
    assert_eq!(bucket(&v, 1), [15]);
    assert!(bucket(&v, 0).is_empty());
    assert!(bucket(&[], 4).is_empty());
}

#[test]
fn a_delta_rounds_half_up_on_the_previous_period() {
    // 12.5% either way reads 13%.
    let up = build(vec![input(0, 9), input(7, 8)], Range::Week);
    assert_eq!(up.kpis[0].delta, Delta::Up(13));
    let down = build(vec![input(0, 7), input(7, 8)], Range::Week);
    assert_eq!(down.kpis[0].delta, Delta::Down(13));
}

#[test]
fn a_change_that_rounds_to_zero_is_flat() {
    let d = build(vec![input(0, 1_001), input(7, 1_000)], Range::Week);
    assert_eq!(d.kpis[0].delta, Delta::Flat);
}

#[test]
fn the_periods_meet_at_the_window_edges() {
    // Week: days 0..7 now, 7..14 before, 14 and older in neither.
    let rows = vec![input(6, 1), input(7, 10), input(13, 100), input(14, 1_000)];
    let d = build(rows, Range::Week);
    assert_eq!(d.kpis[0].value, "1");
    assert_eq!(d.kpis[0].delta, Delta::Down(99));
    let month = build(
        vec![input(29, 1), input(30, 2), input(59, 4), input(60, 8)],
        Range::Month,
    );
    assert_eq!(month.kpis[0].value, "1");
    assert_eq!(month.kpis[0].delta, Delta::Down(83));
}

#[test]
fn output_and_tasks_have_their_own_deltas() {
    let data = UsageData {
        rows: vec![
            row(0, |r| r.output = Some(300)),
            row(8, |r| r.output = Some(200)),
        ],
        tasks: vec![(0, "c1".into()), (1, "c2".into()), (9, "c3".into())],
    };
    let d = Dashboard::build(&data, Range::Week);
    assert_eq!(d.kpis[1].delta, Delta::Up(50));
    assert_eq!(d.kpis[2].delta, Delta::Up(100));
}

#[test]
fn sums_saturate_instead_of_wrapping() {
    let big = |days_ago| {
        row(days_ago, |r| {
            r.input = Some(u64::MAX);
            r.turns = u64::MAX;
            r.workers = u64::MAX;
            r.chat_outputs = u64::MAX;
            r.chat_output = Some(10);
            r.wall_timed = u64::MAX;
            r.wall_ms = Some(1);
        })
    };
    let d = build(vec![big(0), big(1)], Range::Week);
    assert_eq!(d.kpis[0].value, "18446744.1T");
    assert_eq!(d.kpis[2].sub, "18,446,744,073,709,551,615 turns");
    assert_eq!(d.kpis[3].sub, "18,446,744,073,709,551,615 agent turns");
    // The average divides by a saturated count without overflowing.
    assert_eq!(d.kpis[1].sub, "avg 0/turn");
    // Saturated counts still agree, so the time is not marked approximate.
    assert_eq!(d.kpis[3].value, "0s");
    // A previous period that dwarfs the current one stays a valid percentage.
    let d = build(vec![input(0, u64::MAX), input(7, 1)], Range::Week);
    assert_eq!(d.kpis[0].delta, Delta::Up(u64::MAX));
}

#[test]
fn the_all_sparkline_covers_every_day_the_cards_count() {
    // Retention works on seconds and `days_ago` on local dates, so a row can
    // sit at day 90 or 91: the sparkline must still hold it.
    let d = build(vec![input(0, 3), input(90, 5), input(91, 7)], Range::All);
    assert_eq!(d.kpis[0].value, "15");
    assert_eq!(d.kpis[0].spark.len(), 92);
    assert_eq!(d.kpis[0].spark.iter().sum::<u64>(), 15);
    assert_eq!(d.kpis[0].spark[0], 7);
    assert_eq!(d.kpis[0].spark[91], 3);
}

#[test]
fn the_tier_follows_the_width_boundaries() {
    let tiers = [80, 79, 60, 59, 40, 39, 0].map(Tier::of);
    use Tier::*;
    assert_eq!(tiers, [Row, Grid, Grid, Column, Column, Text, Text]);
    assert_eq!(Tier::of(u16::MAX), Row);
}

#[test]
fn the_ranges_cycle_both_ways_and_have_a_label() {
    assert_eq!(Range::ALL.map(Range::label), ["7d", "30d", "90d", "All"]);
    assert_eq!(Range::Week.next(), Range::Month);
    assert_eq!(Range::All.next(), Range::Week);
    assert_eq!(Range::Week.prev(), Range::All);
    assert_eq!(Range::Quarter.prev(), Range::Month);
}

#[test]
fn the_height_is_the_selector_plus_the_card_rows_of_the_tier() {
    let full = build(vec![input(0, 1)], Range::Week);
    // Selector and gap, then 4-row cards: one row, 2x2 with a gap, a column.
    assert_eq!(full.height(100), 2 + 4 + CHART_ROWS);
    assert_eq!(full.height(70), 2 + 9 + CHART_ROWS);
    assert_eq!(full.height(50), 2 + 19 + CHART_ROWS);
    assert_eq!(full.height(39), 0);
    // The empty state is the selector, a gap and one line.
    let empty = build(Vec::new(), Range::Week);
    assert_eq!(empty.height(100), 3);
}

#[test]
fn a_capped_delta_keeps_every_digit_it_shows() {
    assert_eq!(Delta::Up(999).capped(), "▲ 999%");
    assert_eq!(Delta::Up(1000).capped(), "▲ 999%+");
    assert_eq!(Delta::Up(12_000).capped(), "▲ 999%+");
    assert_eq!(Delta::Down(100).capped(), "▼ 100%");
    assert_eq!(Delta::NotAvailable.capped(), "n/a");
}

fn both(days_ago: u32, input: u64, output: u64) -> UsageRow {
    row(days_ago, |r| {
        r.input = Some(input);
        r.output = Some(output);
    })
}

fn day(input: u64, output: u64) -> DayTokens {
    DayTokens {
        input: Some(input),
        output: Some(output),
    }
}

#[test]
fn the_chart_has_one_day_per_column_oldest_first() {
    // Days 400/100 in and 100/200 out, and a third day with no row.
    let d = build(vec![both(0, 100, 200), both(1, 400, 100)], Range::Week);
    assert_eq!(d.daily.len(), 7);
    assert_eq!(d.daily[6], day(100, 200));
    assert_eq!(d.daily[5], day(400, 100));
    assert_eq!(d.daily[4], DayTokens::default());
}

#[test]
fn a_day_with_no_row_is_empty_and_a_zero_row_is_a_known_zero() {
    let d = build(vec![both(0, 0, 0)], Range::Week);
    assert_eq!(d.daily[6], day(0, 0));
    assert_eq!(d.daily[5].input, None);
    assert_eq!(d.daily[5].output, None);
}

#[test]
fn rows_of_a_day_add_up_and_a_missing_figure_stays_missing() {
    let d = build(
        vec![both(0, 10, 5), row(0, |r| r.input = Some(7))],
        Range::Week,
    );
    assert_eq!(d.daily[6], day(17, 5));
    let only_in = build(vec![input(0, 3)], Range::Week);
    assert_eq!(only_in.daily[6].output, None);
}

#[test]
fn the_chart_totals_match_the_input_card_sparkline_for_every_range() {
    let rows = vec![
        both(0, 5, 1),
        both(3, 7, 2),
        both(40, 11, 3),
        both(120, 13, 4),
    ];
    for range in Range::ALL {
        let d = build(rows.clone(), range);
        let chart: u64 = d.daily.iter().filter_map(|x| x.input).sum();
        assert_eq!(chart, d.kpis[0].spark.iter().sum::<u64>(), "{range:?}");
        let out: u64 = d.daily.iter().filter_map(|x| x.output).sum();
        assert_eq!(out, d.kpis[1].spark.iter().sum::<u64>(), "{range:?}");
    }
}

#[test]
fn all_covers_up_to_the_oldest_row_and_an_empty_range_has_no_days() {
    let d = build(vec![both(0, 1, 1), both(20, 1, 1)], Range::All);
    assert_eq!(d.daily.len(), 21);
    assert!(build(Vec::new(), Range::All).daily.is_empty());
}

#[test]
fn a_tiny_day_keeps_one_step_next_to_a_huge_one() {
    assert_eq!(level(1, 100_000, 24), 1);
    assert_eq!(level(100_000, 100_000, 24), 24);
    assert_eq!(level(0, 100_000, 24), 0);
    assert_eq!(level(5, 0, 24), 0);
    assert_eq!(level(u64::MAX, u64::MAX, 24), 24);
}

#[test]
fn both_halves_share_one_scale() {
    let days = [day(400, 100), day(100, 200), DayTokens::default()];
    assert_eq!(peak(&days), 400);
    assert_eq!(peak(&[]), 0);
    assert!(level(400, 400, 24) > level(100, 400, 24));
}

#[test]
fn days_fit_a_width_by_summing_buckets_from_today() {
    let days: Vec<DayTokens> = (1..=90).map(|n| day(n, 1)).collect();
    let whole = fit(&days, 100);
    assert_eq!((whole.days.len(), whole.size), (90, 1));
    let fitted = fit(&days, 45);
    assert_eq!((fitted.days.len(), fitted.size), (45, 2));
    let sum = |d: &[DayTokens]| d.iter().filter_map(|x| x.input).sum::<u64>();
    assert_eq!(sum(&fitted.days), sum(&days));
    let odd = fit(&days, 40);
    assert_eq!(odd.size, 3);
    assert_eq!(sum(&odd.days), sum(&days));
    // The newest bucket is complete; the oldest takes the remainder.
    assert_eq!(odd.days.last().unwrap().input, Some(88 + 89 + 90));
}

#[test]
fn a_bucket_of_empty_days_stays_empty() {
    let days = [DayTokens::default(), DayTokens::default(), day(1, 1)];
    let fitted = fit(&days, 1);
    assert_eq!(fitted.days, vec![day(1, 1)]);
    let none = fit(&[DayTokens::default(); 4], 2);
    assert_eq!(none.days, vec![DayTokens::default(); 2]);
    assert_eq!(fit(&days, 0).days.len(), 0);
}

#[test]
fn bucket_sums_saturate() {
    let days = [day(u64::MAX, 1), day(5, 1)];
    assert_eq!(fit(&days, 1).days[0].input, Some(u64::MAX));
}

#[test]
fn the_legend_names_in_out_and_the_bucket_size_and_always_fits() {
    let full = legend(1, 100);
    assert!(full.contains("in") && full.contains("out"), "{full}");
    assert!(full.contains("1 bar = 1 day"), "{full}");
    assert!(legend(3, 100).contains("1 bar = 3 days"));
    for size in [1, 2, 3, 4, 45, 400] {
        for width in 40..120 {
            let text = legend(size, width);
            assert!(text.chars().count() <= width, "{size} {width}: {text}");
            assert!(text.contains("in") && text.contains("out"), "{text}");
            assert!(text.contains(&size.to_string()), "{size} {width}: {text}");
        }
    }
}

#[test]
fn the_cards_and_the_chart_group_the_same_days() {
    let values: Vec<u64> = (1..=10).collect();
    let days: Vec<DayTokens> = values.iter().map(|v| day(*v, 0)).collect();
    for cols in 1..=12 {
        let chart: Vec<u64> = fit(&days, cols)
            .days
            .iter()
            .map(|d| d.input.unwrap_or(0))
            .collect();
        assert_eq!(bucket(&values, cols), chart, "{cols}");
    }
    // Counted back from today: the newest bucket is whole.
    assert_eq!(bucket(&values, 4), vec![1, 9, 18, 27]);
}

#[test]
fn a_partial_oldest_bucket_is_reported() {
    let days: Vec<DayTokens> = (1..=10).map(|n| day(n, 0)).collect();
    let partial = fit(&days, 4);
    assert_eq!((partial.size, partial.oldest), (3, 1));
    let whole = fit(&days, 5);
    assert_eq!((whole.size, whole.oldest), (2, 2));
    assert_eq!(fit(&days, 20).oldest, 1);
    let note = notes(&partial, None).unwrap();
    assert!(note.contains("oldest bar 1 day"), "{note}");
    assert!(notes(&whole, None).is_none());
}

#[test]
fn a_series_cut_short_says_how_many_days_it_shows() {
    let days = [day(1, 1); 4];
    let note = notes(&fit(&days, 4), Some(400)).unwrap();
    assert!(note.contains("last 400 days"), "{note}");
    let both = notes(&fit(&[day(1, 1); 10], 4), Some(400)).unwrap();
    assert!(both.contains("oldest bar 1 day") && both.contains("last 400 days"));
    for cols in 40..=120 {
        let big = fit(&[day(1, 1); 400], cols);
        assert!(notes(&big, Some(400)).unwrap().chars().count() <= 40);
    }
}

#[test]
fn all_with_more_than_400_days_is_flagged_and_other_ranges_never_are() {
    let old = vec![both(0, 1, 1), both(450, 2, 2)];
    assert_eq!(build(old.clone(), Range::All).last_days, Some(400));
    assert_eq!(build(old, Range::Quarter).last_days, None);
    let edge = vec![both(0, 1, 1), both(399, 2, 2)];
    assert_eq!(build(edge, Range::All).last_days, None);
    assert_eq!(build(vec![both(0, 1, 1)], Range::All).last_days, None);
}

#[test]
fn the_chart_adds_up_to_the_card_headline_when_it_shows_the_whole_range() {
    use super::super::usage_fmt::compact;
    let rows = vec![
        both(0, 1_500, 20),
        both(3, 700, 40),
        both(40, 9_000, 7),
        both(120, 5, 5),
    ];
    for range in Range::ALL {
        let d = build(rows.clone(), range);
        let input: u64 = d.daily.iter().filter_map(|x| x.input).sum();
        let output: u64 = d.daily.iter().filter_map(|x| x.output).sum();
        assert_eq!(compact(input), d.kpis[0].value, "{range:?}");
        assert_eq!(compact(output), d.kpis[1].value, "{range:?}");
    }
}
