//! What a chat pane saves, and when (chat-history: Saving conversations,
//! Reasoning is not stored).

use super::{
    engine::{ChatError, Finish, Role},
    state_tests::{chat, ready},
    store::{StoreCmd, StoreReply, StoredMessage},
    tag_editor::TagTarget,
    tags::Tags,
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
        created_at: 1_000,
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
            extras: vec![],
            first: StoredMessage {
                role: Role::User,
                text: "hello".into(),
                status: MsgStatus::Complete,
                engine: "ollama".into(),
                model: Some("llama3:8b".into()),
            },
            origin: None,
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
            conversation: Some(7),
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

// Spec: an answer that fails after partial text is saved with status error;
// the error text (a system line) is not.
#[test]
fn an_error_after_partial_text_is_saved_as_an_error_answer() {
    let (mut state, turn) = open_conversation("q");
    state.apply(turn, delta("par"));
    state.apply(turn, StreamEvent::Error(ChatError::Failed("boom".into())));
    let out = cmds(&mut state);
    assert_eq!(out.len(), 1);
    assert_eq!(
        (appended(&out[0]).text.as_str(), appended(&out[0]).status),
        ("par", MsgStatus::Error)
    );
    assert!(!format!("{out:?}").contains("boom"));
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
    // The answer follows the create at once, by pane.
    let out = cmds(&mut state);
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

// Messages sent before the conversation exists name the pane: the store
// thread resolves the id it made.
#[test]
fn saves_made_before_created_go_out_at_once_by_pane() {
    let mut state = chat();
    let turn = sent(&mut state, "q");
    let first = cmds(&mut state);
    assert!(matches!(&first[0], StoreCmd::Create { .. }));
    state.apply(turn, delta("A"));
    state.apply(turn, done());
    let out = cmds(&mut state);
    assert!(matches!(
        &out[..],
        [StoreCmd::Append {
            conversation: None,
            ..
        }]
    ));
    state.on_store_reply(pane(), &created(9));
    sent(&mut state, "next");
    assert!(matches!(
        &cmds(&mut state)[..],
        [StoreCmd::Append {
            conversation: Some(9),
            ..
        }]
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
    state.resume_into(
        42,
        Profile::Analyst,
        Tags::new(Some("proj".into()), vec![]),
        stored,
    );
    let turn = sent(&mut state, "new q");
    let out = cmds(&mut state);
    assert_eq!(out.len(), 1);
    assert!(matches!(
        &out[0],
        StoreCmd::Append {
            conversation: Some(42),
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
            conversation: Some(42),
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

// ---- tags (10.5) -----------------------------------------------------------

// Spec (Edit tags): a conversation not yet created keeps the edit for its
// Create; nothing is stored before the first message.
#[test]
fn tags_edited_before_the_first_message_go_with_the_create() {
    let mut state = chat();
    state.set_tag(Some("proj".into()));
    assert_eq!(
        state.edit_tags(pane(), Tags::parse("client, sql, etl")),
        None
    );
    assert_eq!(state.tag(), Some("client"));
    assert_eq!(state.extras(), ["sql", "etl"]);
    sent(&mut state, "hello");
    let out = cmds(&mut state);
    let [StoreCmd::Create { tag, extras, .. }] = out.as_slice() else {
        panic!("expected one Create, got {out:?}");
    };
    assert_eq!(tag.as_deref(), Some("client"));
    assert_eq!(extras, &["sql", "etl"]);
}

#[test]
fn a_project_tag_removed_before_the_first_message_is_not_proposed_again() {
    let mut state = ChatState::new(Some("/work/proj".into()));
    state.set_tag(Some("proj".into()));
    state.edit_tags(pane(), Tags::default());
    sent(&mut state, "hello");
    let out = cmds(&mut state);
    let [StoreCmd::Create { tag, origin, .. }] = out.as_slice() else {
        panic!("expected one Create, got {out:?}");
    };
    assert_eq!((tag, origin), (&None, &None));
}

#[test]
fn an_untouched_tag_keeps_the_origin_for_the_store_to_resolve() {
    let mut state = ChatState::new(Some("/work/proj".into()));
    sent(&mut state, "hello");
    let out = cmds(&mut state);
    let [StoreCmd::Create { origin, .. }] = out.as_slice() else {
        panic!("expected one Create");
    };
    assert_eq!(origin.as_deref(), Some(std::path::Path::new("/work/proj")));
}

#[test]
fn tags_of_an_open_conversation_are_stored_at_once() {
    let (mut state, _) = open_conversation("hi");
    let cmd = state.edit_tags(pane(), Tags::parse("client-a, sql"));
    assert_eq!(
        cmd,
        Some(StoreCmd::SetTags {
            pane: pane(),
            conversation: Some(7),
            tags: Tags::parse("client-a, sql"),
        })
    );
    assert_eq!(state.tags(), Tags::parse("client-a, sql"));
}

#[test]
fn tags_edited_while_the_create_is_in_flight_name_the_pane() {
    let mut state = chat();
    sent(&mut state, "hi");
    cmds(&mut state);
    let cmd = state.edit_tags(pane(), Tags::parse("x"));
    assert!(matches!(
        cmd,
        Some(StoreCmd::SetTags {
            conversation: None,
            ..
        })
    ));
}

#[test]
fn a_detached_conversation_keeps_the_tags_in_the_pane_only() {
    let (mut state, _) = open_conversation("hi");
    state.detach_store();
    assert_eq!(state.edit_tags(pane(), Tags::parse("x")), None);
    assert_eq!(state.tag(), Some("x"));
}

#[test]
fn a_resumed_conversation_brings_its_tags() {
    let mut state = chat();
    state.resume_into(42, Profile::Analyst, Tags::parse("proj, a, b"), Vec::new());
    assert_eq!(state.tags(), Tags::parse("proj, a, b"));
}

#[test]
fn saved_tags_replace_the_pane_tags() {
    let mut state = chat();
    state.set_tags(&Tags::parse("p, q"));
    assert_eq!(
        (state.tag(), state.extras()),
        (Some("p"), ["q".to_owned()].as_slice())
    );
}

// ---- the tag editor view (10.5) --------------------------------------------

#[test]
fn the_pane_editor_opens_on_the_tags_of_the_conversation() {
    let mut state = chat();
    state.set_tags(&Tags::parse("proj, a"));
    assert!(state.open_tag_editor(TagTarget::Pane));
    let editor = state.tag_editor().expect("open");
    assert_eq!(editor.text(), "proj, a");
    assert_eq!(editor.target(), TagTarget::Pane);
}

#[test]
fn a_row_editor_opens_on_the_tags_it_is_given() {
    let mut state = chat();
    state.open_list();
    assert!(state.open_tag_editor_for(TagTarget::Row(4), &Tags::parse("x, y")));
    assert_eq!(state.tag_editor().unwrap().text(), "x, y");
}

#[test]
fn the_pane_editor_does_not_open_over_the_list() {
    let mut state = chat();
    state.open_list();
    assert!(!state.open_tag_editor(TagTarget::Pane));
    assert!(state.tag_editor().is_none());
}

#[test]
fn the_editor_does_not_open_over_the_note_editor_or_the_summary_draft() {
    let (mut state, turn) = open_conversation("hi");
    state.apply(turn, delta("an answer"));
    state.apply(turn, done());
    state.open_note_editor().unwrap();
    assert!(!state.open_tag_editor(TagTarget::Pane));
    state.close_note_editor();
    assert!(state.open_tag_editor(TagTarget::Pane));
}

#[test]
fn an_open_editor_keeps_its_text_when_asked_to_open_again() {
    let mut state = chat();
    state.set_tags(&Tags::parse("proj"));
    state.open_tag_editor(TagTarget::Pane);
    state.tag_editor_mut().unwrap().paste(",typed");
    assert!(state.open_tag_editor(TagTarget::Pane));
    assert_eq!(state.tag_editor().unwrap().text(), "proj,typed");
}

#[test]
fn closing_drops_the_editor_and_changes_no_tag() {
    let mut state = chat();
    state.set_tags(&Tags::parse("proj"));
    state.open_tag_editor(TagTarget::Pane);
    state.tag_editor_mut().unwrap().paste(",typed");
    state.close_tag_editor();
    assert!(state.tag_editor().is_none());
    assert_eq!(state.tags(), Tags::parse("proj"));
    assert!(!state.tags_edited());
}

// The origin's project tag is only given up when the user changed the project
// entry; extras alone leave it for the store to resolve.
#[test]
fn extras_alone_leave_the_project_tag_to_the_origin() {
    let mut state = ChatState::new(Some("/work/proj".into()));
    state.edit_tags(pane(), Tags::parse(", sql"));
    assert!(!state.tags_edited());
    sent(&mut state, "hello");
    let out = cmds(&mut state);
    let [
        StoreCmd::Create {
            tag,
            extras,
            origin,
            ..
        },
    ] = out.as_slice()
    else {
        panic!("expected one Create, got {out:?}");
    };
    assert_eq!(tag, &None);
    assert_eq!(extras, &["sql"]);
    assert_eq!(origin.as_deref(), Some(std::path::Path::new("/work/proj")));
}

#[test]
fn changing_the_project_entry_gives_up_the_origin() {
    let mut state = ChatState::new(Some("/work/proj".into()));
    state.edit_tags(pane(), Tags::parse("mine"));
    assert!(state.tags_edited());
}

#[test]
fn keeping_the_known_project_tag_and_adding_extras_does_not_count_as_a_change() {
    let mut state = ChatState::new(Some("/work/proj".into()));
    state.set_tag(Some("proj".into()));
    state.edit_tags(pane(), Tags::parse("proj, sql"));
    assert!(!state.tags_edited());
}
