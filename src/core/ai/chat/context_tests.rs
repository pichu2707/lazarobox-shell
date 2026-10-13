//! The memory block in a request (engram-memory 10.1 / 10.2): carried apart
//! from the history, and counted against its character cap.

use super::{
    engine::{ChatMessage, Finish, SERVER_HISTORY_CHARS, StreamEvent},
    state_tests::{SYSTEM, chat, ready},
    *,
};
use crate::core::ai::memory::context::ContextBlock;

fn block(chars: usize) -> ContextBlock {
    let text = format!(
        "<project-memory>{}</project-memory>",
        "n".repeat(chars - 33)
    );
    assert_eq!(text.chars().count(), chars);
    ContextBlock {
        bytes: text.len(),
        notes: 3,
        text,
    }
}

/// Sends `text` and answers it with an answer of the same length.
fn exchange(state: &mut ChatState, text: &str) {
    let SendOutcome::Sent { turn, .. } = state.send(text, &ready(), SYSTEM) else {
        panic!("refused");
    };
    state.apply(turn, StreamEvent::Delta("a".repeat(text.chars().count())));
    state.apply(turn, StreamEvent::Done(Finish::Stop));
}

fn request_of(state: &mut ChatState, text: &str) -> ChatRequest {
    match state.send(text, &ready(), SYSTEM) {
        SendOutcome::Sent { request, .. } => request,
        SendOutcome::Refused => panic!("refused"),
    }
}

fn history_chars_of(request: &ChatRequest) -> usize {
    request
        .messages
        .iter()
        .map(|m| m.content.chars().count())
        .sum()
}

#[test]
fn a_block_travels_apart_from_the_system_prompt_and_the_history() {
    let mut state = chat();
    let block = block(100);
    state.inject_memory(block.clone());
    let request = request_of(&mut state, "hello");
    assert_eq!(request.memory.as_deref(), Some(block.text.as_str()));
    assert_eq!(request.system, SYSTEM, "the pinned prompt is untouched");
    assert_eq!(
        request.messages,
        [ChatMessage {
            role: Role::User,
            content: "hello".into()
        }]
    );
}

#[test]
fn without_a_block_the_request_carries_none() {
    let mut state = chat();
    assert_eq!(request_of(&mut state, "hello").memory, None);
}

#[test]
fn the_block_is_never_part_of_a_stored_message() {
    let mut state = chat();
    let block = block(100);
    state.inject_memory(block.clone());
    exchange(&mut state, "hello");
    assert!(
        state.messages().all(|m| !m.text.contains("project-memory")),
        "no message holds the block"
    );
}

#[test]
fn a_block_arriving_mid_turn_applies_from_the_next_turn() {
    let mut state = chat();
    let first = request_of(&mut state, "first");
    state.inject_memory(block(100));
    assert_eq!(first.memory, None, "the turn in flight did not get it");
    state.apply(state.turn(), StreamEvent::Delta("ok".into()));
    state.apply(state.turn(), StreamEvent::Done(Finish::Stop));
    assert!(request_of(&mut state, "next").memory.is_some());
}

#[test]
fn the_block_is_counted_in_the_history_budget() {
    // Nine messages of 1,200 characters (four exchanges and a new question).
    let long = "x".repeat(1200);
    let plain = {
        let mut state = chat();
        for _ in 0..4 {
            exchange(&mut state, &long);
        }
        request_of(&mut state, &long)
    };
    assert_eq!(plain.messages.len(), 9, "10,800 chars fit in 12,000");

    let mut state = chat();
    for _ in 0..4 {
        exchange(&mut state, &long);
    }
    state.inject_memory(block(1500));
    let request = request_of(&mut state, &long);
    assert!(request.memory.is_some());
    assert!(
        history_chars_of(&request) <= SERVER_HISTORY_CHARS - 1500,
        "history {} plus the block stays within 12,000",
        history_chars_of(&request)
    );
    assert!(request.messages.len() < 9, "the oldest were omitted");
    assert_eq!(
        request.messages.last().map(|m| m.content.as_str()),
        Some(long.as_str()),
        "the newest message is kept whole"
    );
    assert!(
        state
            .entries()
            .iter()
            .any(|e| *e == Entry::System(TRUNCATION_MARKER.into())),
        "the marker tells what the model no longer sees"
    );
}

#[test]
fn a_block_larger_than_the_cap_still_keeps_the_newest_message() {
    let mut state = chat();
    state.inject_memory(block(SERVER_HISTORY_CHARS + 500));
    let request = request_of(&mut state, "hello");
    assert_eq!(request.messages.len(), 1);
}

/// Ready, but on an engine whose adapter cannot carry the block.
fn cli() -> Availability {
    Availability::Ready {
        engine: "claude".into(),
        model: None,
    }
}

#[test]
fn an_engine_that_cannot_carry_the_block_gets_none_and_no_budget_is_taken() {
    let long = "x".repeat(1200);
    let mut state = chat();
    for _ in 0..4 {
        exchange(&mut state, &long);
    }
    state.inject_memory(block(1500));
    let SendOutcome::Sent { request, .. } = state.send(&long, &cli(), SYSTEM) else {
        panic!("refused");
    };
    assert_eq!(
        request.memory, None,
        "the model is not given notes it cannot get"
    );
    assert_eq!(request.messages.len(), 9, "the whole history, no deduction");
    assert!(
        state
            .entries()
            .iter()
            .all(|e| *e != Entry::System(TRUNCATION_MARKER.into())),
        "nothing omitted"
    );
}
