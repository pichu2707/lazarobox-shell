//! The handoff, the verdict and the ledger (agent-orchestrator S4, D17).

use std::time::{Duration, Instant};

use serde_json::json;

use super::RunId;
use super::WorkerId;
use super::ledger::{
    Facts, FinishError, Handoff, HandoffError, Judgement, LEDGER_BYTES, Ledger, NO_HANDOFF, NONE,
    Row, Unit, Verdict,
};
use super::run::{NextTurn, Refusal, Roster, Run, RunLimits, RunState, WorkerEvent};
use super::tools::{OrchCall, OrchError, parse};
use super::worker::{ErrorClass, WorkerStatus};
use crate::core::ai::chat::engine::Availability;
use crate::core::ai::config::Task;

fn run() -> Run {
    let ready = Availability::Ready {
        engine: "codex".to_owned(),
        model: Some("gpt".to_owned()),
    };
    let roster = Roster::new(vec![(Task::Review, ready.clone()), (Task::Code, ready)]);
    Run::new(RunId(7), roster, Duration::from_secs(600)).with_limits(super::run::RunLimits {
        parallel: 4,
        ..Default::default()
    })
}

/// A worker of `task` that ended `done` with `text` as its transcript.
fn finished(run: &mut Run, task: Task, text: &str) -> WorkerId {
    let now = Instant::now();
    let id = run.spawn(now, task, "p").expect("spawn");
    run.on_event(
        now,
        id,
        WorkerEvent::Done {
            text: text.to_owned(),
        },
    );
    id
}

fn note(verdict: Verdict) -> Judgement {
    Judgement {
        verdict,
        reason: "fits the goal".to_owned(),
        done: "wrote the parser".to_owned(),
        left: "tests".to_owned(),
        decisions: "kept serde".to_owned(),
    }
}

// Spec (orchestrator-handoffs, The orchestrator writes it with the handoff
// tool): the judgement fields come from the call, the rest from the run.
#[test]
fn a_handoff_takes_its_metadata_from_the_run_and_its_judgement_from_the_call() {
    let mut run = run();
    let id = finished(&mut run, Task::Review, "text");
    let handoff = run.handoff(id, note(Verdict::OnCourse)).expect("stored");
    assert_eq!(handoff.run, RunId(7));
    assert_eq!(handoff.unit, Unit::Worker(id));
    assert_eq!(handoff.kind, Task::Review);
    assert_eq!(handoff.engine_model(), "codex>gpt");
    assert_eq!(handoff.status, WorkerStatus::Done);
    assert_eq!(handoff.verdict, Verdict::OnCourse);
    assert_eq!(
        (&*handoff.reason, &*handoff.done, &*handoff.left),
        ("fits the goal", "wrote the parser", "tests")
    );
    assert_eq!(handoff.decisions, "kept serde");
    assert_eq!(run.stored_handoff(id), Some(&handoff));
}

// Spec (Handoff for a running worker is refused): nothing is stored.
#[test]
fn a_running_or_unknown_worker_has_no_handoff() {
    let mut run = run();
    let running = run.spawn(Instant::now(), Task::Review, "p").expect("spawn");
    assert_eq!(
        run.handoff(running, note(Verdict::OnCourse)),
        Err(HandoffError::NotFinished)
    );
    assert_eq!(
        run.handoff(WorkerId(9), note(Verdict::OnCourse)),
        Err(HandoffError::Unknown)
    );
    assert_eq!(run.stored_handoff(running), None);
}

// Spec (Empty field is explicit; D17 caps): blank fields read `none`, the
// reason is one short line, the long fields are cut.
#[test]
fn blank_fields_read_none_and_long_ones_are_cut() {
    let mut run = run();
    let id = finished(&mut run, Task::Review, "t");
    let judgement = Judgement {
        verdict: Verdict::OnCourse,
        reason: format!("first line\nsecond {}", "x".repeat(400)),
        done: "   \n".to_owned(),
        left: "y".repeat(5000),
        decisions: String::new(),
    };
    let handoff = run.handoff(id, judgement).expect("stored");
    assert_eq!(handoff.reason, "first line");
    assert_eq!((&*handoff.done, &*handoff.decisions), (NONE, NONE));
    assert_eq!(handoff.left.len(), 2048);
    let long = Judgement {
        reason: "r".repeat(500),
        ..note(Verdict::OnCourse)
    };
    assert_eq!(run.handoff(id, long).expect("stored").reason.len(), 200);
}

// Spec (Failure forces blocked): while a failure is unresolved the verdict is
// `blocked` whatever the orchestrator said.
#[test]
fn an_unresolved_failure_forces_blocked() {
    let mut run = run();
    let now = Instant::now();
    let ok = finished(&mut run, Task::Review, "t");
    let bad = run.spawn(now, Task::Code, "p").expect("spawn");
    run.on_event(
        now,
        bad,
        WorkerEvent::Failed {
            class: ErrorClass::Other,
            text: "boom".to_owned(),
        },
    );
    assert!(matches!(run.state(), RunState::Halted { .. }));
    let handoff = run.handoff(ok, note(Verdict::OnCourse)).expect("stored");
    assert_eq!(handoff.verdict, Verdict::Blocked);
    assert_eq!(run.forced_by(), Some(bad));
}

#[test]
fn a_repeated_handoff_replaces_the_first() {
    let mut run = run();
    let id = finished(&mut run, Task::Review, "t");
    run.handoff(id, note(Verdict::OffCourse)).expect("first");
    let second = Judgement {
        done: "redid it".to_owned(),
        ..note(Verdict::OnCourse)
    };
    run.handoff(id, second).expect("second");
    let stored = run.stored_handoff(id).expect("one");
    assert_eq!(
        (stored.verdict, &*stored.done),
        (Verdict::OnCourse, "redid it")
    );
}

// Spec (Verdict stored, Malformed verdict): the three values are kept, any
// other value or none is `unknown`.
#[test]
fn the_handoff_call_is_checked_and_a_bad_verdict_becomes_unknown() {
    let full = json!({
        "id": 2, "verdict": "off_course", "reason": "r", "done": "d",
        "left": "l", "decisions": "x"
    });
    assert_eq!(
        parse("handoff", &full),
        Ok(OrchCall::Handoff {
            id: WorkerId(2),
            note: Judgement {
                verdict: Verdict::OffCourse,
                reason: "r".to_owned(),
                done: "d".to_owned(),
                left: "l".to_owned(),
                decisions: "x".to_owned()
            }
        })
    );
    for (value, want) in [
        (json!("on_course"), Verdict::OnCourse),
        (json!("blocked"), Verdict::Blocked),
        (json!("great"), Verdict::Unknown),
        (json!("ON_COURSE"), Verdict::Unknown),
    ] {
        let Ok(OrchCall::Handoff { note, .. }) =
            parse("handoff", &json!({ "id": 1, "verdict": value }))
        else {
            panic!("{value}");
        };
        assert_eq!(note.verdict, want, "{value}");
    }
    let Ok(OrchCall::Handoff { note, .. }) = parse("handoff", &json!({ "id": 1 })) else {
        panic!("bare");
    };
    assert_eq!(note.verdict, Verdict::Unknown);
    for (args, error) in [
        (json!({}), OrchError::MissingArgument),
        (json!({ "id": 0 }), OrchError::WrongType),
        (json!({ "id": 1, "verdict": 3 }), OrchError::WrongType),
        (json!({ "id": 1, "done": 3 }), OrchError::WrongType),
        (json!({ "id": 1, "x": 1 }), OrchError::ExtraArgument),
    ] {
        assert_eq!(parse("handoff", &args), Err(error), "{args}");
    }
    assert_eq!(
        parse("finish", &json!({ "summary": "all done" })),
        Ok(OrchCall::Finish {
            summary: "all done".to_owned()
        })
    );
    assert_eq!(
        parse("finish", &json!({ "summary": "s", "x": 1 })),
        Err(OrchError::ExtraArgument)
    );
    assert_eq!(parse("finish", &json!({})), Err(OrchError::MissingArgument));
}

// Spec (A missing handoff gets a fallback): the first finish names the
// worker, the second builds the fallback and ends the run.
#[test]
fn finish_names_the_missing_handoffs_once_then_builds_fallbacks() {
    let mut run = run();
    let now = Instant::now();
    let two = finished(&mut run, Task::Review, "SECRET transcript");
    let three = finished(&mut run, Task::Code, "SECRET transcript");
    run.handoff(two, note(Verdict::OnCourse)).expect("stored");
    assert_eq!(
        run.finish("summary"),
        Err(FinishError::Missing(vec![three]))
    );
    assert_eq!(*run.state(), RunState::Open);
    let built = run.finish("summary").expect("second finish");
    assert_eq!(built.len(), 1);
    let fallback = &built[0];
    assert_eq!(fallback.unit, Unit::Worker(three));
    assert_eq!(
        (&*fallback.done, &*fallback.left, &*fallback.decisions),
        (NONE, NONE, NONE)
    );
    assert_eq!(fallback.verdict, Verdict::Unknown);
    assert_eq!(fallback.reason, NO_HANDOFF);
    assert!(!format!("{fallback:?}").contains("SECRET"));
    assert_eq!(run.stored_handoff(three), Some(fallback));
    assert_eq!(*run.state(), RunState::Finished);
    assert_eq!(run.spawn(now, Task::Review, "p"), Err(Refusal::Ended));
}

#[test]
fn finish_is_refused_while_a_worker_is_queued_or_running_and_while_paused() {
    let mut run = run();
    let now = Instant::now();
    let live = run.spawn(now, Task::Review, "p").expect("spawn");
    assert_eq!(run.finish("s"), Err(FinishError::Live(vec![live])));
    run.on_event(
        now,
        live,
        WorkerEvent::Failed {
            class: ErrorClass::Other,
            text: "boom".to_owned(),
        },
    );
    assert_eq!(run.finish("s"), Err(FinishError::Paused));
    assert!(matches!(run.state(), RunState::Halted { .. }));
}

// D17: a cancel or an expiry builds the missing handoffs itself, `blocked`
// when a failure was unresolved.
#[test]
fn fallbacks_are_blocked_while_halted_and_never_replace_a_real_handoff() {
    let mut run = run();
    let now = Instant::now();
    let kept = finished(&mut run, Task::Review, "t");
    let bare = finished(&mut run, Task::Review, "t");
    run.handoff(kept, note(Verdict::OnCourse)).expect("stored");
    let bad = run.spawn(now, Task::Code, "p").expect("spawn");
    run.on_event(
        now,
        bad,
        WorkerEvent::Failed {
            class: ErrorClass::Other,
            text: "boom".to_owned(),
        },
    );
    let built = run.fallbacks();
    let ids: Vec<_> = built.iter().map(|h| h.unit).collect();
    assert_eq!(ids, [Unit::Worker(bare), Unit::Worker(bad)]);
    assert!(built.iter().all(|h| h.verdict == Verdict::Blocked));
    assert_eq!(
        run.stored_handoff(kept).map(|h| h.verdict),
        Some(Verdict::OnCourse)
    );
    assert!(run.fallbacks().is_empty());
}

// Spec (Continuation from the ledger): the goal and one row per worker, no
// transcript.
#[test]
fn the_ledger_holds_the_goal_and_a_row_per_worker_but_no_transcript() {
    let mut run = run();
    let one = finished(&mut run, Task::Review, "SECRET transcript one");
    finished(&mut run, Task::Code, "SECRET transcript two");
    run.handoff(one, note(Verdict::OnCourse)).expect("stored");
    let text = run.run_ledger("build the parser").render();
    assert!(text.contains("build the parser"), "{text}");
    assert!(text.contains("1 review done: wrote the parser"), "{text}");
    assert!(text.contains("2 code done: handoff pending"), "{text}");
    assert!(!text.contains("SECRET"), "{text}");
}

// Spec (Ledger bounded): the oldest summaries shrink first; the goal and an
// unresolved failure stay whole.
#[test]
fn a_big_ledger_shortens_the_oldest_summaries_and_keeps_the_goal_and_failures() {
    let mut run = run();
    let now = Instant::now();
    for _ in 0..120 {
        let id = finished(&mut run, Task::Review, "t");
        let wordy = Judgement {
            done: "w".repeat(150),
            ..note(Verdict::OnCourse)
        };
        run.handoff(id, wordy).expect("stored");
    }
    let bad = run.spawn(now, Task::Code, "p").expect("spawn");
    run.on_event(
        now,
        bad,
        WorkerEvent::Failed {
            class: ErrorClass::Other,
            text: "boom".to_owned(),
        },
    );
    let goal = "ship the whole thing";
    let text = run.run_ledger(goal).render();
    assert!(text.len() <= LEDGER_BYTES, "{}", text.len());
    assert!(text.contains(goal));
    assert!(text.contains(&format!("{} code failed", bad.0)), "{text}");
    // The newest summary is whole, the oldest one is not.
    assert!(text.contains(&format!("120 review done: {}", "w".repeat(150))));
    assert!(!text.contains(&format!("\n1 review done: {}", "w".repeat(150))));
}

#[test]
fn a_pending_handoff_row_is_listed_for_the_continuation() {
    let mut run = run();
    let id = finished(&mut run, Task::Review, "t");
    assert_eq!(run.run_ledger("g").pending(), vec![id]);
    run.handoff(id, note(Verdict::OnCourse)).expect("stored");
    assert!(run.run_ledger("g").pending().is_empty());
    let _: Handoff = run.stored_handoff(id).cloned().expect("kept");
}

// Spec (Ledger bounded): past the point where every old summary is short,
// the oldest resolved rows collapse into one line; the goal stays.
#[test]
fn a_tiny_budget_collapses_the_oldest_rows_and_keeps_the_goal() {
    let mut run = run();
    for _ in 0..30 {
        let id = finished(&mut run, Task::Review, "t");
        run.handoff(id, note(Verdict::OnCourse)).expect("stored");
    }
    let text = run.run_ledger("the goal").render_within(600);
    assert!(text.len() <= 600, "{}", text.len());
    assert!(text.contains("the goal"));
    assert!(text.contains("earlier workers"), "{text}");
    assert!(text.contains("30 review done"), "{text}");
    assert!(!text.contains("\n1 review done"), "{text}");
}

fn capped(turns: usize) -> Run {
    run().with_limits(RunLimits {
        parallel: 4,
        turns,
        ..Default::default()
    })
}

// Spec (orchestrator-handoffs, Continuation from the ledger): the turn ends
// while a worker runs or a finished one has no handoff: the run goes on in a
// new turn, counted; with nothing pending the run settles.
#[test]
fn a_turn_that_ends_with_work_pending_continues_the_run() {
    let mut run = capped(6);
    assert_eq!(run.next_turn(), NextTurn::Settle, "nothing was spawned");
    let id = run.spawn(Instant::now(), Task::Review, "p").expect("spawn");
    assert_eq!(run.next_turn(), NextTurn::Continue(2), "a worker runs");
    run.on_event(Instant::now(), id, WorkerEvent::Done { text: "t".into() });
    assert_eq!(run.next_turn(), NextTurn::Continue(3), "no handoff yet");
    run.handoff(id, note(Verdict::OnCourse)).expect("stored");
    assert_eq!(run.next_turn(), NextTurn::Settle, "the ledger is whole");
}

// Spec (max_turns): the cap counts the first turn; at it nothing continues.
#[test]
fn the_turn_cap_counts_the_first_turn_and_stops_the_continuations() {
    let mut run = capped(2);
    run.spawn(Instant::now(), Task::Review, "p").expect("spawn");
    assert_eq!(run.next_turn(), NextTurn::Continue(2));
    assert_eq!(run.next_turn(), NextTurn::Capped(2));
    let mut once = capped(1);
    once.spawn(Instant::now(), Task::Review, "p")
        .expect("spawn");
    assert_eq!(once.next_turn(), NextTurn::Capped(1));
}

// A halted or finished run is not continued: the user's answer, or the end,
// comes first.
#[test]
fn a_halted_or_finished_run_is_not_continued() {
    let mut halted = capped(6);
    let id = halted
        .spawn(Instant::now(), Task::Review, "p")
        .expect("spawn");
    halted.on_event(
        Instant::now(),
        id,
        WorkerEvent::Failed {
            class: ErrorClass::Conflict,
            text: "409".into(),
        },
    );
    assert!(matches!(halted.state(), RunState::Halted { .. }));
    assert_eq!(halted.next_turn(), NextTurn::Settle);
    let mut done = capped(6);
    finished(&mut done, Task::Review, "t");
    done.fallbacks();
    done.finish("ok").expect("finish");
    assert_eq!(done.next_turn(), NextTurn::Settle);
}

/// A failed worker whose handoff says `done`.
fn failed_row(id: u32, done: &str) -> Row {
    let facts = Facts {
        run: RunId(7),
        worker: WorkerId(id),
        kind: Task::Code,
        engine: "codex".to_owned(),
        model: None,
        status: WorkerStatus::Failed,
    };
    let note = Judgement {
        done: done.to_owned(),
        ..note(Verdict::Blocked)
    };
    let handoff = Handoff::of(facts, note, true);
    Row::new(
        WorkerId(id),
        Task::Code,
        WorkerStatus::Failed,
        Some(&handoff),
    )
}

// Spec (Ledger bounded): the bound holds for any input; a failure keeps its
// id and status, its summary shrinks like the others.
#[test]
fn many_failures_with_long_summaries_stay_within_the_bound_and_listed() {
    let rows: Vec<_> = (1..=32)
        .map(|id| failed_row(id, &"f".repeat(2048)))
        .collect();
    let text = Ledger::new("the goal", rows).render();
    assert!(text.len() <= LEDGER_BYTES, "{}", text.len());
    for id in 1..=32 {
        assert!(
            text.contains(&format!("\n{id} code failed")),
            "{id}: {text}"
        );
    }
}

// An old failure does not stop the resolved rows after it from collapsing.
#[test]
fn an_old_failure_does_not_block_later_rows_from_collapsing() {
    let mut rows = vec![failed_row(1, "boom")];
    for id in 2..=30 {
        let facts = Facts {
            run: RunId(7),
            worker: WorkerId(id),
            kind: Task::Review,
            engine: "codex".to_owned(),
            model: None,
            status: WorkerStatus::Done,
        };
        let handoff = Handoff::of(facts, note(Verdict::OnCourse), false);
        rows.push(Row::new(
            WorkerId(id),
            Task::Review,
            WorkerStatus::Done,
            Some(&handoff),
        ));
    }
    let text = Ledger::new("the goal", rows).render_within(600);
    assert!(text.len() <= 600, "{}", text.len());
    assert!(text.contains("\n1 code failed"), "{text}");
    assert!(text.contains("earlier workers"), "{text}");
    assert!(text.contains("30 review done"), "{text}");
    assert!(!text.contains("\n2 review done"), "{text}");
}

// D17: a terminal worker that shows up after the warning makes finish refuse
// again, naming it; the warning is still once per worker.
#[test]
fn finish_refuses_again_for_a_worker_that_ended_after_the_warning() {
    let mut run = run();
    let one = finished(&mut run, Task::Review, "t");
    assert_eq!(run.finish("s"), Err(FinishError::Missing(vec![one])));
    run.handoff(one, note(Verdict::OnCourse)).expect("stored");
    let two = finished(&mut run, Task::Review, "t");
    assert_eq!(run.finish("s"), Err(FinishError::Missing(vec![two])));
    assert!(run.finish("s").is_ok());
}

// D17: the reason is one clean line: no control characters, and a leading
// blank line does not turn it into `none`.
#[test]
fn the_reason_is_trimmed_before_its_first_line_and_loses_control_characters() {
    let mut run = run();
    let id = finished(&mut run, Task::Review, "t");
    let dirty = Judgement {
        reason: "\n\n \x1b[2Jfits\rthe goal\x07\nsecond line".to_owned(),
        done: "\x1b[31mdid it\rfast\nmore".to_owned(),
        ..note(Verdict::OnCourse)
    };
    let handoff = run.handoff(id, dirty).expect("stored");
    assert_eq!(handoff.reason, "[2Jfitsthe goal");
    let text = run.run_ledger("g").render();
    assert!(!text.contains(['\x1b', '\r', '\x07']), "{text:?}");
    assert!(text.contains("1 review done: [31mdid itfast"), "{text:?}");
}

// Spec (Handoff for a finished run): refused like finish, with a reason.
#[test]
fn a_handoff_after_the_run_ended_is_refused() {
    let mut run = run();
    let id = finished(&mut run, Task::Review, "t");
    run.handoff(id, note(Verdict::OnCourse)).expect("stored");
    run.finish("s").expect("finished");
    assert_eq!(
        run.handoff(id, note(Verdict::OffCourse)),
        Err(HandoffError::Ended)
    );
    assert_eq!(
        run.stored_handoff(id).map(|h| h.verdict),
        Some(Verdict::OnCourse)
    );
}
