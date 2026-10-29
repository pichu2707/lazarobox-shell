//! The worker rows of an orchestrating chat (agent-orchestrator, Worker
//! result row): one row per worker, updated in place, never stored and
//! never sent as history.

use std::time::Duration;

use super::{
    engine::{Finish, Role},
    state_tests::{chat, ready},
    store::StoreCmd,
    *,
};
use crate::core::{
    ai::{
        chat::engine::Usage,
        config::Task,
        orchestra::{RunId, WorkerId, event::WorkerRow, worker::WorkerStatus},
    },
    layout::PaneId,
};

fn row(id: u32, status: WorkerStatus, elapsed: Option<u64>) -> WorkerRow {
    WorkerRow {
        run: RunId(7),
        id: WorkerId(id),
        kind: Task::Review,
        engine: "codex".into(),
        model: Some("gpt-5".into()),
        status,
        elapsed: elapsed.map(Duration::from_secs),
        usage: None,
    }
}

fn workers(state: &ChatState) -> Vec<&WorkerRow> {
    state
        .entries()
        .iter()
        .filter_map(|e| match e {
            Entry::Worker(row) => Some(row),
            _ => None,
        })
        .collect()
}

fn texts(state: &ChatState) -> Vec<String> {
    state.lines(80).iter().map(|l| l.text.clone()).collect()
}

/// A chat that sent `q` and got `answer` back, the store told of it.
fn answered() -> ChatState {
    let mut state = chat();
    let turn = match state.send("q", &ready(), "sys") {
        SendOutcome::Sent { turn, .. } => turn,
        SendOutcome::Refused => panic!("send refused"),
    };
    state.apply(turn, StreamEvent::Delta("answer".into()));
    state.apply(turn, StreamEvent::Done(Finish::Stop));
    state
}

// agent-orchestrator / Row follows the status: the same row shows each
// status with kind and `engine>model`.
#[test]
fn a_worker_row_follows_its_status_in_place() {
    let mut state = chat();
    for (status, shown) in [
        (WorkerStatus::Queued, "review codex>gpt-5 queued"),
        (WorkerStatus::Running, "review codex>gpt-5 running"),
    ] {
        state.worker_row(row(1, status, None));
        assert_eq!(workers(&state), [&row(1, status, None)]);
        assert!(
            texts(&state).iter().any(|t| t.ends_with(shown)),
            "{:?}",
            texts(&state)
        );
    }
    state.worker_row(row(2, WorkerStatus::Running, None));
    state.worker_row(row(1, WorkerStatus::Done, Some(12)));
    assert_eq!(
        workers(&state),
        [
            &row(1, WorkerStatus::Done, Some(12)),
            &row(2, WorkerStatus::Running, None)
        ],
        "one row per worker, in the order they came"
    );
    let lines = texts(&state);
    assert!(
        lines
            .iter()
            .any(|t| t.ends_with("review codex>gpt-5 done 12.0s n/a")),
        "{lines:?}"
    );
}

// agent-orchestrator / Not in history: a finished worker's row is neither a
// message nor sent with the next request.
#[test]
fn a_worker_row_is_not_sent_as_history() {
    let mut state = answered();
    state.worker_row(row(1, WorkerStatus::Done, Some(3)));
    assert_eq!(state.messages().count(), 2);
    let SendOutcome::Sent { request, .. } = state.send("next", &ready(), "sys") else {
        panic!("send refused");
    };
    let sent: Vec<(Role, &str)> = request
        .messages
        .iter()
        .map(|m| (m.role, m.content.as_str()))
        .collect();
    assert_eq!(
        sent,
        [
            (Role::User, "q"),
            (Role::Assistant, "answer"),
            (Role::User, "next")
        ]
    );
}

// agent-orchestrator / Not in history; chat-history saving unchanged: the
// store gets the same commands with or without worker rows.
#[test]
fn a_worker_row_never_reaches_the_store() {
    let pane = PaneId::FIRST;
    let mut plain = answered();
    let mut with_rows = answered();
    with_rows.worker_row(row(1, WorkerStatus::Running, None));
    with_rows.worker_row(row(1, WorkerStatus::Done, Some(3)));
    let saved = with_rows.store_cmds(pane, &[]);
    assert_eq!(saved, plain.store_cmds(pane, &[]));
    assert!(
        matches!(
            saved.as_slice(),
            [StoreCmd::Create { .. }, StoreCmd::Append { .. }]
        ),
        "{saved:?}"
    );
    assert!(!format!("{saved:?}").contains("codex"));
}

fn used(input: u64, output: u64, cost_micros: Option<u64>) -> Usage {
    Usage {
        input,
        output,
        cost_micros,
    }
}

fn with_usage(mut row: WorkerRow, usage: Usage) -> WorkerRow {
    row.usage = Some(usage);
    row
}

fn ends_with(state: &ChatState, tail: &str) -> bool {
    texts(state).iter().any(|t| t.ends_with(tail))
}

fn run_rows(state: &ChatState) -> Vec<&RunUsage> {
    state
        .entries()
        .iter()
        .filter_map(|e| match e {
            Entry::RunUsage(row) => Some(row),
            _ => None,
        })
        .collect()
}

// agent-orchestrator / Usage summed: a worker that reported shows its
// numbers, one that did not shows `n/a`, and a running one shows neither.
#[test]
fn a_finished_worker_row_shows_its_usage_or_n_a() {
    let mut state = chat();
    state.worker_row(row(1, WorkerStatus::Running, None));
    assert!(ends_with(&state, "review codex>gpt-5 running"));
    state.worker_row(with_usage(
        row(1, WorkerStatus::Done, Some(12)),
        used(12_345, 450, Some(4936)),
    ));
    assert!(
        ends_with(
            &state,
            "review codex>gpt-5 done 12.0s 12.3k in 450 out $0.0049"
        ),
        "{:?}",
        texts(&state)
    );
    state.worker_row(row(2, WorkerStatus::Done, Some(3)));
    assert!(
        ends_with(&state, "review codex>gpt-5 done 3.0s n/a"),
        "{:?}",
        texts(&state)
    );
}

#[test]
fn a_cost_the_engine_did_not_report_is_left_out_not_zero() {
    let mut state = chat();
    state.worker_row(with_usage(
        row(1, WorkerStatus::Done, Some(1)),
        used(950, 1_500, None),
    ));
    assert!(
        ends_with(&state, "done 1.0s 950 in 1.5k out"),
        "{:?}",
        texts(&state)
    );
}

#[test]
fn the_run_row_shows_the_sum_and_skips_the_worker_without_usage() {
    let mut state = chat();
    state.worker_row(with_usage(
        row(1, WorkerStatus::Done, Some(5)),
        used(1_000, 100, Some(1000)),
    ));
    state.worker_row(row(2, WorkerStatus::Done, Some(6)));
    state.worker_row(with_usage(
        row(3, WorkerStatus::Done, Some(7)),
        used(2_000, 200, Some(2000)),
    ));
    assert_eq!(
        run_rows(&state),
        [&RunUsage {
            run: RunId(7),
            usage: Some(used(3_000, 300, Some(3000)))
        }]
    );
    assert!(
        ends_with(&state, "run 3.0k in 300 out $0.0030"),
        "{:?}",
        texts(&state)
    );
}

#[test]
fn the_run_row_is_n_a_when_no_worker_reported_and_waits_for_a_finished_one() {
    let mut state = chat();
    state.worker_row(row(1, WorkerStatus::Running, None));
    assert!(run_rows(&state).is_empty(), "nothing finished yet");
    state.worker_row(row(1, WorkerStatus::Done, Some(2)));
    assert_eq!(
        run_rows(&state),
        [&RunUsage {
            run: RunId(7),
            usage: None
        }]
    );
    assert!(ends_with(&state, "run n/a"), "{:?}", texts(&state));
}

#[test]
fn a_run_row_stays_one_and_last_as_workers_update() {
    let mut state = chat();
    state.worker_row(row(1, WorkerStatus::Done, Some(2)));
    state.worker_row(row(2, WorkerStatus::Running, None));
    state.worker_row(row(2, WorkerStatus::Done, Some(3)));
    assert_eq!(run_rows(&state).len(), 1);
    assert!(matches!(state.entries().last(), Some(Entry::RunUsage(_))));
}

#[test]
fn a_cost_missing_from_one_worker_leaves_the_run_cost_unknown() {
    let mut state = chat();
    state.worker_row(with_usage(
        row(1, WorkerStatus::Done, Some(5)),
        used(10, 1, Some(1000)),
    ));
    state.worker_row(with_usage(
        row(2, WorkerStatus::Done, Some(6)),
        used(10, 1, None),
    ));
    assert!(ends_with(&state, "run 20 in 2 out"), "{:?}", texts(&state));
}

#[test]
fn the_run_row_is_not_sent_as_history() {
    let mut state = answered();
    state.worker_row(row(1, WorkerStatus::Done, Some(3)));
    assert_eq!(state.messages().count(), 2);
}
