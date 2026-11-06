//! Workers that share a local engine (agent-orchestrator, owner decision 5
//! [W10b], design D11): the pure question the run manager asks before it says
//! that two workers may queue behind one server.

use std::time::{Duration, Instant};

use super::run::{Roster, Run, RunLimits, WorkerEvent};
use super::{RunId, WorkerId};
use crate::core::ai::chat::engine::Availability;
use crate::core::ai::config::Task;

fn ready(engine: &str, model: &str) -> Availability {
    Availability::Ready {
        engine: engine.to_owned(),
        model: Some(model.to_owned()),
    }
}

/// A run with `parallel` slots whose kinds are assigned as `each` says.
fn run_of(each: &[(Task, &str, &str)], parallel: usize) -> Run {
    let roster = each
        .iter()
        .map(|(task, engine, model)| (*task, ready(engine, model)))
        .collect();
    let limits = RunLimits {
        parallel,
        ..RunLimits::default()
    };
    Run::new(RunId(1), Roster::new(roster), Duration::from_secs(600)).with_limits(limits)
}

const LOCAL: [(Task, &str, &str); 2] = [
    (Task::General, "ollama", "qwen3:8b"),
    (Task::Review, "ollama", "llama3:8b"),
];

// Two workers in flight on one local server share it, whichever asks, and the
// answer names the other one (the lowest number when there are several).
#[test]
fn two_running_workers_on_a_local_engine_share_it() {
    let (t0, mut run) = (Instant::now(), run_of(&LOCAL, 2));
    let one = run.spawn(t0, Task::General, "a").unwrap();
    assert_eq!(run.shares_engine(one), None, "alone on the engine");
    let two = run.spawn(t0, Task::Review, "b").unwrap();
    assert_eq!(run.shares_engine(two), Some(one));
    assert_eq!(run.shares_engine(one), Some(two));
}

// A worker still queued is not in flight: it asks nothing of the server.
#[test]
fn a_queued_worker_shares_nothing() {
    let (t0, mut run) = (Instant::now(), run_of(&LOCAL, 1));
    let one = run.spawn(t0, Task::General, "a").unwrap();
    let two = run.spawn(t0, Task::Review, "b").unwrap();
    assert_eq!(run.shares_engine(one), None);
    assert_eq!(run.shares_engine(two), None);
    // ... and it does once the first ends and it starts in a slot, but then
    // nobody is left to share with.
    run.on_event(t0, one, WorkerEvent::Done { text: "x".into() });
    assert_eq!(run.shares_engine(two), None);
}

// A worker parked on its own review sends no request: its slot is free.
#[test]
fn a_parked_worker_shares_nothing() {
    let (t0, mut run) = (Instant::now(), run_of(&LOCAL, 2));
    let one = run.spawn(t0, Task::General, "a").unwrap();
    let two = run.spawn(t0, Task::Review, "b").unwrap();
    assert!(run.park(t0, one));
    assert_eq!(run.shares_engine(two), None);
    run.unpark(one);
    assert_eq!(run.shares_engine(two), Some(one));
}

// A worker that ended is gone from the server.
#[test]
fn an_ended_worker_shares_nothing() {
    let (t0, mut run) = (Instant::now(), run_of(&LOCAL, 2));
    let one = run.spawn(t0, Task::General, "a").unwrap();
    let two = run.spawn(t0, Task::Review, "b").unwrap();
    run.on_event(t0, one, WorkerEvent::Done { text: "x".into() });
    assert_eq!(run.shares_engine(two), None);
}

// Different engines do not share a server, and a CLI engine (the provider's
// cloud) is not "a local server" even when two workers use it.
#[test]
fn two_engines_or_a_cloud_engine_share_nothing() {
    let split = [
        (Task::General, "ollama", "qwen3:8b"),
        (Task::Review, "llamacpp", "x"),
    ];
    let (t0, mut run) = (Instant::now(), run_of(&split, 2));
    let one = run.spawn(t0, Task::General, "a").unwrap();
    let two = run.spawn(t0, Task::Review, "b").unwrap();
    assert_eq!(
        (run.shares_engine(one), run.shares_engine(two)),
        (None, None)
    );

    let cloud = [
        (Task::General, "codex", "default"),
        (Task::Review, "codex", "default"),
    ];
    let (t0, mut run) = (Instant::now(), run_of(&cloud, 2));
    let one = run.spawn(t0, Task::General, "a").unwrap();
    let two = run.spawn(t0, Task::Review, "b").unwrap();
    assert_eq!(
        (run.shares_engine(one), run.shares_engine(two)),
        (None, None)
    );
}

// An Ollama `*-cloud` model is served by Ollama's cloud: it does not queue on
// the local daemon, whichever side of the pair it is.
#[test]
fn an_ollama_cloud_model_does_not_share_the_local_server() {
    let mixed = [
        (Task::General, "ollama", "qwen3:8b"),
        (Task::Review, "ollama", "gpt-oss:120b-cloud"),
    ];
    let (t0, mut run) = (Instant::now(), run_of(&mixed, 2));
    let one = run.spawn(t0, Task::General, "a").unwrap();
    let two = run.spawn(t0, Task::Review, "b").unwrap();
    assert_eq!(
        (run.shares_engine(one), run.shares_engine(two)),
        (None, None)
    );
}

// A worker the run never had has no partner.
#[test]
fn an_unknown_worker_shares_nothing() {
    let run = run_of(&LOCAL, 2);
    assert_eq!(run.shares_engine(WorkerId(9)), None);
}
