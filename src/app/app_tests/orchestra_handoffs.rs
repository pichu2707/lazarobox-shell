//! The handoffs of a run in engram (agent-orchestrator D13, D17): saved under
//! a stable key, memory turned on for the run only, and the degraded mode.
//! Effects and transcript asserts only; the memory worker is not involved.

use super::chat_live::chat_id;
use super::chat_notes::{CWD, PROJECT, systems};
use super::orchestra_run::{RUN, event, handed_off, row};
use super::orchestra_send::{detected, orch_ai, prompt_ready};
use super::*;
use crate::app::handoff::RUN_EPOCH;
use crate::core::ai::{
    config::MemoryConfig,
    memory::{
        kind::MemKind,
        port::{MemCmd, MemReply, Reason, SaveId},
    },
    orchestra::{WorkerId, event::OrchestraEvent, ledger::Verdict, worker::WorkerStatus},
};

const MEMORY_ON: &str = "memory on for this run";
const NOT_PERSISTED: &str = "handoffs not persisted";
const NEEDS_NOTE: &str = "handoffs need the memory privacy note accepted";

/// An orchestrating pane opened from `/srv/app`, memory on or off. Off means
/// turned off after the privacy note was accepted.
fn app(memory: bool) -> App {
    app_with(memory, true)
}

fn app_with(enabled: bool, acknowledged: bool) -> App {
    let mut ai = orch_ai();
    ai.orchestrator.acknowledged = true;
    ai.memory = MemoryConfig {
        enabled,
        acknowledged,
        ..MemoryConfig::default()
    };
    let mut a = launched(80, 25)
        .with_ai(ai)
        .with_tool_engines(&["claude", "ollama"])
        .with_clock(mock_now);
    a.update(cwd_event(CWD));
    a.update(detected());
    press(&mut a, "c");
    a.update(prompt_ready(chat_id()));
    a.update(key_ctrl_o());
    a
}

fn key_ctrl_o() -> AppEvent {
    AppEvent::Key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL))
}

/// The project of the pane is known, as after its first memory check.
fn with_project(a: &mut App) {
    a.update(AppEvent::Memory(MemReply::Project {
        pane: chat_id(),
        epoch: 0,
        result: Ok(PROJECT.into()),
    }));
}

fn handoff_of(worker: u32) -> AppEvent {
    let OrchestraEvent::Handoff { handoff, .. } = handed_off(Verdict::OnCourse) else {
        unreachable!()
    };
    let mut handoff = handoff;
    handoff.unit = crate::core::ai::orchestra::ledger::Unit::Worker(WorkerId(worker));
    event(OrchestraEvent::Handoff { run: RUN, handoff })
}

fn saves(effects: &[Effect]) -> Vec<(SaveId, Option<String>)> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Memory(MemCmd::Save { id, topic_key, .. }) => Some((*id, topic_key.clone())),
            _ => None,
        })
        .collect()
}

fn count(a: &App, line: &str) -> usize {
    systems(a).iter().filter(|s| s.as_str() == line).count()
}

fn saved(a: &mut App, token: SaveId, key: &str) -> Vec<Effect> {
    a.update(AppEvent::Memory(MemReply::Saved {
        pane: chat_id(),
        id: token,
        project: PROJECT.into(),
        session: "lazarobox-0123456789abcdef0123456789abcdef".into(),
        observation_id: 1,
        topic_key: key.into(),
        title: "t".into(),
    }))
}

// Spec (Saved with a stable key): the handoff of a worker is saved under
// `orchestra/{run}/worker/{n}`, as an architecture note, and a repeated one
// uses the same key so engram updates the note.
#[test]
fn a_handoff_is_saved_under_a_stable_key_per_worker() {
    let mut a = app(true);
    with_project(&mut a);
    let first = a.update(handoff_of(2));
    let again = a.update(handoff_of(2));
    let keys: Vec<_> = saves(&first)
        .into_iter()
        .chain(saves(&again))
        .map(|(_, key)| key)
        .collect();
    assert_eq!(
        keys,
        [
            Some("orchestra/7/worker/2".to_owned()),
            Some("orchestra/7/worker/2".to_owned())
        ],
        "one key for the worker"
    );
    let Some(Effect::Memory(MemCmd::Save { note, project, .. })) = first.first() else {
        panic!("a save: {first:?}");
    };
    assert_eq!(note.kind, MemKind::Architecture);
    assert_eq!(project, PROJECT);
    assert!(note.content.contains("verdict: on_course"));
    assert!(note.content.contains("worker: 2"));
}

// Spec (Step handoffs follow the same stable-key rule; S5): a step's handoff
// is saved under `orchestra/{run}/step/{step}`, and an off-course one reaches
// the transcript naming the step.
#[test]
fn a_step_handoff_is_saved_under_its_step_key_and_off_course_is_said() {
    let mut a = app(true);
    with_project(&mut a);
    let OrchestraEvent::Handoff { mut handoff, .. } = handed_off(Verdict::OffCourse) else {
        unreachable!()
    };
    handoff.unit = crate::core::ai::orchestra::ledger::Unit::Step(
        crate::core::ai::orchestra::steps::Step::Explore,
    );
    let effects = a.update(event(OrchestraEvent::Handoff { run: RUN, handoff }));
    let keys: Vec<_> = saves(&effects).into_iter().map(|(_, key)| key).collect();
    assert_eq!(keys, [Some("orchestra/7/step/explore".to_owned())]);
    assert!(
        systems(&a)
            .iter()
            .any(|line| line.starts_with("handoff step explore off_course")),
        "{:?}",
        systems(&a)
    );
}

// The ledger and summary of a finished run, under their own key.
#[test]
fn the_closed_ledger_is_saved_under_the_run_ledger_key() {
    let mut a = app(true);
    with_project(&mut a);
    let effects = a.update(event(OrchestraEvent::Closed {
        run: RUN,
        ledger: "Goal: ship it".into(),
        summary: "all done".into(),
    }));
    let [(_, key)] = saves(&effects).try_into().expect("one save");
    assert_eq!(key.as_deref(), Some("orchestra/7/ledger"));
    let Some(Effect::Memory(MemCmd::Save { note, .. })) = effects.first() else {
        panic!("a save");
    };
    assert!(note.content.contains("Goal: ship it"));
    assert!(note.content.contains("Summary: all done"));
}

// Spec (Memory off is turned on for the run): the worker is asked to start for
// the run, the project is looked up, the handoff goes once it is known, the
// hint is shown once and the menu setting is untouched.
#[test]
fn memory_off_is_turned_on_for_the_run_only() {
    let mut a = app(false);
    let first = a.update(handoff_of(1));
    assert!(first.contains(&Effect::Memory(MemCmd::EnableForRun)));
    assert!(
        first
            .iter()
            .any(|e| matches!(e, Effect::Memory(MemCmd::Resolve { .. })))
    );
    assert!(saves(&first).is_empty(), "waiting for the project");
    let second = a.update(handoff_of(2));
    assert!(second.is_empty(), "the project is asked for once");
    let go = a.update(AppEvent::Memory(MemReply::Project {
        pane: chat_id(),
        epoch: RUN_EPOCH,
        result: Ok(PROJECT.into()),
    }));
    let keys: Vec<_> = saves(&go).into_iter().map(|(_, key)| key).collect();
    assert_eq!(
        keys,
        [
            Some("orchestra/7/worker/1".to_owned()),
            Some("orchestra/7/worker/2".to_owned())
        ]
    );
    assert_eq!(count(&a, MEMORY_ON), 1);
    assert_eq!(count(&a, NOT_PERSISTED), 0);
    assert!(!a.ai.memory.active(), "the setting stays off");
}

// Spec (Engram unreachable) and chat-pane (Hint once): the handoffs stay in the
// run, nothing is saved, `handoffs not persisted` is said once and the run
// goes on (its worker rows are still accepted).
#[test]
fn unreachable_engram_says_so_once_and_the_run_goes_on() {
    let mut a = app(false);
    a.update(handoff_of(1));
    a.update(AppEvent::Memory(MemReply::Unavailable {
        pane: Some(chat_id()),
        epoch: Some(RUN_EPOCH),
        reason: Reason::NoBinary,
    }));
    let later = [a.update(handoff_of(2)), a.update(handoff_of(3))];
    assert!(later.iter().all(|effects| saves(effects).is_empty()));
    assert_eq!(count(&a, NOT_PERSISTED), 1);
    assert_eq!(count(&a, MEMORY_ON), 1);
    // The run is not stopped: its next events are still taken.
    a.update(event(row(RUN, 4, WorkerStatus::Running)));
    assert_eq!(count(&a, NOT_PERSISTED), 1);
}

// A project that never answers does not leave the handoffs waiting for ever.
#[test]
fn a_project_that_never_comes_degrades_the_run_after_the_wait() {
    let mut a = app(false);
    at(&mut a, 0, handoff_of(1));
    assert_eq!(count(&a, NOT_PERSISTED), 0);
    assert!(a.memory_waiting(), "the timer stays armed");
    at(&mut a, 6_000, AppEvent::MemoryTick);
    assert_eq!(count(&a, NOT_PERSISTED), 1);
    assert!(!a.memory_waiting());
}

// Spec (Engram dies mid-run): the second save fails, the run goes on in
// memory with the hint once, the first handoff already stored is not touched.
#[test]
fn engram_dying_mid_run_degrades_without_stopping_the_run() {
    let mut a = app(true);
    with_project(&mut a);
    let [(first, key)] = saves(&a.update(handoff_of(1))).try_into().expect("a save");
    saved(&mut a, first, key.as_deref().unwrap());
    assert_eq!(count(&a, NOT_PERSISTED), 0);
    let [(second, _)] = saves(&a.update(handoff_of(2))).try_into().expect("a save");
    a.update(AppEvent::Memory(MemReply::SaveFailed {
        pane: chat_id(),
        id: second,
        reason: "engram did not answer".into(),
    }));
    assert_eq!(count(&a, NOT_PERSISTED), 1);
    let rest = a.update(handoff_of(3));
    assert!(saves(&rest).is_empty(), "nothing more is sent");
    a.update(event(OrchestraEvent::Ended { run: RUN }));
    assert_eq!(count(&a, NOT_PERSISTED), 1, "said once per run");
}

// A save that gets no answer in time counts as a failed save.
#[test]
fn a_save_that_gets_no_answer_degrades_the_run() {
    let mut a = app(true);
    with_project(&mut a);
    at(&mut a, 0, handoff_of(1));
    at(&mut a, 6_000, AppEvent::MemoryTick);
    assert_eq!(count(&a, NOT_PERSISTED), 1);
}

// The notes of a run do not show up as notes of the chat.
#[test]
fn a_saved_handoff_is_not_listed_as_a_chat_note() {
    let mut a = app(true);
    with_project(&mut a);
    let [(token, key)] = saves(&a.update(handoff_of(1))).try_into().expect("a save");
    saved(&mut a, token, key.as_deref().unwrap());
    let chat = a
        .panes
        .get(&chat_id())
        .and_then(PaneState::chat_state)
        .expect("a chat");
    assert!(
        !systems(&a).iter().any(|s| s.starts_with("saved note")),
        "no chat line"
    );
    assert_eq!(chat.pending_save_ids(), Vec::<SaveId>::new());
}

/// Another pane than the chat of the run, which holds the same token.
fn other() -> PaneId {
    PaneId::FIRST
}

// Review fix: a token is the chat's own counter, so two panes hold the same
// one. A reply or an expiry of another pane never reaches this run's store.
#[test]
fn a_run_save_is_matched_by_pane_and_token() {
    let mut a = app(true);
    with_project(&mut a);
    let [(token, key)] = saves(&a.update(handoff_of(1))).try_into().expect("a save");
    assert_eq!(token, SaveId(1));
    for reply in [
        MemReply::SaveFailed {
            pane: other(),
            id: token,
            reason: "x".into(),
        },
        MemReply::SaveUnavailable {
            pane: other(),
            id: token,
            reason: Reason::NoAnswer,
        },
    ] {
        a.update(AppEvent::Memory(reply));
    }
    a.run_save_lost(other(), token);
    assert_eq!(
        count(&a, NOT_PERSISTED),
        0,
        "another pane's save is not ours"
    );
    // The chat note of the other pane with the same token is not consumed
    // here either: its reply is left to the chat path.
    let (consumed, _) = a.on_run_reply(
        other(),
        &MemReply::Saved {
            pane: other(),
            id: token,
            project: PROJECT.into(),
            session: "s".into(),
            observation_id: 1,
            topic_key: "chat/decision/x".into(),
            title: "x".into(),
        },
    );
    assert!(!consumed);
    // Our own reply still lands.
    saved(&mut a, token, key.as_deref().unwrap());
    let chat = a
        .panes
        .get(&chat_id())
        .and_then(PaneState::chat_state)
        .unwrap();
    assert!(chat.pending_save_ids().is_empty());
}

// Review fix: an answer after the wait ran out is the run's, already degraded;
// it is not a chat note and says nothing.
#[test]
fn a_late_reply_of_an_expired_run_save_is_dropped_quietly() {
    let mut a = app(true);
    with_project(&mut a);
    let [(token, key)] = saves(&at(&mut a, 0, handoff_of(1)))
        .try_into()
        .expect("a save");
    at(&mut a, 6_000, AppEvent::MemoryTick);
    assert_eq!(count(&a, NOT_PERSISTED), 1);
    let effects = saved(&mut a, token, key.as_deref().unwrap());
    assert!(effects.is_empty(), "{effects:?}");
    assert!(!systems(&a).iter().any(|s| s.starts_with("saved note")));
    let chat = a
        .panes
        .get(&chat_id())
        .and_then(PaneState::chat_state)
        .unwrap();
    assert!(chat.session().is_none(), "no session adopted");
    assert_eq!(count(&a, NOT_PERSISTED), 1);
}

// Review fix: a run save adopts no session when chat memory is off.
#[test]
fn a_run_save_adopts_no_session_while_chat_memory_is_off() {
    let mut a = app(false);
    a.update(handoff_of(1));
    let go = a.update(AppEvent::Memory(MemReply::Project {
        pane: chat_id(),
        epoch: RUN_EPOCH,
        result: Ok(PROJECT.into()),
    }));
    let [(token, key)] = saves(&go).try_into().expect("a save");
    let effects = saved(&mut a, token, key.as_deref().unwrap());
    assert!(effects.is_empty(), "{effects:?}");
    let chat = a
        .panes
        .get(&chat_id())
        .and_then(PaneState::chat_state)
        .unwrap();
    assert!(chat.session().is_none());
}

// Review fix: a reply from before a folder change adopts nothing.
#[test]
fn a_stale_run_save_reply_adopts_no_session() {
    let mut a = app(true);
    with_project(&mut a);
    let [(token, key)] = saves(&a.update(handoff_of(1))).try_into().expect("a save");
    a.apply_folder(chat_id(), std::path::PathBuf::from("/srv/other"));
    saved(&mut a, token, key.as_deref().unwrap());
    let chat = a
        .panes
        .get(&chat_id())
        .and_then(PaneState::chat_state)
        .unwrap();
    assert!(chat.session().is_none());
}

// Privacy decision: only a memory the user accepted the note for can be turned
// on for a run.
#[test]
fn memory_never_acknowledged_is_not_turned_on_for_the_run() {
    for (enabled, acknowledged) in [(false, false), (true, false)] {
        let mut a = app_with(enabled, acknowledged);
        let first = a.update(handoff_of(1));
        let second = a.update(handoff_of(2));
        let all: Vec<_> = first.iter().chain(&second).collect();
        assert!(
            all.iter()
                .all(|e| !matches!(e, Effect::Memory(MemCmd::EnableForRun))),
            "{enabled} {acknowledged}: {all:?}"
        );
        assert!(
            all.iter()
                .all(|e| !matches!(e, Effect::Memory(MemCmd::Resolve { .. })) || enabled),
            "no lookup for a memory that was never accepted"
        );
        assert!(saves(&first).is_empty() && saves(&second).is_empty());
        assert_eq!(count(&a, NOT_PERSISTED), 1);
        assert_eq!(count(&a, NEEDS_NOTE), 1);
        assert_eq!(count(&a, MEMORY_ON), 0);
    }
}

// Review fix: the stores of a pane go with it, and an ended run stores nothing.
#[test]
fn closing_the_pane_drops_the_stores_of_its_runs() {
    let mut a = app(false);
    a.update(handoff_of(1));
    assert_eq!(a.run_stores.len(), 1);
    a.remove_pane(chat_id());
    assert!(a.run_stores.is_empty());
}

#[test]
fn an_ended_run_stores_nothing_more() {
    let mut a = app(true);
    with_project(&mut a);
    a.update(event(OrchestraEvent::Ended { run: RUN }));
    let effects = a.update(handoff_of(1));
    assert!(saves(&effects).is_empty());
    assert!(a.run_stores.is_empty());
}

// Review fix: the run's own project request has its own epoch; the chat's
// replies (its context, an old folder) neither resolve nor degrade the run.
#[test]
fn only_the_runs_own_project_reply_resolves_it() {
    let mut a = app(true);
    let effects = a.update(handoff_of(1));
    assert!(effects.iter().any(|e| matches!(
        e,
        Effect::Memory(MemCmd::Resolve { epoch, .. }) if *epoch == RUN_EPOCH
    )));
    let chat_reply = a.update(AppEvent::Memory(MemReply::Project {
        pane: chat_id(),
        epoch: 0,
        result: Ok(PROJECT.into()),
    }));
    assert!(
        saves(&chat_reply).is_empty(),
        "the chat's reply is not ours"
    );
    a.update(AppEvent::Memory(MemReply::Unavailable {
        pane: Some(chat_id()),
        epoch: Some(0),
        reason: Reason::NoAnswer,
    }));
    assert_eq!(count(&a, NOT_PERSISTED), 0);
    let own = a.update(AppEvent::Memory(MemReply::Project {
        pane: chat_id(),
        epoch: RUN_EPOCH,
        result: Ok(PROJECT.into()),
    }));
    assert_eq!(saves(&own).len(), 1);
}

// Re-review fix: a folder change clears the chat's pending saves, so a run
// save in flight must expire on its own. The user hears it once, and the
// store of an ended run goes.
#[test]
fn a_run_save_in_flight_over_a_folder_change_still_expires() {
    let mut a = app(true);
    with_project(&mut a);
    at(&mut a, 0, handoff_of(1));
    a.apply_folder(chat_id(), std::path::PathBuf::from("/srv/other"));
    a.update(event(OrchestraEvent::Ended { run: RUN }));
    assert_eq!(a.run_stores.len(), 1, "the save still awaits its answer");
    assert!(a.memory_waiting(), "the timer stays armed");
    at(&mut a, 6_000, AppEvent::MemoryTick);
    assert_eq!(count(&a, NOT_PERSISTED), 1);
    assert!(a.run_stores.is_empty(), "the ended run's store is dropped");
    assert!(!a.memory_waiting());
    at(&mut a, 12_000, AppEvent::MemoryTick);
    assert_eq!(count(&a, NOT_PERSISTED), 1, "said once");
}
