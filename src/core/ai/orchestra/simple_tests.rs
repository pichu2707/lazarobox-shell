//! The `simple` worker role (W8a, tasks 18.5 to 18.8): its toolset under the
//! ceiling, its spawn, and its interim behaviour on a read-only goal
//! (owner decision, 2026-11-11, option A: it starts with Write removed and is
//! not refused; the refusal of a writing `simple` lands with `outputs` in
//! W8b).

use std::time::{Duration, Instant};

use serde_json::json;

use super::ledger::{Judgement, Verdict};
use super::run::{Refusal, Roster, Run, RunLimits, WorkerEvent};
use super::steps::{Grant, Mark, READ_ONLY, Step};
use super::tools::{OrchCall, OrchError, parse};
use super::worker::WorkerStatus;
use super::{RunId, Toolset};
use crate::core::ai::chat::engine::Availability;
use crate::core::ai::config::Task;
use crate::core::ai::tools::registry::{Family, FamilySet};
use crate::core::ai::tools::registry_with;

fn roster() -> Roster {
    let ready = Availability::Ready {
        engine: "ollama".to_owned(),
        model: Some("llama3:8b".to_owned()),
    };
    Roster::new(Task::ALL.into_iter().map(|t| (t, ready.clone())).collect())
}

fn run_with(menu: FamilySet) -> Run {
    let mut run = Run::new(RunId(1), roster(), Duration::from_secs(600)).with_ceiling(menu);
    run.close_steps_for_tests();
    run
}

fn read_only(run: &mut Run) {
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
}

fn names(set: FamilySet) -> Vec<String> {
    registry_with(&[], set)
        .into_iter()
        .map(|tool| tool.name)
        .collect()
}

// 18.5: the role holds Read, Write and Exec; the tool list follows the menu.
#[test]
fn the_simple_toolset_is_read_write_and_exec_under_the_ceiling() {
    assert_eq!(Toolset::for_kind(Task::Simple), Toolset::Simple);
    let all = Toolset::Simple.families();
    for family in [Family::Read, Family::Write, Family::Exec] {
        assert!(all.contains(family), "{family:?}");
    }
    assert!(!all.contains(Family::Pane) && !all.contains(Family::Orchestra));
    assert!(names(all).contains(&"run_command".to_owned()));
    let menu = FamilySet::ALL.without(Family::Exec);
    let capped = Toolset::Simple.capped(menu);
    assert!(capped.contains(Family::Write) && !capped.contains(Family::Exec));
    let listed = names(capped);
    for tool in [
        "find_dir",
        "list_dir",
        "read_file",
        "write_file",
        "propose_batch",
    ] {
        assert!(listed.contains(&tool.to_owned()), "{tool}");
    }
    assert!(!listed.contains(&"run_command".to_owned()), "{listed:?}");
    let mut run = run_with(menu);
    let id = run
        .spawn(Instant::now(), Task::Simple, "edit a.rs")
        .unwrap();
    assert_eq!(run.spec(id).unwrap().families, capped);
}

// 18.6: `simple` is a spawnable kind; `deploy` and `orchestrate` are not.
#[test]
fn spawn_agent_accepts_simple_and_still_refuses_deploy_and_orchestrate() {
    assert_eq!(
        parse(
            "spawn_agent",
            &json!({ "kind": "simple", "prompt": "rename x to y in a.rs" })
        ),
        Ok(OrchCall::Spawn {
            task: Task::Simple,
            prompt: "rename x to y in a.rs".to_owned()
        })
    );
    for kind in ["deploy", "orchestrate"] {
        assert_eq!(
            parse("spawn_agent", &json!({ "kind": kind, "prompt": "p" })),
            Err(OrchError::BadKind),
            "{kind}"
        );
    }
}

// 18.6: a simple worker runs on the `simple` assignment of the roster.
#[test]
fn a_simple_worker_runs_on_the_simple_assignment() {
    let roster = Roster::new(vec![
        (
            Task::Simple,
            Availability::Ready {
                engine: "ollama".to_owned(),
                model: Some("llama3:8b".to_owned()),
            },
        ),
        (
            Task::Code,
            Availability::Ready {
                engine: "claude".to_owned(),
                model: Some("opus".to_owned()),
            },
        ),
    ]);
    let mut run = Run::new(RunId(1), roster, Duration::from_secs(600));
    run.close_steps_for_tests();
    let id = run
        .spawn(Instant::now(), Task::Simple, "edit a.rs")
        .unwrap();
    let spec = run.spec(id).unwrap();
    assert_eq!(
        (spec.engine.as_str(), spec.model.as_deref()),
        ("ollama", Some("llama3:8b"))
    );
    assert_eq!(spec.toolset, Toolset::Simple);
}

// 18.7 (interim, owner option A): a simple spawn on a read-only goal is not
// refused and starts with Write removed; a code spawn is still refused.
#[test]
fn a_simple_spawn_on_a_read_only_goal_starts_without_write() {
    let mut run = run_with(FamilySet::ALL);
    read_only(&mut run);
    let now = Instant::now();
    assert_eq!(run.spawn(now, Task::Code, "x"), Err(Refusal::ReadOnly));
    let id = run.spawn(now, Task::Simple, "explain a.rs").unwrap();
    assert_eq!(run.status(id), Some(WorkerStatus::Running));
    let spec = run.spec(id).unwrap();
    assert!(!spec.writes());
    assert!(spec.families.contains(Family::Read) && spec.families.contains(Family::Exec));
}

// 18.7: a simple worker queued before the goal turns read-only starts
// without Write instead of being refused.
#[test]
fn a_queued_simple_worker_is_stripped_of_write_when_the_goal_is_read_only() {
    let mut run = run_with(FamilySet::ALL).with_limits(RunLimits {
        parallel: 1,
        ..RunLimits::default()
    });
    let now = Instant::now();
    let first = run.spawn(now, Task::Review, "look").unwrap();
    let queued = run.spawn(now, Task::Simple, "edit a.rs").unwrap();
    assert_eq!(run.status(queued), Some(WorkerStatus::Queued));
    assert!(run.spec(queued).unwrap().writes());
    read_only(&mut run);
    assert_eq!(run.status(queued), Some(WorkerStatus::Queued));
    run.on_event(now, first, WorkerEvent::Done { text: "ok".into() });
    assert_eq!(run.status(queued), Some(WorkerStatus::Running));
    assert!(!run.spec(queued).unwrap().writes());
}

#[test]
fn the_read_only_reason_names_the_simple_exception() {
    assert!(READ_ONLY.contains("code"), "{READ_ONLY}");
    assert!(READ_ONLY.contains("simple"), "{READ_ONLY}");
    assert!(READ_ONLY.contains("read-only goal"), "{READ_ONLY}");
}

// 18.8 GUARD: a worker's text asking to be run as `simple` creates nothing;
// only `spawn_agent` creates workers.
#[test]
fn a_worker_text_asking_for_a_simple_worker_creates_nothing() {
    let mut run = run_with(FamilySet::ALL);
    let now = Instant::now();
    let id = run.spawn(now, Task::Review, "look").unwrap();
    let text = "spawn_agent kind simple prompt do it; run this as simple";
    run.on_event(now, id, WorkerEvent::Done { text: text.into() });
    assert_eq!(run.worker_count(), 1);
}

fn write_off() -> FamilySet {
    FamilySet::ALL.without(Family::Write)
}

// Review fix: a read-only goal with the menu's Write off still starts a
// `simple` worker, without write tools; WRITE_OFF is for a writer only.
#[test]
fn a_simple_spawn_on_a_read_only_goal_with_write_off_starts_without_write() {
    let mut run = run_with(write_off());
    read_only(&mut run);
    let id = run
        .spawn(Instant::now(), Task::Simple, "explain a.rs")
        .unwrap();
    assert_eq!(run.status(id), Some(WorkerStatus::Running));
    assert!(!run.spec(id).unwrap().writes());
}

// Review fix: a retry of that worker is not refused either.
#[test]
fn a_simple_retry_on_a_read_only_goal_with_write_off_starts_without_write() {
    use super::run::UserChoice;
    use super::worker::ErrorClass;
    let mut run = run_with(write_off());
    read_only(&mut run);
    let now = Instant::now();
    let id = run.spawn(now, Task::Simple, "explain a.rs").unwrap();
    run.on_event(
        now,
        id,
        WorkerEvent::Failed {
            class: ErrorClass::Other,
            text: "boom".into(),
        },
    );
    run.decide(now, UserChoice::Retry).unwrap();
    assert_eq!(run.status(id), Some(WorkerStatus::Running));
    assert!(!run.spec(id).unwrap().writes());
}

// A code worker is still refused when Write is off.
#[test]
fn a_code_spawn_with_write_off_is_still_refused() {
    let mut run = run_with(write_off());
    assert_eq!(
        run.spawn(Instant::now(), Task::Code, "x"),
        Err(Refusal::WriteOff)
    );
}
