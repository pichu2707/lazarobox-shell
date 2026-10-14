//! What a chat pane saves, and when (chat-history: Saving conversations,
//! Reasoning is not stored).

use super::{
    engine::{ChatError, Finish, Role},
    state_tests::{chat, ready},
    store::{StoreCmd, StoreReply, StoredMessage},
    *,
};
use crate::core::{ai::config::Profile, layout::PaneId};

fn pane() -> PaneId {
    PaneId::FIRST
}

fn sent(state: &mut ChatState, text: &str) -> u64 {
    match state.send(text, &ready(), "sys") {
        SendOutcome::Sent { turn, .. } => turn,
        SendOutcome::Refused => panic!("send refused"),
    }
}

fn delta(text: &str) -> StreamEvent {
    StreamEvent::Delta(text.into())
}

fn thinking(text: &str) -> StreamEvent {
    StreamEvent::ThinkingDelta(text.into())
}

fn done() -> StreamEvent {
    StreamEvent::Done(Finish::Stop)
}

fn created(conversation: i64) -> StoreReply {
    StoreReply::Created {
        pane: pane(),
        conversation,
        evicted: false,
        evicted_notes: 0,
    }
}

/// The commands for what the pane holds now, with no conversation open
/// elsewhere.
fn cmds(state: &mut ChatState) -> Vec<StoreCmd> {
    state.store_cmds(pane(), &[])
}

/// A chat that sent `question` and whose conversation exists as `7`.
fn open_conversation(question: &str) -> (ChatState, u64) {
    let mut state = chat();
    let turn = sent(&mut state, question);
    cmds(&mut state);
    state.on_store_reply(pane(), &created(7));
    (state, turn)
}

fn appended(cmd: &StoreCmd) -> &StoredMessage {
    match cmd {
        StoreCmd::Append { message, .. } => message,
        other => panic!("expected Append, got {other:?}"),
    }
}

// Spec: Created on first send.
#[test]
fn the_first_send_creates_the_conversation_with_profile_tag_and_message() {
    let mut state = chat();
    state.set_profile(Profile::Analyst);
    state.set_tag(Some("proj".into()));
    sent(&mut state, "hello");
    let out = state.store_cmds(pane(), &[3, 4]);
    assert_eq!(
        out,
        vec![StoreCmd::Create {
            pane: pane(),
            profile: "analyst".into(),
            tag: Some("proj".into()),
            first: StoredMessage {
                role: Role::User,
                text: "hello".into(),
                status: MsgStatus::Complete,
                engine: "ollama".into(),
                model: Some("llama3:8b".into()),
            },
            open: vec![3, 4],
        }]
    );
}

#[test]
fn a_command_is_handed_out_once() {
    let mut state = chat();
    sent(&mut state, "hello");
    assert_eq!(cmds(&mut state).len(), 1);
    assert_eq!(cmds(&mut state), Vec::new());
}

// Spec: Assistant saved on finish.
#[test]
fn a_finished_answer_is_appended_complete() {
    let (mut state, turn) = open_conversation("q");
    state.apply(turn, delta("Hi "));
    state.apply(turn, delta("there"));
    state.apply(turn, done());
    let out = cmds(&mut state);
    assert_eq!(out.len(), 1);
    let message = appended(&out[0]);
    assert_eq!(
        (message.role, message.text.as_str(), message.status),
        (Role::Assistant, "Hi there", MsgStatus::Complete)
    );
    assert_eq!(
        (message.engine.as_str(), message.model.as_deref()),
        ("ollama", Some("llama3:8b"))
    );
    assert!(matches!(
        &out[0],
        StoreCmd::Append {
            conversation: 7,
            ..
        }
    ));
}

// Spec: Partial saved on cancel.
#[test]
fn a_cancelled_partial_is_appended_cancelled() {
    let (mut state, turn) = open_conversation("q");
    state.apply(turn, delta("Hel"));
    state.cancel();
    let out = cmds(&mut state);
    assert_eq!(out.len(), 1);
    let message = appended(&out[0]);
    assert_eq!(
        (message.text.as_str(), message.status),
        ("Hel", MsgStatus::Cancelled)
    );
}

// Spec: Cut and over-long answers keep their status.
#[test]
fn a_cut_stream_and_an_over_long_answer_keep_their_status() {
    let (mut state, turn) = open_conversation("q");
    state.apply(turn, delta("part"));
    state.apply(turn, StreamEvent::Error(ChatError::Interrupted));
    let out = cmds(&mut state);
    assert_eq!(appended(&out[0]).status, MsgStatus::Interrupted);

    let turn = sent(&mut state, "again");
    state.apply(turn, delta("long"));
    state.apply(turn, StreamEvent::Error(ChatError::Truncated));
    let out = cmds(&mut state);
    // The user message first, then the partial answer.
    assert_eq!(appended(&out[0]).role, Role::User);
    assert_eq!(appended(&out[1]).status, MsgStatus::Truncated);
}

// Spec: Error lines not stored.
#[test]
fn an_error_with_no_text_stores_only_the_user_message() {
    let mut state = chat();
    let turn = sent(&mut state, "q");
    state.apply(turn, StreamEvent::Error(ChatError::Failed("boom".into())));
    let out = cmds(&mut state);
    assert_eq!(out.len(), 1, "{out:?}");
    assert!(matches!(&out[0], StoreCmd::Create { .. }));
    assert!(!format!("{out:?}").contains("boom"));
}

#[test]
fn an_error_after_partial_text_is_not_stored_either() {
    let (mut state, turn) = open_conversation("q");
    state.apply(turn, delta("par"));
    state.apply(turn, StreamEvent::Error(ChatError::Failed("boom".into())));
    assert_eq!(cmds(&mut state), Vec::new());
}

// Spec: Reasoning only, then cancel, error or cut.
#[test]
fn a_turn_with_only_reasoning_stores_no_assistant_message() {
    for end in [
        None,
        Some(StreamEvent::Error(ChatError::Failed("boom".into()))),
        Some(StreamEvent::Error(ChatError::Interrupted)),
        Some(done()),
    ] {
        let (mut state, turn) = open_conversation("q");
        state.apply(turn, thinking("secret plan"));
        match end {
            Some(event) => {
                state.apply(turn, event);
            }
            None => {
                state.cancel();
            }
        }
        assert_eq!(cmds(&mut state), Vec::new());
    }
}

// Spec: Reasoning never reaches the store.
#[test]
fn reasoning_never_reaches_the_store_commands() {
    let mut state = chat();
    let turn = sent(&mut state, "q");
    state.apply(turn, thinking("secret plan"));
    state.apply(turn, delta("Hi"));
    state.apply(turn, done());
    let mut out = cmds(&mut state);
    // The answer was queued behind the create and comes out with the reply.
    out.extend(state.on_store_reply(pane(), &created(7)));
    assert!(!format!("{out:?}").contains("secret plan"));
    let saved: Vec<(&str, MsgStatus)> = out
        .iter()
        .map(|c| match c {
            StoreCmd::Create { first, .. } => (first.text.as_str(), first.status),
            StoreCmd::Append { message, .. } => (message.text.as_str(), message.status),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        saved,
        vec![("q", MsgStatus::Complete), ("Hi", MsgStatus::Complete)]
    );
}

// Spec: Partial answer keeps no reasoning.
#[test]
fn a_partial_answer_keeps_no_reasoning() {
    let (mut state, turn) = open_conversation("q");
    state.apply(turn, thinking("secret plan"));
    state.apply(turn, delta("Hel"));
    state.cancel();
    let out = cmds(&mut state);
    assert_eq!(out.len(), 1);
    assert_eq!(appended(&out[0]).text, "Hel");
    assert!(!format!("{out:?}").contains("secret plan"));
}

#[test]
fn a_summary_request_is_never_saved() {
    let (mut state, turn) = open_conversation("q");
    state.apply(turn, delta("answer"));
    state.apply(turn, done());
    cmds(&mut state);
    let (summary_turn, _) = state.begin_summary(&ready()).expect("a summary request");
    state.apply(summary_turn, delta("summary text"));
    state.apply(summary_turn, done());
    assert_eq!(cmds(&mut state), Vec::new());
    // Closing the pane while a summary streams saves nothing either.
    let (summary_turn, _) = state.begin_summary(&ready()).expect("a summary request");
    state.apply(summary_turn, delta("half"));
    state.close_stream();
    assert_eq!(cmds(&mut state), Vec::new());
}

// Messages sent before the conversation exists wait for its id.
#[test]
fn saves_made_before_created_wait_for_the_reply() {
    let mut state = chat();
    let turn = sent(&mut state, "q");
    let first = cmds(&mut state);
    assert!(matches!(&first[0], StoreCmd::Create { .. }));
    state.apply(turn, delta("A"));
    state.apply(turn, done());
    assert_eq!(cmds(&mut state), Vec::new(), "queued behind the create");
    let released = state.on_store_reply(pane(), &created(9));
    assert_eq!(released.len(), 1);
    assert!(matches!(
        &released[0],
        StoreCmd::Append {
            conversation: 9,
            ..
        }
    ));
}

// Spec (Delete): Deleted while open elsewhere.
#[test]
fn a_detached_conversation_saves_nothing_and_says_so_once() {
    let (mut state, turn) = open_conversation("q");
    state.detach_store_deleted();
    assert!(state.store_detached());
    let lines = state
        .entries()
        .iter()
        .filter(|e| matches!(e, Entry::System(l) if l == store::DELETED_LINE))
        .count();
    assert_eq!(lines, 1);
    state.apply(turn, delta("A"));
    state.apply(turn, done());
    sent(&mut state, "next");
    assert_eq!(cmds(&mut state), Vec::new());
    state.detach_store_deleted();
    let lines = state
        .entries()
        .iter()
        .filter(|e| matches!(e, Entry::System(l) if l == store::DELETED_LINE))
        .count();
    assert_eq!(lines, 1, "a second delete adds no second line");
}

// A pane closed while streaming keeps what was written so far.
#[test]
fn closing_a_streaming_pane_saves_the_partial_answer_cancelled() {
    let (mut state, turn) = open_conversation("q");
    state.apply(turn, delta("par"));
    state.close_stream();
    let out = cmds(&mut state);
    assert_eq!(out.len(), 1);
    assert_eq!(
        (appended(&out[0]).text.as_str(), appended(&out[0]).status),
        ("par", MsgStatus::Cancelled)
    );
}

#[test]
fn closing_an_idle_pane_saves_nothing() {
    let (mut state, _) = open_conversation("q");
    state.close_stream();
    assert_eq!(cmds(&mut state), Vec::new());
}

// Spec: Resume: appends to the same conversation.
#[test]
fn a_resumed_conversation_appends_to_the_same_id() {
    let mut state = chat();
    let stored = vec![
        Msg {
            role: Role::User,
            text: "old q".into(),
            status: MsgStatus::Complete,
        },
        Msg {
            role: Role::Assistant,
            text: "old a".into(),
            status: MsgStatus::Cancelled,
        },
    ];
    state.resume_into(42, Profile::Analyst, Some("proj".into()), stored);
    let turn = sent(&mut state, "new q");
    let out = cmds(&mut state);
    assert_eq!(out.len(), 1);
    assert!(matches!(
        &out[0],
        StoreCmd::Append {
            conversation: 42,
            ..
        }
    ));
    assert_eq!(appended(&out[0]).text, "new q");
    state.apply(turn, delta("new a"));
    state.apply(turn, done());
    let out = cmds(&mut state);
    assert!(matches!(
        &out[0],
        StoreCmd::Append {
            conversation: 42,
            ..
        }
    ));
}

// A late event of another turn is not a save.
#[test]
fn an_event_of_another_turn_saves_nothing() {
    let (mut state, turn) = open_conversation("q");
    state.apply(turn + 5, delta("stray"));
    state.apply(turn + 5, done());
    assert_eq!(cmds(&mut state), Vec::new());
}
