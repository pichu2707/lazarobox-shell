//! Spec (command-runner, Refusals before start) and design D17: one run per
//! chat, two at once in the app, the wait capped.

use std::time::Duration;

use super::gate::{Refused, RunGate};
use crate::core::{ai::tools::stage::StageKey, layout::PaneId};

fn chat(n: u32) -> StageKey {
    StageKey::Pane(PaneId::for_test(n))
}

const WAIT: Duration = Duration::from_secs(60);

// Scenario "One run per chat": a busy chat is refused at once and another
// chat is not affected.
#[tokio::test(start_paused = true)]
async fn a_chat_runs_one_command_and_another_chat_is_not_affected() {
    let gate = RunGate::new(2, WAIT);
    let first = gate.acquire(chat(1)).await.unwrap();
    assert!(gate.is_busy(chat(1)) && !gate.is_busy(chat(2)));
    assert_eq!(gate.acquire(chat(1)).await.err(), Some(Refused::Chat));
    let other = gate.acquire(chat(2)).await.unwrap();
    drop((first, other));
    assert!(!gate.is_busy(chat(1)) && !gate.is_busy(chat(2)));
    drop(gate.acquire(chat(1)).await.unwrap());
}

// Scenario "Global cap": the third run waits for a slot, then is refused once
// the wait is over; the chat is free again after the refusal.
#[tokio::test(start_paused = true)]
async fn the_third_command_waits_the_cap_and_is_then_refused() {
    let gate = RunGate::new(2, WAIT);
    let _a = gate.acquire(chat(1)).await.unwrap();
    let _b = gate.acquire(chat(2)).await.unwrap();
    let started = tokio::time::Instant::now();
    assert_eq!(gate.acquire(chat(3)).await.err(), Some(Refused::Full));
    assert_eq!(started.elapsed(), WAIT);
    assert!(!gate.is_busy(chat(3)));
}

// The wait ends as soon as a slot frees, long before the cap.
#[tokio::test(start_paused = true)]
async fn a_waiting_command_starts_when_a_slot_frees() {
    let gate = RunGate::new(2, WAIT);
    let a = gate.acquire(chat(1)).await.unwrap();
    let _b = gate.acquire(chat(2)).await.unwrap();
    let started = tokio::time::Instant::now();
    let (got, ()) = tokio::join!(gate.acquire(chat(3)), async {
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(a);
    });
    assert!(got.is_ok());
    assert_eq!(started.elapsed(), Duration::from_secs(5));
}

// A call cancelled while it waits gives its chat back: the next call is not
// told that a command is already running.
#[tokio::test(start_paused = true)]
async fn a_cancelled_wait_frees_the_chat() {
    let gate = RunGate::new(1, WAIT);
    let _held = gate.acquire(chat(1)).await.unwrap();
    let waiting = tokio::time::timeout(Duration::from_secs(1), gate.acquire(chat(2))).await;
    assert!(waiting.is_err(), "the wait was cut by the caller");
    assert!(!gate.is_busy(chat(2)));
}
