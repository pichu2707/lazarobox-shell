//! The auto-run block after a GitHub read (pane-execution, Auto-run block):
//! the flag of the conversation, its system line and the marker it saves.

use super::{
    approval::{GITHUB_BLOCK_LINE, NEWER_BLOCK_LINE},
    engine::{CallId, Finish, ToolStatus},
    state_tests::{chat, systems},
    store::{StoreCmd, StoreReply, StoredMessage},
    *,
};
use crate::core::layout::PaneId;

fn pane() -> PaneId {
    PaneId::FIRST
}

fn sent(state: &mut ChatState, text: &str) -> u64 {
    match state.send(text, &super::state_tests::ready(), "sys") {
        SendOutcome::Sent { turn, .. } => turn,
        SendOutcome::Refused => panic!("send refused"),
    }
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

/// A chat whose conversation exists as `7`.
fn open_conversation() -> (ChatState, u64) {
    let mut state = chat();
    let turn = sent(&mut state, "look at #12");
    state.store_cmds(pane(), &[]);
    state.on_store_reply(pane(), &created(7));
    (state, turn)
}

fn marks(cmds: &[StoreCmd]) -> usize {
    cmds.iter()
        .filter(|cmd| matches!(cmd, StoreCmd::MarkGithubRead { .. }))
        .count()
}

// Spec: Block starts at the first read.
#[test]
fn the_first_read_blocks_and_says_so_once() {
    let (mut state, _) = open_conversation();
    assert!(!state.github_read());
    assert!(state.note_github_read());
    assert!(!state.note_github_read(), "only the first one starts it");
    assert!(state.github_read());
    let lines = systems(&state);
    assert_eq!(
        lines.iter().filter(|l| **l == GITHUB_BLOCK_LINE).count(),
        1,
        "{lines:?}"
    );
    assert!(GITHUB_BLOCK_LINE.is_ascii());
}

// Spec: Started calls that fail still block; Later turns stay blocked.
#[test]
fn the_block_survives_an_error_result_and_the_next_turn() {
    let (mut state, turn) = open_conversation();
    state.apply(
        turn,
        StreamEvent::ToolCall {
            id: CallId(1),
            name: "gh_issue_view".into(),
            summary: "#9999".into(),
            approval: None,
        },
    );
    state.note_github_read();
    state.apply(
        turn,
        StreamEvent::ToolResult {
            id: CallId(1),
            status: ToolStatus::Error,
            summary: "not found".into(),
        },
    );
    state.apply(turn, StreamEvent::Done(Finish::Stop));
    assert!(state.github_read());
    sent(&mut state, "and now?");
    assert!(state.github_read());
}

// Spec: a call that is not a GitHub read does not block.
#[test]
fn other_tool_events_do_not_block() {
    let (mut state, turn) = open_conversation();
    state.apply(
        turn,
        StreamEvent::ToolCall {
            id: CallId(1),
            name: "read_file".into(),
            summary: "a.rs".into(),
            approval: None,
        },
    );
    state.apply(
        turn,
        StreamEvent::ToolResult {
            id: CallId(1),
            status: ToolStatus::Ok,
            summary: "done".into(),
        },
    );
    assert!(!state.github_read());
    assert!(!systems(&state).contains(&GITHUB_BLOCK_LINE));
}

// The line is shown, never stored and never sent.
#[test]
fn the_block_line_is_not_stored_and_not_sent() {
    let (mut state, turn) = open_conversation();
    state.note_github_read();
    state.apply(turn, StreamEvent::Delta("an answer".into()));
    state.apply(turn, StreamEvent::Done(Finish::Stop));
    let cmds = state.store_cmds(pane(), &[]);
    assert!(
        cmds.iter().all(|cmd| match cmd {
            StoreCmd::Append { message, .. } => !message.text.contains("auto-run"),
            _ => true,
        }),
        "{cmds:?}"
    );
    let SendOutcome::Sent { request, .. } = state.send("next", &super::state_tests::ready(), "s")
    else {
        panic!("send refused");
    };
    assert!(
        request
            .messages
            .iter()
            .all(|m| !m.content.contains("auto-run is off")),
        "{:?}",
        request.messages
    );
}

// Spec: Resume stays blocked (the marker row is written once).
#[test]
fn the_marker_is_sent_once_when_the_conversation_exists() {
    let (mut state, _) = open_conversation();
    state.note_github_read();
    let first = state.store_cmds(pane(), &[]);
    assert_eq!(
        first,
        vec![StoreCmd::MarkGithubRead {
            pane: pane(),
            conversation: Some(7)
        }]
    );
    assert_eq!(marks(&state.store_cmds(pane(), &[])), 0);
}

// A conversation still being created is named by its pane.
#[test]
fn the_marker_waits_for_the_pane_name_while_the_conversation_is_created() {
    let mut state = chat();
    sent(&mut state, "hi");
    state.store_cmds(pane(), &[]);
    state.note_github_read();
    assert_eq!(
        state.store_cmds(pane(), &[]),
        vec![StoreCmd::MarkGithubRead {
            pane: pane(),
            conversation: None
        }]
    );
}

// Nothing stored yet: the first message carries the marker with it.
#[test]
fn a_fresh_conversation_marks_through_its_first_message() {
    let mut state = chat();
    state.note_github_read();
    assert_eq!(state.store_cmds(pane(), &[]), vec![]);
    sent(&mut state, "hi");
    let cmds = state.store_cmds(pane(), &[]);
    assert_eq!(marks(&cmds), 0, "{cmds:?}");
    assert!(matches!(
        &cmds[..],
        [StoreCmd::Create {
            first: StoredMessage {
                github_read: true,
                ..
            },
            ..
        }]
    ));
}

// Every later row carries the marker in its own transaction.
#[test]
fn later_messages_carry_the_marker() {
    let (mut state, turn) = open_conversation();
    state.note_github_read();
    state.store_cmds(pane(), &[]);
    state.apply(turn, StreamEvent::Delta("an answer".into()));
    state.apply(turn, StreamEvent::Done(Finish::Stop));
    let cmds = state.store_cmds(pane(), &[]);
    assert!(
        matches!(&cmds[..], [StoreCmd::Append { message, .. }] if message.github_read),
        "{cmds:?}"
    );
}

#[test]
fn an_unblocked_conversation_marks_nothing() {
    let (mut state, turn) = open_conversation();
    state.apply(turn, StreamEvent::Delta("an answer".into()));
    state.apply(turn, StreamEvent::Done(Finish::Stop));
    let cmds = state.store_cmds(pane(), &[]);
    assert!(
        matches!(&cmds[..], [StoreCmd::Append { message, .. }] if !message.github_read),
        "{cmds:?}"
    );
}

// Spec: History off or store detached: the live flag still blocks.
#[test]
fn a_detached_store_writes_nothing_but_the_flag_holds() {
    let (mut state, _) = open_conversation();
    state.detach_store();
    state.note_github_read();
    assert_eq!(state.store_cmds(pane(), &[]), vec![]);
    assert!(state.github_read());
}

// Spec: Resume keeps it: the stored marker raises the flag again, with the
// line, and nothing is written.
#[test]
fn a_resumed_blocked_conversation_is_blocked_and_says_so() {
    let mut state = chat();
    state.adopt_github_read(BlockWhy::Marker);
    assert!(state.github_read());
    assert_eq!(
        systems(&state)
            .iter()
            .filter(|l| **l == GITHUB_BLOCK_LINE)
            .count(),
        1
    );
    assert_eq!(state.store_cmds(pane(), &[]), vec![]);
}

// Resume with rows of a role this build does not know: blocked, with its own
// truthful line, never the GitHub one.
#[test]
fn a_resume_for_newer_rows_blocks_with_its_own_line() {
    let mut state = chat();
    state.adopt_github_read(BlockWhy::NewerRows);
    assert!(state.github_read());
    let lines = systems(&state);
    assert_eq!(lines.iter().filter(|l| **l == NEWER_BLOCK_LINE).count(), 1);
    assert!(!lines.contains(&GITHUB_BLOCK_LINE), "{lines:?}");
    assert!(!NEWER_BLOCK_LINE.contains("GitHub"));
    assert!(NEWER_BLOCK_LINE.is_ascii() && NEWER_BLOCK_LINE.len() <= 80);
}

// Never show an untrue notice: the block of rows from a newer version says so
// in every line it owns, and the GitHub block keeps its wording.
#[test]
fn every_text_of_the_newer_rows_block_is_true() {
    let newer = BlockWhy::NewerRows;
    for text in [newer.reason(), newer.read_ask(), newer.run_line()] {
        assert!(!text.contains("GitHub"), "{text}");
        assert!(text.contains("newer version"), "{text}");
        assert!(text.is_ascii() && text.len() <= 80, "{text}");
    }
    let github = BlockWhy::Marker;
    assert_eq!(github.reason(), approval::GITHUB_READ);
    assert_eq!(github.read_ask(), approval::GITHUB_READ_ASK);
    assert_eq!(github.run_line(), approval::RUN_BLOCKED_LINE);
    assert_ne!(newer.reason(), github.reason());
    assert_ne!(newer.read_ask(), github.read_ask());
    assert_ne!(newer.run_line(), github.run_line());
}

// The reason follows the facts: a conversation blocked by newer rows that then
// reads GitHub text is blocked for GitHub too, and says so once.
#[test]
fn the_block_reason_follows_what_happened() {
    let mut state = chat();
    assert_eq!(state.block_why(), None);
    state.adopt_github_read(BlockWhy::NewerRows);
    assert_eq!(state.block_why(), Some(BlockWhy::NewerRows));
    assert!(state.note_github_read(), "the first real read is news");
    assert_eq!(state.block_why(), Some(BlockWhy::Marker));
    assert_eq!(
        systems(&state)
            .iter()
            .filter(|l| **l == GITHUB_BLOCK_LINE)
            .count(),
        1
    );
    assert!(!state.note_github_read(), "the second is not");
    let mut fresh = chat();
    fresh.note_github_read();
    assert_eq!(fresh.block_why(), Some(BlockWhy::Marker));
}
