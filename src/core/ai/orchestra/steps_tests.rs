//! The ODD steps and their handoffs (agent-orchestrator S5, D15).

use std::time::{Duration, Instant};

use super::RunId;
use super::ledger::{Judgement, Unit, Verdict};
use super::run::{Refusal, Roster, Run, RunLimits};
use super::steps::{Grant, Mark, Size, Step, StepError};
use super::tools::{OrchCall, OrchError, parse};
use crate::core::ai::chat::engine::Availability;
use crate::core::ai::config::Task;
use serde_json::json;

fn run() -> Run {
    let ready = Availability::Ready {
        engine: "claude".to_owned(),
        model: Some("opus".to_owned()),
    };
    let roster = Roster::new(vec![
        (Task::Orchestrate, ready.clone()),
        (Task::Code, ready.clone()),
        (Task::Review, ready),
    ]);
    Run::new(RunId(5), roster, Duration::from_secs(600)).with_limits(RunLimits {
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
        .expect("stored");
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
    close_all(&mut large, &[Step::Track, Step::Implement, Step::Close]);

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
    run.handoff_step(
        Step::Authorize,
        Mark::Grant(Grant::Implement),
        note("again"),
    )
    .expect("replaced");
    assert_eq!(run.spawn(now, Task::Code, "p"), Err(Refusal::ReadOnly));
}

#[test]
fn a_goal_that_allows_implementing_spawns_a_code_worker() {
    let mut run = run();
    run.handoff_step(Step::Authorize, Mark::Grant(Grant::Implement), note("fix"))
        .expect("authorized");
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
