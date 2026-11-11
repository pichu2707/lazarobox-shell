//! The brief at spawn (W8b, tasks 19.3 and 19.4): its refusals create no
//! worker and do not pause the run; `outputs` decide whether a `simple`
//! worker writes, under Write off and under a read-only goal (D18, D25).

use std::path::Path;
use std::time::{Duration, Instant};

use super::brief::{Brief, BriefError};
use super::ledger::{Judgement, Verdict};
use super::run::{Refusal, Roster, Run, RunLimits, RunState, UserChoice, WorkerEvent};
use super::steps::{Grant, Mark, READ_ONLY, Step};
use super::worker::{ErrorClass, WorkerStatus};
use super::{RunId, WorkerId};
use crate::core::ai::chat::engine::Availability;
use crate::core::ai::config::Task;
use crate::core::ai::tools::registry::{Family, FamilySet};

const ORIGIN: &str = "/work/app";

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

fn read_only(run: &mut Run) -> super::run::StepDone {
    let note = Judgement {
        verdict: Verdict::OnCourse,
        reason: "fits".to_owned(),
        done: "explain".to_owned(),
        left: String::new(),
        decisions: String::new(),
    };
    run.handoff_step(Step::Authorize, Mark::Grant(Grant::ReadOnly), note)
        .expect("authorized")
}

fn brief(outputs: Option<&[&str]>) -> Brief {
    Brief {
        goal: "add a flag".to_owned(),
        inputs: vec!["src/a.rs".to_owned()],
        outputs: outputs.map(|o| o.iter().map(|p| (*p).to_owned()).collect()),
        done_when: "it parses".to_owned(),
        out_of_scope: "docs".to_owned(),
        ..Brief::default()
    }
}

fn spawn(run: &mut Run, brief: Brief) -> Result<WorkerId, Refusal> {
    run.spawn_simple(Instant::now(), brief, Path::new(ORIGIN))
}

fn only_read(run: &Run, id: WorkerId) {
    let spec = run.spec(id).unwrap();
    assert!(!spec.writes());
    assert!(spec.families.contains(Family::Read));
    assert!(!spec.families.contains(Family::Exec));
}

// 19.3: a refused brief names its cause, creates no worker and pauses
// nothing (spec "Missing done-when refused", "A refusal before a worker does
// not pause the run").
#[test]
fn a_refused_brief_creates_no_worker_and_does_not_pause_the_run() {
    let mut run = run_with(FamilySet::ALL);
    let mut bad = brief(Some(&["src/b.rs"]));
    bad.done_when.clear();
    assert_eq!(
        spawn(&mut run, bad),
        Err(Refusal::Brief(BriefError::Missing("done when".to_owned())))
    );
    assert_eq!(run.worker_count(), 0);
    assert_eq!(*run.state(), RunState::Open);
    let id = spawn(&mut run, brief(Some(&["src/b.rs"]))).unwrap();
    assert_eq!(run.status(id), Some(WorkerStatus::Running));
}

// Review fix: a paused or ended run says so before the brief is judged, and
// free text for a `simple` is refused by name.
#[test]
fn a_paused_run_is_named_before_the_brief_and_free_text_is_refused() {
    let mut run = run_with(FamilySet::ALL);
    let now = Instant::now();
    let free = Brief {
        free_text: true,
        ..brief(Some(&["src/b.rs"]))
    };
    assert_eq!(
        spawn(&mut run, free.clone()),
        Err(Refusal::Brief(BriefError::FreeText))
    );
    let id = spawn(&mut run, brief(Some(&["src/b.rs"]))).unwrap();
    let failed = WorkerEvent::Failed {
        class: ErrorClass::Other,
        text: "boom".into(),
    };
    run.on_event(now, id, failed);
    let mut bad = brief(None);
    bad.goal.clear();
    assert_eq!(spawn(&mut run, bad.clone()), Err(Refusal::Paused));
    assert_eq!(spawn(&mut run, free), Err(Refusal::Paused));
    run.cancel_run(now);
    assert_eq!(spawn(&mut run, bad), Err(Refusal::Ended));
}

// 19.3: a path outside the project is refused by name, too.
#[test]
fn a_brief_with_a_bad_path_is_refused_by_name() {
    let mut run = run_with(FamilySet::ALL);
    assert_eq!(
        spawn(&mut run, brief(Some(&["../x.rs"]))),
        Err(Refusal::Brief(BriefError::BadPath("../x.rs".to_owned())))
    );
    assert_eq!(run.worker_count(), 0);
}

// 19.3 (spec "Code worker needs no brief"): free text of a `code` spawn is
// not brief-checked.
#[test]
fn a_code_spawn_with_free_text_needs_no_brief() {
    let mut run = run_with(FamilySet::ALL);
    let id = run
        .spawn(Instant::now(), Task::Code, "fix it, no fields")
        .unwrap();
    assert!(run.spec(id).unwrap().brief.is_none());
}

// 19.6: the worker's one message is built from the brief, nothing else.
#[test]
fn the_worker_prompt_is_built_from_the_brief() {
    let mut run = run_with(FamilySet::ALL);
    let given = brief(Some(&["src/b.rs"]));
    let id = spawn(&mut run, given.clone()).unwrap();
    let spec = run.spec(id).unwrap();
    assert_eq!(spec.prompt, given.prompt());
    assert_eq!(spec.brief.as_ref(), Some(&given));
}

// 19.4 (spec "Write off with outputs"): refused with the Write notice.
#[test]
fn write_off_with_outputs_is_refused() {
    let mut run = run_with(FamilySet::ALL.without(Family::Write));
    assert_eq!(
        spawn(&mut run, brief(Some(&["scripts/run.sh"]))),
        Err(Refusal::WriteOff)
    );
    assert_eq!(run.worker_count(), 0);
}

// 19.4 (spec "Write off with inputs only"): it starts with no write tool.
#[test]
fn write_off_with_inputs_only_starts_without_a_write_tool() {
    for outputs in [None, Some(&[][..])] {
        let mut run = run_with(FamilySet::ALL.without(Family::Write));
        let id = spawn(&mut run, brief(outputs)).unwrap();
        assert_eq!(run.status(id), Some(WorkerStatus::Running));
        assert!(!run.spec(id).unwrap().writes());
    }
}

// Decision 14: with no `outputs` there is nothing to stage or run after, so
// the worker keeps Read only even when Write is on.
#[test]
fn a_brief_with_no_outputs_starts_with_the_read_tools_only() {
    let mut run = run_with(FamilySet::ALL);
    let id = spawn(&mut run, brief(None)).unwrap();
    only_read(&run, id);
    let id = spawn(&mut run, brief(Some(&["src/b.rs"]))).unwrap();
    assert!(run.spec(id).unwrap().writes());
}

// 19.4 (spec "Read-only goal refuses a writing simple"): refused at spawn;
// the one strip point still gives Read only to a brief with no outputs.
#[test]
fn a_read_only_goal_refuses_a_simple_with_outputs() {
    let mut run = run_with(FamilySet::ALL);
    read_only(&mut run);
    assert_eq!(
        spawn(&mut run, brief(Some(&["src/b.rs"]))),
        Err(Refusal::ReadOnly)
    );
    assert_eq!(run.worker_count(), 0);
    let id = spawn(&mut run, brief(Some(&[]))).unwrap();
    only_read(&run, id);
}

// 19.4: a queued `simple` with outputs is refused when the goal turns
// read-only (it is never started).
#[test]
fn a_queued_simple_with_outputs_is_refused_when_the_goal_turns_read_only() {
    let mut run = run_with(FamilySet::ALL).with_limits(RunLimits {
        parallel: 1,
        ..RunLimits::default()
    });
    let now = Instant::now();
    let first = run.spawn(now, Task::Review, "look").unwrap();
    let queued = spawn(&mut run, brief(Some(&["src/b.rs"]))).unwrap();
    let reading = spawn(&mut run, brief(Some(&[]))).unwrap();
    let done = read_only(&mut run);
    assert_eq!(done.refused, [queued]);
    assert_eq!(run.status(queued), Some(WorkerStatus::Refused));
    let reason = run.result(queued).unwrap().error.clone().unwrap().1;
    assert_eq!(reason, READ_ONLY);
    run.on_event(now, first, WorkerEvent::Done { text: "ok".into() });
    assert_eq!(run.status(reading), Some(WorkerStatus::Running));
    only_read(&run, reading);
}

// 19.4: a retry of a writing `simple` on a goal that turned read-only is
// refused, not started.
#[test]
fn a_retry_of_a_writing_simple_on_a_read_only_goal_is_refused() {
    let mut run = run_with(FamilySet::ALL);
    let now = Instant::now();
    let id = spawn(&mut run, brief(Some(&["src/b.rs"]))).unwrap();
    let failed = WorkerEvent::Failed {
        class: ErrorClass::Other,
        text: "boom".into(),
    };
    run.on_event(now, id, failed);
    read_only(&mut run);
    run.decide(now, UserChoice::Retry).unwrap();
    assert_eq!(run.status(id), Some(WorkerStatus::Refused));
}

// A retry keeps the brief: outputs and the prompt survive it.
#[test]
fn a_retry_keeps_the_brief() {
    let mut run = run_with(FamilySet::ALL);
    let now = Instant::now();
    let given = brief(Some(&["src/b.rs"]));
    let id = spawn(&mut run, given.clone()).unwrap();
    let failed = WorkerEvent::Failed {
        class: ErrorClass::Other,
        text: "boom".into(),
    };
    run.on_event(now, id, failed);
    run.decide(now, UserChoice::Relaunch(roster())).unwrap();
    let spec = run.spec(id).unwrap();
    assert_eq!(spec.brief.as_ref(), Some(&given));
    assert_eq!(spec.prompt, given.prompt());
    assert!(spec.writes());
}
