//! The menu ceiling of a worker's toolset (agent-orchestrator, "Menu ceiling
//! for worker toolsets [W2]"): what the AI menu allows caps what a spawn is
//! offered, and never widens it.

use std::time::{Duration, Instant};

use super::ledger::{Judgement, Verdict};
use super::run::{Refusal, Roster, Run, RunState, UserChoice, WorkerEvent};
use super::steps::{Grant, Mark, Step, WRITE_OFF};
use super::worker::{ErrorClass, WorkerStatus};
use super::{RunId, Toolset, WorkerId};
use crate::core::ai::chat::engine::Availability;
use crate::core::ai::config::Task;
use crate::core::ai::tools::registry::{Family, FamilySet};

const EVERY_FAMILY: [Family; 9] = [
    Family::Read,
    Family::Search,
    Family::Write,
    Family::Exec,
    Family::Git,
    Family::GitWrite,
    Family::Pane,
    Family::Orchestra,
    Family::Github,
];

const EVERY_TOOLSET: [Toolset; 3] = [Toolset::ReadOnly, Toolset::Code, Toolset::Orchestrator];

/// The menus the ceiling can take: Write and Exec each on or off, with the
/// rest of the families on.
fn menus() -> Vec<FamilySet> {
    let rest = FamilySet::ALL;
    let mut out = Vec::new();
    for write in [false, true] {
        for exec in [false, true] {
            let mut menu = rest;
            if !write {
                menu = menu.without(Family::Write);
            }
            if !exec {
                menu = menu.without(Family::Exec);
            }
            out.push(menu);
        }
    }
    out
}

fn ready() -> Availability {
    Availability::Ready {
        engine: "ollama".to_owned(),
        model: Some("qwen".to_owned()),
    }
}

fn full_roster() -> Roster {
    Roster::new(
        Task::SET_ALL
            .into_iter()
            .map(|task| (task, ready()))
            .collect(),
    )
}

fn write_off() -> FamilySet {
    FamilySet::ALL.without(Family::Write)
}

fn run_with(menu: FamilySet) -> Run {
    let mut run = Run::new(RunId(1), full_roster(), Duration::from_secs(600)).with_ceiling(menu);
    run.close_steps_for_tests();
    run
}

// Spec (agent-orchestrator, "The menu never widens [W2, auto]"): whatever the
// menu holds, the capped set is a subset of the role's own set.
#[test]
fn the_cap_never_widens_a_toolset() {
    for toolset in EVERY_TOOLSET {
        for menu in menus() {
            let capped = toolset.capped(menu);
            for family in EVERY_FAMILY {
                assert!(
                    !capped.contains(family) || toolset.families().contains(family),
                    "{toolset:?} {family:?}"
                );
            }
        }
    }
}

// Spec ("Ceiling limits a writer"): Write and Exec go when the menu lacks
// them, and nothing else goes.
#[test]
fn the_cap_removes_write_and_exec_and_nothing_else() {
    for toolset in EVERY_TOOLSET {
        for menu in menus() {
            let capped = toolset.capped(menu);
            for family in EVERY_FAMILY {
                let held = toolset.families().contains(family);
                let gone = matches!(family, Family::Write | Family::Exec) && !menu.contains(family);
                assert_eq!(
                    capped.contains(family),
                    held && !gone,
                    "{toolset:?} {family:?}"
                );
            }
        }
    }
}

// Spec ("Role toolsets [auto]"): with every family on, a role gets exactly
// its own set, so the read-only kinds and the coordinator never change.
#[test]
fn a_full_menu_leaves_every_toolset_as_it_was() {
    for toolset in EVERY_TOOLSET {
        assert_eq!(toolset.capped(FamilySet::ALL), toolset.families());
    }
    let read_only = Toolset::ReadOnly.families();
    for menu in menus() {
        assert_eq!(Toolset::ReadOnly.capped(menu), read_only);
        assert_eq!(
            Toolset::Orchestrator.capped(menu),
            Toolset::Orchestrator.families()
        );
    }
}

// D25: a spec writes when its own set holds Write, not when its kind is `code`.
#[test]
fn a_spec_carries_the_families_it_was_capped_to() {
    let mut run = run_with(FamilySet::ALL);
    let now = Instant::now();
    let code = run.spawn(now, Task::Code, "write").unwrap();
    let review = run.spawn(now, Task::Review, "look").unwrap();
    let (code, review) = (run.spec(code).unwrap(), run.spec(review).unwrap());
    assert_eq!(code.families, Toolset::Code.families());
    assert!(code.writes());
    assert_eq!(review.families, Toolset::ReadOnly.families());
    assert!(!review.writes());
}

// Spec ("Code worker refused with Write off", "A refusal before a worker does
// not pause the run [W2, auto]").
#[test]
fn a_code_spawn_with_write_off_makes_no_worker_and_does_not_pause_the_run() {
    let mut run = run_with(FamilySet::ALL.without(Family::Write));
    let now = Instant::now();
    assert_eq!(run.spawn(now, Task::Code, "write"), Err(Refusal::WriteOff));
    assert_eq!(run.worker_count(), 0);
    assert_eq!(run.state(), &super::run::RunState::Open);
    // The next spawn behaves as if nothing happened.
    let id = run.spawn(now, Task::Review, "look").unwrap();
    assert_eq!(run.status(id), Some(WorkerStatus::Running));
    assert!(!run.spec(id).unwrap().writes());
}

// A toggle change applies from the next spawn; a started worker keeps its spec.
#[test]
fn a_new_ceiling_applies_to_the_next_spawn_only() {
    let mut run = run_with(FamilySet::ALL);
    let now = Instant::now();
    let first = run.spawn(now, Task::Code, "one").unwrap();
    run.set_ceiling(FamilySet::ALL.without(Family::Write));
    assert!(run.spec(first).unwrap().writes());
    assert_eq!(run.spawn(now, Task::Code, "two"), Err(Refusal::WriteOff));
    run.set_ceiling(FamilySet::ALL);
    let third = run.spawn(now, Task::Code, "three").unwrap();
    assert!(run.spec(third).unwrap().writes());
}

// A read-only goal says so before the menu is blamed: turning Write on would
// not help.
#[test]
fn a_read_only_goal_is_refused_as_read_only_even_with_write_off() {
    let mut run = run_with(FamilySet::ALL.without(Family::Write));
    run.handoff_step(
        Step::Authorize,
        Mark::Grant(Grant::ReadOnly),
        Judgement {
            verdict: Verdict::OnCourse,
            reason: "fits".to_owned(),
            done: "explain".to_owned(),
            left: String::new(),
            decisions: String::new(),
        },
    )
    .expect("authorized");
    assert_eq!(
        run.spawn(Instant::now(), Task::Code, "x"),
        Err(Refusal::ReadOnly)
    );
}

#[test]
fn the_refusal_text_names_the_toggle_and_says_no_worker_started() {
    assert!(WRITE_OFF.contains("Write"), "{WRITE_OFF}");
    assert!(WRITE_OFF.contains("not started"), "{WRITE_OFF}");
    assert!(WRITE_OFF.contains("/write"), "{WRITE_OFF}");
    assert!(WRITE_OFF.contains("this chat"), "{WRITE_OFF}");
    assert!(WRITE_OFF.is_ascii());
}

fn failed() -> WorkerEvent {
    WorkerEvent::Failed {
        class: ErrorClass::Other,
        text: "x".to_owned(),
    }
}

/// Fails the running worker `id` and checks it, and any queued sibling named
/// in `also`, ended refused with the Write-off text: never read-only.
fn assert_refused_for_write(run: &Run, ids: &[WorkerId]) {
    for id in ids {
        assert_eq!(run.status(*id), Some(WorkerStatus::Refused), "{id:?}");
        let error = run.result(*id).and_then(|r| r.error.as_ref());
        assert!(
            error.is_some_and(|(_, text)| text.contains(WRITE_OFF)),
            "{id:?} {error:?}"
        );
    }
}

// A worker re-resolved after the menu lost Write must not run read-only
// without a word: relaunch.
#[test]
fn a_relaunched_code_worker_is_refused_when_write_went_off() {
    let (now, mut run) = (Instant::now(), run_with(FamilySet::ALL));
    let id = run.spawn(now, Task::Code, "write").unwrap();
    run.on_event(now, id, failed());
    run.set_ceiling(write_off());
    run.decide(now, UserChoice::Relaunch(full_roster()))
        .unwrap();
    assert_refused_for_write(&run, &[id]);
}

// ... and the workers queued behind it, re-resolved with it.
#[test]
fn queued_code_workers_are_refused_with_it_when_write_went_off() {
    let (now, mut run) = (Instant::now(), run_with(FamilySet::ALL));
    let first = run.spawn(now, Task::Code, "one").unwrap();
    let second = run.spawn(now, Task::Code, "two").unwrap();
    assert_eq!(run.status(second), Some(WorkerStatus::Queued));
    run.on_event(now, first, failed());
    run.set_ceiling(write_off());
    run.decide(now, UserChoice::Relaunch(full_roster()))
        .unwrap();
    assert_refused_for_write(&run, &[first, second]);
}

// ... and a retry of a worker that was refused before it started (its engine
// was missing), once the engine is back.
#[test]
fn a_retried_refused_code_worker_is_refused_when_write_went_off() {
    let (now, mut run) = (Instant::now(), run_with(FamilySet::ALL));
    run.set_roster(Roster::new(vec![(Task::Code, Availability::Unassigned)]));
    let id = run.spawn(now, Task::Code, "write").unwrap();
    assert_eq!(run.status(id), Some(WorkerStatus::Refused));
    run.set_roster(full_roster());
    run.set_ceiling(write_off());
    run.decide(now, UserChoice::Retry).unwrap();
    assert_refused_for_write(&run, &[id]);
}

// Guard: with Write still on, the same paths keep the write tools.
#[test]
fn a_relaunched_code_worker_keeps_its_tools_while_write_is_on() {
    let (now, mut run) = (Instant::now(), run_with(FamilySet::ALL));
    let id = run.spawn(now, Task::Code, "write").unwrap();
    run.on_event(now, id, failed());
    run.decide(now, UserChoice::Relaunch(full_roster()))
        .unwrap();
    assert_eq!(run.status(id), Some(WorkerStatus::Running));
    assert!(run.spec(id).unwrap().writes());
}

// A retry is a new process: a writer that failed while Write was on and is
// retried after it went off is refused, not relaunched with Write.
#[test]
fn a_retried_failed_code_worker_is_refused_when_write_went_off() {
    let (now, mut run) = (Instant::now(), run_with(FamilySet::ALL));
    let id = run.spawn(now, Task::Code, "write").unwrap();
    run.on_event(now, id, failed());
    run.set_ceiling(write_off());
    run.decide(now, UserChoice::Retry).unwrap();
    assert_refused_for_write(&run, &[id]);
}

// A writer queued while Write was on is checked again when it leaves the
// queue: if the menu (or a `/write` grant) lost Write meanwhile it is refused
// with the visible Write-off text and never started.
#[test]
fn a_queued_code_worker_is_refused_when_write_went_off_before_it_starts() {
    let (now, mut run) = (Instant::now(), run_with(FamilySet::ALL));
    let first = run.spawn(now, Task::Code, "one").unwrap();
    let second = run.spawn(now, Task::Code, "two").unwrap();
    assert_eq!(run.status(second), Some(WorkerStatus::Queued));
    run.set_ceiling(write_off());
    run.on_event(now, first, WorkerEvent::Done { text: "ok".into() });
    assert_refused_for_write(&run, &[second]);
    assert_ne!(run.status(second), Some(WorkerStatus::Running));
    // The same visible path as a refused retry: the run asks the user.
    assert!(matches!(run.state(), RunState::Halted { worker, .. } if *worker == second));
}

// Guard: with Write still on, the queued writer starts with the write tools.
#[test]
fn a_queued_code_worker_starts_with_write_while_write_is_on() {
    let (now, mut run) = (Instant::now(), run_with(FamilySet::ALL));
    let first = run.spawn(now, Task::Code, "one").unwrap();
    let second = run.spawn(now, Task::Code, "two").unwrap();
    run.on_event(now, first, WorkerEvent::Done { text: "ok".into() });
    assert_eq!(run.status(second), Some(WorkerStatus::Running));
    assert!(run.spec(second).unwrap().writes());
}

// Guard: a queued reader is not touched by a menu without Write.
#[test]
fn a_queued_reader_starts_when_write_went_off() {
    let (now, mut run) = (Instant::now(), run_with(FamilySet::ALL));
    let first = run.spawn(now, Task::Code, "one").unwrap();
    let second = run.spawn(now, Task::Review, "look").unwrap();
    run.set_ceiling(write_off());
    run.on_event(now, first, WorkerEvent::Done { text: "ok".into() });
    assert_eq!(run.status(second), Some(WorkerStatus::Running));
}

// Guards: a retry with Write still on keeps the tools, and a reader is not
// touched by a menu without Write.
#[test]
fn a_retried_worker_keeps_what_the_menu_still_allows() {
    let (now, mut run) = (Instant::now(), run_with(FamilySet::ALL));
    let code = run.spawn(now, Task::Code, "write").unwrap();
    run.on_event(now, code, failed());
    run.decide(now, UserChoice::Retry).unwrap();
    assert_eq!(run.status(code), Some(WorkerStatus::Running));
    assert!(run.spec(code).unwrap().writes());

    let mut run = run_with(FamilySet::ALL);
    let review = run.spawn(now, Task::Review, "look").unwrap();
    run.on_event(now, review, failed());
    run.set_ceiling(write_off());
    run.decide(now, UserChoice::Retry).unwrap();
    assert_eq!(run.status(review), Some(WorkerStatus::Running));
}
