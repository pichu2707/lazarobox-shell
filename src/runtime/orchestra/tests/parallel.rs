//! Parallel writers (agent-orchestrator [W10b]): one batch per writing worker,
//! reviews that read their proposer's batch, and the notice for workers that
//! share a local engine (owner decision 5, design D11).

use super::*;

// The engine notice -------------------------------------------------------

/// `general` and `review` assigned to `engine>model` pairs, with `ollama`
/// and `llamacpp` found.
fn assigned(general: (&str, &str), review: (&str, &str)) -> Roster {
    let assigned = [
        (Task::Orchestrate, "claude", "opus"),
        (Task::General, general.0, general.1),
        (Task::Review, review.0, review.1),
    ];
    let engines = vec![
        found("claude", &[]),
        found("codex", &[]),
        found("ollama", &["qwen3:8b", "llama3:8b", "gpt-oss:120b-cloud"]),
        found("llamacpp", &["x"]),
    ];
    roster(&assigned, engines, JAIL)
}

fn local_pair() -> Roster {
    assigned(("ollama", "qwen3:8b"), ("ollama", "llama3:8b"))
}

fn with_parallel(lab: &Lab, roster: Roster, parallel: usize) -> (RunId, OrchHandle) {
    let limits = RunLimits {
        parallel,
        ..RunLimits::default()
    };
    let scope = OrchScope {
        limits,
        ..lab.scope(roster, &[])
    };
    lab.manager.start(pane(), scope).unwrap()
}

/// The texts of the notices the pane got.
fn notices(lab: &mut Lab) -> Vec<String> {
    let mut out = Vec::new();
    for event in lab.sent() {
        if let OrchestraEvent::Notice { text } = event {
            out.push(text);
        }
    }
    out
}

const SHARED: &str = "workers 1 and 2 share ollama, a local server: it may queue their requests; set max_parallel = 1 in [ai.orchestrator] to run one at a time";

// Decision 5: two workers in flight on one local server are told once, with
// the one key that fixes it; a third on the same engine says nothing more.
#[tokio::test]
async fn two_workers_on_a_local_engine_get_one_notice() {
    let mut lab = lab(Script::Hang);
    let (_, handle) = with_parallel(&lab, local_pair(), 3);
    handle.call(spawn(Task::General, "a")).await;
    assert!(notices(&mut lab).is_empty(), "alone on the server");
    handle.call(spawn(Task::Review, "b")).await;
    handle.call(spawn(Task::General, "c")).await;
    lab.seen.nth(2).await;
    let told = notices(&mut lab);
    assert_eq!(told, [SHARED]);
    assert!(told[0].is_ascii());
}

// Triangulation: no notice when the workers do not meet on one local server.
#[tokio::test]
async fn workers_that_do_not_meet_on_a_local_server_get_no_notice() {
    let cases = [
        // one at a time: the second waits in the queue
        ("sequential", local_pair(), 1),
        // two local servers
        (
            "two engines",
            assigned(("ollama", "qwen3:8b"), ("llamacpp", "x")),
            2,
        ),
        // a provider's cloud
        (
            "a cloud engine",
            assigned(("codex", "default"), ("codex", "default")),
            2,
        ),
        // Ollama serving a cloud model
        (
            "a cloud model",
            assigned(("ollama", "qwen3:8b"), ("ollama", "gpt-oss:120b-cloud")),
            2,
        ),
    ];
    for (name, roster, parallel) in cases {
        let mut lab = lab(Script::Hang);
        let (_, handle) = with_parallel(&lab, roster, parallel);
        handle.call(spawn(Task::General, "a")).await;
        handle.call(spawn(Task::Review, "b")).await;
        lab.seen.nth(parallel.min(2) - 1).await;
        assert!(notices(&mut lab).is_empty(), "{name}");
    }
}

// A second run is a new condition: it tells again.
#[tokio::test]
async fn a_new_run_tells_again() {
    let mut lab = lab(Script::Hang);
    let (_, handle) = with_parallel(&lab, local_pair(), 2);
    handle.call(spawn(Task::General, "a")).await;
    handle.call(spawn(Task::Review, "b")).await;
    assert_eq!(notices(&mut lab), [SHARED]);
    lab.manager.cancel(pane());
    lab.sent();
    let (_, handle) = with_parallel(&lab, local_pair(), 2);
    handle.call(spawn(Task::General, "a")).await;
    handle.call(spawn(Task::Review, "b")).await;
    assert_eq!(notices(&mut lab), [SHARED]);
}

/// `ollama` for `general`, `codex` for `review`: the answer of `codex` is
/// `done`, anything else hangs.
fn codex_answers(request: &ChatRequest) -> Option<&'static str> {
    (request.engine == "codex").then_some("done")
}

// Queue promote reaches the notice: the third worker waits, a slot frees, and
// it starts next to the first, on the same server.
#[tokio::test]
async fn a_worker_promoted_from_the_queue_next_to_a_local_one_is_told() {
    let mut lab = lab(Script::By(codex_answers));
    let roster = assigned(("ollama", "qwen3:8b"), ("codex", "default"));
    let (_, handle) = with_parallel(&lab, roster, 2);
    handle.call(spawn(Task::General, "a")).await;
    handle.call(spawn(Task::Review, "b")).await;
    let third = handle.call(spawn(Task::General, "c")).await;
    assert_eq!(third, fine("worker 3: queued"));
    assert!(notices(&mut lab).is_empty(), "nobody shares yet");
    assert_eq!(handle.call(wait(2)).await, fine("2: done"));
    lab.seen.nth(2).await;
    assert_eq!(
        notices(&mut lab),
        [
            "workers 1 and 3 share ollama, a local server: it may queue their requests; set max_parallel = 1 in [ai.orchestrator] to run one at a time"
        ]
    );
}

// Relaunch reaches it: a worker that failed on a cloud engine is relaunched
// on the local server where another one runs.
#[tokio::test]
async fn a_worker_relaunched_onto_a_local_engine_is_told() {
    let mut lab = lab(Script::FailOn(2, "409 conflict: session busy"));
    let roster = assigned(("ollama", "qwen3:8b"), ("codex", "default"));
    let (run, handle) = with_parallel(&lab, roster, 2);
    handle.call(spawn(Task::General, "a")).await;
    handle.call(spawn(Task::Review, "b")).await;
    assert_eq!(handle.call(wait(2)).await, fine("2: awaiting_user"));
    assert!(notices(&mut lab).is_empty());
    let onto = UserChoice::Relaunch(local_pair());
    let decided = lab.manager.decide(pane(), run, onto, FamilySet::ALL);
    assert_eq!(decided, Ok(()));
    lab.seen.nth(2).await;
    assert_eq!(notices(&mut lab), [SHARED]);
}

// GUARD: a retry on the engine already told does not tell again (the memory
// is per run and engine, not per launch).
#[tokio::test]
async fn a_retry_on_the_engine_already_told_says_nothing_more() {
    let mut lab = lab(Script::FailOn(2, "409 conflict: session busy"));
    let (run, handle) = with_parallel(&lab, local_pair(), 2);
    handle.call(spawn(Task::General, "a")).await;
    handle.call(spawn(Task::Review, "b")).await;
    assert_eq!(handle.call(wait(2)).await, fine("2: awaiting_user"));
    assert_eq!(notices(&mut lab), [SHARED]);
    let retried = lab
        .manager
        .decide(pane(), run, UserChoice::Retry, FamilySet::ALL);
    assert_eq!(retried, Ok(()));
    lab.seen.nth(2).await;
    assert!(notices(&mut lab).is_empty(), "told once");
}
