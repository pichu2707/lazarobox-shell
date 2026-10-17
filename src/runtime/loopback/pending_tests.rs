//! The first-byte and idle limits do not run while a tool call waits for the
//! user (D13). The total limit does, and nothing here ever approves.
//! Everything runs on the paused clock over an in-memory duplex.

use std::time::Duration;

use tokio::{
    io::{AsyncWriteExt, DuplexStream, duplex},
    sync::mpsc,
    task::JoinHandle,
    time::{Instant, sleep},
};

use super::*;
use crate::{
    core::ai::chat::{engine::ChatError, stream::NdjsonSplitter},
    runtime::tools::{Pending, pending::TURN},
};

const HEAD: &[u8] = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n";
const HELLO: &str = "{\"message\":{\"content\":\"Hel\"},\"done\":false}\n";

fn chunk(text: &str) -> Vec<u8> {
    format!("{:x}\r\n{text}\r\n", text.len()).into_bytes()
}

fn failed(text: &str) -> StreamEvent {
    StreamEvent::Error(ChatError::Failed(text.into()))
}

struct Turn {
    server: DuplexStream,
    events: mpsc::Receiver<StreamEvent>,
    start: Instant,
    _task: JoinHandle<()>,
}

/// A turn that runs inside `pending`'s task-local, like a real one.
fn start(pending: &Pending, timeouts: Timeouts) -> Turn {
    let (tx, events) = mpsc::channel(256);
    let (client, server) = duplex(64 * 1024);
    let task = tokio::spawn(TURN.scope(pending.clone(), async move {
        let mut decoder = NdjsonSplitter::server();
        drive(client, b"POST", &timeouts, &mut decoder, &tx).await;
    }));
    Turn {
        server,
        events,
        start: Instant::now(),
        _task: task,
    }
}

async fn last(turn: &mut Turn) -> Option<StreamEvent> {
    let mut last = None;
    while let Some(event) = turn.events.recv().await {
        last = Some(event);
    }
    last
}

// A call starts waiting 5 s into the silence and is answered at 305 s. The 115 s
// that were left of the first-byte limit run after the answer.
#[tokio::test(start_paused = true)]
async fn the_first_byte_limit_does_not_run_while_a_call_waits() {
    let pending = Pending::new();
    let mut turn = start(&pending, Timeouts::SERVER);
    sleep(Duration::from_secs(5)).await;
    let wait = pending.enter();
    sleep(Duration::from_secs(300)).await;
    assert_eq!(pending.count(), 1);
    drop(wait);
    assert_eq!(last(&mut turn).await, Some(failed(NO_ANSWER)));
    assert_eq!(turn.start.elapsed(), Duration::from_secs(5 + 300 + 115));
}

#[tokio::test(start_paused = true)]
async fn the_idle_limit_does_not_run_while_a_call_waits() {
    let pending = Pending::new();
    let mut turn = start(&pending, Timeouts::SERVER);
    turn.server.write_all(HEAD).await.unwrap();
    turn.server.write_all(&chunk(HELLO)).await.unwrap();
    sleep(Duration::from_secs(10)).await;
    let wait = pending.enter();
    sleep(Duration::from_secs(400)).await;
    drop(wait);
    // The idle limit starts over once no call waits.
    assert_eq!(last(&mut turn).await, Some(failed(STALLED)));
    assert_eq!(turn.start.elapsed(), Duration::from_secs(410 + 60));
}

#[tokio::test(start_paused = true)]
async fn the_quiet_hint_waits_too() {
    let pending = Pending::new();
    let mut turn = start(&pending, Timeouts::CLI);
    turn.server.write_all(HEAD).await.unwrap();
    turn.server.write_all(&chunk(HELLO)).await.unwrap();
    assert!(matches!(
        turn.events.recv().await,
        Some(StreamEvent::Delta(_))
    ));
    let wait = pending.enter();
    sleep(Duration::from_secs(120)).await;
    assert!(
        turn.events.try_recv().is_err(),
        "no Slow while a call waits"
    );
    drop(wait);
    assert_eq!(turn.events.recv().await, Some(StreamEvent::Slow));
    assert_eq!(turn.start.elapsed(), Duration::from_secs(120 + 10));
}

// The user is asked for as long as it takes, but the turn as a whole is not
// unbounded: the 15-minute total keeps running.
#[tokio::test(start_paused = true)]
async fn the_total_limit_keeps_running_while_a_call_waits() {
    let pending = Pending::new();
    let _wait = pending.enter();
    let mut turn = start(&pending, Timeouts::SERVER);
    assert_eq!(last(&mut turn).await, Some(failed(TOO_LONG)));
    assert_eq!(turn.start.elapsed(), Duration::from_secs(15 * 60));
}

#[tokio::test(start_paused = true)]
async fn a_wait_that_is_over_leaves_the_limits_as_they_were() {
    let pending = Pending::new();
    let mut turn = start(&pending, Timeouts::SERVER);
    drop(pending.enter());
    assert_eq!(last(&mut turn).await, Some(failed(NO_ANSWER)));
    assert_eq!(turn.start.elapsed(), Duration::from_secs(120));
}

#[tokio::test(start_paused = true)]
async fn a_turn_without_a_pending_count_keeps_its_limits() {
    let (tx, mut events) = mpsc::channel(256);
    let (client, _server) = duplex(1024);
    let start = Instant::now();
    tokio::spawn(async move {
        let mut decoder = NdjsonSplitter::server();
        drive(client, b"POST", &Timeouts::SERVER, &mut decoder, &tx).await;
    });
    let mut last = None;
    while let Some(event) = events.recv().await {
        last = Some(event);
    }
    assert_eq!(last, Some(failed(NO_ANSWER)));
    assert_eq!(start.elapsed(), Duration::from_secs(120));
}
