//! The visible end of a turn (spec chat-pane "A turn never ends silently"):
//! no text after the last tool call, and a command typed but not run.

use super::engine::{CallId, ChatError, Finish, ToolStatus};
use super::next_step::{EMPTY_NOTICE, STOPPED_NOTICE};
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
    result_with(id, ToolStatus::Ok)
}

fn result_with(id: u64, status: ToolStatus) -> StreamEvent {
    StreamEvent::ToolResult {
        id: CallId(id),
        status,
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
    assert_eq!(systems(&state), [waiting_for_enter(2, false)]);
    assert_eq!(
        waiting_for_enter(2, false),
        "waiting for you: press Enter in pane 2 to run it, then send a message to go on"
    );
    assert!(!waiting_for_enter(2, false).contains("secret-command"));
}

#[test]
fn a_followed_command_promises_the_chat_goes_on_and_only_for_its_pane() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "go");
    state.apply(turn, call(1));
    state.note_typed_not_run(2);
    state.note_followed(2);
    state.apply(turn, result(1));
    state.apply(turn, done());
    let tracked =
        "waiting for you: press Enter in pane 2 to run it; the chat goes on when it finishes";
    assert!(systems(&state).contains(&tracked));
    // The mark of another pane promises nothing for this one, and resets.
    let (turn, _) = sent(&mut state, "again");
    state.apply(turn, call(2));
    state.note_typed_not_run(3);
    state.note_followed(2);
    state.apply(turn, result(2));
    state.apply(turn, done());
    assert!(
        systems(&state)
            .iter()
            .any(|l| l.ends_with("then send a message to go on"))
    );
}

#[test]
fn two_typed_commands_still_give_one_line_and_it_resets_each_turn() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "go");
    state.note_typed_not_run(1);
    state.note_typed_not_run(3);
    state.apply(turn, delta("ok"));
    state.apply(turn, done());
    assert_eq!(systems(&state), [waiting_for_enter(3, false)]);
    let (turn, _) = sent(&mut state, "again");
    state.apply(turn, delta("ok"));
    state.apply(turn, done());
    assert_eq!(systems(&state), [waiting_for_enter(3, false)]);
}

#[test]
fn the_waiting_line_follows_a_turn_that_ended_with_an_error() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "go");
    state.note_typed_not_run(1);
    state.apply(turn, StreamEvent::Error(ChatError::Failed("boom".into())));
    assert_eq!(
        systems(&state),
        ["boom", waiting_for_enter(1, false).as_str()]
    );
}

#[test]
fn an_auto_typed_command_counts_only_once_the_runtime_typed_it() {
    for (status, shown) in [
        (ToolStatus::Ok, true),
        (ToolStatus::Denied, false),
        (ToolStatus::Error, false),
    ] {
        let mut state = chat();
        let (turn, _) = sent(&mut state, "go");
        state.apply(turn, call(1));
        state.expect_typed(CallId(1), 2);
        state.apply(turn, result_with(1, status));
        state.apply(turn, delta("ok"));
        state.apply(turn, done());
        let want: Vec<String> = shown
            .then(|| waiting_for_enter(2, false))
            .into_iter()
            .collect();
        assert_eq!(systems(&state), want, "{status:?}");
    }
}

#[test]
fn an_auto_typed_command_with_no_result_gives_no_line() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "go");
    state.apply(turn, call(1));
    state.expect_typed(CallId(1), 2);
    state.apply(turn, delta("ok"));
    state.apply(turn, done());
    assert!(systems(&state).is_empty());
}

#[test]
fn a_late_note_after_the_turn_ended_does_not_reach_the_next_turn() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "go");
    state.apply(turn, delta("ok"));
    state.apply(turn, done());
    state.note_typed_not_run(4);
    let (turn, _) = sent(&mut state, "again");
    state.apply(turn, delta("ok"));
    state.apply(turn, done());
    assert!(systems(&state).is_empty());
}

#[test]
fn every_end_notice_after_the_last_tool_call_stands_alone() {
    let notices = [
        STOPPED_NOTICE.to_owned(),
        "tool limit reached for this turn".to_owned(),
    ];
    for line in notices {
        let mut state = chat();
        let (turn, _) = sent(&mut state, "go");
        state.apply(turn, delta("Let me check."));
        state.apply(turn, call(1));
        state.apply(turn, result(1));
        state.apply(turn, StreamEvent::Notice(line.clone()));
        state.apply(turn, done());
        assert_eq!(systems(&state), [line.as_str()]);
    }
}

#[test]
fn too_long_and_jail_endings_keep_one_line() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "go");
    state.apply(turn, delta("Let me check."));
    state.apply(turn, call(1));
    state.apply(turn, StreamEvent::Error(ChatError::Truncated));
    assert_eq!(systems(&state), [TOO_LONG]);
    let mut state = chat();
    let (turn, _) = sent(&mut state, "go");
    state.apply(turn, delta("Let me check."));
    state.apply(turn, call(1));
    state.apply(turn, StreamEvent::Error(ChatError::Jail("no bwrap".into())));
    assert_eq!(systems(&state), [jail_error("no bwrap")]);
}
