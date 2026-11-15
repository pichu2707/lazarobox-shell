//! The `/exec` grant reaches a worker only through a new process (task 16.10,
//! D16): the next spawn, a queue promote and a retry or a relaunch re-read the
//! ceiling; a worker that already runs keeps the spec it started with.

use std::time::{Duration, Instant};

use super::run::{Roster, Run, UserChoice, WorkerEvent};
use super::worker::{ErrorClass, WorkerStatus};
use super::{RunId, WorkerId};
use crate::core::ai::chat::engine::Availability;
use crate::core::ai::config::Task;
use crate::core::ai::tools::registry::{Family, FamilySet};

fn no_exec() -> FamilySet {
    FamilySet::ALL.without(Family::Exec)
}

fn roster() -> Roster {
    let ready = || Availability::Ready {
        engine: "ollama".to_owned(),
        model: Some("qwen".to_owned()),
    };
    Roster::new(Task::SET_ALL.into_iter().map(|t| (t, ready())).collect())
}

fn run_with(menu: FamilySet) -> Run {
    let mut run = Run::new(RunId(1), roster(), Duration::from_secs(600)).with_ceiling(menu);
    run.close_steps_for_tests();
    run
}

fn has_exec(run: &Run, id: WorkerId) -> bool {
    run.spec(id).unwrap().families.contains(Family::Exec)
}

fn failed() -> WorkerEvent {
    WorkerEvent::Failed {
        class: ErrorClass::Other,
        text: "x".to_owned(),
    }
}

// The grant reaches the next spawn, never the worker that already runs.
#[test]
fn a_grant_reaches_the_next_spawn_and_never_a_running_worker() {
    let (now, mut run) = (Instant::now(), run_with(no_exec()));
    let first = run.spawn(now, Task::Code, "one").unwrap();
    assert!(!has_exec(&run, first));
    run.set_ceiling(FamilySet::ALL);
    assert!(!has_exec(&run, first), "a running worker keeps its tools");
    let second = run.spawn(now, Task::Code, "two").unwrap();
    assert!(has_exec(&run, second) && run.spec(second).unwrap().writes());
    // Ending the grant takes it from the next spawn, not from this one.
    run.set_ceiling(no_exec());
    assert!(has_exec(&run, second));
    let third = run.spawn(now, Task::Code, "three").unwrap();
    assert!(!has_exec(&run, third));
}

// A worker that waited in the queue starts under the menu of that moment.
#[test]
fn a_queue_promote_reads_the_ceiling_again() {
    let (now, mut run) = (Instant::now(), run_with(no_exec()));
    let first = run.spawn(now, Task::Code, "one").unwrap();
    let second = run.spawn(now, Task::Code, "two").unwrap();
    assert_eq!(run.status(second), Some(WorkerStatus::Queued));
    run.set_ceiling(FamilySet::ALL);
    run.on_event(now, first, WorkerEvent::Done { text: "ok".into() });
    assert_eq!(run.status(second), Some(WorkerStatus::Running));
    assert!(has_exec(&run, second));
    assert!(!has_exec(&run, first), "the finished one is as it was");
}

// A retry and a relaunch are new processes: they follow the menu now.
#[test]
fn a_retry_and_a_relaunch_read_the_ceiling_again() {
    for choice in [UserChoice::Retry, UserChoice::Relaunch(roster())] {
        let (now, mut run) = (Instant::now(), run_with(no_exec()));
        let id = run.spawn(now, Task::Code, "write").unwrap();
        assert!(!has_exec(&run, id));
        run.on_event(now, id, failed());
        run.set_ceiling(FamilySet::ALL);
        run.decide(now, choice).unwrap();
        assert_eq!(run.status(id), Some(WorkerStatus::Running));
        assert!(has_exec(&run, id));
    }
}
