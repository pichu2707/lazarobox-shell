//! The context window of the engine sizes a turn's history (chat-memory S1)
//! and the fill of the context is kept for the status line (S2).

use super::{
    engine::{ChatError, Finish, SERVER_HISTORY_CHARS, StreamEvent},
    state_tests::{SYSTEM, chat, ready},
    *,
};

/// Sends `text` and answers it with an answer of the same length.
fn exchange(state: &mut ChatState, text: &str) -> ChatRequest {
    let SendOutcome::Sent { turn, request } = state.send(text, &ready(), SYSTEM) else {
        panic!("refused");
    };
    state.apply(turn, StreamEvent::Delta("a".repeat(text.chars().count())));
    state.apply(turn, StreamEvent::Done(Finish::Stop));
    request
}

fn history_chars_of(request: &ChatRequest) -> usize {
    request
        .messages
        .iter()
        .map(|m| m.content.chars().count())
        .sum()
}

#[test]
fn a_chat_that_never_sent_has_no_fill() {
    assert_eq!(chat().context_percent("ollama", Some("llama3:8b")), None);
}

#[test]
fn an_unknown_window_keeps_the_character_cap() {
    let mut state = chat();
    let long = "x".repeat(5_000);
    let mut last = exchange(&mut state, &long);
    for _ in 0..5 {
        last = exchange(&mut state, &long);
    }
    assert!(history_chars_of(&last) <= SERVER_HISTORY_CHARS);
    assert!(last.messages.len() < 11, "the cap dropped the oldest");
}

#[test]
fn a_known_window_lets_more_history_through_than_the_cap() {
    let mut state = chat();
    state.set_window(Some(32_768));
    let long = "x".repeat(5_000);
    let mut last = exchange(&mut state, &long);
    for _ in 0..5 {
        last = exchange(&mut state, &long);
    }
    // 11 messages, 55k chars: far past the 12k cap, well inside the window.
    assert_eq!(last.messages.len(), 11);
    assert!(history_chars_of(&last) > SERVER_HISTORY_CHARS);
    assert!(
        !state
            .entries()
            .iter()
            .any(|e| *e == Entry::System(TRUNCATION_MARKER.into()))
    );
}

#[test]
fn a_small_window_drops_history_the_cap_would_have_kept() {
    let mut state = chat();
    state.set_window(Some(4_096));
    state.set_tools_offered(true);
    let long = "x".repeat(2_000);
    let mut last = exchange(&mut state, &long);
    for _ in 0..4 {
        last = exchange(&mut state, &long);
    }
    // 4096 * 3/4 = 3072, minus the tools reserve: about 1500 tokens of room,
    // so three messages of 500 tokens; the 12k cap would have kept all nine.
    assert!(last.messages.len() <= 3, "{}", last.messages.len());
    assert!(
        state
            .entries()
            .iter()
            .any(|e| *e == Entry::System(TRUNCATION_MARKER.into()))
    );
}

const ENGINE: &str = "ollama";
const MODEL: Option<&str> = Some("llama3:8b");

#[test]
fn the_fill_counts_the_request_until_the_answer_lands() {
    let mut state = chat();
    state.set_window(Some(10_000));
    state.set_tools_offered(true);
    let SendOutcome::Sent { turn, .. } = state.send(&"x".repeat(4_000), &ready(), SYSTEM) else {
        panic!("refused");
    };
    // 1000 history + 4 system + 1500 tools = 2504 of 10000.
    assert_eq!(state.context_percent(ENGINE, MODEL), Some(25));
    state.apply(turn, StreamEvent::Delta("a".repeat(4_000)));
    state.apply(turn, StreamEvent::Done(Finish::Stop));
    // The 1000 tokens of the answer are in the context now: 3504.
    assert_eq!(state.context_percent(ENGINE, MODEL), Some(35));
}

#[test]
fn the_fill_leaves_the_tools_reserve_out_when_no_tools_are_offered() {
    let mut state = chat();
    state.set_window(Some(10_000));
    exchange(&mut state, &"x".repeat(4_000));
    // 1000 history + 4 system + 1000 answer = 2004.
    assert_eq!(state.context_percent(ENGINE, MODEL), Some(20));
}

#[test]
fn the_fill_without_a_window_is_not_shown() {
    let mut state = chat();
    exchange(&mut state, &"x".repeat(6_000));
    assert_eq!(state.context_percent(ENGINE, MODEL), None);
}

#[test]
fn the_fill_of_another_engine_or_model_is_not_shown() {
    let mut state = chat();
    state.set_window(Some(10_000));
    exchange(&mut state, &"x".repeat(4_000));
    assert!(state.context_percent(ENGINE, MODEL).is_some());
    assert_eq!(state.context_percent("llamacpp", MODEL), None);
    assert_eq!(state.context_percent(ENGINE, Some("qwen")), None);
    assert_eq!(state.context_percent(ENGINE, None), None);
}

#[test]
fn a_turn_that_does_not_finish_adds_no_answer_to_the_fill() {
    let mut state = chat();
    state.set_window(Some(10_000));
    let SendOutcome::Sent { turn, .. } = state.send(&"x".repeat(4_000), &ready(), SYSTEM) else {
        panic!("refused");
    };
    state.apply(turn, StreamEvent::Delta("a".repeat(4_000)));
    state.apply(turn, StreamEvent::Error(ChatError::Interrupted));
    // 1000 history + 4 system, no tools.
    assert_eq!(state.context_percent(ENGINE, MODEL), Some(10));
}
