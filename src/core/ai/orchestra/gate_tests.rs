//! The step gates (agent-orchestrator, "Gates on step state are visible and
//! bounded [W3]"; design D23): a write-capable spawn needs the steps before
//! `implement` closed, and `finish` after an `implement` grant needs a
//! write-capable worker.

use std::path::Path;
use std::time::{Duration, Instant};

use super::brief::Brief;
use super::ledger::{FinishError, Judgement, Verdict};
use super::run::{Refusal, Roster, Run, RunLimits, RunState, UserChoice, WorkerEvent};
use super::steps::{Gate, Grant, Mark, Size, Step, StepError, WRITE_OFF};
use super::worker::{ErrorClass, WorkerStatus};
use super::{RunId, WorkerId};
use crate::core::ai::chat::engine::Availability;
use crate::core::ai::chat::stream::ERROR_CAP;
use crate::core::ai::config::Task;
use crate::core::ai::tools::registry::Family;

fn roster() -> Roster {
    let ready = Availability::Ready {
        engine: "claude".to_owned(),
        model: Some("opus".to_owned()),
    };
    Roster::new(vec![
        (Task::Orchestrate, ready.clone()),
        (Task::Code, ready.clone()),
        (Task::Review, ready.clone()),
        (Task::General, ready.clone()),
        (Task::Summarize, ready),
    ])
}

fn run() -> Run {
    Run::new(RunId(3), roster(), Duration::from_secs(600)).with_limits(RunLimits {
        parallel: 4,
        ..Default::default()
    })
}

fn note() -> Judgement {
    Judgement {
        verdict: Verdict::OnCourse,
        reason: "fits".to_owned(),
        done: "x".to_owned(),
        left: String::new(),
        decisions: String::new(),
    }
}

fn close(run: &mut Run, step: Step, mark: Mark) {
    run.handoff_step(step, mark, note()).expect("closed");
}

fn authorize(run: &mut Run, grant: Grant) {
    close(run, Step::Authorize, Mark::Grant(grant));
}

/// Closes every step before `implement` for a small goal (`track` is skipped).
fn close_early(run: &mut Run) {
    authorize(run, Grant::Implement);
    close(run, Step::Explore, Mark::None);
    close(run, Step::Resolve, Mark::None);
    close(run, Step::Classify, Mark::Size(Size::Small));
}

// Spec (W3, A gate names itself): a `code` spawn with the early steps open is
// refused with the gate and the first open step.
#[test]
fn a_code_spawn_with_steps_open_is_refused_naming_the_first_open_step() {
    let mut run = run();
    let now = Instant::now();
    assert_eq!(
        run.spawn(now, Task::Code, "p"),
        Err(Refusal::Gate(Step::Authorize))
    );
    authorize(&mut run, Grant::Implement);
    assert_eq!(
        run.spawn(now, Task::Code, "p"),
        Err(Refusal::Gate(Step::Explore))
    );
    close(&mut run, Step::Explore, Mark::None);
    close(&mut run, Step::Resolve, Mark::None);
    assert_eq!(
        run.spawn(now, Task::Code, "p"),
        Err(Refusal::Gate(Step::Classify))
    );
    assert_eq!(run.last_gate(), Some((Gate::Spawn, Step::Classify)));
}

// A refusal creates no slot: the worker never existed.
#[test]
fn a_gate_refusal_creates_no_worker() {
    let mut run = run();
    let _ = run.spawn(Instant::now(), Task::Code, "p");
    assert_eq!(run.status(WorkerId(1)), None);
}

// A small goal skips `track`: closing through `classify` is enough.
#[test]
fn a_code_spawn_passes_once_the_steps_through_classify_are_closed() {
    let mut run = run();
    close_early(&mut run);
    assert_eq!(run.spawn(Instant::now(), Task::Code, "p"), Ok(WorkerId(1)));
}

// Past `authorize`, the gate is on the worker's role: reading and summarising
// never wait for the other steps.
#[test]
fn only_a_write_capable_role_is_gated_past_authorize() {
    let mut run = run();
    authorize(&mut run, Grant::Implement);
    let now = Instant::now();
    for task in [Task::Review, Task::Summarize, Task::General] {
        assert!(run.spawn(now, task, "p").is_ok(), "{task:?}");
    }
    assert_eq!(run.last_gate(), None);
}

// Precedence, pinned: paused or ended first, then read-only (it says
// read-only, not "open step"), then the worker cap, then the gate, then
// Write off.
#[test]
fn the_gate_comes_after_read_only_and_the_cap_and_before_write_off() {
    let now = Instant::now();

    let mut paused = run();
    authorize(&mut paused, Grant::Implement);
    let id = paused.spawn(now, Task::Review, "p").unwrap();
    paused.on_event(
        now,
        id,
        WorkerEvent::Failed {
            class: ErrorClass::Other,
            text: "x".to_owned(),
        },
    );
    assert_eq!(paused.spawn(now, Task::Code, "p"), Err(Refusal::Paused));

    let mut read_only = run();
    authorize(&mut read_only, Grant::ReadOnly);
    assert_eq!(
        read_only.spawn(now, Task::Code, "p"),
        Err(Refusal::ReadOnly)
    );

    let mut capped = Run::new(RunId(4), roster(), Duration::from_secs(60)).with_limits(RunLimits {
        workers: 1,
        ..Default::default()
    });
    authorize(&mut capped, Grant::Implement);
    capped.spawn(now, Task::Review, "p").unwrap();
    assert_eq!(
        capped.spawn(now, Task::Code, "p"),
        Err(Refusal::WorkerCap(1))
    );

    // Write off with the steps open is still the gate's refusal.
    let mut off = run();
    off.set_ceiling(crate::core::ai::tools::registry::FamilySet::ALL.without(Family::Write));
    assert_eq!(
        off.spawn(now, Task::Code, "p"),
        Err(Refusal::Gate(Step::Authorize))
    );
    close_early(&mut off);
    assert_eq!(off.spawn(now, Task::Code, "p"), Err(Refusal::WriteOff));
}

// Spec (W3, Only the user overrides): lifting passes both gates for the run.
#[test]
fn lifting_the_gates_lets_a_code_spawn_through_with_steps_open() {
    let mut run = run();
    assert!(run.spawn(Instant::now(), Task::Code, "p").is_err());
    run.lift_gates();
    assert_eq!(run.spawn(Instant::now(), Task::Code, "p"), Ok(WorkerId(1)));
    assert_eq!(run.last_gate(), None);
}

// Spec (W3, A gate names itself): the text names the gate and the step, and
// tells the model when it may spawn again: after the handoff, which it can
// see, not after a lift, which it cannot. Whole after the runtime's cap.
#[test]
fn the_refusal_text_names_the_gate_and_the_step() {
    for step in Step::ALL {
        let spawn = Gate::Spawn.refusal(step, &[]);
        assert!(spawn.starts_with("spawn gate:"), "{spawn}");
        assert!(spawn.contains(&format!("step {}", step.label())), "{spawn}");
        assert!(!spawn.contains("do not retry"), "{spawn}");
        assert!(
            spawn.contains("spawn again only after it is handed off"),
            "{spawn}"
        );
        let finish = Gate::Finish.refusal(step, &[]);
        assert!(finish.starts_with("finish gate:"), "{finish}");
        assert!(finish.contains("Write off"), "{finish}");
        for text in [spawn, finish] {
            assert!(
                text.ends_with("Only the user can lift the gate (Enter gates off)"),
                "{text}"
            );
            assert!(text.is_ascii(), "{text}");
            assert!(
                text.len() <= ERROR_CAP,
                "{} > {ERROR_CAP}: {text}",
                text.len()
            );
        }
    }
}

// The gate memory is for the turn cap: it names a gate that still stands.
#[test]
fn the_remembered_gate_goes_when_the_gate_passes() {
    let mut run = run();
    let _ = run.spawn(Instant::now(), Task::Code, "p");
    assert!(run.last_gate().is_some());
    close_early(&mut run);
    run.spawn(Instant::now(), Task::Code, "p").unwrap();
    assert_eq!(run.last_gate(), None);
}

// Spec (W3, 13.2): `finish` after an `implement` grant needs a write-capable
// worker.
#[test]
fn finish_after_an_implement_grant_with_no_code_worker_is_refused() {
    let mut run = run();
    authorize(&mut run, Grant::Implement);
    assert_eq!(run.finish("done"), Err(FinishError::Gate));
    assert_eq!(run.state(), &RunState::Open);
    assert_eq!(run.last_gate(), Some((Gate::Finish, Step::Implement)));
}

// Owner decision: the gate also covers the handoffs that end the run's work.
// `implement` and `close` need a writer after an `implement` grant, or the
// model could close both with no worker and settle the run.
#[test]
fn handing_off_implement_or_close_without_a_writer_is_refused() {
    let mut run = run();
    close_early(&mut run);
    let err = run.handoff_step(Step::Implement, Mark::None, note());
    assert_eq!(err.err(), Some(StepError::Gate));
    assert_eq!(run.last_gate(), Some((Gate::Finish, Step::Implement)));
    assert!(!run.steps().is_closed(Step::Implement));

    // `close` is not reachable before `implement` closes: the order check
    // answers it first and the gate is not touched.
    run.forget_gate();
    let err = run.handoff_step(Step::Close, Mark::None, note());
    assert_eq!(
        err.err(),
        Some(StepError::Order {
            step: Step::Close,
            open: Step::Implement
        })
    );
    assert_eq!(run.last_gate(), None);
}

#[test]
fn a_queued_writer_lets_implement_and_close_be_handed_off() {
    let mut run = run();
    close_early(&mut run);
    run.spawn(Instant::now(), Task::Code, "p").unwrap();
    close(&mut run, Step::Implement, Mark::None);
    close(&mut run, Step::Close, Mark::None);
}

// A refused spawn (the engine cannot run) leaves a `Refused` slot: no worker
// ran, so it must not lift the gate.
#[test]
fn a_refused_write_slot_does_not_lift_the_gate() {
    let down = Availability::Undetected {
        engine: "claude".to_owned(),
        status: "not found".to_owned(),
    };
    let ready = Availability::Ready {
        engine: "claude".to_owned(),
        model: Some("opus".to_owned()),
    };
    let roster = Roster::new(vec![
        (Task::Orchestrate, ready.clone()),
        (Task::Code, down),
        (Task::Review, ready.clone()),
        (Task::General, ready.clone()),
        (Task::Summarize, ready),
    ]);
    let mut run = Run::new(RunId(3), roster, Duration::from_secs(600));
    close_early(&mut run);
    let id = run.spawn(Instant::now(), Task::Code, "p").unwrap();
    assert_eq!(run.status(id), Some(WorkerStatus::Refused));
    assert_eq!(
        run.handoff_step(Step::Implement, Mark::None, note()).err(),
        Some(StepError::Gate)
    );
}

#[test]
fn a_read_only_grant_closes_every_step_with_no_worker() {
    let mut run = run();
    authorize(&mut run, Grant::ReadOnly);
    close(&mut run, Step::Explore, Mark::None);
    close(&mut run, Step::Resolve, Mark::None);
    close(&mut run, Step::Classify, Mark::Size(Size::Small));
    close(&mut run, Step::Implement, Mark::None);
    close(&mut run, Step::Close, Mark::None);
    assert!(run.finish("done").is_ok());
}

#[test]
fn lifted_gates_let_implement_and_close_close_with_no_writer() {
    let mut run = run();
    close_early(&mut run);
    run.lift_gates();
    close(&mut run, Step::Implement, Mark::None);
    close(&mut run, Step::Close, Mark::None);
}

// A code worker that ended still satisfies the gate: it ran.
#[test]
fn finish_passes_once_a_code_worker_ran_even_an_ended_one() {
    let mut run = run();
    close_early(&mut run);
    let now = Instant::now();
    let id = run.spawn(now, Task::Code, "p").unwrap();
    run.cancel(now, id);
    assert_eq!(run.status(id), Some(WorkerStatus::Cancelled));
    assert!(run.finish("done").is_err()); // handoff missing, asked once
    assert!(run.finish("done").is_ok());
}

// A text-only answer is not gated: no `authorize`, or a read-only grant.
#[test]
fn finish_is_not_gated_without_an_implement_grant() {
    let mut none = run();
    assert_eq!(none.finish("answer"), Ok(vec![]));

    let mut read_only = run();
    authorize(&mut read_only, Grant::ReadOnly);
    assert_eq!(read_only.finish("answer"), Ok(vec![]));
}

// Risk 4 (the gate trap): a spawn the gate never saw leaves no slot, so Write
// off, the worker cap and a goal narrowed to read-only each leave `finish`
// refused. The trap is bounded: the text names the override, the lift passes
// it, and a narrowed goal does not trap at all.
#[test]
fn a_code_worker_that_could_not_start_does_not_satisfy_the_finish_gate() {
    let now = Instant::now();

    let mut write_off = run();
    close_early(&mut write_off);
    write_off.set_ceiling(crate::core::ai::tools::registry::FamilySet::ALL.without(Family::Write));
    assert_eq!(
        write_off.spawn(now, Task::Code, "p"),
        Err(Refusal::WriteOff)
    );
    assert_eq!(write_off.finish("done"), Err(FinishError::Gate));
    assert!(
        Gate::Finish
            .refusal(Step::Implement, &[])
            .contains("Write off")
    );
    write_off.lift_gates();
    assert_eq!(write_off.finish("done"), Ok(vec![]));

    let mut capped = Run::new(RunId(4), roster(), Duration::from_secs(60)).with_limits(RunLimits {
        workers: 1,
        ..Default::default()
    });
    close_early(&mut capped);
    let lens = capped.spawn(now, Task::Review, "p").unwrap();
    capped.cancel(now, lens);
    assert_eq!(
        capped.spawn(now, Task::Code, "p"),
        Err(Refusal::WorkerCap(1))
    );
    assert!(capped.finish("done").is_err());
    assert_eq!(capped.finish("done"), Err(FinishError::Gate));

    // Narrowing to read-only after `implement` was granted: no trap.
    let mut narrowed = run();
    close_early(&mut narrowed);
    authorize(&mut narrowed, Grant::ReadOnly);
    assert_eq!(narrowed.spawn(now, Task::Code, "p"), Err(Refusal::ReadOnly));
    assert_eq!(narrowed.finish("done"), Ok(vec![]));
}

// A gated run is not trapped by the coordinator alone: a text reply with
// nothing pending settles it.
#[test]
fn a_refused_finish_does_not_keep_the_run_going() {
    let mut run = run();
    authorize(&mut run, Grant::Implement);
    assert_eq!(run.finish("done"), Err(FinishError::Gate));
    assert_eq!(run.next_turn(), super::run::NextTurn::Settle);
}

// W3 3: the correction worker lazarobox starts is not the coordinator's
// spawn. GUARD: it passes by design and must stay ungated.
#[test]
fn an_internal_code_worker_is_not_gated_and_never_rechecked() {
    let mut run = run();
    let now = Instant::now();
    let id = run.spawn_internal(now, Task::Code, "fix").unwrap();
    assert_eq!(run.last_gate(), None);
    // Retry, relaunch and the queue never ask the gate again.
    let fail = || WorkerEvent::Failed {
        class: ErrorClass::RateLimit,
        text: "429".to_owned(),
    };
    run.on_event(now, id, fail());
    assert_eq!(run.decide(now, UserChoice::Retry), Ok(vec![]));
    assert_eq!(run.status(id), Some(WorkerStatus::Running));
    run.on_event(now, id, fail());
    run.decide(now, UserChoice::Relaunch(roster())).unwrap();
    assert_eq!(run.status(id), Some(WorkerStatus::Running));
}

#[test]
fn a_queued_internal_code_worker_starts_from_the_queue_with_steps_open() {
    let mut run = Run::new(RunId(3), roster(), Duration::from_secs(60));
    authorize(&mut run, Grant::Implement);
    let now = Instant::now();
    let first = run.spawn(now, Task::Review, "p").unwrap();
    let queued = run.spawn_internal(now, Task::Code, "fix").unwrap();
    assert_eq!(run.status(queued), Some(WorkerStatus::Queued));
    run.on_event(
        now,
        first,
        WorkerEvent::Done {
            text: "ok".to_owned(),
        },
    );
    assert_eq!(run.status(queued), Some(WorkerStatus::Running));
}

// The write-off text stays the refusal for a closed-steps spawn.
#[test]
fn write_off_keeps_its_text() {
    assert!(WRITE_OFF.contains("/write"));
}

// The cap message names a gate that still stands: handing off the step that
// blocked the spawn moves the memory to the next open step, and closing them
// all clears it.
#[test]
fn handing_off_the_blocking_step_updates_the_remembered_gate() {
    let mut run = run();
    let _ = run.spawn(Instant::now(), Task::Code, "p");
    assert_eq!(run.last_gate(), Some((Gate::Spawn, Step::Authorize)));
    authorize(&mut run, Grant::Implement);
    assert_eq!(run.last_gate(), Some((Gate::Spawn, Step::Explore)));
    close(&mut run, Step::Explore, Mark::None);
    close(&mut run, Step::Resolve, Mark::None);
    close(&mut run, Step::Classify, Mark::Size(Size::Small));
    assert_eq!(run.last_gate(), None);
}

// The finish gate asks that a code worker RAN, not that it succeeded: one that
// failed and was retried, or was cancelled at once, still counts.
#[test]
fn a_code_worker_that_failed_or_was_cancelled_counts_as_having_run() {
    let now = Instant::now();
    let mut failed = run();
    close_early(&mut failed);
    let id = failed.spawn(now, Task::Code, "p").unwrap();
    failed.on_event(
        now,
        id,
        WorkerEvent::Failed {
            class: ErrorClass::Other,
            text: "boom".to_owned(),
        },
    );
    assert_eq!(failed.finish("done"), Err(FinishError::Paused));
    failed.decide(now, UserChoice::Retry).unwrap();
    failed.on_event(
        now,
        id,
        WorkerEvent::Done {
            text: "ok".to_owned(),
        },
    );
    assert!(
        failed.finish("done").is_err(),
        "handoff missing, asked once"
    );
    assert!(failed.finish("done").is_ok());

    let mut cancelled = run();
    close_early(&mut cancelled);
    let id = cancelled.spawn(now, Task::Code, "p").unwrap();
    cancelled.cancel(now, id);
    assert_ne!(cancelled.finish("done"), Err(FinishError::Gate));
}

// Option B (W3 follow-up): a turn that ends with a spawn refusal standing and
// nothing pending does not settle the run; it waits for the user's reply.
#[test]
fn a_standing_spawn_refusal_makes_the_run_wait_for_the_reply() {
    use super::run::NextTurn;
    let mut run = run();
    let _ = run.spawn(Instant::now(), Task::Code, "p");
    assert_eq!(run.next_turn(), NextTurn::Wait(Step::Authorize));
    // The reply turn takes a turn of the budget, like a continuation.
    assert_eq!(run.turns(), 2);
}

#[test]
fn a_lifted_or_passed_gate_does_not_make_the_run_wait() {
    use super::run::NextTurn;
    let mut lifted = run();
    let _ = lifted.spawn(Instant::now(), Task::Code, "p");
    lifted.lift_gates();
    assert!(lifted.gates_lifted());
    assert_eq!(lifted.next_turn(), NextTurn::Settle);
    let mut forgotten = run();
    let _ = forgotten.spawn(Instant::now(), Task::Code, "p");
    forgotten.forget_gate();
    assert_eq!(forgotten.next_turn(), NextTurn::Settle);
}

#[test]
fn waiting_for_the_reply_stops_at_the_turn_cap() {
    use super::run::NextTurn;
    let mut run = run().with_limits(RunLimits {
        turns: 1,
        ..Default::default()
    });
    let _ = run.spawn(Instant::now(), Task::Code, "p");
    assert_eq!(run.next_turn(), NextTurn::Capped(1));
}

// A spawn refusal names the steps already closed, so a model cannot think its
// handoffs were lost.
#[test]
fn the_spawn_refusal_names_the_closed_steps() {
    let closed = [Step::Authorize, Step::Explore];
    let text = Gate::Spawn.refusal(Step::Resolve, &closed);
    assert!(
        text.starts_with("spawn gate: step resolve is not closed (closed: authorize, explore)."),
        "{text}"
    );
    assert!(text.ends_with("Only the user can lift the gate (Enter gates off)"));
    assert!(text.len() <= ERROR_CAP && text.is_ascii());
    let none = Gate::Spawn.refusal(Step::Authorize, &[]);
    assert!(none.contains("(closed: none)"), "{none}");
}

// The longest spawn refusal still fits the coordinator's error cap: the open
// step comes after every closed one.
#[test]
fn the_spawn_refusal_fits_the_cap_with_every_earlier_step_closed() {
    // The spawn gate only ever names a step before `implement`.
    for (at, step) in Step::ALL.into_iter().take(5).enumerate() {
        let text = Gate::Spawn.refusal(step, &Step::ALL[..at]);
        assert!(text.len() <= ERROR_CAP && text.is_ascii(), "{text}");
        assert!(text.ends_with("(Enter gates off)"), "{text}");
    }
}

fn read_brief() -> Brief {
    Brief {
        goal: "explain the flag".to_owned(),
        inputs: vec!["src/a.rs".to_owned()],
        outputs: None,
        done_when: "it is explained".to_owned(),
        out_of_scope: "docs".to_owned(),
        ..Brief::default()
    }
}

fn read_simple(run: &mut Run) -> Result<WorkerId, Refusal> {
    run.spawn_simple(Instant::now(), read_brief(), Path::new("/work/app"))
}

// Spec (W3, A read worker is refused before authorize; Every worker kind is
// covered): nothing starts while `authorize` is open, whatever the kind.
#[test]
fn every_worker_kind_is_refused_while_authorize_is_open() {
    let mut run = run();
    let now = Instant::now();
    for task in [
        Task::Review,
        Task::Summarize,
        Task::General,
        Task::Chat,
        Task::Code,
    ] {
        assert_eq!(
            run.spawn(now, task, "p"),
            Err(Refusal::Gate(Step::Authorize)),
            "{task:?}"
        );
    }
    assert_eq!(read_simple(&mut run), Err(Refusal::Gate(Step::Authorize)));
    assert_eq!(run.status(WorkerId(1)), None);
    assert_eq!(run.last_gate(), Some((Gate::Spawn, Step::Authorize)));
}

// Spec (Read workers start after authorize read_only).
#[test]
fn read_workers_start_after_authorize_read_only() {
    let mut run = run();
    authorize(&mut run, Grant::ReadOnly);
    assert_eq!(
        run.spawn(Instant::now(), Task::Review, "p"),
        Ok(WorkerId(1))
    );
    assert_eq!(read_simple(&mut run), Ok(WorkerId(2)));
}

// Spec (Writers keep the full gate): a review starts while a code worker
// waits at `explore`.
#[test]
fn a_writer_is_refused_at_explore_while_a_review_starts() {
    let mut run = run();
    authorize(&mut run, Grant::Implement);
    let now = Instant::now();
    assert_eq!(
        run.spawn(now, Task::Code, "p"),
        Err(Refusal::Gate(Step::Explore))
    );
    assert_eq!(run.spawn(now, Task::Review, "p"), Ok(WorkerId(1)));
    assert_eq!(run.last_gate(), Some((Gate::Spawn, Step::Explore)));
}

// A refused read spawn leaves no stale gate once authorize closes: the turn
// settles instead of waiting.
#[test]
fn a_refused_read_spawn_leaves_no_stale_gate_after_authorize() {
    use super::run::NextTurn;
    let mut run = run();
    let _ = run.spawn(Instant::now(), Task::Review, "p");
    assert_eq!(run.last_gate(), Some((Gate::Spawn, Step::Authorize)));
    authorize(&mut run, Grant::ReadOnly);
    assert_eq!(run.last_gate(), None);
    assert_eq!(
        run.spawn(Instant::now(), Task::Review, "p"),
        Ok(WorkerId(1))
    );
    assert_eq!(run.last_gate(), None);
    assert_ne!(run.next_turn(), NextTurn::Wait(Step::Authorize));
}

// A.7: lenses, refuter, corrector and judges are lazarobox's own spawns.
#[test]
fn an_internal_review_worker_is_never_gated() {
    let mut run = run();
    let now = Instant::now();
    assert_eq!(run.spawn_internal(now, Task::Review, "p"), Ok(WorkerId(1)));
    assert_eq!(run.last_gate(), None);
}

// The answer gate (agent-orchestrator, "Gates on step state are visible and
// bounded"): what follows a turn that ended with no tool call.
mod answer {
    use super::*;
    use crate::core::ai::orchestra::run::{NextTurn, TurnEnd};

    const TEXT: TurnEnd = TurnEnd {
        answered: true,
        written: None,
    };

    fn written(tool: &'static str) -> TurnEnd {
        TurnEnd {
            answered: true,
            written: Some(tool),
        }
    }

    fn reask(step: Step, written: Option<&'static str>) -> NextTurn {
        NextTurn::Reask { step, written }
    }

    #[test]
    fn a_text_end_before_authorize_is_re_asked_for_authorize() {
        let mut run = run();
        assert_eq!(run.next_turn_after(TEXT), reask(Step::Authorize, None));
        assert_eq!(run.last_gate(), Some((Gate::Answer, Step::Authorize)));
    }

    #[test]
    fn an_implement_grant_without_a_writer_is_re_asked_at_the_next_open_step() {
        let mut run = run();
        authorize(&mut run, Grant::Implement);
        assert_eq!(run.next_turn_after(TEXT), reask(Step::Explore, None));
        close(&mut run, Step::Explore, Mark::None);
        assert_eq!(run.next_turn_after(TEXT), reask(Step::Resolve, None));
    }

    #[test]
    fn an_end_after_a_writer_started_is_not_re_asked() {
        let mut run = run();
        close_early(&mut run);
        let id = run.spawn(Instant::now(), Task::Code, "p").unwrap();
        run.on_event(
            Instant::now(),
            id,
            WorkerEvent::Done {
                text: "ok".to_owned(),
            },
        );
        run.fallbacks();
        assert_eq!(run.next_turn_after(TEXT), NextTurn::Settle);
    }

    #[test]
    fn a_read_only_answer_settles_but_a_written_call_is_re_asked() {
        let mut plain = run();
        authorize(&mut plain, Grant::ReadOnly);
        assert_eq!(plain.next_turn_after(TEXT), NextTurn::Settle);
        let mut call = run();
        authorize(&mut call, Grant::ReadOnly);
        assert_eq!(
            call.next_turn_after(written("spawn_agent")),
            reask(Step::Explore, Some("spawn_agent"))
        );
    }

    #[test]
    fn lifted_gates_a_pending_worker_and_an_unanswered_turn_are_not_re_asked() {
        let mut lifted = run();
        lifted.lift_gates();
        assert_eq!(lifted.next_turn_after(TEXT), NextTurn::Settle);
        let mut unanswered = run();
        assert_eq!(
            unanswered.next_turn_after(TurnEnd::default()),
            NextTurn::Settle
        );
        let mut pending = run();
        authorize(&mut pending, Grant::ReadOnly);
        pending.spawn(Instant::now(), Task::Review, "p").unwrap();
        assert_eq!(
            pending.next_turn_after(written("wait")),
            NextTurn::Continue(2)
        );
    }

    #[test]
    fn a_standing_spawn_refusal_takes_the_wait_not_a_re_ask() {
        let mut run = run();
        let _ = run.spawn(Instant::now(), Task::Code, "p");
        assert_eq!(run.next_turn_after(TEXT), NextTurn::Wait(Step::Authorize));
    }

    #[test]
    fn each_step_gets_one_re_ask_then_the_run_waits_and_a_reply_clears_nothing() {
        let mut run = run();
        assert_eq!(run.next_turn_after(TEXT), reask(Step::Authorize, None));
        assert_eq!(run.next_turn_after(TEXT), NextTurn::Wait(Step::Authorize));
        assert_eq!(run.last_gate(), Some((Gate::Answer, Step::Authorize)));
        // The user's reply starts clean; a text-only turn waits again.
        run.forget_gate();
        assert_eq!(run.next_turn_after(TEXT), NextTurn::Wait(Step::Authorize));
        authorize(&mut run, Grant::Implement);
        assert_eq!(run.next_turn_after(TEXT), reask(Step::Explore, None));
        assert_eq!(run.next_turn_after(TEXT), NextTurn::Wait(Step::Explore));
    }

    #[test]
    fn a_re_ask_and_a_wait_each_count_a_turn_and_the_cap_names_the_gate() {
        let mut run = run().with_limits(RunLimits {
            turns: 3,
            ..Default::default()
        });
        assert_eq!(run.next_turn_after(TEXT), reask(Step::Authorize, None));
        assert_eq!(run.turns(), 2);
        assert_eq!(run.next_turn_after(TEXT), NextTurn::Wait(Step::Authorize));
        assert_eq!(run.turns(), 3);
        assert_eq!(run.next_turn_after(TEXT), NextTurn::Capped(3));
        assert_eq!(run.last_gate(), Some((Gate::Answer, Step::Authorize)));
    }

    #[test]
    fn the_authorize_handoff_ends_the_gate_for_that_step() {
        let mut run = run();
        assert_eq!(run.next_turn_after(TEXT), reask(Step::Authorize, None));
        authorize(&mut run, Grant::ReadOnly);
        assert_eq!(run.next_turn_after(TEXT), NextTurn::Settle);
        assert_eq!(run.last_gate(), None);
    }

    #[test]
    fn the_re_ask_texts_name_the_step_and_a_written_call_without_a_gap() {
        let mut run = run();
        let (text, line) = run.reask_texts(Step::Authorize, None);
        assert!(
            text.contains("step authorize is not closed (closed: none)"),
            "{text}"
        );
        assert!(line.contains("before handing off authorize"), "{line}");
        authorize(&mut run, Grant::ReadOnly);
        let (text, line) = run.reask_texts(Step::Explore, Some("handoff"));
        assert_eq!(
            text,
            "answer gate: you wrote a call to handoff as text; nothing was run. Make a real tool call."
        );
        assert!(line.contains("wrote a handoff call as text"), "{line}");
    }

    #[test]
    fn undelegated_is_false_after_a_read_only_grant_and_true_with_the_gates_lifted() {
        let mut read_only = run();
        assert!(read_only.undelegated());
        authorize(&mut read_only, Grant::ReadOnly);
        assert!(!read_only.undelegated());
        let mut lifted = run();
        authorize(&mut lifted, Grant::Implement);
        lifted.lift_gates();
        assert!(lifted.undelegated());
    }
}
