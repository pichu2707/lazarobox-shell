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
        cost_micros: None,
        total: Some(120),
        partial: false,
    }
}

fn view(rows: &[UsageRow]) -> Vec<String> {
    report_lines(rows, &Session::default(), &default_billing)
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
        "today: 5000 tok · $0.4200"
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
    assert_eq!(
        session_lines(&Session::default(), &default_billing)[0],
        "AI usage (alpha)"
    );
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
    assert_eq!(lines.last().unwrap(), "this chat: 120 tokens · 3 turns");
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
    assert_eq!(lines.last().unwrap(), "this session: 15 tokens · 1 turns");
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
fn history_off_shows_the_session_only_with_the_explaining_line() {
    let lines = session_lines(&Session::default(), &default_billing);
    assert_eq!(
        lines,
        vec![HEADER, HISTORY_OFF_LINE, EMPTY_LINE],
        "{lines:?}"
    );
    assert_eq!(
        HISTORY_OFF_LINE,
        "chat history is off: showing this session only"
    );
    let mut session = Session::default();
    session.add(
        "claude",
        Run::Chat,
        &TurnUsage {
            input: Some(2),
            output: Some(4),
            cost_micros: Some(4_936),
            model: Some("sonnet".into()),
            ..TurnUsage::default()
        },
    );
    let lines = session_lines(&session, &default_billing);
    assert_eq!(lines[2], "this session: 6 tokens · 1 turns");
    assert_eq!(lines[3], "by engine and model:");
    assert_eq!(
        lines[4],
        "  claude › sonnet: in 2 · out 4 · $0.0049 list-price equivalent"
    );
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
    assert!(view(&[mine]).last().unwrap().ends_with("· partial"));
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
    let lines = session_lines(&session, &default_billing);
    assert_eq!(lines[2], "this session: no data · 1 turns · partial");
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
    let shown = session_lines(&session, &default_billing);
    assert!(shown.contains(&SESSION_FULL_LINE.to_owned()), "{shown:?}");
    let report = report_lines(&[row(0, "claude", "s")], &session, &default_billing);
    assert!(report.contains(&SESSION_FULL_LINE.to_owned()));
    assert!(
        !session_lines(&Session::default(), &default_billing)
            .contains(&SESSION_FULL_LINE.to_owned())
    );
}
