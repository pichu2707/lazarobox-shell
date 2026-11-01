//! Resuming an interrupted run (agent-orchestrator S9 9.1c; Run view and
//! persistence, "Restart shows an interrupted run"): `Ctrl+Space r r` starts a
//! new run seeded with the stored goal, `Ctrl+Space r d` discards it. Nothing
//! runs without that choice, the read scope is asked again by the new run, and
//! a run answered either way is not offered again.

use std::path::PathBuf;

use super::chat_live::{chat_id, chat_of, ctrl};
use super::orchestra_run::{RUN, event, row, running};
use super::orchestra_send::{orch_ai, orch_app_with, toggle};
use super::*;
use crate::core::ai::{
    chat::{Entry, store::StoreReply},
    orchestra::{
        RunId,
        record::{RunPhase, RunRecord, WorkerRecord},
        worker::WorkerStatus,
    },
};

fn stored(id: u64, goal: &str) -> RunRecord {
    elsewhere(id, goal, Some(HERE))
}

/// The folder the test chat opens in.
const HERE: &str = "/launch";

fn elsewhere(id: u64, goal: &str, folder: Option<&str>) -> RunRecord {
    RunRecord {
        id,
        goal: goal.to_owned(),
        phase: RunPhase::Interrupted,
        workers: vec![
            WorkerRecord {
                id: 1,
                kind: "review".into(),
                engine: "codex".into(),
                model: None,
                status: "running".into(),
            },
            WorkerRecord {
                id: 2,
                kind: "review".into(),
                engine: "codex".into(),
                model: None,
                status: "done".into(),
            },
        ],
        folder: folder.map(PathBuf::from),
        supersedes: None,
    }
}

fn recovered(a: &mut App, runs: Vec<RunRecord>) {
    a.update(AppEvent::Store(StoreReply::RunsRecovered {
        pane: chat_id(),
        runs,
    }));
}

fn keys(a: &mut App, keys: &str) -> Vec<Effect> {
    NOW_MS.with(|n| n.set(n.get() + 1_000));
    press(a, keys)
}

fn system(a: &App) -> Vec<String> {
    (chat_of(a).entries().iter())
        .filter_map(|e| match e {
            Entry::System(l) => Some(l.clone()),
            _ => None,
        })
        .collect()
}

fn closes(effects: &[Effect], run: u64) -> usize {
    (effects.iter())
        .filter(|e| {
            matches!(e, Effect::Store(StoreCmd::SaveRun { run: r, .. })
                if r.id == run && r.phase == RunPhase::Ended)
        })
        .count()
}

fn turn_of(effects: &[Effect]) -> Option<(&ChatRequest, &OrchSend)> {
    effects.iter().find_map(|e| match e {
        Effect::OrchestrateSend { request, orch, .. } => Some((request, &**orch)),
        _ => None,
    })
}

/// A ready pane in ORCH with run 9 found interrupted.
fn interrupted_app() -> App {
    let mut ai = orch_ai();
    ai.orchestrator.acknowledged = true;
    let mut a = orch_app_with(ai);
    toggle(&mut a);
    recovered(&mut a, vec![stored(9, "port the parser")]);
    a
}

#[test]
fn the_pane_names_how_to_resume_or_discard() {
    let a = interrupted_app();
    let said = system(&a);
    assert!(
        said.iter()
            .any(|l| l.contains("Ctrl+Space r r") && l.contains("Ctrl+Space r d")),
        "{said:?}"
    );
}

// Scenario: Restart shows an interrupted run. Nothing runs until the choice.
#[test]
fn nothing_runs_until_the_user_chooses() {
    let mut a = interrupted_app();
    let effects = a.update(event(row(RUN, 1, WorkerStatus::Done)));
    assert!(turn_of(&effects).is_none());
}

#[test]
fn resume_starts_a_new_run_seeded_with_the_goal_and_closes_the_old_row() {
    let mut a = interrupted_app();
    let effects = keys(&mut a, "rr");
    let (request, orch) = turn_of(&effects).expect("an orchestrator turn");
    let text = &request.messages.last().unwrap().content;
    assert!(
        text.contains("port the parser") && text.contains("run 9"),
        "{text}"
    );
    assert!(
        text.contains("worker 1") && !text.contains("worker 2"),
        "{text}"
    );
    assert!(text.contains("read scope"), "{text}");
    // A new run: no continuation of an old one.
    assert!(orch.attach.is_none());
    // The old row stays interrupted until the new run has a worker.
    assert_eq!(closes(&effects, 9), 0);
    // Not offered twice here while the new run starts.
    assert!(turn_of(&keys(&mut a, "rr")).is_none());
    assert_eq!(a.notice(), Some("No interrupted run to resume or discard"));
}

// The new run stores the old goal, not the seed message around it.
#[test]
fn the_resumed_run_keeps_the_original_goal() {
    let mut a = interrupted_app();
    let effects = keys(&mut a, "rr");
    assert!(turn_of(&effects).is_some());
    let saved = a.update(event(row(RunId(900), 1, WorkerStatus::Running)));
    let goals: Vec<_> = saved
        .iter()
        .filter_map(|e| match e {
            Effect::Store(StoreCmd::SaveRun { run, .. }) => Some(run.goal.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(goals, ["port the parser"]);
    assert_eq!(effects_goal(&effects), Some("port the parser".to_owned()));
}

#[test]
fn resume_with_orch_off_says_how_to_turn_it_on_and_keeps_the_run() {
    let mut ai = orch_ai();
    ai.orchestrator.acknowledged = true;
    let mut a = orch_app_with(ai);
    toggle(&mut a);
    recovered(&mut a, vec![stored(9, "g")]);
    toggle(&mut a);
    let effects = keys(&mut a, "rr");
    assert!(turn_of(&effects).is_none() && closes(&effects, 9) == 0);
    assert!(
        a.notice().is_some_and(|n| n.contains("Ctrl+O")),
        "{:?}",
        a.notice()
    );
    toggle(&mut a);
    assert!(turn_of(&keys(&mut a, "rr")).is_some());
}

#[test]
fn resume_with_the_gate_off_says_so_and_changes_nothing() {
    let mut ai = orch_ai();
    ai.orchestrator.enabled = false;
    let mut a = orch_app_with(ai);
    let effects = keys(&mut a, "rr");
    assert!(effects.is_empty());
    assert!(
        a.notice()
            .is_some_and(|n| n.contains("orchestrator is off"))
    );
}

#[test]
fn resume_refuses_while_a_run_is_active_and_keeps_the_interrupted_one() {
    let (mut a, _) = running();
    recovered(&mut a, vec![stored(9, "g")]);
    let effects = keys(&mut a, "rr");
    assert!(turn_of(&effects).is_none() && closes(&effects, 9) == 0);
    assert!(
        a.notice().is_some_and(|n| n.contains("run")),
        "{:?}",
        a.notice()
    );
}

#[test]
fn discard_closes_the_row_and_starts_nothing() {
    let mut a = interrupted_app();
    let effects = keys(&mut a, "rd");
    assert!(turn_of(&effects).is_none());
    assert_eq!(closes(&effects, 9), 1);
    assert!(
        system(&a)
            .iter()
            .any(|l| l.contains("run 9") && l.contains("discarded"))
    );
    assert!(closes(&keys(&mut a, "rd"), 9) == 0);
    assert_eq!(a.notice(), Some("No interrupted run to resume or discard"));
}

#[test]
fn the_newest_interrupted_run_goes_first() {
    let mut a = interrupted_app();
    recovered(&mut a, vec![stored(12, "newer"), stored(9, "older")]);
    assert_eq!(closes(&keys(&mut a, "rd"), 12), 1);
    assert_eq!(closes(&keys(&mut a, "rd"), 9), 1);
}

#[test]
fn resume_never_wipes_a_draft_in_the_composer() {
    let mut a = interrupted_app();
    super::chat_live::type_text(&mut a, "half a thought");
    let effects = keys(&mut a, "rr");
    assert!(turn_of(&effects).is_none() && closes(&effects, 9) == 0);
    assert_eq!(chat_of(&a).composer().text(), "half a thought");
    assert!(a.notice().is_some_and(|n| n.contains("composer")));
}

fn effects_goal(effects: &[Effect]) -> Option<String> {
    turn_of(effects).map(|(_, orch)| orch.goal.clone())
}

fn saved_runs(effects: &[Effect]) -> Vec<RunRecord> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Store(StoreCmd::SaveRun { run, .. }) => Some((**run).clone()),
            _ => None,
        })
        .collect()
}

// 9.1c review W1: a run is resumed or discarded only in the folder it started
// in. A goal from another folder never leaves in this pane's turn.
#[test]
fn a_run_of_another_folder_is_refused_and_its_goal_never_sent() {
    let mut ai = orch_ai();
    ai.orchestrator.acknowledged = true;
    let mut a = orch_app_with(ai);
    toggle(&mut a);
    recovered(
        &mut a,
        vec![elsewhere(9, "secret plan", Some("/secret-proj"))],
    );
    for key in ["rr", "rd"] {
        let effects = keys(&mut a, key);
        assert!(effects.is_empty(), "{key}: {effects:?}");
        assert!(
            a.notice().is_some_and(|n| n.contains("other folder")),
            "{:?}",
            a.notice()
        );
        assert!(!format!("{effects:?}").contains("secret plan"));
    }
}

// A run stored before folders were kept has no folder: it is treated as
// another folder and never resumed from a pane that has one.
#[test]
fn a_run_with_no_stored_folder_is_not_resumable() {
    let mut ai = orch_ai();
    ai.orchestrator.acknowledged = true;
    let mut a = orch_app_with(ai);
    toggle(&mut a);
    recovered(&mut a, vec![elsewhere(9, "old goal", None)]);
    let effects = keys(&mut a, "rr");
    assert!(effects.is_empty());
    assert!(a.notice().is_some_and(|n| n.contains("other folder")));
}

// The start-up notice counts the runs of other folders without their goal.
#[test]
fn the_notice_counts_other_folders_runs_and_never_shows_their_goal() {
    let mut ai = orch_ai();
    ai.orchestrator.acknowledged = true;
    let mut a = orch_app_with(ai);
    toggle(&mut a);
    recovered(
        &mut a,
        vec![
            elsewhere(12, "secret plan", Some("/secret-proj")),
            stored(11, "mine"),
            elsewhere(9, "old secret", None),
        ],
    );
    let said = system(&a);
    let all = said.join("\n");
    assert!(!all.contains("secret"), "{said:?}");
    assert!(all.contains("run 11") && all.contains("mine"), "{said:?}");
    assert!(
        said.iter()
            .any(|l| l.contains("2 runs interrupted in other folders")),
        "{said:?}"
    );
    // Only the run of this folder is acted on.
    assert_eq!(closes(&keys(&mut a, "rd"), 11), 1);
    let discarded = keys(&mut a, "rd");
    assert!(discarded.is_empty(), "{discarded:?}");
}

// 9.1c review W2: the old row closes with the new run's first worker, in the
// same save, naming the run it supersedes and the folder the run started in.
#[test]
fn the_first_worker_of_the_resumed_run_closes_the_old_row() {
    let mut a = interrupted_app();
    keys(&mut a, "rr");
    let saved = saved_runs(&a.update(event(row(RunId(900), 1, WorkerStatus::Running))));
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].supersedes, Some(9));
    assert_eq!(saved[0].folder.as_deref(), Some(std::path::Path::new(HERE)));
    // Later saves of that run do not name it again.
    let more = saved_runs(&a.update(event(row(RunId(900), 2, WorkerStatus::Running))));
    assert_eq!(more[0].supersedes, None);
}

fn still_offered(a: &mut App) {
    let said = system(a);
    assert!(
        said.iter()
            .any(|l| l.contains("run 9 is still interrupted") && l.contains("Ctrl+Space r r")),
        "{said:?}"
    );
    let again = keys(a, "rr");
    assert!(turn_of(&again).is_some(), "offered again");
}

#[test]
fn a_resume_turn_that_fails_before_any_worker_leaves_the_run_offered() {
    use crate::core::ai::chat::engine::{ChatError, StreamEvent};
    let mut a = interrupted_app();
    let effects = keys(&mut a, "rr");
    let turn = match effects.iter().find_map(|e| match e {
        Effect::OrchestrateSend { turn, .. } => Some(*turn),
        _ => None,
    }) {
        Some(turn) => turn,
        None => panic!("no turn"),
    };
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Error(ChatError::Failed("could not start".into())),
    ));
    still_offered(&mut a);
}

#[test]
fn a_resume_turn_cancelled_before_any_worker_leaves_the_run_offered() {
    let mut a = interrupted_app();
    keys(&mut a, "rr");
    NOW_MS.with(|n| n.set(n.get() + 1_000));
    a.update(ctrl('c'));
    still_offered(&mut a);
}

#[test]
fn a_resume_turn_that_ends_with_no_worker_leaves_the_run_offered() {
    use crate::core::ai::chat::engine::{Finish, StreamEvent};
    let mut a = interrupted_app();
    let effects = keys(&mut a, "rr");
    let turn = effects
        .iter()
        .find_map(|e| match e {
            Effect::OrchestrateSend { turn, .. } => Some(*turn),
            _ => None,
        })
        .expect("a turn");
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Done(Finish::Stop),
    ));
    still_offered(&mut a);
}

// Re-review W2: the stream's `Done` and the run manager's worker rows travel
// on separate channels, so the first worker can arrive after the turn ended.
// The run is then put back, and that worker still closes it and withdraws it.
#[test]
fn a_worker_arriving_after_the_turns_done_still_closes_the_old_run() {
    use crate::core::ai::chat::engine::{Finish, StreamEvent};
    let mut a = interrupted_app();
    let effects = keys(&mut a, "rr");
    let turn = effects
        .iter()
        .find_map(|e| match e {
            Effect::OrchestrateSend { turn, .. } => Some(*turn),
            _ => None,
        })
        .expect("a turn");
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Done(Finish::Stop),
    ));
    let saved = saved_runs(&a.update(event(row(RunId(900), 1, WorkerStatus::Running))));
    assert_eq!(saved[0].supersedes, Some(9));
    // Closed in the store and no longer offered here.
    assert!(turn_of(&keys(&mut a, "rr")).is_none());
    assert_eq!(a.notice(), Some("No interrupted run to resume or discard"));
}

// The link does not outlive the next turn: a later, unrelated run of the pane
// never closes an interrupted run.
#[test]
fn a_later_fresh_run_does_not_close_a_run_whose_resume_was_abandoned() {
    use crate::core::ai::chat::engine::{Finish, StreamEvent};
    let mut a = interrupted_app();
    let effects = keys(&mut a, "rr");
    let turn = effects
        .iter()
        .find_map(|e| match e {
            Effect::OrchestrateSend { turn, .. } => Some(*turn),
            _ => None,
        })
        .expect("a turn");
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Done(Finish::Stop),
    ));
    super::chat_live::ask(&mut a, "something else");
    let saved = saved_runs(&a.update(event(row(RunId(901), 1, WorkerStatus::Running))));
    assert_eq!(saved[0].supersedes, None);
    // Still on the list: the refusal is the active run, not an empty list.
    keys(&mut a, "rr");
    assert_ne!(a.notice(), Some("No interrupted run to resume or discard"));
}
