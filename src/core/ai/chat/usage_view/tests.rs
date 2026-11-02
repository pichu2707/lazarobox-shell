use super::*;
use crate::core::ai::chat::usage::TurnUsage;
use crate::core::ai::config::UsageConfig;

fn default_billing(engine: &str) -> Option<Billing> {
    UsageConfig::default().billing(engine)
}

fn row(days_ago: u32, engine: &str, model: &str) -> UsageRow {
    UsageRow {
        days_ago,
        engine: engine.into(),
        model: Some(model.into()),
        in_chat: false,
        turns: 1,
        records: 1,
        costed: 0,
        input: Some(100),
        output: Some(20),
        cache_read: None,
        cache_hit: None,
        cache_base: None,
        cache_gap: 0,
        cost_micros: None,
        total: Some(120),
        partial: false,
        workers: 0,
        worker_total: None,
        worker_partial: false,
    }
}

/// The lines before the charts.
fn before_charts(mut lines: Vec<String>) -> Vec<String> {
    let at = lines.iter().position(|l| l.starts_with("per day"));
    lines.truncate(at.unwrap_or(lines.len()));
    lines
}

fn view(rows: &[UsageRow]) -> Vec<String> {
    report_lines(rows, &Session::default(), &default_billing)
}

/// A row of worker records only.
fn worker_row(days_ago: u32, total: Option<u64>) -> UsageRow {
    UsageRow {
        turns: 0,
        input: total.map(|_| 1),
        output: total.map(|_| 1),
        total,
        workers: 1,
        worker_total: total,
        ..row(days_ago, "ollama", "m")
    }
}

#[test]
fn dollars_have_four_decimals_below_one_and_two_from_one_up() {
    assert_eq!(format_usd(4_936), "$0.0049");
    assert_eq!(format_usd(500_000), "$0.5000");
    assert_eq!(format_usd(12_345_600), "$12.35");
    assert_eq!(format_usd(1_000_000), "$1.00");
    assert_eq!(format_usd(0), "$0.0000");
}

#[test]
fn the_billing_mode_only_labels_the_figure() {
    assert_eq!(
        dollar_part(4_936, Billing::Subscription),
        "$0.0049 list-price equivalent"
    );
    assert_eq!(dollar_part(4_936, Billing::PerToken), "$0.0049");
}

fn used(input: Option<u64>, output: Option<u64>) -> TurnUsage {
    TurnUsage {
        input,
        output,
        ..TurnUsage::default()
    }
}

// Spec (ai-usage, Usage text formats).
#[test]
fn the_usage_line_shows_in_and_out_then_what_is_present() {
    let full = TurnUsage {
        cache_read: Some(0),
        cost_micros: Some(4_936),
        ..used(Some(2), Some(4))
    };
    assert_eq!(
        usage_line(Some(&full), Some(Billing::Subscription)),
        "usage: in 2 · out 4 · cached 0 · $0.0049 list-price equivalent"
    );
    assert_eq!(
        usage_line(Some(&full), Some(Billing::PerToken)),
        "usage: in 2 · out 4 · cached 0 · $0.0049"
    );
    let reasoning = TurnUsage {
        reasoning: Some(7),
        ..used(Some(12), Some(34))
    };
    assert_eq!(
        usage_line(Some(&reasoning), None),
        "usage: in 12 · out 34 · reasoning 7"
    );
}

#[test]
fn a_missing_in_or_out_reads_no_data_and_never_zero() {
    assert_eq!(
        usage_line(Some(&used(None, Some(567))), None),
        "usage: in no data · out 567"
    );
    assert_eq!(
        usage_line(Some(&used(Some(5), None)), None),
        "usage: in 5 · out no data"
    );
}

#[test]
fn a_partial_record_is_marked_last() {
    let partial = TurnUsage {
        partial: true,
        cost_micros: Some(1_000_000),
        ..used(Some(100), Some(20))
    };
    assert_eq!(
        usage_line(Some(&partial), None),
        "usage: in 100 · out 20 · partial"
    );
    assert_eq!(
        usage_line(Some(&partial), Some(Billing::PerToken)),
        "usage: in 100 · out 20 · $1.00 · partial"
    );
}

#[test]
fn a_local_engine_never_shows_dollars_even_if_reported() {
    let local = TurnUsage {
        cost_micros: Some(10),
        ..used(Some(1), Some(2))
    };
    assert_eq!(usage_line(Some(&local), None), "usage: in 1 · out 2");
}

#[test]
fn no_record_reads_exactly_no_data() {
    assert_eq!(
        usage_line(None, Some(Billing::Subscription)),
        "usage: no data"
    );
}

// Spec (statusline, Usage segment).
#[test]
fn the_today_segment_is_tokens_then_per_token_spend() {
    assert_eq!(today_segment(Some(1234), None), "today: 1234 tok");
    assert_eq!(
        today_segment(Some(5000), Some(420_000)),
        "today: 5000 tok · $0.42"
    );
    assert_eq!(
        today_segment(Some(5000), Some(12_345_600)),
        "today: 5000 tok · $12.35"
    );
    // A zero sum shows no figure; a sub-cent one is not rounded down to $0.00.
    assert_eq!(today_segment(Some(5000), Some(0)), "today: 5000 tok");
    assert_eq!(
        today_segment(Some(5000), Some(4_936)),
        "today: 5000 tok · <$0.01"
    );
    assert_eq!(today_segment(None, None), "today: no data");
    assert_eq!(today_segment(Some(0), None), "today: 0 tok");
}

#[test]
fn an_empty_store_says_so_under_the_alpha_header() {
    assert_eq!(view(&[]), vec![HEADER, EMPTY_LINE]);
}

#[test]
fn the_header_is_the_first_line_in_every_view() {
    assert_eq!(view(&[row(0, "claude", "s")])[0], "AI usage (alpha)");
    assert_eq!(UNAVAILABLE_LINE, "usage history is not available");
}

#[test]
fn today_and_the_window_count_all_chats_and_exclude_older_days() {
    let rows = [
        row(0, "claude", "s"),
        row(6, "claude", "s"),
        row(8, "claude", "s"),
    ];
    let lines = view(&rows);
    assert_eq!(lines[1], "today: 120 tokens · 1 turns");
    assert_eq!(lines[2], "last 7 days: 240 tokens · 2 turns");
}

#[test]
fn an_unknown_total_is_no_data_never_zero() {
    let mut only_output = row(0, "ollama", "llama3:8b");
    only_output.input = None;
    only_output.total = None;
    only_output.partial = true;
    let lines = view(&[only_output]);
    assert_eq!(lines[1], "today: no data · 1 turns · partial");
    assert_eq!(
        lines[4],
        "  ollama › llama3:8b: in no data · out 20 · partial"
    );
}

#[test]
fn pairs_are_sorted_by_total_and_dollars_follow_the_billing_mode() {
    let mut claude = row(1, "claude", "sonnet");
    claude.total = Some(3_000);
    claude.cost_micros = Some(500_000);
    claude.costed = 1;
    claude.cache_read = Some(0);
    let mut local = row(1, "ollama", "llama3:8b");
    local.total = Some(5_000);
    local.cost_micros = Some(9_000_000);
    let lines = view(&[claude, local]);
    assert_eq!(lines[3], "by engine and model (last 7 days):");
    assert_eq!(lines[4], "  ollama › llama3:8b: in 100 · out 20");
    assert_eq!(
        lines[5],
        "  claude › sonnet: in 100 · out 20 · cached 0 · $0.5000 list-price equivalent"
    );
    let per_token = report_lines(&[row_with_cost()], &Session::default(), &|_| {
        Some(Billing::PerToken)
    });
    assert!(per_token[4].ends_with("· $0.0049"), "{per_token:?}");
}

fn row_with_cost() -> UsageRow {
    let mut r = row(0, "claude", "sonnet");
    r.cost_micros = Some(4_936);
    r.costed = 1;
    r
}

#[test]
fn rows_of_a_pair_over_several_days_are_summed_skipping_none() {
    let mut a = row(0, "codex", "gpt");
    let mut b = row(2, "codex", "gpt");
    a.output = None;
    b.cache_read = Some(7);
    let lines = view(&[a, b]);
    assert_eq!(
        lines[4],
        "  codex › gpt: in 200 · out 20 · cached 7 · partial"
    );
}

#[test]
fn this_chat_counts_only_its_rows_over_any_age_and_none_is_no_data() {
    assert_eq!(view(&[row(0, "claude", "s")])[5], "this chat: no data");
    let mut mine = row(30, "claude", "s");
    mine.in_chat = true;
    mine.turns = 3;
    let lines = view(&[mine, row(0, "claude", "s")]);
    assert_eq!(
        before_charts(lines).last().unwrap(),
        "this chat: 120 tokens · 3 turns"
    );
}

#[test]
fn a_session_adds_its_own_line_after_the_chat_line() {
    let mut session = Session::default();
    session.add(
        "ollama",
        Run::Chat,
        &TurnUsage {
            input: Some(10),
            output: Some(5),
            model: Some("m".into()),
            ..TurnUsage::default()
        },
    );
    let lines = report_lines(&[row(0, "claude", "s")], &session, &default_billing);
    assert_eq!(
        before_charts(lines).last().unwrap(),
        "this session: 15 tokens · 1 turns"
    );
}

#[test]
fn a_session_folds_turns_per_engine_and_model_with_none_not_zero() {
    let mut session = Session::default();
    let turn = |input, output| TurnUsage {
        input,
        output,
        model: Some("m".into()),
        ..TurnUsage::default()
    };
    session.add("ollama", Run::Chat, &turn(Some(10), Some(5)));
    session.add("ollama", Run::Chat, &turn(None, Some(7)));
    let rows = session.rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].turns, 2);
    assert_eq!(rows[0].input, Some(10));
    assert_eq!(rows[0].output, Some(12));
    assert_eq!(rows[0].total, Some(15));
    assert!(rows[0].partial, "a turn lacked its input");
}

#[test]
fn control_characters_in_ids_never_reach_a_line() {
    let mut r = row(0, "claude", "so\u{1b}[31mnnet\nZZ");
    r.engine = "cla\u{7}ude".into();
    for line in view(&[r]) {
        assert!(!line.contains(['\u{1b}', '\u{7}', '\n']), "{line:?}");
    }
}

#[test]
fn summary_lines_carry_partial_when_any_record_is_partial_or_lacks_a_count() {
    let mut cut = row(0, "claude", "s");
    cut.partial = true;
    let lines = view(&[cut, row(1, "claude", "s")]);
    assert_eq!(lines[1], "today: 120 tokens · 1 turns · partial");
    assert_eq!(lines[2], "last 7 days: 240 tokens · 2 turns · partial");
    let mut mine = row(0, "claude", "s");
    mine.in_chat = true;
    mine.input = None;
    assert!(
        before_charts(view(&[mine]))
            .last()
            .unwrap()
            .ends_with("· partial")
    );
    assert_eq!(
        view(&[row(0, "claude", "s")])[2],
        "last 7 days: 120 tokens · 1 turns"
    );
}

#[test]
fn a_session_summary_is_partial_too() {
    let mut session = Session::default();
    session.add(
        "ollama",
        Run::Chat,
        &TurnUsage {
            output: Some(5),
            model: Some("m".into()),
            ..TurnUsage::default()
        },
    );
    let lines = report_lines(&[row(0, "claude", "s")], &session, &default_billing);
    assert!(
        lines.contains(&"this session: no data · 1 turns · partial".to_owned()),
        "{lines:?}"
    );
}

#[test]
fn a_cost_gap_in_a_pair_marks_its_dollar_figure_partial() {
    let mut a = row(0, "claude", "sonnet");
    a.cost_micros = Some(4_936);
    a.records = 2;
    a.costed = 1;
    let lines = view(&[a]);
    assert!(
        lines[4].ends_with("· $0.0049 list-price equivalent · partial"),
        "{lines:?}"
    );
    // The gap shows across days too.
    let mut with = row(0, "claude", "sonnet");
    with.cost_micros = Some(10);
    with.costed = 1;
    let without = row(1, "claude", "sonnet");
    assert!(view(&[with, without])[4].ends_with("· partial"));
    // A pair that never had dollars has no gap.
    assert!(!view(&[row(0, "codex", "g")])[4].contains("partial"));
}

#[test]
fn only_chat_runs_count_as_turns_and_every_run_counts_as_tokens() {
    let mut session = Session::default();
    let usage = tokens(10, 5);
    session.add("ollama", Run::Chat, &usage);
    session.add("ollama", Run::Side, &usage);
    assert_eq!(session.rows()[0].turns, 1);
    assert_eq!(session.rows()[0].records, 2);
    assert_eq!(session.rows()[0].total, Some(30));
}

fn tokens(input: u64, output: u64) -> TurnUsage {
    TurnUsage {
        input: Some(input),
        output: Some(output),
        model: Some("m".into()),
        ..TurnUsage::default()
    }
}

#[test]
fn a_full_session_says_so_instead_of_dropping_silently() {
    let mut session = Session::default();
    for n in 0..70 {
        let usage = TurnUsage {
            model: Some(format!("m{n}")),
            ..tokens(1, 1)
        };
        session.add("ollama", Run::Chat, &usage);
    }
    assert!(session.full());
    let report = report_lines(&[row(0, "claude", "s")], &session, &default_billing);
    assert!(report.contains(&SESSION_FULL_LINE.to_owned()));
    assert!(!view(&[row(0, "claude", "s")]).contains(&SESSION_FULL_LINE.to_owned()));
}

// Charts (ai-usage, "Charts").
fn day(days_ago: u32, engine: &str, total: Option<u64>) -> UsageRow {
    UsageRow {
        total,
        input: total.map(|_| 1),
        output: total.map(|_| 1),
        ..row(days_ago, engine, "m")
    }
}

fn charts(lines: &[String]) -> Vec<String> {
    let at = lines
        .iter()
        .position(|l| l.starts_with("per day"))
        .expect("charts shown");
    lines[at..].to_vec()
}

#[test]
fn the_per_day_line_scales_to_the_biggest_day_and_marks_gaps() {
    let rows = [
        day(3, "claude", Some(0)),
        day(2, "claude", Some(100)),
        day(1, "claude", Some(200)),
        day(0, "claude", Some(400)),
    ];
    let shown = charts(&view(&rows));
    assert_eq!(shown[0], "per day (last 7 days, oldest first):");
    assert_eq!(shown[1], "···▁▂▄█ max 400");
}

#[test]
fn an_unknown_day_is_a_gap_not_the_lowest_bar() {
    let rows = [day(1, "ollama", None), day(0, "ollama", Some(50))];
    let shown = charts(&view(&rows));
    assert_eq!(shown[1], "······█ max 50 *");
}

#[test]
fn a_day_mixing_known_and_unknown_records_counts_only_the_known() {
    let rows = [day(0, "ollama", None), day(0, "claude", Some(10))];
    assert_eq!(charts(&view(&rows))[1], "······█ max 10 *");
}

#[test]
fn only_zero_days_draw_the_lowest_bar_and_never_divide_by_zero() {
    let shown = charts(&view(&[day(0, "claude", Some(0))]));
    assert_eq!(shown[1], "······▁ max 0");
    assert_eq!(shown[3], "  claude █ 0");
}

#[test]
fn engine_bars_follow_the_biggest_and_unknown_says_no_data() {
    let rows = [
        day(0, "claude", Some(1000)),
        day(1, "ollama", Some(250)),
        day(0, "codex", None),
    ];
    let shown = charts(&view(&rows));
    assert_eq!(shown[2], "per engine (last 7 days):");
    assert_eq!(shown[3], format!("  claude {} 1000", "█".repeat(20)));
    assert_eq!(shown[4], format!("  ollama {} 250", "█".repeat(5)));
    assert_eq!(shown[5], "  codex  no data");
}

#[test]
fn a_small_engine_still_gets_one_glyph() {
    let rows = [day(0, "claude", Some(100_000)), day(0, "ollama", Some(1))];
    assert_eq!(charts(&view(&rows))[4], "  ollama █ 1");
}

#[test]
fn days_older_than_the_window_are_not_charted() {
    let rows = [day(0, "claude", Some(10)), day(8, "ollama", Some(999))];
    let shown = charts(&view(&rows));
    assert_eq!(shown[1], "······█ max 10");
    assert_eq!(shown.len(), 4, "{shown:?}");
}

#[test]
fn no_records_or_none_in_the_window_show_no_charts() {
    assert!(!view(&[]).iter().any(|l| l.contains("per day")));
    let old = view(&[day(9, "claude", Some(10))]);
    assert!(!old.iter().any(|l| l.contains("per day")), "{old:?}");
}

#[test]
fn charts_never_carry_dollars() {
    let mut r = day(0, "claude", Some(10));
    r.cost_micros = Some(5_000_000);
    r.costed = 1;
    let shown = charts(&view(&[r]));
    assert!(shown.iter().all(|l| !l.contains('$')), "{shown:?}");
}

#[test]
fn a_tiny_known_day_never_looks_like_a_known_zero() {
    let rows = [day(1, "claude", Some(0)), day(0, "claude", Some(1))];
    assert_eq!(charts(&view(&rows))[1], "·····▁█ max 1");
    let rows = [day(1, "claude", Some(1)), day(0, "claude", Some(100_000))];
    assert_eq!(charts(&view(&rows))[1], "·····▂█ max 100000");
}

#[test]
fn a_partial_or_unknown_record_marks_the_day_line_and_its_engine() {
    let mut cut = day(0, "claude", Some(1000));
    cut.partial = true;
    let rows = [cut, day(1, "ollama", Some(250)), day(0, "codex", None)];
    let shown = charts(&view(&rows));
    assert_eq!(shown[1], "·····▂█ max 1000 *");
    assert_eq!(shown[3], format!("  claude {} 1000 *", "█".repeat(20)));
    assert_eq!(shown[4], format!("  ollama {} 250", "█".repeat(5)));
    assert_eq!(shown[5], "  codex  no data");
}

#[test]
fn an_unknown_record_on_a_charted_day_marks_the_line() {
    let rows = [day(0, "ollama", None), day(0, "claude", Some(10))];
    let shown = charts(&view(&rows));
    assert_eq!(shown[1], "······█ max 10 *");
    assert_eq!(shown[3], format!("  claude {} 10", "█".repeat(20)));
    assert_eq!(shown[4], "  ollama no data");
}

#[test]
fn complete_records_carry_no_marker() {
    let shown = charts(&view(&[day(0, "claude", Some(10))]));
    assert!(shown.iter().all(|l| !l.contains('*')), "{shown:?}");
}

#[test]
fn the_days_run_oldest_first() {
    let rows = [day(6, "claude", Some(400)), day(0, "claude", Some(100))];
    assert_eq!(charts(&view(&rows))[1], "█·····▂ max 400");
}

#[test]
fn day_six_is_charted_and_day_seven_is_not() {
    let rows = [day(6, "claude", Some(10)), day(7, "ollama", Some(99))];
    let shown = charts(&view(&rows));
    assert_eq!(shown[1], "█······ max 10");
    assert!(!shown.iter().any(|l| l.contains("ollama")), "{shown:?}");
}

#[test]
fn engine_bars_line_up_by_display_width() {
    let rows = [
        day(0, "claude", Some(100)),
        day(0, "オラマ", Some(50)),
        day(0, "gemini-cli", None),
    ];
    let shown = charts(&view(&rows));
    // "gemini-cli" is the widest name (10 columns); the others pad to it.
    assert_eq!(shown[3], format!("  claude     {} 100", "█".repeat(20)));
    assert_eq!(shown[4], format!("  オラマ     {} 50", "█".repeat(10)));
    assert_eq!(shown[5], "  gemini-cli no data");
}

#[test]
fn long_engine_names_are_cut_at_16_columns_and_empty_ones_read_a_question_mark() {
    let rows = [
        day(0, "abcdefghijklmnopqrstuvwxyz", Some(10)),
        day(0, "", Some(10)),
    ];
    let shown = charts(&view(&rows));
    assert!(shown[4].starts_with("  abcdefghijklmno… "), "{shown:?}");
    assert!(shown[3].starts_with("  ?    "), "{shown:?}");
    assert_eq!(
        shown[3].chars().position(|c| c == '█'),
        shown[4].chars().position(|c| c == '█')
    );
}

// ---- worker usage (ai-cost 5.2) -------------------------------------------

#[test]
fn the_workers_line_follows_the_seven_day_total_and_is_part_of_it() {
    let mut chat = row(0, "claude", "s");
    chat.total = Some(300);
    let mut worker = worker_row(0, Some(200));
    worker.total = Some(200);
    let lines = view(&[chat, worker]);
    assert_eq!(lines[1], "today: 500 tokens · 1 turns");
    assert_eq!(lines[2], "last 7 days: 500 tokens · 1 turns");
    assert_eq!(lines[3], "of which workers: 200 tokens");
    assert_eq!(lines[4], "by engine and model (last 7 days):");
}

#[test]
fn there_is_no_workers_line_without_worker_records() {
    assert!(
        view(&[row(0, "claude", "s")])
            .iter()
            .all(|l| !l.starts_with("of which workers"))
    );
}

#[test]
fn the_workers_line_counts_the_window_only_and_adds_the_days_up() {
    let rows = [
        worker_row(0, Some(10)),
        worker_row(6, Some(5)),
        worker_row(8, Some(1000)),
    ];
    assert_eq!(view(&rows)[3], "of which workers: 15 tokens");
    assert!(
        view(&[worker_row(8, Some(7))])
            .iter()
            .all(|l| !l.starts_with("of which workers"))
    );
}

#[test]
fn the_workers_line_never_invents_a_total_and_marks_a_gap() {
    let mut gap = worker_row(0, None);
    gap.output = Some(5);
    // The store marks a worker row that lacks a count.
    gap.worker_partial = true;
    assert_eq!(view(&[gap])[3], "of which workers: no data · partial");
    let mut cut = worker_row(0, Some(20));
    cut.worker_partial = true;
    assert_eq!(view(&[cut])[3], "of which workers: 20 tokens · partial");
}

#[test]
fn a_worker_record_is_not_a_turn() {
    assert_eq!(
        view(&[worker_row(0, Some(9))])[1],
        "today: 9 tokens · 0 turns"
    );
}

#[test]
fn the_coordinator_label_replaces_the_usage_prefix_only() {
    assert_eq!(
        coordinator_line("usage: in 12 · out 34".to_owned()),
        "usage (coordinator): in 12 · out 34"
    );
    assert_eq!(
        coordinator_line(usage_line(None, None)),
        "usage (coordinator): no data"
    );
    assert_eq!(coordinator_line("other".to_owned()), "other");
}

// Cache share (#515).
fn cached(engine: &str, hit: Option<u64>, base: Option<u64>) -> UsageRow {
    UsageRow {
        cache_read: hit,
        cache_hit: hit,
        cache_base: base,
        ..row(0, engine, "m")
    }
}

#[test]
fn each_period_shows_the_cache_share_per_engine() {
    let mut mine = cached("claude", Some(90), Some(100));
    mine.in_chat = true;
    let lines = before_charts(view(&[mine, cached("codex", Some(79), Some(100))]));
    assert!(lines.contains(&"cache today: claude 90% · codex 79%".to_owned()));
    assert!(lines.contains(&"cache last 7 days: claude 90% · codex 79%".to_owned()));
    assert!(lines.contains(&"cache this chat: claude 90%".to_owned()));
}

#[test]
fn no_cache_data_shows_no_cache_line_and_never_zero() {
    let lines = before_charts(view(&[cached("ollama", None, None)]));
    assert!(lines.iter().all(|l| !l.contains("cache ")), "{lines:?}");
    // A base of 0 is no data either.
    let lines = before_charts(view(&[cached("x", Some(0), Some(0))]));
    assert!(lines.iter().all(|l| !l.starts_with("cache ")), "{lines:?}");
}

#[test]
fn a_reported_zero_hit_is_shown_as_zero_and_days_are_summed() {
    let mut old = cached("gemini", Some(0), Some(50));
    old.days_ago = 3;
    let lines = before_charts(view(&[old, cached("gemini", Some(50), Some(50))]));
    assert!(lines.contains(&"cache last 7 days: gemini 50%".to_owned()));
    assert!(lines.contains(&"cache today: gemini 100%".to_owned()));
}

#[test]
fn an_incomplete_record_marks_the_cache_line_partial() {
    let mut r = cached("claude", Some(1), Some(4));
    r.cache_gap = 1;
    let lines = before_charts(view(&[r]));
    assert!(lines.contains(&"cache today: claude 25% · partial".to_owned()));
}

#[test]
fn a_row_mixing_records_with_and_without_cache_is_partial() {
    // 1 record with a cache report, 2 without.
    let mut r = cached("claude", Some(90), Some(100));
    r.cache_gap = 2;
    let lines = before_charts(view(&[r]));
    assert!(lines.contains(&"cache today: claude 90% · partial".to_owned()));
    assert!(lines.contains(&"cache last 7 days: claude 90% · partial".to_owned()));
}

#[test]
fn a_day_without_cache_data_marks_the_window_partial() {
    let mut old = cached("codex", None, None);
    old.days_ago = 2;
    old.cache_gap = 1;
    let lines = before_charts(view(&[old, cached("codex", Some(50), Some(100))]));
    assert!(lines.contains(&"cache last 7 days: codex 50% · partial".to_owned()));
    assert!(lines.contains(&"cache today: codex 50%".to_owned()));
}

#[test]
fn an_incomplete_output_alone_does_not_mark_the_cache_partial() {
    let mut r = cached("claude", Some(90), Some(100));
    r.partial = true;
    let lines = before_charts(view(&[r]));
    assert!(lines.contains(&"cache today: claude 90%".to_owned()));
}
