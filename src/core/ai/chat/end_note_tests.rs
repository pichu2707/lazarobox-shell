//! The visible end of a turn (spec chat-pane "A turn never ends silently"):
//! no text after the last tool call, and a command typed but not run.

use super::engine::{CallId, ChatError, Finish, ToolStatus};
use super::next_step::EMPTY_NOTICE;
use super::state_tests::{chat, sent, systems};
use super::*;

fn call(id: u64) -> StreamEvent {
    StreamEvent::ToolCall {
        id: CallId(id),
        name: "run_in_pane".into(),
        summary: "secret-command --flag".into(),
        approval: None,
    }
}

fn result(id: u64) -> StreamEvent {
    StreamEvent::ToolResult {
        id: CallId(id),
        status: ToolStatus::Ok,
        summary: "typed, not run".into(),
    }
}

fn delta(text: &str) -> StreamEvent {
    StreamEvent::Delta(text.into())
}

fn done() -> StreamEvent {
    StreamEvent::Done(Finish::Stop)
}

#[test]
fn an_empty_last_round_after_a_tool_call_says_so() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "go");
    state.apply(turn, delta("Let me check."));
    state.apply(turn, call(1));
    state.apply(turn, result(1));
    state.apply(turn, done());
    assert_eq!(systems(&state), [AFTER_TOOL_NO_ANSWER]);
}

#[test]
fn text_after_the_last_tool_call_needs_no_line() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "go");
    state.apply(turn, delta("Let me check."));
    state.apply(turn, call(1));
    state.apply(turn, result(1));
    state.apply(turn, delta("Done."));
    state.apply(turn, done());
    assert!(systems(&state).is_empty());
}

#[test]
fn text_before_a_tool_call_does_not_count_for_a_later_call() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "go");
    state.apply(turn, call(1));
    state.apply(turn, result(1));
    state.apply(turn, delta("Done."));
    state.apply(turn, call(2));
    state.apply(turn, result(2));
    state.apply(turn, done());
    assert_eq!(systems(&state), [AFTER_TOOL_NO_ANSWER]);
}

#[test]
fn a_turn_with_no_text_at_all_keeps_the_plain_line() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "go");
    state.apply(turn, call(1));
    state.apply(turn, result(1));
    state.apply(turn, done());
    assert_eq!(systems(&state), [NO_ANSWER]);
}

#[test]
fn a_notice_after_the_last_tool_call_is_not_repeated() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "go");
    state.apply(turn, delta("Let me check."));
    state.apply(turn, call(1));
    state.apply(turn, result(1));
    state.apply(turn, StreamEvent::Notice(EMPTY_NOTICE.into()));
    state.apply(turn, done());
    assert_eq!(systems(&state), [EMPTY_NOTICE]);
}

#[test]
fn a_failure_or_an_interrupt_keeps_its_own_single_line() {
    for (error, line) in [
        (ChatError::Failed("boom".into()), "boom".to_owned()),
        (ChatError::Interrupted, INTERRUPTED.to_owned()),
    ] {
        let mut state = chat();
        let (turn, _) = sent(&mut state, "go");
        state.apply(turn, delta("Let me check."));
        state.apply(turn, call(1));
        state.apply(turn, StreamEvent::Error(error));
        assert_eq!(systems(&state), [line.as_str()]);
    }
}

#[test]
fn a_typed_command_gets_one_waiting_line_with_the_pane_only() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "go");
    state.apply(turn, call(1));
    state.note_typed_not_run(2);
    state.apply(turn, result(1));
    state.apply(turn, delta("Typed it."));
    state.apply(turn, done());
    assert_eq!(systems(&state), [waiting_for_enter(2)]);
    assert_eq!(
        waiting_for_enter(2),
        "waiting for you: press Enter in pane 2 to run it, then send a message to go on"
    );
    assert!(!waiting_for_enter(2).contains("secret-command"));
}

#[test]
fn two_typed_commands_still_give_one_line_and_it_resets_each_turn() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "go");
    state.note_typed_not_run(1);
    state.note_typed_not_run(3);
    state.apply(turn, delta("ok"));
    state.apply(turn, done());
    assert_eq!(systems(&state), [waiting_for_enter(3)]);
    let (turn, _) = sent(&mut state, "again");
    state.apply(turn, delta("ok"));
    state.apply(turn, done());
    assert_eq!(systems(&state), [waiting_for_enter(3)]);
}

#[test]
fn the_waiting_line_follows_a_turn_that_ended_with_an_error() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "go");
    state.note_typed_not_run(1);
    state.apply(turn, StreamEvent::Error(ChatError::Failed("boom".into())));
    assert_eq!(systems(&state), ["boom", waiting_for_enter(1).as_str()]);
}
