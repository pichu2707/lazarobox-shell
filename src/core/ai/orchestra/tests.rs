use std::time::{Duration, Instant};

use serde_json::json;

use super::run::{
    CancelOutcome, CollectError, Refusal, Roster, Run, RunState, UserChoice, WorkerEvent,
};
use super::tools::{MAX_PROMPT_BYTES, OrchCall, OrchError, WAIT_MAX_SECS, parse, specs};
use super::worker::{ErrorClass, WorkerStatus};
use super::{RunId, Toolset, WorkerId};
use crate::core::ai::chat::engine::Availability;
use crate::core::ai::config::Task;

#[test]
fn spawn_accepts_the_five_worker_kinds() {
    for task in Task::SET_ALL {
        let args = json!({ "kind": task.id(), "prompt": "look at src" });
        assert_eq!(
            parse("spawn_agent", &args),
            Ok(OrchCall::Spawn {
                task,
                prompt: "look at src".to_owned()
            })
        );
    }
}

#[test]
fn spawn_refuses_what_a_worker_cannot_be() {
    let long = "x".repeat(MAX_PROMPT_BYTES + 1);
    for (args, error) in [
        (
            json!({ "kind": "deploy", "prompt": "p" }),
            OrchError::BadKind,
        ),
        (
            json!({ "kind": "orchestrate", "prompt": "p" }),
            OrchError::BadKind,
        ),
        (
            json!({ "kind": "review", "prompt": "" }),
            OrchError::EmptyPrompt,
        ),
        (
            json!({ "kind": "review", "prompt": long }),
            OrchError::TooLong,
        ),
        (json!({ "kind": "review" }), OrchError::MissingArgument),
        (json!({ "kind": 3, "prompt": "p" }), OrchError::WrongType),
        (
            json!({ "kind": "review", "prompt": "p", "x": 1 }),
            OrchError::ExtraArgument,
        ),
        (json!(["review"]), OrchError::NotAnObject),
    ] {
        assert_eq!(parse("spawn_agent", &args), Err(error), "{args}");
    }
}

#[test]
fn wait_is_clamped_to_the_ceiling() {
    for (args, secs) in [
        (json!({ "ids": [1], "timeout": 600 }), WAIT_MAX_SECS),
        (json!({ "ids": [1], "timeout": 5 }), 5),
        (json!({ "ids": [1] }), WAIT_MAX_SECS),
    ] {
        assert_eq!(
            parse("wait", &args),
            Ok(OrchCall::Wait {
                ids: vec![WorkerId(1)],
                timeout: secs
            }),
            "{args}"
        );
    }
}

#[test]
fn wait_collect_and_cancel_validate_their_ids() {
    assert_eq!(
        parse("collect", &json!({ "id": 2 })),
        Ok(OrchCall::Collect { id: WorkerId(2) })
    );
    assert_eq!(
        parse("cancel", &json!({ "id": 2 })),
        Ok(OrchCall::Cancel { id: WorkerId(2) })
    );
    for (name, args, error) in [
        ("wait", json!({ "ids": [] }), OrchError::MissingArgument),
        ("wait", json!({ "ids": ["a"] }), OrchError::WrongType),
        (
            "wait",
            json!({ "ids": [1], "timeout": -1 }),
            OrchError::WrongType,
        ),
        (
            "collect",
            json!({ "id": 4294967296u64 }),
            OrchError::WrongType,
        ),
        ("cancel", json!({}), OrchError::MissingArgument),
        (
            "run_in_pane",
            json!({ "command": "ls" }),
            OrchError::UnknownTool,
        ),
    ] {
        assert_eq!(parse(name, &args), Err(error), "{name} {args}");
    }
}

#[test]
fn the_registry_is_exactly_the_four_tools_and_workers_are_read_only() {
    let names: Vec<_> = specs().into_iter().map(|spec| spec.name).collect();
    assert_eq!(names, ["spawn_agent", "wait", "collect", "cancel"]);
    for task in Task::SET_ALL {
        assert_eq!(Toolset::for_kind(task), Toolset::ReadOnly);
    }
}

fn ready(engine: &str, model: &str) -> Availability {
    Availability::Ready {
        engine: engine.to_owned(),
        model: Some(model.to_owned()),
    }
}

fn roster(review: Availability) -> Roster {
    Roster::new(vec![
        (Task::Code, ready("claude", "opus")),
        (Task::Review, review),
    ])
}

fn run() -> Run {
    Run::new(
        RunId(1),
        roster(ready("codex", "default")),
        Duration::from_secs(600),
    )
}

fn failed(class: ErrorClass, text: &str) -> WorkerEvent {
    WorkerEvent::Failed {
        class,
        text: text.to_owned(),
    }
}

#[test]
fn a_worker_runs_on_the_assignment_of_its_kind_and_extras_queue() {
    let (t0, mut run) = (Instant::now(), run());
    let one = run.spawn(t0, Task::Review, "p1").unwrap();
    let two = run.spawn(t0, Task::Code, "p2").unwrap();
    let spec = run.spec(one).unwrap();
    assert_eq!(
        (spec.engine.as_str(), spec.model.as_deref()),
        ("codex", Some("default"))
    );
    assert_eq!(spec.toolset, Toolset::ReadOnly);
    assert_eq!(run.status(one), Some(WorkerStatus::Running));
    assert_eq!(run.status(two), Some(WorkerStatus::Queued));

    let t1 = t0 + Duration::from_secs(7);
    run.on_event(
        t1,
        one,
        WorkerEvent::Done {
            text: "ok".to_owned(),
        },
    );
    assert_eq!(run.status(one), Some(WorkerStatus::Done));
    assert_eq!(run.result(one).unwrap().elapsed, Duration::from_secs(7));
    assert_eq!(run.status(two), Some(WorkerStatus::Running));
}

#[test]
fn a_menu_change_after_spawn_does_not_reach_a_started_worker() {
    let (t0, mut run) = (Instant::now(), run());
    let id = run.spawn(t0, Task::Review, "p").unwrap();
    run.set_roster(roster(ready("claude", "sonnet")));
    run.on_event(
        t0,
        id,
        WorkerEvent::Done {
            text: String::new(),
        },
    );
    let result = run.result(id).unwrap();
    assert_eq!(
        (result.engine.as_str(), result.model.as_deref()),
        ("codex", Some("default"))
    );
}

#[test]
fn an_unavailable_engine_is_refused_and_halts_the_run_with_no_fallback() {
    let no_jail = Availability::NoJail {
        engine: "gemini".to_owned(),
        reason: "no jail here".to_owned(),
    };
    let (t0, mut run) = (
        Instant::now(),
        Run::new(RunId(1), roster(no_jail), Duration::from_secs(600)),
    );
    let id = run.spawn(t0, Task::Review, "p").unwrap();
    assert_eq!(run.status(id), Some(WorkerStatus::Refused));
    let Some(RunState::Halted {
        worker,
        class,
        text,
    }) = Some(run.state().clone())
    else {
        panic!("not halted");
    };
    assert_eq!((worker, class), (id, ErrorClass::Other));
    assert!(text.contains("no jail here"), "{text}");
    assert_eq!(run.spawn(t0, Task::Code, "q"), Err(Refusal::Paused));
}

#[test]
fn a_failure_pauses_the_orchestrator_until_the_user_decides() {
    let (t0, mut run) = (Instant::now(), run());
    let id = run.spawn(t0, Task::Review, "p").unwrap();
    run.on_event(
        t0,
        id,
        failed(ErrorClass::Conflict, "409 conflict: session busy"),
    );
    assert_eq!(
        run.state(),
        &RunState::Halted {
            worker: id,
            class: ErrorClass::Conflict,
            text: "409 conflict: session busy".to_owned()
        }
    );
    assert_eq!(
        run.wait_report(&[id, WorkerId(9)]),
        [(id, "awaiting_user"), (WorkerId(9), "awaiting_user")]
    );
    assert_eq!(run.spawn(t0, Task::Code, "q"), Err(Refusal::Paused));
}

#[test]
fn the_deadline_times_the_worker_out_and_halts_the_run() {
    let (t0, mut run) = (Instant::now(), run());
    let id = run.spawn(t0, Task::Review, "p").unwrap();
    assert!(run.on_tick(t0 + Duration::from_secs(599)).is_empty());
    assert_eq!(run.on_tick(t0 + Duration::from_secs(600)), [id]);
    assert_eq!(run.status(id), Some(WorkerStatus::TimedOut));
    assert!(matches!(
        run.state(),
        RunState::Halted {
            class: ErrorClass::Timeout,
            ..
        }
    ));
}

#[test]
fn retry_relaunches_the_same_spec_and_relaunch_takes_the_new_roster() {
    let (t0, mut run) = (Instant::now(), run());
    let id = run.spawn(t0, Task::Review, "p").unwrap();
    let spec = run.spec(id).unwrap().clone();
    run.on_event(t0, id, failed(ErrorClass::RateLimit, "429"));
    assert_eq!(run.decide(t0, UserChoice::Retry), Ok(vec![]));
    assert_eq!(
        (run.state(), run.status(id), run.spec(id)),
        (&RunState::Open, Some(WorkerStatus::Running), Some(&spec))
    );

    run.on_event(t0, id, failed(ErrorClass::RateLimit, "429"));
    run.decide(t0, UserChoice::Relaunch(roster(ready("claude", "sonnet"))))
        .unwrap();
    let spec = run.spec(id).unwrap();
    assert_eq!(
        (spec.engine.as_str(), spec.prompt.as_str()),
        ("claude", "p")
    );
    assert_eq!(run.status(id), Some(WorkerStatus::Running));

    run.on_event(t0, id, failed(ErrorClass::Other, "x"));
    let gone = Availability::Undetected {
        engine: "codex".to_owned(),
        status: "not installed".to_owned(),
    };
    run.decide(t0, UserChoice::Relaunch(roster(gone))).unwrap();
    assert_eq!(run.status(id), Some(WorkerStatus::Refused));
    assert!(matches!(run.state(), RunState::Halted { .. }));
}

#[test]
fn cancel_ends_the_run_and_collect_waits_for_a_finished_worker() {
    let (t0, mut run) = (Instant::now(), run());
    let one = run.spawn(t0, Task::Review, "p").unwrap();
    let two = run.spawn(t0, Task::Code, "q").unwrap();
    assert_eq!(run.collect(one), Err(CollectError::NotFinished));
    assert_eq!(run.cancel(t0, WorkerId(9)), CancelOutcome::Unknown);
    run.on_event(t0, one, failed(ErrorClass::Other, "x"));
    assert_eq!(run.decide(t0, UserChoice::Cancel), Ok(vec![]));
    assert_eq!(run.state(), &RunState::Cancelled);
    assert_eq!(run.status(two), Some(WorkerStatus::Cancelled));
    assert_eq!(run.spawn(t0, Task::Code, "r"), Err(Refusal::Ended));
    assert!(run.decide(t0, UserChoice::Retry).is_err());
}

#[test]
fn cancelling_one_worker_frees_the_slot_and_a_finished_one_stays_as_it_was() {
    let (t0, mut run) = (Instant::now(), run());
    let one = run.spawn(t0, Task::Review, "p").unwrap();
    let two = run.spawn(t0, Task::Code, "q").unwrap();
    assert_eq!(run.cancel(t0, one), CancelOutcome::Cancelled);
    assert_eq!(run.status(two), Some(WorkerStatus::Running));
    run.on_event(
        t0,
        two,
        WorkerEvent::Done {
            text: "ok".to_owned(),
        },
    );
    assert_eq!(run.cancel(t0, two), CancelOutcome::AlreadyEnded);
    assert_eq!(run.status(two), Some(WorkerStatus::Done));
    assert_eq!(run.collect(two), Ok("ok"));
}

fn done() -> WorkerEvent {
    WorkerEvent::Done {
        text: "ok".to_owned(),
    }
}

#[test]
fn a_second_failure_while_halted_surfaces_after_the_first_is_decided() {
    let (t0, mut run) = (Instant::now(), run());
    let one = run.spawn(t0, Task::Review, "p").unwrap();
    let no_engine = Availability::Unassigned;
    run.set_roster(roster(no_engine));
    let two = run.spawn(t0, Task::Review, "q").unwrap();
    assert_eq!(run.status(two), Some(WorkerStatus::Refused));
    assert!(matches!(run.state(), RunState::Halted { worker, .. } if *worker == two));

    run.on_event(t0, one, failed(ErrorClass::RateLimit, "429 from one"));
    run.set_roster(roster(ready("codex", "default")));
    assert_eq!(run.decide(t0, UserChoice::Retry), Ok(vec![]));
    assert_eq!(
        run.state(),
        &RunState::Halted {
            worker: one,
            class: ErrorClass::RateLimit,
            text: "429 from one".to_owned()
        }
    );
    assert_eq!(run.decide(t0, UserChoice::Retry), Ok(vec![]));
    assert_eq!(run.state(), &RunState::Open);
}

#[test]
fn queued_workers_start_in_spawn_order_and_a_retried_one_goes_to_the_back() {
    let (t0, mut run) = (Instant::now(), run());
    let one = run.spawn(t0, Task::Review, "1").unwrap();
    let two = run.spawn(t0, Task::Review, "2").unwrap();
    let three = run.spawn(t0, Task::Review, "3").unwrap();
    run.on_event(t0, one, failed(ErrorClass::Other, "x"));
    // Nothing starts while halted.
    assert_eq!(run.status(two), Some(WorkerStatus::Queued));
    run.decide(t0, UserChoice::Retry).unwrap();
    assert_eq!(run.status(two), Some(WorkerStatus::Running));
    assert_eq!(run.status(one), Some(WorkerStatus::Queued));
    assert_eq!(run.status(three), Some(WorkerStatus::Queued));
    run.on_event(t0, two, done());
    assert_eq!(run.status(three), Some(WorkerStatus::Running));
    assert_eq!(run.status(one), Some(WorkerStatus::Queued));
    run.on_event(t0, three, done());
    assert_eq!(run.status(one), Some(WorkerStatus::Running));
}

#[test]
fn collect_hands_over_only_a_done_worker() {
    let (t0, mut run) = (Instant::now(), run());
    let one = run.spawn(t0, Task::Review, "1").unwrap();
    let two = run.spawn(t0, Task::Review, "2").unwrap();
    run.on_event(t0, one, failed(ErrorClass::Other, "x"));
    assert_eq!(
        run.collect(one),
        Err(CollectError::Ended(WorkerStatus::Failed))
    );
    assert_eq!(run.cancel(t0, two), CancelOutcome::Cancelled);
    assert_eq!(
        run.collect(two),
        Err(CollectError::Ended(WorkerStatus::Cancelled))
    );
    run.decide(t0, UserChoice::Retry).unwrap();
    run.on_event(t0, one, done());
    assert_eq!(run.collect(one), Ok("ok"));
}

#[test]
fn an_enormous_deadline_never_expires_and_does_not_panic() {
    let (t0, mut run) = (
        Instant::now(),
        Run::new(RunId(1), roster(ready("codex", "default")), Duration::MAX),
    );
    run.spawn(t0, Task::Review, "p").unwrap();
    assert!(run.on_tick(t0 + Duration::from_secs(3600)).is_empty());
}
