//! Usage rows in the app (ai-cost 1b.3 to 1b.5): written only with history
//! on, and a lost row is told once per run, never blocking a turn.

use super::chat_history::*;
use super::*;
use crate::core::ai::chat::{
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
