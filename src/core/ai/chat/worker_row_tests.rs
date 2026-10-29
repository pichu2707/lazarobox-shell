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
            .any(|t| t.ends_with("review codex>gpt-5 done 12.0s")),
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
