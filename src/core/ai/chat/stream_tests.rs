use super::engine::{ChatError, Finish};
use super::state_tests::{SYSTEM, chat, ready, sent, systems, texts};
use super::*;

fn delta(text: &str) -> StreamEvent {
    StreamEvent::Delta(text.into())
}

#[test]
fn a_send_while_streaming_is_refused_with_a_system_line() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "one");
    assert_eq!(state.send("two", &ready(), SYSTEM), SendOutcome::Refused);
    assert_eq!(systems(&state), [BUSY]);
    assert_eq!(state.turn(), turn);
    state.apply(turn, delta("Hel"));
    assert_eq!(state.send("two", &ready(), SYSTEM), SendOutcome::Refused);
    assert_eq!(state.messages().count(), 2);
    assert_eq!(state.phase(), Phase::Streaming);
}
#[test]
fn waiting_text_follows_the_first_byte_and_the_slow_event() {
    let mut state = chat();
    assert_eq!(state.waiting_text(), None);
    let (turn, _) = sent(&mut state, "hi");
    assert_eq!(state.waiting_text(), Some("waiting for the model…"));
    state.apply(turn, StreamEvent::Slow);
    assert_eq!(state.waiting_text(), Some("loading model…"));
    state.apply(turn, delta("a"));
    assert_eq!(state.waiting_text(), None);
}
#[test]
fn slow_after_the_first_byte_or_when_idle_is_ignored() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "hi");
    state.apply(turn, delta("a"));
    state.apply(turn, StreamEvent::Slow);
    assert_eq!(state.phase(), Phase::Streaming);
    state.apply(turn, StreamEvent::Done(Finish::Stop));
    state.apply(turn, StreamEvent::Slow);
    assert_eq!(state.phase(), Phase::Idle);
}
#[test]
fn deltas_build_the_answer_in_order_and_done_finishes_it() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "hi");
    assert_eq!(state.apply(turn, delta("Hel")), Follow::Nothing);
    assert_eq!(state.apply(turn, delta("lo")), Follow::Nothing);
    assert_eq!(state.phase(), Phase::Streaming);
    state.apply(turn, StreamEvent::Done(Finish::Stop));
    assert_eq!(
        texts(&state)[1],
        (Role::Assistant, "Hello".into(), MsgStatus::Complete)
    );
    assert_eq!(state.phase(), Phase::Idle);
    assert_eq!(state.waiting_text(), None);
    assert!(systems(&state).is_empty());
}
#[test]
fn a_length_finish_is_still_complete() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "hi");
    state.apply(turn, delta("abc"));
    state.apply(turn, StreamEvent::Done(Finish::Length));
    assert_eq!(texts(&state)[1].2, MsgStatus::Complete);
}
#[test]
fn an_error_keeps_the_partial_text_and_adds_a_system_line() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "hi");
    state.apply(turn, delta("Hel"));
    let error = StreamEvent::Error(ChatError::Failed("connection reset".into()));
    assert_eq!(state.apply(turn, error), Follow::Nothing);
    assert_eq!(
        texts(&state)[1],
        (Role::Assistant, "Hel".into(), MsgStatus::Error)
    );
    assert_eq!(systems(&state), ["connection reset"]);
    assert_eq!(state.phase(), Phase::Idle);
    assert!(matches!(
        state.send("again", &ready(), SYSTEM),
        SendOutcome::Sent { .. }
    ));
}
#[test]
fn an_error_before_any_text_leaves_no_empty_message() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "hi");
    state.apply(turn, StreamEvent::Error(ChatError::Failed("boom".into())));
    assert_eq!(texts(&state).len(), 1);
    assert_eq!(systems(&state), ["boom"]);
    assert_eq!(state.phase(), Phase::Idle);
}
#[test]
fn a_cut_stream_keeps_the_text_and_says_interrupted() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "hi");
    state.apply(turn, delta("Hel"));
    state.apply(turn, StreamEvent::Error(ChatError::Interrupted));
    assert_eq!(
        texts(&state)[1],
        (Role::Assistant, "Hel".into(), MsgStatus::Interrupted)
    );
    assert_eq!(systems(&state), ["stream interrupted"]);
    assert_eq!(state.phase(), Phase::Idle);
}
#[test]
fn late_events_of_a_finished_turn_are_dropped() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "hi");
    state.apply(turn, delta("ok"));
    state.apply(turn, StreamEvent::Done(Finish::Stop));
    let before = state.clone();
    for event in [
        delta("late"),
        StreamEvent::Done(Finish::Stop),
        StreamEvent::Error(ChatError::Failed("late".into())),
    ] {
        assert_eq!(state.apply(turn, event), Follow::Nothing);
    }
    assert_eq!(state, before);
}
#[test]
fn events_of_another_turn_are_dropped() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "hi");
    let before = state.clone();
    state.apply(turn + 1, delta("x"));
    state.apply(turn.wrapping_sub(1), delta("x"));
    assert_eq!(state, before);
}
#[test]
fn cancel_keeps_the_partial_answer_as_cancelled() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "hi");
    state.apply(turn, delta("Hel"));
    assert_eq!(state.cancel(), Follow::CancelEngine);
    assert_eq!(
        texts(&state)[1],
        (Role::Assistant, "Hel".into(), MsgStatus::Cancelled)
    );
    assert_eq!(state.phase(), Phase::Idle);
    assert!(state.turn() > turn);
}
#[test]
fn late_events_of_a_cancelled_turn_are_dropped() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "hi");
    state.apply(turn, delta("Hel"));
    state.cancel();
    let before = state.clone();
    for event in [
        delta("lo"),
        StreamEvent::Done(Finish::Stop),
        StreamEvent::Error(ChatError::Failed("x".into())),
    ] {
        assert_eq!(state.apply(turn, event), Follow::Nothing);
    }
    assert_eq!(state, before);
}
#[test]
fn cancel_while_waiting_leaves_no_empty_message() {
    let mut state = chat();
    sent(&mut state, "hi");
    assert_eq!(state.cancel(), Follow::CancelEngine);
    assert_eq!(texts(&state).len(), 1);
    assert_eq!(state.phase(), Phase::Idle);
    assert_eq!(state.waiting_text(), None);
}
#[test]
fn cancel_when_idle_is_a_no_op() {
    let mut state = chat();
    let before = state.clone();
    assert_eq!(state.cancel(), Follow::Nothing);
    assert_eq!(state, before);
}
#[test]
fn the_answer_is_cut_at_one_mib_and_stored_truncated() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "hi");
    let chunk = "a".repeat(ANSWER_CAP - 10);
    assert_eq!(state.apply(turn, delta(&chunk)), Follow::Nothing);
    assert_eq!(
        state.apply(turn, delta(&"b".repeat(100))),
        Follow::CancelEngine
    );
    let answer = &texts(&state)[1];
    assert_eq!(answer.1.len(), ANSWER_CAP);
    assert!(answer.1.ends_with("bbbbbbbbbb"));
    assert_eq!(answer.2, MsgStatus::Truncated);
    assert_eq!(systems(&state), [TOO_LONG]);
    assert_eq!(state.phase(), Phase::Idle);
    let before = state.clone();
    state.apply(turn, delta("more"));
    assert_eq!(state, before);
}
#[test]
fn the_cap_never_splits_a_character() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "hi");
    state.apply(turn, delta(&"a".repeat(ANSWER_CAP - 1)));
    assert_eq!(state.apply(turn, delta("é")), Follow::CancelEngine);
    let answer = &texts(&state)[1];
    assert_eq!(answer.1.len(), ANSWER_CAP - 1);
    assert_eq!(answer.2, MsgStatus::Truncated);
}
#[test]
fn a_truncated_error_from_the_adapter_is_stored_truncated() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "hi");
    state.apply(turn, delta("abc"));
    state.apply(turn, StreamEvent::Error(ChatError::Truncated));
    assert_eq!(texts(&state)[1].2, MsgStatus::Truncated);
    assert_eq!(systems(&state), [TOO_LONG]);
}
#[test]
fn system_lines_never_reach_the_history() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "one");
    state.apply(turn, delta("partial"));
    state.apply(turn, StreamEvent::Error(ChatError::Failed("boom".into())));
    let (_, request) = sent(&mut state, "two");
    let roles: Vec<_> = request
        .messages
        .iter()
        .map(|m| m.content.as_str())
        .collect();
    assert_eq!(roles, ["one", "partial", "two"]);
}
#[test]
fn a_done_with_no_text_says_the_model_sent_no_answer() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "hi");
    assert_eq!(
        state.apply(turn, StreamEvent::Done(Finish::Stop)),
        Follow::Nothing
    );
    assert_eq!(texts(&state).len(), 1, "no empty assistant message");
    assert_eq!(systems(&state), ["the model sent no answer"]);
    assert_eq!(state.phase(), Phase::Idle);
}
#[test]
fn a_user_message_that_got_an_error_before_any_text_is_not_sent_again() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "first");
    state.apply(turn, StreamEvent::Error(ChatError::Failed("boom".into())));
    let (_, request) = sent(&mut state, "second");
    let sent_texts: Vec<_> = request
        .messages
        .iter()
        .map(|m| m.content.as_str())
        .collect();
    assert_eq!(sent_texts, ["second"]);
    assert_eq!(state.messages().count(), 2);
}
#[test]
fn a_partial_answer_still_counts_as_an_answer() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "first");
    state.apply(turn, delta("par"));
    state.cancel();
    let (_, request) = sent(&mut state, "second");
    let sent_texts: Vec<_> = request
        .messages
        .iter()
        .map(|m| m.content.as_str())
        .collect();
    assert_eq!(sent_texts, ["first", "par", "second"]);
}
