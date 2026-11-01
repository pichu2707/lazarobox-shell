use super::*;
use crate::core::ai::chat::usage::TurnUsage;

fn row(days_ago: u32, engine: &str, model: &str) -> UsageRow {
    UsageRow {
        days_ago,
        engine: engine.into(),
        model: Some(model.into()),
        in_chat: false,
        turns: 1,
        input: Some(100),
        output: Some(20),
        cache_read: None,
        cost_micros: None,
        total: Some(120),
        partial: false,
    }
}

fn view(rows: &[UsageRow]) -> Vec<String> {
    report_lines(rows, &[], &default_billing)
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

#[test]
fn cli_engines_default_to_subscription_and_local_ones_have_no_mode() {
    for engine in ["claude", "codex", "gemini"] {
        assert_eq!(default_billing(engine), Some(Billing::Subscription));
    }
    for engine in ["ollama", "lmstudio", "llamacpp", "vllm", "other"] {
        assert_eq!(default_billing(engine), None);
    }
}

#[test]
fn an_empty_store_says_so_under_the_alpha_header() {
    assert_eq!(view(&[]), vec![HEADER, EMPTY_LINE]);
}

#[test]
fn the_header_is_the_first_line_in_every_view() {
    assert_eq!(view(&[row(0, "claude", "s")])[0], "AI usage (alpha)");
    assert_eq!(session_lines(&[], &default_billing)[0], "AI usage (alpha)");
    assert_eq!(UNAVAILABLE_LINE, "usage history is not available");
    assert_eq!(READING_LINE, "reading usage…");
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
    assert_eq!(lines[1], "today: no data · 1 turns");
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
    let per_token = report_lines(&[row_with_cost()], &[], &|_| Some(Billing::PerToken));
    assert!(per_token[4].ends_with("· $0.0049"), "{per_token:?}");
}

fn row_with_cost() -> UsageRow {
    let mut r = row(0, "claude", "sonnet");
    r.cost_micros = Some(4_936);
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
        &TurnUsage {
            input: Some(10),
            output: Some(5),
            model: Some("m".into()),
            ..TurnUsage::default()
        },
    );
    let lines = report_lines(&[row(0, "claude", "s")], session.rows(), &default_billing);
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
    session.add("ollama", &turn(Some(10), Some(5)));
    session.add("ollama", &turn(None, Some(7)));
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
    let lines = session_lines(&[], &default_billing);
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
        &TurnUsage {
            input: Some(2),
            output: Some(4),
            cost_micros: Some(4_936),
            model: Some("sonnet".into()),
            ..TurnUsage::default()
        },
    );
    let lines = session_lines(session.rows(), &default_billing);
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
