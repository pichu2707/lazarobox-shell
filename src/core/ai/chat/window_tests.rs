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
    *request
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
    state.set_tools_reserve(1_500);
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
    state.set_tools_reserve(1_500);
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

#[test]
fn a_trim_drops_whole_pairs_oldest_first_and_starts_on_a_user_message() {
    use crate::core::ai::chat::engine::{ChatMessage, Role, build_context_tokens};
    let msg = |role, text: &str| ChatMessage {
        role,
        content: text.into(),
    };
    let history = [
        msg(Role::User, &"1".repeat(400)),
        msg(Role::Assistant, &"2".repeat(400)),
        msg(Role::User, &"3".repeat(400)),
        msg(Role::Assistant, &"4".repeat(400)),
        msg(Role::User, &"5".repeat(400)),
    ];
    // Room for 250 tokens: the newest (100) and one answer (100) fit, but a
    // window may not start on an answer, so the pair u2 a2 goes whole.
    let context = build_context_tokens(&history, 250);
    assert_eq!(context.messages, history[4..]);
    assert!(context.omitted);
    // Room for 300: u3 and a4 fit with the newest, and the window starts on u3.
    let context = build_context_tokens(&history, 300);
    assert_eq!(context.messages, history[2..]);
    assert_eq!(context.messages[0].role, Role::User);
    // The newest alone is kept whole even with no room at all.
    let context = build_context_tokens(&history, 0);
    assert_eq!(context.messages, history[4..]);
}

fn over_window_lines(state: &ChatState) -> Vec<String> {
    state
        .entries()
        .iter()
        .filter_map(|e| match e {
            Entry::System(line) if line.starts_with("this message and the setup") => {
                Some(line.clone())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_message_that_cannot_fit_the_window_says_so_once() {
    let mut state = chat();
    state.set_window(Some(1_000));
    // 8,000 characters are about 2,000 tokens, past the 1,000-token window.
    let SendOutcome::Sent { request, .. } = state.send(&"x".repeat(8_000), &ready(), SYSTEM) else {
        panic!("refused");
    };
    // Still sent whole: the server decides, the user was told.
    assert_eq!(request.messages.last().unwrap().content.len(), 8_000);
    let lines = over_window_lines(&state);
    assert_eq!(lines.len(), 1, "{:?}", state.entries());
    assert!(
        lines[0].contains("over the 1000-token window"),
        "{}",
        lines[0]
    );
    assert!(
        lines[0].ends_with("the server may cut them"),
        "{}",
        lines[0]
    );
}

#[test]
fn a_message_that_fits_or_an_unknown_window_say_nothing() {
    let mut fits = chat();
    fits.set_window(Some(4_096));
    exchange(&mut fits, &"x".repeat(400));
    assert!(over_window_lines(&fits).is_empty());
    let mut unknown = chat();
    let _ = exchange(&mut unknown, &"x".repeat(80_000));
    assert!(over_window_lines(&unknown).is_empty());
}

#[test]
fn the_setup_counts_toward_the_over_window_notice() {
    let mut state = chat();
    state.set_window(Some(1_000));
    // The message alone (500 tokens) fits; with 2,000 tokens of tools it does not.
    state.set_tools_reserve(2_000);
    state.send(&"x".repeat(2_000), &ready(), SYSTEM);
    assert_eq!(over_window_lines(&state).len(), 1);
}

#[test]
fn an_overflow_streak_says_so_once_and_again_only_after_a_send_that_fits() {
    let mut state = chat();
    state.set_window(Some(1_000));
    // Three sends past the window with different sizes: one line, not three.
    for size in [8_000, 9_000, 10_000] {
        exchange(&mut state, &"x".repeat(size));
    }
    assert_eq!(over_window_lines(&state).len(), 1, "{:?}", state.entries());
    // A send that fits ends the streak; the next overflow is told again.
    exchange(&mut state, "short");
    assert_eq!(over_window_lines(&state).len(), 1);
    exchange(&mut state, &"x".repeat(8_000));
    assert_eq!(over_window_lines(&state).len(), 2, "{:?}", state.entries());
}

// #556: the runtime reports what the external tools of the turn weigh. The
// fill of the turn in flight is corrected, and the next send can reserve it.
#[test]
fn the_external_tools_the_runtime_reports_count_in_the_fill() {
    let mut state = chat();
    state.set_window(Some(10_000));
    state.set_tools_reserve(1_500);
    let SendOutcome::Sent { turn, .. } = state.send(&"x".repeat(4_000), &ready(), SYSTEM) else {
        panic!("refused");
    };
    assert_eq!(state.external_tools_tokens(), 0);
    assert_eq!(state.context_percent(ENGINE, MODEL), Some(25));
    state.apply(turn, StreamEvent::ExternalTools(2_000));
    // 2504 + 2000 external = 4504 of 10000.
    assert_eq!(state.context_percent(ENGINE, MODEL), Some(45));
    assert_eq!(state.external_tools_tokens(), 2_000);
}

// A send that already reserved the last report replaces it with the new one,
// never counting both.
#[test]
fn a_new_report_replaces_the_external_part_the_send_counted() {
    let mut state = chat();
    state.set_window(Some(10_000));
    let SendOutcome::Sent { turn, .. } = state.send("a", &ready(), SYSTEM) else {
        panic!("refused");
    };
    state.set_tools_reserve(1_500);
    state.apply(turn, StreamEvent::ExternalTools(2_000));
    state.apply(turn, StreamEvent::Done(Finish::Stop));
    // The app reserves the built-ins and the reported external tools.
    state.set_tools_reserve(1_500 + state.external_tools_tokens());
    let SendOutcome::Sent { turn, .. } = state.send(&"x".repeat(4_000), &ready(), SYSTEM) else {
        panic!("refused");
    };
    // 1000 + 1 history + 4 system + 3500 reserve = 4505.
    assert_eq!(state.context_percent(ENGINE, MODEL), Some(45));
    state.apply(turn, StreamEvent::ExternalTools(500));
    // 4505 - 2000 + 500 = 3005.
    assert_eq!(state.context_percent(ENGINE, MODEL), Some(30));
}

// A turn with the tools off stops the servers: nothing external is kept.
#[test]
fn forgetting_the_external_tools_leaves_nothing_to_reserve() {
    let mut state = chat();
    state.set_window(Some(10_000));
    let SendOutcome::Sent { turn, .. } = state.send("a", &ready(), SYSTEM) else {
        panic!("refused");
    };
    state.apply(turn, StreamEvent::ExternalTools(2_000));
    state.forget_external_tools();
    assert_eq!(state.external_tools_tokens(), 0);
}
