//! The step trail of a run (#771): the coordinator's tool calls and the
//! handoffs, saved with the run and shown when it stops.

use super::chat_live::ask;
use super::chat_live::{chat_id, chat_of};
use super::orchestra_run::{RUN, event, handed_off, next_turn, row, running};
use super::*;
use crate::core::ai::orchestra::RunId;
use crate::core::ai::{
    chat::{
        Entry,
        engine::{CallId, ChatError, Finish, ToolStatus},
        store::StoreReply,
    },
    orchestra::{
        event::OrchestraEvent,
        ledger::Verdict,
        record::{RunPhase, RunRecord},
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

fn system(a: &App) -> Vec<String> {
    (chat_of(a).entries().iter())
        .filter_map(|e| match e {
            Entry::System(l) => Some(l.clone()),
            _ => None,
        })
        .collect()
}

fn call(a: &mut App, turn: u64, id: u64, name: &str, summary: &str) -> Vec<Effect> {
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::ToolCall {
            id: CallId(id),
            name: name.into(),
            summary: summary.into(),
            approval: None,
        },
    ))
}

fn result(a: &mut App, turn: u64, id: u64, status: ToolStatus, summary: &str) -> Vec<Effect> {
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::ToolResult {
            id: CallId(id),
            status,
            summary: summary.into(),
        },
    ))
}

#[test]
fn a_stopped_run_shows_the_steps_it_took() {
    let (mut a, turn) = running();
    call(&mut a, turn, 1, "spawn", "code worker");
    result(
        &mut a,
        turn,
        1,
        ToolStatus::Denied,
        "step authorize is open",
    );
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    a.update(event(handed_off(Verdict::OffCourse)));
    a.update(event(OrchestraEvent::Stopped {
        run: RUN,
        reason: "run stopped: it took max_turns (6) orchestrator turns".to_owned(),
    }));
    let said = system(&a);
    let at = said
        .iter()
        .position(|l| l == "step trail:")
        .expect("{said:?}");
    assert_eq!(
        said[at + 1],
        "  spawn code worker -> refused: step authorize is open"
    );
    assert!(
        said[at + 2].starts_with("  handoff worker 2 off_course:"),
        "{said:?}"
    );
}

#[test]
fn the_trail_is_saved_with_the_run_as_steps_happen() {
    let (mut a, turn) = running();
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    call(&mut a, turn, 1, "read_file", "src/lib.rs");
    let effects = result(&mut a, turn, 1, ToolStatus::Ok, "");
    let [saved] = saves(&effects).try_into().expect("one save");
    assert_eq!((saved.id, saved.phase), (RUN.0, RunPhase::Live));
    assert!(saved.workers.is_empty());
    assert_eq!(
        saved.trail.as_deref(),
        Some("cut 0\nread_file src/lib.rs -> ok")
    );
}

#[test]
fn steps_before_the_first_worker_join_the_first_save() {
    let (mut a, turn) = running();
    call(&mut a, turn, 1, "list_panes", "");
    result(&mut a, turn, 1, ToolStatus::Ok, "");
    let first = a.update(event(row(RUN, 1, WorkerStatus::Running)));
    let saved = saves(&first).pop().expect("a save");
    assert_eq!(saved.trail.as_deref(), Some("cut 0\nlist_panes -> ok"));
}

#[test]
fn a_cancelled_run_shows_its_trail() {
    let (mut a, turn) = running();
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    call(&mut a, turn, 1, "wait", "worker 1");
    a.update(super::chat_notes::ctrl('c'));
    a.update(event(OrchestraEvent::Ended { run: RUN }));
    let said = system(&a);
    assert!(
        said.contains(&"  wait worker 1 -> no result".to_owned()),
        "{said:?}"
    );
}

#[test]
fn a_clean_finish_shows_its_trail_and_saves_it() {
    let (mut a, turn) = running();
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    call(&mut a, turn, 1, "read_file", "src/lib.rs");
    result(&mut a, turn, 1, ToolStatus::Ok, "");
    let ended = a.update(event(OrchestraEvent::Ended { run: RUN }));
    let said = system(&a);
    let at = said
        .iter()
        .position(|l| l == "step trail:")
        .expect("{said:?}");
    assert_eq!(said[at + 1], "  read_file src/lib.rs -> ok");
    let [closed] = saves(&ended).try_into().expect("one save");
    assert_eq!(closed.phase, RunPhase::Ended);
    assert_eq!(
        closed.trail.as_deref(),
        Some("cut 0\nread_file src/lib.rs -> ok")
    );
}

#[test]
fn a_clean_finish_with_no_step_says_nothing() {
    let (mut a, _) = running();
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    a.update(event(OrchestraEvent::Ended { run: RUN }));
    assert!(!system(&a).iter().any(|l| l.starts_with("step trail")));
}

#[test]
fn a_stopped_run_shows_its_trail_once_when_it_ends_after() {
    let (mut a, turn) = running();
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    call(&mut a, turn, 1, "wait", "worker 1");
    a.update(event(OrchestraEvent::Stopped {
        run: RUN,
        reason: "run stopped: it took max_turns (6) orchestrator turns".to_owned(),
    }));
    a.update(event(OrchestraEvent::Ended { run: RUN }));
    let shown = system(&a).iter().filter(|l| *l == "step trail:").count();
    assert_eq!(shown, 1);
}

#[test]
fn the_end_of_a_replaced_run_does_not_show_its_trail_in_the_new_one() {
    let (mut a, turn) = running();
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    call(&mut a, turn, 1, "wait", "worker 1");
    let newer = RunId(RUN.0 + 1);
    a.update(event(row(newer, 1, WorkerStatus::Running)));
    a.update(event(OrchestraEvent::Ended { run: RUN }));
    assert!(!system(&a).iter().any(|l| l.starts_with("step trail")));
}

#[test]
fn an_engine_error_with_the_run_open_shows_the_trail() {
    let (mut a, turn) = running();
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    call(&mut a, turn, 1, "read_file", "src/lib.rs");
    result(&mut a, turn, 1, ToolStatus::Error, "no such file");
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Error(ChatError::Failed("boom".into())),
    ));
    let said = system(&a);
    let at = said
        .iter()
        .position(|l| l == "step trail:")
        .expect("{said:?}");
    assert_eq!(
        said[at + 1],
        "  read_file src/lib.rs -> error: no such file"
    );
}

#[test]
fn an_engine_error_with_no_step_says_nothing_more() {
    let (mut a, turn) = running();
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Error(ChatError::Failed("boom".into())),
    ));
    assert!(!system(&a).iter().any(|l| l.starts_with("step trail")));
}

fn fail(a: &mut App, turn: u64) {
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Error(ChatError::Failed("boom".into())),
    ));
}

fn trails_listed(a: &App) -> usize {
    system(a).iter().filter(|l| *l == "step trail:").count()
}

#[test]
fn an_engine_error_that_ends_the_run_lists_the_trail_once() {
    let (mut a, turn) = running();
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    call(&mut a, turn, 1, "read_file", "src/lib.rs");
    result(&mut a, turn, 1, ToolStatus::Error, "no such file");
    fail(&mut a, turn);
    a.update(event(OrchestraEvent::Ended { run: RUN }));
    assert_eq!(trails_listed(&a), 1, "{:?}", system(&a));
}

#[test]
fn an_engine_error_then_a_later_clean_end_lists_the_trail_once() {
    let (mut a, turn) = running();
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    call(&mut a, turn, 1, "read_file", "src/lib.rs");
    result(&mut a, turn, 1, ToolStatus::Ok, "");
    fail(&mut a, turn);
    assert_eq!(trails_listed(&a), 1);
    a.update(event(OrchestraEvent::Ended { run: RUN }));
    assert_eq!(trails_listed(&a), 1, "{:?}", system(&a));
}

#[test]
fn an_engine_error_before_any_run_is_named_lists_the_held_steps_once() {
    let (mut a, turn) = running();
    call(&mut a, turn, 1, "list_panes", "");
    result(&mut a, turn, 1, ToolStatus::Ok, "");
    fail(&mut a, turn);
    assert_eq!(trails_listed(&a), 1, "{:?}", system(&a));
    a.update(event(OrchestraEvent::Ended { run: RUN }));
    assert_eq!(trails_listed(&a), 1, "{:?}", system(&a));
}

#[test]
fn a_failed_save_says_the_trail_may_be_missing() {
    let (mut a, _) = running();
    a.update(AppEvent::Store(StoreReply::RunsFailed {
        pane: chat_id(),
        saving: true,
    }));
    assert!(
        system(&a)
            .iter()
            .any(|l| l.contains("step trail") && l.contains("could not save")),
        "{:?}",
        system(&a)
    );
}

#[test]
fn an_interrupted_run_names_its_last_steps_after_a_restart() {
    let (mut a, _) = running();
    let mut run = RunRecord {
        id: 9,
        goal: "goal".into(),
        phase: RunPhase::Interrupted,
        workers: vec![],
        folder: None,
        supersedes: None,
        trail: Some("cut 0\nexec cargo test -> error: exit 101".into()),
    };
    run.folder = chat_of(&a).origin_cwd().map(Into::into);
    a.update(AppEvent::Store(StoreReply::RunsRecovered {
        pane: chat_id(),
        runs: vec![run],
    }));
    assert!(
        system(&a)
            .iter()
            .any(|l| l == "  exec cargo test -> error: exit 101"),
        "{:?}",
        system(&a)
    );
}

fn trails(effects: &[Effect]) -> Vec<(u64, String)> {
    saves(effects)
        .into_iter()
        .filter_map(|r| Some((r.id, r.trail?)))
        .collect()
}

// A run with no worker, stopped at the cap, is still stored with its trail.
#[test]
fn a_run_with_no_worker_is_stored_with_its_trail_when_it_stops() {
    let (mut a, turn) = running();
    call(&mut a, turn, 1, "wait", "worker 9");
    result(&mut a, turn, 1, ToolStatus::Error, "no such worker");
    let mut all = a.update(event(OrchestraEvent::Stopped {
        run: RUN,
        reason: "run stopped: it took max_turns (6) orchestrator turns".to_owned(),
    }));
    all.extend(a.update(event(OrchestraEvent::Ended { run: RUN })));
    let stored = saves(&all);
    let last = stored.last().expect("a save");
    assert_eq!((last.id, last.phase), (RUN.0, RunPhase::Ended));
    assert!(
        last.trail
            .as_deref()
            .unwrap_or("")
            .contains("wait worker 9 -> error: no such worker"),
        "{last:?}"
    );
    assert!(
        system(&a).iter().any(|l| l == "step trail:"),
        "{:?}",
        system(&a)
    );
}

// Another run's handoff never lands in this run's trail, nor the reverse.
#[test]
fn a_handoff_of_another_run_stays_out_of_this_runs_trail() {
    let (mut a, turn) = running();
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    call(&mut a, turn, 1, "wait", "worker 1");
    let OrchestraEvent::Handoff { mut handoff, .. } = handed_off(Verdict::OnCourse) else {
        unreachable!()
    };
    handoff.run = RunId(8);
    let other = a.update(event(OrchestraEvent::Handoff {
        run: RunId(8),
        handoff,
    }));
    let after = result(&mut a, turn, 1, ToolStatus::Ok, "");
    let of_other = trails(&other);
    assert!(
        of_other
            .iter()
            .all(|(id, t)| *id == 8 && !t.contains("wait")),
        "{of_other:?}"
    );
    let mine = trails(&after);
    assert!(
        mine.iter()
            .all(|(id, t)| *id == RUN.0 && !t.contains("handoff")),
        "{mine:?}"
    );
    assert!(
        mine.iter().any(|(_, t)| t.contains("wait worker 1 -> ok")),
        "{mine:?}"
    );
}

fn next_turn_of(effects: &[Effect]) -> u64 {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::OrchestrateSend { turn, .. } => Some(*turn),
            _ => None,
        })
        .expect("an orchestrator turn")
}

// The new goal's steps are its own: never the previous run's, never lost.
#[test]
fn a_replacing_goal_keeps_its_steps_apart_from_the_run_it_replaces() {
    let (mut a, turn) = running();
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    call(&mut a, turn, 1, "wait", "a_step");
    result(&mut a, turn, 1, ToolStatus::Ok, "");
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Done(Finish::Stop),
    ));
    let second = next_turn_of(&ask(&mut a, "second goal"));
    call(&mut a, second, 1, "list_panes", "b_step");
    let mid = result(&mut a, second, 1, ToolStatus::Ok, "");
    let b = RunId(8);
    let mut all = mid;
    all.extend(a.update(event(row(b, 1, WorkerStatus::Running))));
    all.extend(a.update(event(OrchestraEvent::Ended { run: RUN })));
    let stored = saves(&all);
    for r in stored.iter().filter(|r| r.id == RUN.0) {
        let trail = r.trail.clone().unwrap_or_default();
        assert!(!trail.contains("b_step"), "{r:?}");
        assert_eq!(r.goal, "plan it", "{r:?}");
    }
    let mine = stored.iter().rfind(|r| r.id == b.0).expect("b is stored");
    assert!(
        mine.trail.as_deref().unwrap_or("").contains("b_step"),
        "{mine:?}"
    );
    assert_eq!(mine.goal, "second goal");
}

#[test]
fn steps_of_a_run_that_never_started_are_not_dropped_silently() {
    let (mut a, turn) = running();
    call(&mut a, turn, 1, "wait", "x");
    result(&mut a, turn, 1, ToolStatus::Ok, "");
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Done(Finish::Stop),
    ));
    let second = next_turn_of(&ask(&mut a, "another goal"));
    assert!(
        !system(&a).iter().any(|l| l.contains("discarded")),
        "held for its run"
    );
    a.update(AppEvent::Chat(
        chat_id(),
        second,
        StreamEvent::Done(Finish::Stop),
    ));
    ask(&mut a, "a third goal");
    assert!(
        system(&a)
            .iter()
            .any(|l| l == "step trail of the unstarted run discarded (1 step)"),
        "{:?}",
        system(&a)
    );
}

// Done, a new goal, and only then the end of the coordinator-only run: its
// steps and its goal are its own.
#[test]
fn the_end_of_a_run_after_the_next_goal_keeps_its_steps_and_goal() {
    let (mut a, turn) = running();
    call(&mut a, turn, 1, "wait", "a_step");
    result(&mut a, turn, 1, ToolStatus::Ok, "");
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Done(Finish::Stop),
    ));
    ask(&mut a, "second goal");
    let ended = a.update(event(OrchestraEvent::Ended { run: RUN }));
    let stored = saves(&ended);
    let last = stored.iter().rfind(|r| r.id == RUN.0).expect("stored");
    assert_eq!(last.goal, "plan it", "{last:?}");
    assert!(
        last.trail.as_deref().unwrap_or("").contains("a_step"),
        "{last:?}"
    );
    assert!(
        !system(&a).iter().any(|l| l.contains("discarded")),
        "{:?}",
        system(&a)
    );
}

// A worker row of a run adopts the steps and both saves reach the store.
#[test]
fn a_worker_row_that_adopts_steps_saves_the_trail_and_the_worker() {
    let (mut a, turn) = running();
    call(&mut a, turn, 1, "list_panes", "");
    result(&mut a, turn, 1, ToolStatus::Ok, "");
    let effects = a.update(event(row(RUN, 1, WorkerStatus::Running)));
    let stored = saves(&effects);
    assert!(stored.iter().any(|r| !r.workers.is_empty()), "{stored:?}");
    assert!(
        stored
            .iter()
            .any(|r| r.trail.as_deref().unwrap_or("").contains("list_panes")),
        "{stored:?}"
    );
}

fn done(a: &mut App, turn: u64) {
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Done(Finish::Stop),
    ));
}

fn stored_of(effects: &[Effect], run: u64) -> RunRecord {
    saves(effects)
        .into_iter()
        .rfind(|r| r.id == run)
        .expect("stored")
}

// S1: a turn that only looked around is not the run the next goal starts.
#[test]
fn a_chat_only_turn_does_not_give_its_steps_or_goal_to_the_next_run() {
    let (mut a, turn) = running();
    call(&mut a, turn, 1, "list_panes", "first_turn_step");
    result(&mut a, turn, 1, ToolStatus::Ok, "");
    done(&mut a, turn);
    let second = next_turn_of(&ask(&mut a, "second goal"));
    call(&mut a, second, 1, "list_panes", "second_turn_step");
    result(&mut a, second, 1, ToolStatus::Ok, "");
    let started = a.update(event(row(RunId(8), 1, WorkerStatus::Running)));
    let b = stored_of(&started, 8);
    let trail = b.trail.clone().unwrap_or_default();
    assert!(
        trail.contains("second_turn_step") && !trail.contains("first_turn_step"),
        "{b:?}"
    );
    assert_eq!(b.goal, "second goal");
}

// S2: the end of an earlier worker-less run does not name the next turn's run.
#[test]
fn the_end_of_an_earlier_run_does_not_take_the_next_turns_steps() {
    let (mut a, turn) = running();
    done(&mut a, turn);
    let second = next_turn_of(&ask(&mut a, "second goal"));
    a.update(event(OrchestraEvent::Ended { run: RUN }));
    call(&mut a, second, 1, "list_panes", "second_turn_step");
    result(&mut a, second, 1, ToolStatus::Ok, "");
    let started = a.update(event(row(RunId(8), 1, WorkerStatus::Running)));
    let b = stored_of(&started, 8);
    assert!(
        b.trail
            .as_deref()
            .unwrap_or("")
            .contains("second_turn_step"),
        "{b:?}"
    );
}

// S3: a start that failed leaves no steps for the next run.
#[test]
fn a_failed_start_leaves_no_steps_for_the_next_run() {
    let (mut a, turn) = running();
    call(&mut a, turn, 1, "list_panes", "failed_turn_step");
    result(&mut a, turn, 1, ToolStatus::Ok, "");
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Error(ChatError::Failed("boom".into())),
    ));
    let second = next_turn_of(&ask(&mut a, "second goal"));
    call(&mut a, second, 1, "list_panes", "second_turn_step");
    result(&mut a, second, 1, ToolStatus::Ok, "");
    let started = a.update(event(row(RunId(8), 1, WorkerStatus::Running)));
    let b = stored_of(&started, 8);
    let trail = b.trail.clone().unwrap_or_default();
    assert!(
        trail.contains("second_turn_step") && !trail.contains("failed_turn_step"),
        "{b:?}"
    );
}

// A later turn that names no run is not the run the pane still has: its
// error says nothing of that run, whose own end lists the trail once.
#[test]
fn an_unnamed_turns_error_does_not_list_or_silence_the_panes_live_run() {
    let (mut a, turn) = running();
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    call(&mut a, turn, 1, "read_file", "src/lib.rs");
    result(&mut a, turn, 1, ToolStatus::Ok, "");
    done(&mut a, turn);
    let second = next_turn_of(&ask(&mut a, "something else"));
    fail(&mut a, second);
    assert_eq!(trails_listed(&a), 0, "{:?}", system(&a));
    a.update(event(OrchestraEvent::Ended { run: RUN }));
    assert_eq!(trails_listed(&a), 1, "{:?}", system(&a));
}

// With the run alive after the error, the steps taken after it are listed
// at the end; a run with nothing new says nothing a second time.
#[test]
fn steps_after_an_engine_error_are_listed_when_the_run_ends() {
    let (mut a, turn) = running();
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    call(&mut a, turn, 1, "read_file", "src/lib.rs");
    result(&mut a, turn, 1, ToolStatus::Ok, "");
    fail(&mut a, turn);
    assert_eq!(trails_listed(&a), 1);
    a.update(event(handed_off(Verdict::OnCourse)));
    a.update(event(OrchestraEvent::Ended { run: RUN }));
    let said = system(&a);
    assert_eq!(trails_listed(&a), 2, "{said:?}");
    let last = said.iter().rposition(|l| l == "step trail:").unwrap();
    assert!(
        said[last + 1..].iter().any(|l| l.contains("handoff")),
        "{said:?}"
    );
}

#[test]
fn an_engine_error_then_an_immediate_end_lists_the_trail_once() {
    let (mut a, turn) = running();
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    call(&mut a, turn, 1, "read_file", "src/lib.rs");
    result(&mut a, turn, 1, ToolStatus::Ok, "");
    fail(&mut a, turn);
    a.update(event(OrchestraEvent::Ended { run: RUN }));
    assert_eq!(trails_listed(&a), 1, "{:?}", system(&a));
}

#[test]
fn an_engine_error_before_a_continuation_lists_the_trail_once_at_the_end() {
    let (mut a, turn) = running();
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    call(&mut a, turn, 1, "read_file", "src/lib.rs");
    result(&mut a, turn, 1, ToolStatus::Ok, "");
    a.update(event(OrchestraEvent::Continue(next_turn(2))));
    fail(&mut a, turn);
    assert_eq!(trails_listed(&a), 0, "{:?}", system(&a));
    a.update(event(OrchestraEvent::Ended { run: RUN }));
    assert_eq!(trails_listed(&a), 1, "{:?}", system(&a));
}

#[test]
fn a_user_interrupt_lists_the_trail_once() {
    let (mut a, turn) = running();
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    call(&mut a, turn, 1, "read_file", "src/lib.rs");
    result(&mut a, turn, 1, ToolStatus::Ok, "");
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Error(ChatError::Interrupted),
    ));
    assert_eq!(trails_listed(&a), 0, "{:?}", system(&a));
    a.update(event(OrchestraEvent::Ended { run: RUN }));
    assert_eq!(trails_listed(&a), 1, "{:?}", system(&a));
}
