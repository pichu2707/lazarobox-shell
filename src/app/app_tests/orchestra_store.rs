//! Runs in the local store (agent-orchestrator S9): what the app saves as a
//! run goes by, and what it says when a restart finds one interrupted.

use super::chat_live::{ask, chat_id, chat_of, plain};
use super::orchestra_run::{RUN, event, row, running};
use super::orchestra_send::{orch_ai, orch_app_with, toggle};
use super::*;
use crate::core::ai::{
    chat::{Entry, store::StoreReply},
    orchestra::{
        event::OrchestraEvent,
        record::{RunPhase, RunRecord, WorkerRecord},
        worker::WorkerStatus,
    },
};

fn saves(effects: &[Effect]) -> Vec<RunRecord> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Store(StoreCmd::SaveRun { run, .. }) => Some((**run).clone()),
            _ => None,
        })
        .collect()
}

fn recovers(effects: &[Effect]) -> usize {
    (effects.iter())
        .filter(|e| matches!(e, Effect::Store(StoreCmd::RecoverRuns { .. })))
        .count()
}

fn system(a: &App) -> Vec<String> {
    (chat_of(a).entries().iter())
        .filter_map(|e| match e {
            Entry::System(l) => Some(l.clone()),
            _ => None,
        })
        .collect()
}

fn interrupted(id: u64, running: usize) -> RunRecord {
    RunRecord {
        id,
        goal: format!("goal {id}"),
        phase: RunPhase::Interrupted,
        workers: (1..=running as u32)
            .map(|n| WorkerRecord {
                id: n,
                kind: "review".into(),
                engine: "codex".into(),
                model: None,
                status: "running".into(),
            })
            .collect(),
    }
}

fn reply(a: &mut App, reply: StoreReply) -> Vec<Effect> {
    a.update(AppEvent::Store(reply))
}

// Spec: Restart shows an interrupted run. The first time the pane goes into
// ORCH after a start, the store is asked which runs the restart interrupted.
#[test]
fn the_first_orch_toggle_after_a_start_asks_for_interrupted_runs_once() {
    let mut a = orch_app_with(orch_ai());
    assert_eq!(recovers(&toggle(&mut a)), 1);
    assert_eq!(recovers(&toggle(&mut a)), 0);
    assert_eq!(recovers(&toggle(&mut a)), 0);
}

// Gate OFF changes nothing: no run command is ever sent.
#[test]
fn with_the_gate_off_nothing_about_runs_is_sent() {
    let mut ai = orch_ai();
    ai.orchestrator.enabled = false;
    let mut a = orch_app_with(ai);
    assert_eq!(recovers(&toggle(&mut a)), 0);
    let sent = ask(&mut a, "hello");
    assert!(saves(&sent).is_empty() && recovers(&sent) == 0);
}

#[test]
fn an_interrupted_run_is_shown_with_its_goal_and_nothing_runs() {
    let mut a = orch_app_with(orch_ai());
    let effects = reply(
        &mut a,
        StoreReply::RunsRecovered {
            pane: chat_id(),
            runs: vec![interrupted(9, 1)],
        },
    );
    assert!(effects.is_empty());
    assert!(
        system(&a).contains(
            &"run 9 was interrupted by a restart: 1 worker unfinished, nothing runs \u{b7} goal 9"
                .to_owned()
        )
    );
}

#[test]
fn more_than_three_interrupted_runs_name_three_and_count_the_rest() {
    let mut a = orch_app_with(orch_ai());
    let runs = (1..=5).rev().map(|n| interrupted(n, 1)).collect();
    reply(
        &mut a,
        StoreReply::RunsRecovered {
            pane: chat_id(),
            runs,
        },
    );
    let said = system(&a);
    assert_eq!(
        said.iter()
            .filter(|l| l.contains("was interrupted by a restart"))
            .count(),
        3
    );
    assert!(
        said.contains(&"2 more runs were interrupted by a restart".to_owned()),
        "{said:?}"
    );
}

#[test]
fn no_interrupted_run_says_nothing() {
    let mut a = orch_app_with(orch_ai());
    let before = system(&a);
    reply(
        &mut a,
        StoreReply::RunsRecovered {
            pane: chat_id(),
            runs: vec![],
        },
    );
    assert_eq!(system(&a), before);
}

#[test]
fn a_run_store_failure_is_said_once_per_kind() {
    let mut a = orch_app_with(orch_ai());
    let pane = chat_id();
    for _ in 0..2 {
        reply(&mut a, StoreReply::RunsFailed { pane, saving: true });
        reply(
            &mut a,
            StoreReply::RunsFailed {
                pane,
                saving: false,
            },
        );
    }
    let said = system(&a);
    let count = |text: &str| said.iter().filter(|l| l.as_str() == text).count();
    assert_eq!(
        count("could not save the run: it will not be listed after a restart"),
        1
    );
    assert_eq!(count("could not read the stored runs"), 1);
}

#[test]
fn a_run_is_saved_as_its_workers_change_and_closed_when_it_ends() {
    let (mut a, _) = running();
    let first = a.update(event(row(RUN, 1, WorkerStatus::Running)));
    let [saved] = saves(&first).try_into().unwrap();
    assert_eq!((saved.id, saved.phase), (RUN.0, RunPhase::Live));
    assert_eq!(saved.goal, "plan it");
    assert_eq!(
        saved.workers,
        vec![WorkerRecord {
            id: 1,
            kind: "review".into(),
            engine: "codex".into(),
            model: Some("gpt-5".into()),
            status: "running".into(),
        }]
    );
    // Only the worker that changed is sent.
    let second = a.update(event(row(RUN, 2, WorkerStatus::Queued)));
    let third = a.update(event(row(RUN, 1, WorkerStatus::Done)));
    let [second] = saves(&second).try_into().unwrap();
    let [third] = saves(&third).try_into().unwrap();
    assert_eq!((second.workers.len(), second.workers[0].id), (1, 2));
    assert_eq!(
        (third.workers.len(), third.workers[0].status.as_str()),
        (1, "done")
    );
    let end = a.update(event(OrchestraEvent::Ended { run: RUN }));
    let [closed] = saves(&end).try_into().unwrap();
    // The close carries every known worker, so a lost earlier save is repaired.
    assert_eq!((closed.phase, closed.workers.len()), (RunPhase::Ended, 2));
    // An ended run stores nothing more.
    let late = a.update(event(row(RUN, 2, WorkerStatus::Running)));
    assert!(saves(&late).is_empty());
}

// Write amplification: a row that changes only its usage or elapsed time is
// not a change of the run, so nothing is written.
#[test]
fn a_row_that_changes_nothing_stored_is_not_saved_again() {
    let (mut a, _) = running();
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    let OrchestraEvent::Worker(mut busier) = row(RUN, 1, WorkerStatus::Running) else {
        unreachable!()
    };
    busier.elapsed = Some(std::time::Duration::from_secs(3));
    busier.partial = true;
    let again = a.update(event(OrchestraEvent::Worker(busier)));
    assert!(saves(&again).is_empty(), "{again:?}");
    let moved = a.update(event(row(RUN, 1, WorkerStatus::Done)));
    assert_eq!(saves(&moved).len(), 1);
}

// A pane closed with its run live leaves no `live` row for the next start to
// report as an interruption by a restart.
#[test]
fn closing_the_pane_of_a_live_run_closes_the_stored_run() {
    let (mut a, _) = running();
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    a.update(event(row(RUN, 2, WorkerStatus::Queued)));
    press(&mut a, "wq");
    let effects = a.update(key('y'));
    let [closed] = saves(&effects).try_into().unwrap_or_else(|_| {
        panic!("one close save expected: {effects:?}");
    });
    assert_eq!((closed.id, closed.phase), (RUN.0, RunPhase::Ended));
    assert_eq!(closed.workers.len(), 2, "the close carries every worker");
    // The run manager's own `Ended` for the gone pane saves nothing more.
    let late = a.update(event(OrchestraEvent::Ended { run: RUN }));
    assert!(saves(&late).is_empty());
}

// The goal of a start that failed is not kept for a later run.
#[test]
fn a_failed_start_forgets_its_goal() {
    use crate::core::ai::chat::engine::{ChatError, StreamEvent};
    let (mut a, turn) = running();
    assert_eq!(a.kept_goals(), 1);
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Error(ChatError::Failed("could not start".into())),
    ));
    assert_eq!(a.kept_goals(), 0);
}

// Closing the pane forgets the goal too.
#[test]
fn closing_a_pane_forgets_its_goal() {
    let (mut a, _) = running();
    assert_eq!(a.kept_goals(), 1);
    press(&mut a, "wq");
    a.update(key('y'));
    assert_eq!(a.kept_goals(), 0);
}

// A save that failed is repaired: the next save of that run resends every
// worker, not just the one that changed.
#[test]
fn after_a_failed_save_the_next_save_carries_every_worker() {
    let (mut a, _) = running();
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    a.update(event(row(RUN, 2, WorkerStatus::Queued)));
    reply(
        &mut a,
        StoreReply::RunsFailed {
            pane: chat_id(),
            saving: true,
        },
    );
    let next = a.update(event(row(RUN, 1, WorkerStatus::Done)));
    let [full] = saves(&next).try_into().unwrap();
    let ids: Vec<_> = full.workers.iter().map(|w| w.id).collect();
    assert_eq!(ids, [1, 2]);
    // Repaired once: the save after that is a delta again.
    let after = a.update(event(row(RUN, 2, WorkerStatus::Running)));
    assert_eq!(saves(&after)[0].workers.len(), 1);
}

#[test]
fn a_failed_save_resends_even_a_row_that_changed_nothing() {
    let (mut a, _) = running();
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    reply(
        &mut a,
        StoreReply::RunsFailed {
            pane: chat_id(),
            saving: true,
        },
    );
    let again = a.update(event(row(RUN, 1, WorkerStatus::Running)));
    assert_eq!(saves(&again).len(), 1);
}

// A turn cancelled before any run began leaves no goal behind.
#[test]
fn a_cancelled_turn_with_no_run_forgets_its_goal() {
    let (mut a, _) = running();
    assert_eq!(a.kept_goals(), 1);
    a.update(plain(crossterm::event::KeyCode::Esc));
    assert_eq!(a.kept_goals(), 0);
}
