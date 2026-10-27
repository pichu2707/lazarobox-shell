//! The context window of the engine sizes a turn's history (chat-memory S1)
//! and the fill of the context is kept for the status line (S2).

use super::{
    engine::{Finish, SERVER_HISTORY_CHARS, StreamEvent},
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
    assert_eq!(chat().context_percent(), None);
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

#[test]
fn the_fill_is_a_percent_of_the_known_window() {
    let mut state = chat();
    state.set_window(Some(10_000));
    exchange(&mut state, &"x".repeat(4_000));
    // 1000 history + 4 system + 1500 tools = 2504 of 10000.
    assert_eq!(state.context_percent(), Some(25));
}

#[test]
fn the_fill_without_a_window_is_a_percent_of_the_cap() {
    let mut state = chat();
    exchange(&mut state, &"x".repeat(6_000));
    // 1500 of the 3000 tokens the 12k cap stands for.
    assert_eq!(state.context_percent(), Some(50));
}
