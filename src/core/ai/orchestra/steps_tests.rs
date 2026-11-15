//! The ODD steps and their handoffs (agent-orchestrator S5, D15).

use std::time::{Duration, Instant};

use super::RunId;
use super::WorkerId;
use super::ledger::{Judgement, Unit, Verdict};
use super::run::{Refusal, Roster, Run, RunLimits, RunState, UserChoice, WorkerEvent};
use super::steps::{Grant, Mark, Size, Step, StepError};
use super::tools::{OrchCall, OrchError, parse};
use super::worker::{ErrorClass, WorkerStatus};
use crate::core::ai::chat::engine::Availability;
use crate::core::ai::config::Task;
use serde_json::json;

fn roster() -> Roster {
    let ready = Availability::Ready {
        engine: "claude".to_owned(),
        model: Some("opus".to_owned()),
    };
    Roster::new(vec![
        (Task::Orchestrate, ready.clone()),
        (Task::Code, ready.clone()),
        (Task::Review, ready),
    ])
}

fn run() -> Run {
    Run::new(RunId(5), roster(), Duration::from_secs(600)).with_limits(RunLimits {
        parallel: 4,
        ..Default::default()
    })
}

fn note(done: &str) -> Judgement {
    Judgement {
        verdict: Verdict::OnCourse,
        reason: "fits".to_owned(),
        done: done.to_owned(),
        left: String::new(),
        decisions: String::new(),
    }
}

/// Closes `steps` in order with plain handoffs.
fn close_all(run: &mut Run, steps: &[Step]) {
    for step in steps {
        let mark = match step {
            Step::Authorize => Mark::Grant(Grant::Implement),
            Step::Classify => Mark::Size(Size::Small),
            _ => Mark::None,
        };
        run.handoff_step(*step, mark, note("x")).expect("closed");
    }
}

// Spec (orchestrator-handoffs, Step needs a handoff): a step is not closed
// before its handoff exists, and the handoff is the ledger's.
#[test]
fn a_step_is_not_closed_without_its_handoff() {
    let mut run = run();
    assert_eq!(
        run.steps_mut().close(Step::Authorize, Mark::None),
        Err(StepError::NoHandoff(Step::Authorize))
    );
    assert!(!run.steps().is_closed(Step::Authorize));
    let handoff = run
        .handoff_step(Step::Authorize, Mark::Grant(Grant::Implement), note("ok"))
        .expect("stored")
        .handoff;
    assert_eq!(handoff.unit, Unit::Step(Step::Authorize));
    assert_eq!(handoff.kind, Task::Orchestrate);
    assert_eq!(handoff.engine_model(), "claude>opus");
    assert!(run.steps().is_closed(Step::Authorize));
    assert_eq!(run.steps().handoff(Step::Authorize), Some(&handoff));
}

// Spec (agent-orchestrator, ODD steps in order): a step opens only after the
// ones before it; nothing is stored for a refused one.
#[test]
fn steps_close_in_order() {
    let mut run = run();
    assert_eq!(
        run.handoff_step(Step::Explore, Mark::None, note("x")),
        Err(StepError::Order {
            step: Step::Explore,
            open: Step::Authorize
        })
    );
    assert_eq!(run.steps().handoff(Step::Explore), None);
    close_all(&mut run, &[Step::Authorize, Step::Explore, Step::Resolve]);
    assert_eq!(
        run.handoff_step(Step::Implement, Mark::None, note("x")),
        Err(StepError::Order {
            step: Step::Implement,
            open: Step::Classify
        })
    );
}

// Spec (A large goal needs its document): a large goal names the missing
// `track`; a small one skips it, and cannot close it.
#[test]
fn track_is_for_large_goals_only() {
    let mut large = run();
    close_all(&mut large, &[Step::Authorize, Step::Explore, Step::Resolve]);
    large
        .handoff_step(Step::Classify, Mark::Size(Size::Large), note("big"))
        .expect("classified");
    assert_eq!(large.steps().size(), Some(Size::Large));
    assert_eq!(
        large.handoff_step(Step::Implement, Mark::None, note("x")),
        Err(StepError::Order {
            step: Step::Implement,
            open: Step::Track
        })
    );
    large
        .feature_build("goal", &["a".to_owned()], &[], note("x"))
        .expect("tracked");
    close_all(&mut large, &[Step::Implement, Step::Close]);

    let mut small = run();
    close_all(
        &mut small,
        &[
            Step::Authorize,
            Step::Explore,
            Step::Resolve,
            Step::Classify,
        ],
    );
    assert_eq!(
        small.handoff_step(Step::Track, Mark::None, note("x")),
        Err(StepError::NotLarge)
    );
    close_all(&mut small, &[Step::Implement, Step::Close]);
}

// Spec (Read-only goal spawns no code worker): refused with a reason, other
// kinds still spawn; the grant cannot be lifted by a later handoff.
#[test]
fn a_read_only_goal_refuses_a_code_worker() {
    let mut run = run();
    let now = Instant::now();
    run.handoff_step(
        Step::Authorize,
        Mark::Grant(Grant::ReadOnly),
        note("explain"),
    )
    .expect("authorized");
    assert_eq!(run.spawn(now, Task::Code, "p"), Err(Refusal::ReadOnly));
    assert_eq!(run.worker_count(), 0);
    assert!(run.spawn(now, Task::Review, "p").is_ok());
    assert_eq!(
        run.handoff_step(
            Step::Authorize,
            Mark::Grant(Grant::Implement),
            note("again")
        ),
        Err(StepError::Widen)
    );
    assert_eq!(run.spawn(now, Task::Code, "p"), Err(Refusal::ReadOnly));
    assert_eq!(
        run.steps()
            .handoff(Step::Authorize)
            .map(|h| h.done.as_str()),
        Some("explain")
    );
}

// Review 1(a): a code worker already running keeps the grant from turning
// read-only; cancelling it first lets the grant through.
#[test]
fn read_only_is_refused_while_a_code_worker_runs() {
    let mut run = run();
    let now = Instant::now();
    run.close_steps_for_tests();
    let code = run.spawn(now, Task::Code, "p").expect("spawn");
    assert_eq!(
        run.handoff_step(Step::Authorize, Mark::Grant(Grant::ReadOnly), note("x")),
        Err(StepError::CodeRunning(code))
    );
    assert!(!run.steps().read_only());
    // The refused handoff changed nothing: the grant is still `implement`.
    assert_eq!(run.steps().grant(), Some(Grant::Implement));
    run.cancel(now, code);
    assert!(
        run.handoff_step(Step::Authorize, Mark::Grant(Grant::ReadOnly), note("x"))
            .is_ok()
    );
}

// Review 1(b): a queued code worker is ended refused with a reason when the
// goal becomes read-only, without halting the run; other kinds still start.
#[test]
fn read_only_refuses_the_queued_code_workers() {
    let mut run = run().with_limits(RunLimits::default());
    let now = Instant::now();
    run.close_steps_for_tests();
    let review = run.spawn(now, Task::Review, "a").expect("spawn");
    let code = run.spawn(now, Task::Code, "b").expect("spawn");
    let second = run.spawn(now, Task::Review, "c").expect("spawn");
    assert_eq!(run.status(code), Some(WorkerStatus::Queued));
    let done = run
        .handoff_step(Step::Authorize, Mark::Grant(Grant::ReadOnly), note("x"))
        .expect("authorized");
    assert_eq!(done.refused, vec![code]);
    assert_eq!(run.status(code), Some(WorkerStatus::Refused));
    let error = run.result(code).and_then(|r| r.error.clone());
    assert!(error.is_some_and(|(_, text)| text.contains("read-only")));
    assert_eq!(*run.state(), RunState::Open);
    run.on_event(
        now,
        review,
        WorkerEvent::Done {
            text: "t".to_owned(),
        },
    );
    assert_eq!(run.status(second), Some(WorkerStatus::Running));
    assert_eq!(run.status(code), Some(WorkerStatus::Refused));
}

fn failed_code(run: &mut Run) -> WorkerId {
    let now = Instant::now();
    run.close_steps_for_tests();
    let code = run.spawn(now, Task::Code, "p").expect("spawn");
    run.on_event(
        now,
        code,
        WorkerEvent::Failed {
            class: ErrorClass::Other,
            text: "boom".to_owned(),
        },
    );
    code
}

// Review 1(c): a halted code worker is not started again on a read-only run,
// by a retry or a relaunch; the halt names the reason.
#[test]
fn a_retry_or_relaunch_never_restarts_a_code_worker_on_a_read_only_run() {
    for relaunch in [false, true] {
        let mut run = run();
        let code = failed_code(&mut run);
        run.handoff_step(Step::Authorize, Mark::Grant(Grant::ReadOnly), note("x"))
            .expect("authorized while halted");
        let choice = if relaunch {
            UserChoice::Relaunch(roster())
        } else {
            UserChoice::Retry
        };
        run.decide(Instant::now(), choice).expect("halted");
        assert_eq!(run.status(code), Some(WorkerStatus::Refused), "{relaunch}");
        let RunState::Halted { text, .. } = run.state() else {
            panic!("{:?}", run.state());
        };
        assert!(text.contains("read-only"), "{text}");
    }
}

// Review 3: a classification without its reason is refused.
#[test]
fn classify_needs_a_reason() {
    let mut run = run();
    close_all(&mut run, &[Step::Authorize, Step::Explore, Step::Resolve]);
    let mut blank = note("x");
    blank.reason = " \n".to_owned();
    assert_eq!(
        run.handoff_step(Step::Classify, Mark::Size(Size::Small), blank),
        Err(StepError::NoReason)
    );
    assert!(!run.steps().is_closed(Step::Classify));
    assert_eq!(run.steps().size(), None);
}

// Review 4: authorize may repeat to narrow; classify only until a later step
// closes.
#[test]
fn a_repeat_may_narrow_authorize_and_classify_locks_with_the_next_step() {
    let mut run = run();
    close_all(&mut run, &[Step::Authorize, Step::Explore, Step::Resolve]);
    run.handoff_step(
        Step::Authorize,
        Mark::Grant(Grant::ReadOnly),
        note("narrow"),
    )
    .expect("implement to read_only");
    assert!(run.steps().read_only());
    run.handoff_step(Step::Classify, Mark::Size(Size::Large), note("a"))
        .expect("first");
    run.handoff_step(Step::Classify, Mark::Size(Size::Small), note("b"))
        .expect("repeat before a later step");
    assert_eq!(run.steps().size(), Some(Size::Small));
    close_all(&mut run, &[Step::Implement]);
    assert_eq!(
        run.handoff_step(Step::Classify, Mark::Size(Size::Large), note("c")),
        Err(StepError::Locked {
            step: Step::Classify,
            later: Step::Implement
        })
    );
    assert_eq!(run.steps().size(), Some(Size::Small));
}

// Review 5: a step closes while the run is halted, its verdict forced to
// blocked as for a worker's handoff.
#[test]
fn a_step_closes_while_halted_with_a_blocked_verdict() {
    let mut run = run();
    failed_code(&mut run);
    let done = run
        .handoff_step(Step::Authorize, Mark::Grant(Grant::Implement), note("x"))
        .expect("closed");
    assert_eq!(done.handoff.verdict, Verdict::Blocked);
    assert!(run.steps().is_closed(Step::Authorize));
}

#[test]
fn a_goal_that_allows_implementing_spawns_a_code_worker() {
    let mut run = run();
    close_all(
        &mut run,
        &[
            Step::Authorize,
            Step::Explore,
            Step::Resolve,
            Step::Classify,
        ],
    );
    assert!(run.spawn(Instant::now(), Task::Code, "p").is_ok());
}

// Spec (the ledger keeps the steps): a degraded run still shows them.
#[test]
fn the_ledger_lists_the_closed_steps_with_grant_and_size() {
    let mut run = run();
    assert!(!run.run_ledger("goal").render().contains("Steps"));
    run.handoff_step(Step::Authorize, Mark::Grant(Grant::ReadOnly), note("x"))
        .expect("a");
    close_all(&mut run, &[Step::Explore, Step::Resolve]);
    let mut big = note("x");
    big.reason = "needs a design\nsecond line".to_owned();
    run.handoff_step(Step::Classify, Mark::Size(Size::Large), big)
        .expect("c");
    let text = run.run_ledger("goal").render();
    assert!(
        text.contains(
            "Steps: authorize (read_only), explore, resolve, classify (large: needs a design)"
        ),
        "{text}"
    );
}

#[test]
fn a_step_handoff_after_the_run_ended_is_refused() {
    let mut run = run();
    run.finish("s").expect("finished");
    assert_eq!(
        run.handoff_step(Step::Authorize, Mark::Grant(Grant::Implement), note("x")),
        Err(StepError::Ended)
    );
}

// The tool: `id` xor `step`; the mark belongs to its step alone.
#[test]
fn the_handoff_tool_takes_a_step_or_a_worker_never_both() {
    let step = parse(
        "handoff",
        &json!({"step": "authorize", "scope": "read_only", "verdict": "on_course"}),
    );
    let Ok(OrchCall::StepHandoff { step, mark, note }) = step else {
        panic!("{step:?}");
    };
    assert_eq!(
        (step, mark, note.verdict),
        (
            Step::Authorize,
            Mark::Grant(Grant::ReadOnly),
            Verdict::OnCourse
        )
    );
    let size = parse("handoff", &json!({"step": "classify", "size": "large"}));
    assert!(matches!(
        size,
        Ok(OrchCall::StepHandoff {
            mark: Mark::Size(Size::Large),
            ..
        })
    ));
    assert!(matches!(
        parse("handoff", &json!({"step": "explore", "done": "d"})),
        Ok(OrchCall::StepHandoff {
            mark: Mark::None,
            ..
        })
    ));
    for (args, error) in [
        (
            json!({"id": 1, "step": "explore"}),
            OrchError::ExtraArgument,
        ),
        (json!({}), OrchError::MissingArgument),
        (json!({"step": "plan"}), OrchError::WrongType),
        (json!({"step": 3}), OrchError::WrongType),
        (json!({"step": "authorize"}), OrchError::MissingArgument),
        (
            json!({"step": "authorize", "scope": "root"}),
            OrchError::WrongType,
        ),
        (json!({"step": "classify"}), OrchError::MissingArgument),
        (
            json!({"step": "classify", "size": "huge"}),
            OrchError::WrongType,
        ),
        (
            json!({"step": "explore", "size": "small"}),
            OrchError::ExtraArgument,
        ),
        (
            json!({"step": "classify", "size": "small", "scope": "read_only"}),
            OrchError::ExtraArgument,
        ),
        (
            json!({"id": 1, "scope": "read_only"}),
            OrchError::ExtraArgument,
        ),
    ] {
        assert_eq!(parse("handoff", &args), Err(error), "{args}");
    }
    assert!(matches!(
        parse("handoff", &json!({"id": 1})),
        Ok(OrchCall::Handoff { .. })
    ));
}

// Spec (orchestrator-handoffs, A step handoff says what closed): the run's own
// step state names what is closed and what comes next; `track` is skipped for a
// small goal and named for a large one.
#[test]
fn the_steps_name_what_is_closed_and_what_comes_next() {
    let mut run = run();
    assert_eq!(run.steps().closed_steps(), Vec::<Step>::new());
    assert_eq!(run.steps().next_open(), Some(Step::Authorize));
    close_all(&mut run, &[Step::Authorize, Step::Explore]);
    assert_eq!(run.steps().closed_steps(), [Step::Authorize, Step::Explore]);
    assert_eq!(run.steps().next_open(), Some(Step::Resolve));
    close_all(&mut run, &[Step::Resolve, Step::Classify]);
    assert_eq!(run.steps().next_open(), Some(Step::Implement));
    close_all(&mut run, &[Step::Implement, Step::Close]);
    assert_eq!(run.steps().next_open(), None);

    let mut large = self::run();
    close_all(&mut large, &[Step::Authorize, Step::Explore, Step::Resolve]);
    let mark = Mark::Size(Size::Large);
    large
        .handoff_step(Step::Classify, mark, note("x"))
        .expect("closed");
    assert_eq!(large.steps().next_open(), Some(Step::Track));
}

// A handoff closes its step whatever the verdict, and the reply says so; a
// missing verdict says how to give it.
#[test]
fn the_step_reply_says_closed_the_verdict_and_the_next_step() {
    let mut run = run();
    close_all(&mut run, &[Step::Authorize]);
    let reply = run.steps().reply(Step::Authorize, Verdict::OnCourse);
    assert_eq!(
        reply,
        "step authorize closed; verdict on_course; next: explore"
    );
    let reply = run.steps().reply(Step::Authorize, Verdict::Unknown);
    assert_eq!(
        reply,
        "step authorize closed; verdict unknown (give verdict: on_course, off_course or \
         blocked); next: explore"
    );
    close_all(&mut run, &[Step::Explore, Step::Resolve, Step::Classify]);
    let reply = run.steps().reply(Step::Classify, Verdict::Blocked);
    assert!(
        reply.ends_with("next: implement (the spawn gate is open)"),
        "{reply}"
    );
    close_all(&mut run, &[Step::Implement, Step::Close]);
    let reply = run.steps().reply(Step::Close, Verdict::OnCourse);
    assert!(reply.ends_with("all steps closed"), "{reply}");
    assert!(reply.is_ascii());
}

// A read-only goal still refuses code workers, so the reply says what may be
// spawned instead of calling the spawn open.
#[test]
fn the_step_reply_on_a_read_only_goal_says_what_may_be_spawned() {
    let mut run = run();
    run.handoff_step(Step::Authorize, Mark::Grant(Grant::ReadOnly), note("x"))
        .expect("closed");
    close_all(&mut run, &[Step::Explore, Step::Resolve, Step::Classify]);
    let reply = run.steps().reply(Step::Classify, Verdict::OnCourse);
    assert!(
        reply.ends_with("next: implement (read-only: a simple worker with no files to write)"),
        "{reply}"
    );
    assert!(
        !reply.contains("spawn gate is open") && reply.is_ascii(),
        "{reply}"
    );
}
