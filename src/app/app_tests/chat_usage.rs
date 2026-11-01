//! Usage rows in the app (ai-cost 1b.3 to 1b.5): written only with history
//! on, and a lost row is told once per run, never blocking a turn.

use super::chat_history::*;
use super::*;
use crate::core::ai::chat::{
    Entry,
    engine::StreamEvent,
    store::{StoreCmd, StoreReply},
    usage::TurnUsage,
};

fn used(output: u64) -> StreamEvent {
    StreamEvent::usage(TurnUsage {
        output: Some(output),
        ..TurnUsage::default()
    })
}

/// One answered turn that reported `output` tokens; the effects of its end.
fn answered(a: &mut App, question: &str, output: u64) -> Vec<Effect> {
    let turn = sent_turn(&ask(a, question));
    a.update(delta(chat_id(), turn, "answer"));
    a.update(AppEvent::Chat(chat_id(), turn, used(output)));
    a.update(finish(chat_id(), turn))
}

fn recorded(effects: &[Effect]) -> usize {
    stores(effects)
        .iter()
        .filter(|cmd| matches!(cmd, StoreCmd::RecordUsage { .. }))
        .count()
}

fn recorded_reply(problem: Option<&str>) -> AppEvent {
    AppEvent::Store(StoreReply::UsageRecorded {
        pane: chat_id(),
        today: problem.is_none().then_some(10),
        cost: Vec::new(),
        until: problem.is_none().then_some(NOON + DAY),
        read_problem: None,
        problem: problem.map(str::to_owned),
    })
}

fn notices(a: &App) -> usize {
    systems(chat_of(a))
        .iter()
        .filter(|l| l.starts_with("Usage not saved:"))
        .count()
}

#[test]
fn a_finished_turn_with_history_on_records_its_usage() {
    let mut a = conversation();
    let end = answered(&mut a, "one", 7);
    assert_eq!(recorded(&end), 1, "{end:?}");
}

#[test]
fn with_history_off_no_usage_row_is_sent() {
    let mut a = launched(120, 30).with_ai(ollama());
    a.update(AppEvent::EnginesDetected(super::chat_history::detection()));
    press(&mut a, "c");
    a.update(prompt_for(chat_id()));
    let end = answered(&mut a, "one", 7);
    assert_eq!(stores(&end), Vec::new());
    // And nothing is kept for a later switch of the setting.
    assert_eq!(notices(&a), 0);
}

#[test]
fn a_failed_usage_write_says_so_once_per_run_and_turns_still_complete() {
    let mut a = conversation();
    answered(&mut a, "one", 1);
    a.update(created(chat_id(), 5));
    a.update(recorded_reply(Some("disk full")));
    assert_eq!(
        systems(chat_of(&a))
            .iter()
            .filter(|l| *l == "Usage not saved: disk full")
            .count(),
        1
    );
    for n in 2..4 {
        let end = answered(&mut a, "again", n);
        assert_eq!(recorded(&end), 1, "turn {n} still records");
        a.update(recorded_reply(Some("disk full")));
    }
    assert_eq!(notices(&a), 1, "once per run");
}

#[test]
fn a_stored_row_and_a_quiet_prune_show_nothing() {
    let mut a = conversation();
    answered(&mut a, "one", 1);
    a.update(recorded_reply(None));
    assert_eq!(notices(&a), 0);
}

#[test]
fn the_notice_reason_is_one_bounded_line() {
    let mut a = conversation();
    answered(&mut a, "one", 1);
    let raw = format!("ZZ-MARKER-ZZ\n{}", "x".repeat(300));
    a.update(recorded_reply(Some(&raw)));
    let line = systems(chat_of(&a))
        .into_iter()
        .find(|l| l.starts_with("Usage not saved:"))
        .expect("a notice");
    let reason = line.trim_start_matches("Usage not saved: ");
    assert!(
        reason.chars().count() <= 120 && !reason.contains('\n'),
        "{line}"
    );
}

/// A fixed wall clock: some day at noon, so `NOON + DAY` is later than the
/// next midnight.
const NOON: i64 = 1_700_000_000;
const DAY: i64 = 86_400;

fn read_requests(effects: &[Effect]) -> usize {
    stores(effects)
        .iter()
        .filter(|cmd| matches!(cmd, StoreCmd::UsageToday { .. }))
        .count()
}

fn failed_read(reason: &str) -> AppEvent {
    AppEvent::Store(StoreReply::UsageRecorded {
        pane: chat_id(),
        today: None,
        cost: Vec::new(),
        until: None,
        read_problem: Some(reason.to_owned()),
        problem: None,
    })
}

fn usage_lines(a: &App) -> Vec<String> {
    chat_of(a)
        .entries()
        .iter()
        .filter_map(|e| match e {
            Entry::Usage(line) => Some(line.clone()),
            _ => None,
        })
        .collect()
}

// Spec (chat-pane, Per-answer usage line): on by default, from the app's
// settings, and never stored.
#[test]
fn the_usage_line_is_on_by_default_and_follows_the_settings() {
    let mut a = conversation();
    let end = answered(&mut a, "one", 7);
    assert_eq!(usage_lines(&a), ["usage: in no data · out 7"]);
    // A line is a view of the turn: the store gets the row and the message,
    // never the line.
    let leaked = stores(&end)
        .iter()
        .any(|cmd| format!("{cmd:?}").contains("usage: in"));
    assert!(!leaked, "{end:?}");
    let mut off = ollama();
    off.set_usage(crate::core::ai::config::UsageConfig::without_line());
    let mut a = launched(120, 30).with_ai(off);
    a.update(AppEvent::EnginesDetected(super::chat_history::detection()));
    press(&mut a, "c");
    a.update(prompt_for(chat_id()));
    answered(&mut a, "one", 7);
    assert!(usage_lines(&a).is_empty());
}

#[test]
fn a_turn_the_engine_never_reported_on_says_no_data() {
    let mut a = conversation();
    let turn = sent_turn(&ask(&mut a, "one"));
    a.update(delta(chat_id(), turn, "answer"));
    a.update(finish(chat_id(), turn));
    assert_eq!(usage_lines(&a), ["usage: no data"]);
}

// Spec (statusline, Usage segment): the daily figure comes from the store's
// reply, never from the UI thread.
fn recorded_with(today: Option<u64>, cost: &[(&str, u64)]) -> AppEvent {
    AppEvent::Store(StoreReply::UsageRecorded {
        pane: chat_id(),
        today,
        cost: cost.iter().map(|(e, m)| ((*e).to_owned(), *m)).collect(),
        until: Some(NOON + DAY),
        read_problem: None,
        problem: None,
    })
}

#[test]
fn the_segment_is_absent_before_the_first_read_and_then_shows_the_total() {
    let mut a = conversation();
    assert_eq!(a.status_usage(), None, "absent, not 0");
    a.update(recorded_with(Some(1234), &[]));
    assert_eq!(a.status_usage().as_deref(), Some("today: 1234 tok"));
    a.update(recorded_with(Some(1434), &[]));
    assert_eq!(a.status_usage().as_deref(), Some("today: 1434 tok"));
}

#[test]
fn a_day_without_a_known_total_reads_no_data() {
    let mut a = conversation();
    a.update(recorded_with(None, &[]));
    assert_eq!(a.status_usage().as_deref(), Some("today: no data"));
}

#[test]
fn only_per_token_engines_add_dollars_to_the_segment() {
    let mut per_token = ollama();
    per_token.set_usage(
        "[ai.usage.billing]\nclaude = \"per-token\"\n"
            .parse::<toml::Table>()
            .map(|t| {
                crate::core::ai::config::read_with_port_env(&t, &mut Vec::new(), None)
                    .usage()
                    .clone()
            })
            .unwrap(),
    );
    let mut a = launched(120, 30).with_ai(per_token);
    a.update(AppEvent::EnginesDetected(super::chat_history::detection()));
    press(&mut a, "c");
    a.update(prompt_for(chat_id()));
    a.update(recorded_with(
        Some(5000),
        &[("claude", 420_000), ("codex", 900_000), ("ollama", 5)],
    ));
    assert_eq!(a.status_usage().as_deref(), Some("today: 5000 tok · $0.42"));
    // Subscription engines only: tokens and no dollars.
    let mut a = conversation();
    a.update(recorded_with(Some(5000), &[("claude", 420_000)]));
    assert_eq!(a.status_usage().as_deref(), Some("today: 5000 tok"));
}

#[test]
fn a_zero_dollar_sum_adds_no_figure() {
    let mut per_token = ollama();
    per_token.set_usage(billing_config(
        "[ai.usage.billing]\nclaude = \"per-token\"\n",
    ));
    let mut a = launched(120, 30).with_ai(per_token);
    a.update(AppEvent::EnginesDetected(super::chat_history::detection()));
    press(&mut a, "c");
    a.update(prompt_for(chat_id()));
    a.update(recorded_with(Some(5000), &[("claude", 0)]));
    assert_eq!(a.status_usage().as_deref(), Some("today: 5000 tok"));
}

fn billing_config(text: &str) -> crate::core::ai::config::UsageConfig {
    let table: toml::Table = text.parse().unwrap();
    crate::core::ai::config::read_with_port_env(&table, &mut Vec::new(), None)
        .usage()
        .clone()
}

#[test]
fn a_failed_write_keeps_the_last_figure() {
    let mut a = conversation();
    a.update(recorded_with(Some(10), &[]));
    a.update(recorded_reply(Some("disk full")));
    assert_eq!(a.status_usage().as_deref(), Some("today: 10 tok"));
}

#[test]
fn the_segment_follows_its_setting_and_the_focused_pane() {
    let mut off = ollama();
    off.set_usage(
        "[ai.usage]\nstatusline = false\n"
            .parse::<toml::Table>()
            .map(|t| {
                crate::core::ai::config::read_with_port_env(&t, &mut Vec::new(), None)
                    .usage()
                    .clone()
            })
            .unwrap(),
    );
    let mut a = launched(120, 30).with_ai(off);
    a.update(AppEvent::EnginesDetected(super::chat_history::detection()));
    press(&mut a, "c");
    a.update(prompt_for(chat_id()));
    a.update(recorded_with(Some(10), &[]));
    assert_eq!(a.status_usage(), None);
    let mut shell = launched(120, 30);
    shell.update(recorded_with(Some(10), &[]));
    assert_eq!(shell.status_usage(), None, "a shell pane shows no usage");
}

// Spec (statusline, Usage segment): read once at open, off the UI thread, and
// again when the local day changes. A test app's wall clock reads `NOON`.
#[test]
fn the_first_focused_chat_asks_the_store_once_for_the_day() {
    let (mut a, opening) = opened();
    assert_eq!(read_requests(&opening), 1, "{opening:?}");
    assert_eq!(read_requests(&a.update(AppEvent::AnimTick)), 0, "in flight");
    a.update(recorded_with(Some(5), &[]));
    assert_eq!(read_requests(&a.update(AppEvent::AnimTick)), 0, "today's");
}

#[test]
fn nothing_is_asked_with_the_segment_off_over_a_shell_or_without_history() {
    let mut off = ollama();
    off.set_usage(billing_config("[ai.usage]\nstatusline = false\n"));
    let mut a = launched(120, 30).with_ai(off).with_chat_history();
    a.update(AppEvent::EnginesDetected(super::chat_history::detection()));
    assert_eq!(read_requests(&press(&mut a, "c")), 0);
    let mut shell = launched(120, 30).with_chat_history();
    assert_eq!(read_requests(&shell.update(AppEvent::AnimTick)), 0);
    let mut no_history = launched(120, 30).with_ai(ollama());
    no_history.update(AppEvent::EnginesDetected(super::chat_history::detection()));
    assert_eq!(read_requests(&press(&mut no_history, "c")), 0);
}

static NOW: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(NOON);

#[test]
fn yesterdays_figure_is_never_shown_and_the_day_is_read_again() {
    use std::sync::atomic::Ordering::SeqCst;
    NOW.store(NOON, SeqCst);
    let mut a = history_app().with_wall_clock(|| NOW.load(SeqCst));
    press(&mut a, "c");
    a.update(AppEvent::Store(StoreReply::UsageRecorded {
        pane: chat_id(),
        today: Some(900),
        cost: Vec::new(),
        until: Some(NOON + 3600),
        read_problem: None,
        problem: None,
    }));
    assert_eq!(a.status_usage().as_deref(), Some("today: 900 tok"));
    NOW.store(NOON + 3600, SeqCst);
    assert_eq!(a.status_usage(), None, "past midnight: not yesterday's");
    let again = a.update(AppEvent::AnimTick);
    assert_eq!(read_requests(&again), 1, "{again:?}");
}

// Spec (never fail silently): a day that cannot be read is told once per run,
// keeps the last figure and is not retried on every event.
#[test]
fn a_failed_read_keeps_the_figure_says_so_once_and_waits_before_retrying() {
    let mut a = conversation();
    a.update(recorded_with(Some(77), &[]));
    a.update(failed_read("database busy"));
    a.update(failed_read("database busy"));
    assert_eq!(a.status_usage().as_deref(), Some("today: 77 tok"));
    let lines: Vec<_> = systems(chat_of(&a))
        .into_iter()
        .filter(|l| l.starts_with("Usage not read:"))
        .collect();
    assert_eq!(lines, ["Usage not read: database busy"]);
    assert_eq!(read_requests(&a.update(AppEvent::AnimTick)), 0);
}

#[test]
fn a_failed_first_read_shows_nothing_rather_than_no_data() {
    let mut a = conversation();
    a.update(failed_read("database damaged"));
    assert_eq!(a.status_usage(), None);
}
