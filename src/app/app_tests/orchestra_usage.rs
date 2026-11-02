//! Worker usage in the app (ai-cost 5.4): a worker turn's record becomes a
//! `worker` row of the pane, under the same rules as a chat turn's, and the
//! coordinator's own usage line says whose it is.

use super::chat_history::{stores, systems};
use super::chat_live::{chat_id, chat_of, delta, finish};
use super::orchestra_run::{RUN, event, running};
use super::*;
use crate::core::ai::{
    chat::{
        Entry,
        engine::StreamEvent,
        store::{StoreCmd, StoreReply},
        usage::TurnUsage,
    },
    orchestra::{WorkerId, event::OrchestraEvent},
};

fn worker_used(input: u64, output: u64) -> OrchestraEvent {
    OrchestraEvent::Usage {
        run: RUN,
        worker: WorkerId(1),
        engine: "codex".into(),
        usage: Box::new(TurnUsage {
            input: Some(input),
            output: Some(output),
            model: Some("gpt-5".into()),
            ..TurnUsage::default()
        }),
    }
}

fn worker_rows(effects: &[Effect]) -> Vec<(String, String, Option<String>, TurnUsage)> {
    stores(effects)
        .into_iter()
        .filter_map(|cmd| match cmd {
            StoreCmd::RecordUsage {
                engine,
                role,
                worker,
                usage,
                ..
            } => Some((role, engine, worker, *usage)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_worker_record_is_stored_with_its_role_engine_and_worker_id() {
    let (mut a, _) = running();
    a.history = true;
    let effects = a.update(event(worker_used(120, 30)));
    let rows = worker_rows(&effects);
    assert_eq!(rows.len(), 1, "{effects:?}");
    let (role, engine, worker, usage) = &rows[0];
    assert_eq!((role.as_str(), engine.as_str()), ("worker", "codex"));
    assert_eq!(worker.as_deref(), Some("7.1"));
    assert_eq!((usage.input, usage.output), (Some(120), Some(30)));
}

#[test]
fn a_partial_worker_record_stays_partial_and_a_missing_count_stays_unknown() {
    let (mut a, _) = running();
    a.history = true;
    let effects = a.update(event(OrchestraEvent::Usage {
        run: RUN,
        worker: WorkerId(2),
        engine: "ollama".into(),
        usage: Box::new(TurnUsage {
            output: Some(7),
            partial: true,
            ..TurnUsage::default()
        }),
    }));
    let rows = worker_rows(&effects);
    assert_eq!(rows.len(), 1, "{effects:?}");
    let usage = &rows[0].3;
    assert_eq!(
        (usage.input, usage.output, usage.partial),
        (None, Some(7), true)
    );
}

#[test]
fn with_history_off_a_worker_record_is_not_stored_but_the_session_counts_it() {
    let (mut a, _) = running();
    let effects = a.update(event(worker_used(120, 30)));
    assert_eq!(stores(&effects), Vec::new());
    let session = chat_of(&a).session_usage();
    assert_eq!(session.rows().len(), 1);
    assert_eq!(session.rows()[0].total, Some(150));
    // Nothing is kept for a later switch of the setting.
    a.history = true;
    assert_eq!(worker_rows(&a.update(event(worker_used(1, 1)))).len(), 1);
}

#[test]
fn a_lost_worker_row_is_told_once_per_run() {
    let (mut a, _) = running();
    a.history = true;
    a.update(event(worker_used(1, 1)));
    for _ in 0..2 {
        a.update(AppEvent::Store(StoreReply::UsageRecorded {
            pane: chat_id(),
            today: None,
            cost: Vec::new(),
            until: None,
            read_problem: None,
            problem: Some("disk full".into()),
        }));
    }
    let told = systems(chat_of(&a))
        .iter()
        .filter(|l| l.starts_with("Usage not saved: disk full"))
        .count();
    assert_eq!(told, 1);
}

#[test]
fn a_worker_turn_never_raises_the_per_turn_note() {
    let table: toml::Table = "[ai.usage]\nturn_tokens = 10\n".parse().unwrap();
    let mut ai = super::orchestra_send::orch_ai();
    ai.orchestrator.acknowledged = true;
    ai.set_usage(
        crate::core::ai::config::read_with_port_env(&table, &mut Vec::new(), None)
            .usage()
            .clone(),
    );
    let mut a = super::orchestra_send::orch_app_with(ai);
    a.history = true;
    a.update(event(worker_used(5_000, 5_000)));
    assert!(
        systems(chat_of(&a))
            .iter()
            .all(|l| !l.starts_with("turn used")),
        "{:?}",
        systems(chat_of(&a))
    );
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

#[test]
fn the_coordinators_usage_line_says_it_is_the_coordinators() {
    let (mut a, turn) = running();
    a.update(delta(turn, "plan"));
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::usage(TurnUsage {
            input: Some(12),
            output: Some(34),
            ..TurnUsage::default()
        }),
    ));
    a.update(finish(turn));
    assert_eq!(usage_lines(&a), ["usage (coordinator): in 12 · out 34"]);
}

fn orphan_usage(a: &mut App) -> Vec<Effect> {
    // A pane that is not a chat any more (closed).
    a.update(AppEvent::Orchestra(
        PaneId::for_test(99),
        worker_used(120, 30),
    ))
}

#[test]
fn spend_of_a_pane_that_is_gone_is_still_stored_unlinked() {
    let (mut a, _) = running();
    a.history = true;
    let effects = orphan_usage(&mut a);
    let cmds: Vec<_> = stores(&effects)
        .into_iter()
        .filter(|c| matches!(c, StoreCmd::RecordUsage { .. }))
        .collect();
    let [
        StoreCmd::RecordUsage {
            conversation,
            role,
            worker,
            usage,
            ..
        },
    ] = cmds.as_slice()
    else {
        panic!("one record: {effects:?}");
    };
    assert_eq!(*conversation, None);
    assert_eq!((role.as_str(), worker.as_deref()), ("worker", Some("7.1")));
    assert_eq!((usage.input, usage.output), (Some(120), Some(30)));
}

#[test]
fn with_history_off_the_usage_of_a_pane_that_is_gone_is_not_stored() {
    let (mut a, _) = running();
    assert_eq!(stores(&orphan_usage(&mut a)), Vec::new());
}
