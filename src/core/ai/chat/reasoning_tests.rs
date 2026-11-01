//! Reasoning (thinking deltas), the stream phase and the toggle.

use std::time::{Duration, Instant};

use super::engine::{ChatError, Finish};
use super::state_tests::{chat, ready, systems, texts};
use super::*;

fn thinking(text: &str) -> StreamEvent {
    StreamEvent::ThinkingDelta(text.into())
}

fn text(text: &str) -> StreamEvent {
    StreamEvent::Delta(text.into())
}

fn done() -> StreamEvent {
    StreamEvent::Done(Finish::Stop)
}

fn at(base: Instant, ms: u64) -> Instant {
    base + Duration::from_millis(ms)
}

fn sent_at(state: &mut ChatState, body: &str, now: Instant) -> (u64, ChatRequest) {
    match state.send_at(body, &ready(), "sys", now) {
        SendOutcome::Sent { turn, request } => (turn, request),
        SendOutcome::Refused => panic!("send refused"),
    }
}

fn blocks(state: &ChatState) -> Vec<&Reasoning> {
    state
        .entries()
        .iter()
        .filter_map(|e| match e {
            Entry::Reasoning(r) => Some(r),
            _ => None,
        })
        .collect()
}

// The seam and the block ------------------------------------------------------

#[test]
fn a_thinking_delta_before_the_answer_starts_the_block_and_replaces_the_waiting_text() {
    let base = Instant::now();
    let mut state = chat();
    let (turn, _) = sent_at(&mut state, "hi", base);
    assert_eq!(state.waiting_text(), Some(WAITING));
    state.apply_at(turn, thinking("Let me"), at(base, 500));
    let all = blocks(&state);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].text(), "Let me");
    assert_eq!(all[0].state(), ReasoningState::Streaming);
    assert_eq!(state.waiting_text(), None);
    state.apply_at(turn, thinking(" check"), at(base, 900));
    assert_eq!(blocks(&state)[0].text(), "Let me check");
    assert_eq!(blocks(&state).len(), 1, "one block per turn");
}

#[test]
fn a_thinking_delta_replaces_the_loading_text_too() {
    let base = Instant::now();
    let mut state = chat();
    let (turn, _) = sent_at(&mut state, "hi", base);
    state.apply_at(turn, StreamEvent::Slow, at(base, 3000));
    assert_eq!(state.waiting_text(), Some(STILL_WAITING));
    state.apply_at(turn, thinking("x"), at(base, 3100));
    assert_eq!(state.waiting_text(), None);
}

#[test]
fn an_empty_thinking_delta_starts_nothing() {
    let base = Instant::now();
    let mut state = chat();
    let (turn, _) = sent_at(&mut state, "hi", base);
    state.apply_at(turn, thinking(""), base);
    assert!(blocks(&state).is_empty());
    assert_eq!(state.waiting_text(), Some(WAITING));
}

#[test]
fn the_first_answer_text_collapses_the_block_once_with_the_seconds() {
    let base = Instant::now();
    let mut state = chat();
    let (turn, _) = sent_at(&mut state, "hi", base);
    state.apply_at(turn, thinking("Let me"), at(base, 1000));
    state.apply_at(turn, thinking(" check"), at(base, 2000));
    state.apply_at(turn, text("Hi"), at(base, 3300));
    let all = blocks(&state);
    assert_eq!(all[0].state(), ReasoningState::Collapsed);
    assert_eq!(all[0].seconds_label(), "2.3s");
    // The block sits before the answer it belongs to.
    let kinds: Vec<_> = state
        .entries()
        .iter()
        .map(|e| match e {
            Entry::Message(m) => format!("{:?}", m.role),
            Entry::Reasoning(_) => "Reasoning".to_owned(),
            Entry::System(_) => "System".to_owned(),
            Entry::Memory(_) => "Memory".to_owned(),
            Entry::Tool(_) => "Tool".to_owned(),
            Entry::Usage(_) => "Usage".to_owned(),
            Entry::Worker(_) => "Worker".to_owned(),
            Entry::RunUsage(_) => "RunUsage".to_owned(),
        })
        .collect();
    assert_eq!(kinds, ["User", "Reasoning", "Assistant"]);
    // More answer text does not re-collapse or re-time it.
    state.apply_at(turn, text("!"), at(base, 9000));
    assert_eq!(blocks(&state)[0].seconds_label(), "2.3s");
}

#[test]
fn seconds_have_one_decimal() {
    let base = Instant::now();
    let mut state = chat();
    let (turn, _) = sent_at(&mut state, "hi", base);
    state.apply_at(turn, thinking("a"), at(base, 0));
    state.apply_at(turn, done(), at(base, 1000));
    assert_eq!(blocks(&state)[0].seconds_label(), "1.0s");
    let (turn, _) = sent_at(&mut state, "again", at(base, 2000));
    state.apply_at(turn, thinking("a"), at(base, 2000));
    state.apply_at(turn, text("x"), at(base, 2000 + 61_400));
    assert_eq!(blocks(&state)[1].seconds_label(), "61.4s");
}

#[test]
fn done_with_no_answer_text_collapses_the_block_and_leaves_no_empty_message() {
    let base = Instant::now();
    let mut state = chat();
    let (turn, _) = sent_at(&mut state, "hi", base);
    state.apply_at(turn, thinking("Let me"), at(base, 100));
    state.apply_at(turn, done(), at(base, 1100));
    let all = blocks(&state);
    assert_eq!(all[0].state(), ReasoningState::Collapsed);
    assert_eq!(all[0].seconds_label(), "1.0s");
    assert_eq!(texts(&state).len(), 1, "only the user message");
    assert_eq!(state.phase(), Phase::Idle);
}

#[test]
fn a_thinking_delta_after_the_collapse_is_dropped() {
    let base = Instant::now();
    let mut state = chat();
    let (turn, _) = sent_at(&mut state, "hi", base);
    state.apply_at(turn, thinking("Let me"), at(base, 0));
    state.apply_at(turn, text("Hi"), at(base, 1000));
    assert_eq!(blocks(&state)[0].text(), "Let me");
    let before = state.clone();
    assert_eq!(
        state.apply_at(turn, thinking("late"), at(base, 1500)),
        Follow::Nothing
    );
    assert_eq!(state, before);
    assert_eq!(blocks(&state)[0].text(), "Let me");
}

#[test]
fn push_thinking_drops_late_reasoning_without_an_error_or_a_second_block() {
    let base = Instant::now();
    let mut state = chat();
    let (turn, _) = sent_at(&mut state, "hi", base);
    state.apply_at(turn, thinking("first"), at(base, 0));
    state.apply_at(turn, text("ok"), at(base, 10));
    // Late pieces between answer pieces, in the same turn.
    assert!(!state.push_thinking("late", at(base, 20)));
    state.apply_at(turn, text("!"), at(base, 30));
    assert!(!state.push_thinking("later", at(base, 40)));
    state.apply_at(turn, done(), at(base, 50));
    assert_eq!(blocks(&state).len(), 1);
    assert_eq!(blocks(&state)[0].text(), "first");
    let said: Vec<String> = texts(&state).into_iter().map(|t| t.1).collect();
    assert_eq!(said, ["hi", "ok!"]);
    assert!(systems(&state).is_empty(), "no error line");
}

#[test]
fn a_thinking_delta_of_an_old_turn_is_dropped() {
    let base = Instant::now();
    let mut state = chat();
    let (old, _) = sent_at(&mut state, "hi", base);
    state.cancel_at(at(base, 10));
    let (new, _) = sent_at(&mut state, "again", at(base, 20));
    state.apply_at(old, thinking("stale"), at(base, 30));
    assert!(blocks(&state).is_empty());
    state.apply_at(new, thinking("fresh"), at(base, 40));
    assert_eq!(blocks(&state)[0].text(), "fresh");
}

#[test]
fn two_chats_reason_independently() {
    let base = Instant::now();
    let (mut a, mut b) = (chat(), chat());
    let (ta, _) = sent_at(&mut a, "hi", base);
    let (tb, _) = sent_at(&mut b, "hi", base);
    a.apply_at(ta, thinking("only a"), at(base, 10));
    assert_eq!(blocks(&a)[0].text(), "only a");
    assert!(blocks(&b).is_empty());
    b.apply_at(tb, thinking("only b"), at(base, 20));
    assert_eq!(blocks(&a)[0].text(), "only a");
    assert_eq!(blocks(&b)[0].text(), "only b");
}

// The phase ---------------------------------------------------------------------

#[test]
fn an_idle_chat_has_no_phase_start_or_model() {
    let state = chat();
    assert_eq!(state.stream_phase(), None);
    assert_eq!(state.stream_started(), None);
    assert_eq!(state.stream_model(), "");
}

#[test]
fn the_phase_is_thinking_from_the_send_until_the_first_answer_text() {
    let base = Instant::now();
    let mut state = chat();
    let (turn, _) = sent_at(&mut state, "hi", base);
    assert_eq!(state.stream_phase(), Some(StreamPhase::Thinking));
    state.apply_at(turn, StreamEvent::Slow, at(base, 3000));
    assert_eq!(state.stream_phase(), Some(StreamPhase::Thinking));
    state.apply_at(turn, thinking("hm"), at(base, 3100));
    assert_eq!(state.stream_phase(), Some(StreamPhase::Thinking));
    state.apply_at(turn, text(""), at(base, 3200));
    assert_eq!(
        state.stream_phase(),
        Some(StreamPhase::Thinking),
        "an empty text delta is not answer text"
    );
    state.apply_at(turn, text("Hi"), at(base, 3300));
    assert_eq!(state.stream_phase(), Some(StreamPhase::Answering));
    state.apply_at(turn, thinking("late"), at(base, 3400));
    assert_eq!(state.stream_phase(), Some(StreamPhase::Answering));
}

#[test]
fn the_start_instant_and_the_request_model_are_those_of_the_send() {
    let base = Instant::now();
    let mut state = chat();
    let (turn, request) = sent_at(&mut state, "hi", at(base, 250));
    assert_eq!(request.model.as_deref(), Some("llama3:8b"));
    assert_eq!(state.stream_started(), Some(at(base, 250)));
    assert_eq!(state.stream_model(), "llama3:8b");
    state.apply_at(turn, text("a"), at(base, 900));
    assert_eq!(state.stream_started(), Some(at(base, 250)));
    assert_eq!(state.stream_model(), "llama3:8b");
}

#[test]
fn a_request_without_a_model_reports_an_empty_one() {
    let base = Instant::now();
    let mut state = chat();
    let cli = Availability::Ready {
        engine: "claude".into(),
        model: None,
    };
    state.send_at("hi", &cli, "", base);
    assert_eq!(state.stream_phase(), Some(StreamPhase::Thinking));
    assert_eq!(state.stream_model(), "");
}

fn ends(final_event: Option<StreamEvent>) -> ChatState {
    let base = Instant::now();
    let mut state = chat();
    let (turn, _) = sent_at(&mut state, "hi", base);
    state.apply_at(turn, text("a"), at(base, 10));
    match final_event {
        Some(event) => {
            state.apply_at(turn, event, at(base, 20));
        }
        None => {
            state.cancel_at(at(base, 20));
        }
    }
    state
}

#[test]
fn the_phase_is_cleared_on_final_error_cut_stream_and_cancel() {
    for state in [
        ends(Some(done())),
        ends(Some(StreamEvent::Error(ChatError::Failed("boom".into())))),
        ends(Some(StreamEvent::Error(ChatError::Interrupted))),
        ends(Some(StreamEvent::Error(ChatError::Truncated))),
        ends(None),
    ] {
        assert_eq!(state.stream_phase(), None);
        assert_eq!(state.stream_started(), None);
        assert_eq!(state.stream_model(), "");
    }
}

#[test]
fn the_phase_is_cleared_when_the_answer_cap_ends_the_turn() {
    let base = Instant::now();
    let mut state = chat();
    let (turn, _) = sent_at(&mut state, "hi", base);
    let follow = state.apply_at(turn, text(&"a".repeat(ANSWER_CAP + 1)), at(base, 5));
    assert_eq!(follow, Follow::CancelEngine);
    assert_eq!(state.stream_phase(), None);
}

// Cut, cancel and error while thinking ------------------------------------------

fn thinking_only() -> (ChatState, u64, Instant) {
    let base = Instant::now();
    let mut state = chat();
    let (turn, _) = sent_at(&mut state, "hi", base);
    state.apply_at(turn, thinking("Let me"), at(base, 100));
    (state, turn, base)
}

#[test]
fn cancel_while_thinking_keeps_the_reasoning_collapsed_and_adds_no_line() {
    let (mut state, _, base) = thinking_only();
    assert_eq!(state.cancel_at(at(base, 2100)), Follow::CancelEngine);
    let all = blocks(&state);
    assert_eq!(all[0].state(), ReasoningState::Collapsed);
    assert_eq!(all[0].text(), "Let me");
    assert_eq!(all[0].seconds_label(), "2.0s");
    assert_eq!(texts(&state).len(), 1, "no empty assistant message");
    assert!(systems(&state).is_empty());
    assert_eq!(state.phase(), Phase::Idle);
}

#[test]
fn an_error_while_thinking_keeps_the_reasoning_and_shows_the_error_line() {
    let (mut state, turn, base) = thinking_only();
    let error = StreamEvent::Error(ChatError::Failed("connection reset".into()));
    state.apply_at(turn, error, at(base, 1100));
    assert_eq!(blocks(&state)[0].state(), ReasoningState::Collapsed);
    assert_eq!(blocks(&state)[0].seconds_label(), "1.0s");
    assert_eq!(texts(&state).len(), 1);
    assert_eq!(systems(&state), ["connection reset"]);
    assert_eq!(state.phase(), Phase::Idle);
}

#[test]
fn a_cut_stream_while_thinking_keeps_the_reasoning_and_says_interrupted() {
    let (mut state, turn, base) = thinking_only();
    state.apply_at(
        turn,
        StreamEvent::Error(ChatError::Interrupted),
        at(base, 600),
    );
    assert_eq!(blocks(&state)[0].state(), ReasoningState::Collapsed);
    assert_eq!(texts(&state).len(), 1);
    assert_eq!(systems(&state), [INTERRUPTED]);
}

#[test]
fn cancelling_with_no_reasoning_adds_no_block() {
    let base = Instant::now();
    let mut state = chat();
    sent_at(&mut state, "hi", base);
    state.cancel_at(at(base, 5));
    assert!(blocks(&state).is_empty());
}

// Cap, history and storage --------------------------------------------------------

#[test]
fn reasoning_stops_at_the_cap_without_an_error_and_the_answer_is_intact() {
    let base = Instant::now();
    let mut state = chat();
    let (turn, _) = sent_at(&mut state, "hi", base);
    let chunk = "é".repeat(1024); // 2 KiB
    for _ in 0..(REASONING_CAP / 2048 + 10) {
        assert_eq!(
            state.apply_at(turn, thinking(&chunk), at(base, 1)),
            Follow::Nothing
        );
    }
    let kept = blocks(&state)[0].text().len();
    assert_eq!(kept, REASONING_CAP);
    state.apply_at(turn, thinking("more"), at(base, 2));
    assert_eq!(blocks(&state)[0].text().len(), REASONING_CAP);
    state.apply_at(turn, text("Hi"), at(base, 3));
    state.apply_at(turn, done(), at(base, 4));
    assert_eq!(texts(&state)[1].1, "Hi");
    assert_eq!(texts(&state)[1].2, MsgStatus::Complete);
    assert!(systems(&state).is_empty());
}

#[test]
fn the_cap_cuts_on_a_character_boundary() {
    let base = Instant::now();
    let mut state = chat();
    let (turn, _) = sent_at(&mut state, "hi", base);
    state.apply_at(turn, thinking(&"a".repeat(REASONING_CAP - 1)), base);
    state.apply_at(turn, thinking("日本"), base);
    assert_eq!(blocks(&state)[0].text().len(), REASONING_CAP - 1);
}

#[test]
fn the_next_request_holds_message_texts_only() {
    let base = Instant::now();
    let mut state = chat();
    let (turn, _) = sent_at(&mut state, "first", base);
    state.apply_at(turn, thinking("secret plan"), at(base, 1));
    state.apply_at(turn, text("answer"), at(base, 2));
    state.apply_at(turn, done(), at(base, 3));
    let (_, request) = sent_at(&mut state, "second", at(base, 4));
    let contents: Vec<&str> = request
        .messages
        .iter()
        .map(|m| m.content.as_str())
        .collect();
    assert_eq!(contents, ["first", "answer", "second"]);
    assert!(!format!("{request:?}").contains("secret plan"));
}

#[test]
fn a_reasoning_only_turn_leaves_no_assistant_message_to_send_back() {
    let base = Instant::now();
    let mut state = chat();
    let (turn, _) = sent_at(&mut state, "first", base);
    state.apply_at(turn, thinking("hmm"), at(base, 1));
    state.apply_at(turn, done(), at(base, 2));
    assert_eq!(texts(&state).len(), 1);
    let (_, request) = sent_at(&mut state, "second", at(base, 3));
    assert_eq!(request.messages.len(), 1, "the unanswered one is left out");
    assert_eq!(request.messages[0].content, "second");
}

#[test]
fn a_resumed_chat_has_its_messages_and_no_reasoning() {
    let stored = vec![
        Msg {
            role: Role::User,
            text: "q".into(),
            status: MsgStatus::Complete,
        },
        Msg {
            role: Role::Assistant,
            text: "a".into(),
            status: MsgStatus::Cancelled,
        },
    ];
    let mut state = ChatState::resumed(None, stored);
    assert_eq!(texts(&state).len(), 2);
    assert!(blocks(&state).is_empty());
    assert!(!state.toggle_reasoning());
    assert_eq!(state.stream_phase(), None);
}

// The toggle ---------------------------------------------------------------------

fn finished_turn(state: &mut ChatState, base: Instant, body: &str, reason: Option<&str>) {
    let (turn, _) = sent_at(state, body, base);
    if let Some(reason) = reason {
        state.apply_at(turn, thinking(reason), at(base, 10));
    }
    state.apply_at(turn, text("answer"), at(base, 1010));
    state.apply_at(turn, done(), at(base, 1020));
}

#[test]
fn the_toggle_expands_and_collapses_the_most_recent_block() {
    let base = Instant::now();
    let mut state = chat();
    finished_turn(&mut state, base, "q", Some("Let me check"));
    assert!(state.toggle_reasoning());
    assert_eq!(blocks(&state)[0].state(), ReasoningState::Expanded);
    assert!(state.toggle_reasoning());
    assert_eq!(blocks(&state)[0].state(), ReasoningState::Collapsed);
}

#[test]
fn the_toggle_does_nothing_while_reasoning_streams() {
    let (mut state, turn, base) = thinking_only();
    let before = state.clone();
    assert!(!state.toggle_reasoning());
    assert_eq!(state, before);
    state.apply_at(turn, text("Hi"), at(base, 500));
    assert!(
        state.toggle_reasoning(),
        "it works once the block collapsed"
    );
}

#[test]
fn the_toggle_does_nothing_without_reasoning() {
    let base = Instant::now();
    let mut state = chat();
    assert!(!state.toggle_reasoning());
    finished_turn(&mut state, base, "q", None);
    let before = state.clone();
    assert!(!state.toggle_reasoning());
    assert_eq!(state, before);
}

#[test]
fn only_the_most_recent_block_toggles() {
    let base = Instant::now();
    let mut state = chat();
    finished_turn(&mut state, base, "one", Some("first"));
    finished_turn(&mut state, at(base, 5000), "two", Some("second"));
    state.toggle_reasoning();
    let all = blocks(&state);
    assert_eq!(all[0].state(), ReasoningState::Collapsed);
    assert_eq!(all[1].state(), ReasoningState::Expanded);
    // A later turn without reasoning leaves the last block the target.
    finished_turn(&mut state, at(base, 9000), "three", None);
    state.toggle_reasoning();
    assert_eq!(blocks(&state)[1].state(), ReasoningState::Collapsed);
    assert_eq!(blocks(&state)[0].state(), ReasoningState::Collapsed);
}

#[test]
fn the_transcript_generation_moves_with_every_change() {
    let base = Instant::now();
    let mut state = chat();
    let g0 = state.generation();
    let (turn, _) = sent_at(&mut state, "hi", base);
    let g1 = state.generation();
    state.apply_at(turn, thinking("a"), at(base, 1));
    let g2 = state.generation();
    state.apply_at(turn, thinking("late-dropped-after-text"), at(base, 2));
    state.apply_at(turn, text("x"), at(base, 3));
    let g3 = state.generation();
    state.apply_at(turn, done(), at(base, 4));
    state.toggle_reasoning();
    let g4 = state.generation();
    assert!(g0 < g1 && g1 < g2 && g2 < g3 && g3 < g4);
}
