//! The delete offer of the conversation list (engram-memory M9): the prompt
//! variants, the notes delete after a confirmed `a`, and the lines that say
//! what happened to the notes.

use super::chat_history::{
    chat_id, chat_of, code, detection, listed, plain, prompt_for, row, stores, systems,
};
use super::chat_notes::ai;
use super::*;
use crate::core::ai::{
    chat::{
        list::DELETE_FAILED,
        store::{ConvRow, NoteRow, StoreCmd, StoreReply},
    },
    memory::port::{MemCmd, MemReply, Reason},
};

fn token() -> bool {
    true
}

fn no_token() -> bool {
    false
}

/// The rows of the list: conversation 30 (the one under the cursor) has
/// `notes` note links.
fn noted_rows(notes: u32) -> Vec<ConvRow> {
    let mut rows = vec![
        row(30, 3_000, Some("proj"), "programmer", "newest"),
        row(20, 2_000, Some("other"), "programmer", "elsewhere"),
        row(10, 1_000, Some("proj"), "analyst", "oldest"),
    ];
    rows[0].notes = notes;
    rows
}

/// A chat pane showing its list, with memory `memory` and the token as
/// `has_token` says.
fn listing(memory: bool, has_token: fn() -> bool, notes: u32) -> App {
    let mut a = launched(120, 30)
        .with_ai(ai(memory))
        .with_chat_history()
        .with_token_check(has_token);
    a.update(AppEvent::EnginesDetected(detection()));
    press(&mut a, "c");
    a.update(prompt_for(chat_id()));
    listed(&mut a, chat_id(), noted_rows(notes));
    a
}

fn memory_cmds(effects: &[Effect]) -> Vec<MemCmd> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Memory(cmd) => Some(cmd.clone()),
            _ => None,
        })
        .collect()
}

fn prompt(a: &App) -> Option<String> {
    chat_of(a).list().and_then(|list| list.prompt())
}

fn line(a: &App) -> Option<String> {
    chat_of(a)
        .list()
        .and_then(|list| list.line().map(str::to_owned))
}

fn note_rows(count: i64) -> Vec<NoteRow> {
    (0..count)
        .map(|n| NoteRow {
            project: "widget".into(),
            observation_id: 11 + n,
        })
        .collect()
}

fn deleted(conversation: i64, notes: Vec<NoteRow>) -> AppEvent {
    AppEvent::Store(StoreReply::Deleted {
        pane: chat_id(),
        conversation,
        notes,
    })
}

fn mem_deleted(ok: usize, failed: usize, refused: bool) -> AppEvent {
    AppEvent::Memory(MemReply::Deleted {
        pane: chat_id(),
        ok,
        failed,
        refused,
    })
}

// ---- the prompt -------------------------------------------------------------

// Spec (Delete): Three-way prompt with a token.
#[test]
fn the_prompt_offers_the_notes_with_memory_on_and_a_token() {
    let mut a = listing(true, token, 3);
    a.update(key('d'));
    assert_eq!(
        prompt(&a).as_deref(),
        Some("Delete conversation? y · n · a also delete its 3 notes")
    );
    let mut one = listing(true, token, 1);
    one.update(key('d'));
    assert_eq!(
        prompt(&one).as_deref(),
        Some("Delete conversation? y · n · a also delete its 1 note")
    );
}

// Spec (Delete): No token / Memory off / Conversation without notes.
#[test]
fn the_prompt_is_plain_without_notes_a_token_or_memory() {
    for (memory, has_token, notes) in [
        (true, no_token as fn() -> bool, 3),
        (false, token as fn() -> bool, 3),
        (true, token as fn() -> bool, 0),
    ] {
        let mut a = listing(memory, has_token, notes);
        a.update(key('d'));
        assert_eq!(
            prompt(&a).as_deref(),
            Some("Delete conversation? (y/n)"),
            "memory {memory}, {notes} notes"
        );
    }
}

static TOKEN_READS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn counted_token() -> bool {
    TOKEN_READS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    true
}

// The environment is looked at only when it matters: memory on and a
// conversation with notes under the cursor.
#[test]
fn the_token_is_looked_up_only_for_a_conversation_with_notes_and_memory_on() {
    let reads = || TOKEN_READS.load(std::sync::atomic::Ordering::SeqCst);
    let before = reads();
    let mut off = listing(false, counted_token, 3);
    off.update(key('d'));
    let mut none = listing(true, counted_token, 0);
    none.update(key('d'));
    assert_eq!(reads(), before, "memory off, and no notes: no lookup");
    let mut on = listing(true, counted_token, 2);
    on.update(key('d'));
    assert_eq!(reads(), before + 1);
}

// Spec (Delete): Other keys cancel the three-way prompt.
#[test]
fn every_other_key_cancels_the_three_way_prompt_with_no_request() {
    for ev in [
        key('n'),
        plain(KeyCode::Esc),
        key('x'),
        code(KeyCode::Char('Y'), KeyModifiers::SHIFT),
        code(KeyCode::Char('A'), KeyModifiers::SHIFT),
    ] {
        let mut a = listing(true, token, 3);
        a.update(key('d'));
        let effects = a.update(ev.clone());
        assert_eq!(stores(&effects), Vec::new(), "{ev:?}");
        assert_eq!(memory_cmds(&effects), Vec::new(), "{ev:?}");
        assert_eq!(prompt(&a), None, "{ev:?}");
        assert_eq!(chat_of(&a).list().unwrap().visible().len(), 2);
    }
}

// ---- y: the conversation only ----------------------------------------------

// Spec (Delete): Delete conversation only.
#[test]
fn y_deletes_the_conversation_and_asks_engram_nothing() {
    let mut a = listing(true, token, 3);
    a.update(key('d'));
    let effects = a.update(key('y'));
    assert_eq!(
        stores(&effects),
        vec![StoreCmd::Delete {
            pane: chat_id(),
            conversation: 30
        }]
    );
    let effects = a.update(deleted(30, note_rows(3)));
    assert_eq!(memory_cmds(&effects), Vec::new());
    assert_eq!(line(&a), None, "the user chose to keep them");
}

// ---- a: the conversation and its notes -------------------------------------

// Spec (Delete): Delete both.
#[test]
fn a_deletes_the_conversation_and_then_sends_the_notes_to_the_worker() {
    let mut a = listing(true, token, 3);
    a.update(key('d'));
    let effects = a.update(key('a'));
    assert_eq!(
        stores(&effects),
        vec![StoreCmd::Delete {
            pane: chat_id(),
            conversation: 30
        }]
    );
    assert_eq!(memory_cmds(&effects), Vec::new(), "the conversation first");
    let effects = a.update(deleted(30, note_rows(3)));
    assert_eq!(
        memory_cmds(&effects),
        vec![MemCmd::Delete {
            pane: chat_id(),
            notes: vec![
                ("widget".into(), 11),
                ("widget".into(), 12),
                ("widget".into(), 13)
            ],
        }]
    );
    assert_eq!(
        chat_of(&a).list().unwrap().visible().len(),
        1,
        "the conversation is gone already"
    );
    a.update(mem_deleted(3, 0, false));
    assert_eq!(line(&a).as_deref(), Some("deleted 3 notes from engram"));
}

// The rows the store returns are the ones deleted, not the count the list
// had: a note saved since the list was read goes too.
#[test]
fn the_notes_deleted_are_the_rows_of_the_delete_not_the_list_count() {
    let mut a = listing(true, token, 1);
    a.update(key('d'));
    a.update(key('a'));
    let effects = a.update(deleted(30, note_rows(2)));
    let cmds = memory_cmds(&effects);
    let [MemCmd::Delete { notes, .. }] = cmds.as_slice() else {
        panic!("expected one MemCmd::Delete, got {effects:?}");
    };
    assert_eq!(notes.len(), 2);
}

// Spec (Delete): Partial result.
#[test]
fn a_partial_result_says_how_many_went_and_why_the_rest_did_not() {
    let mut a = listing(true, token, 3);
    a.update(key('d'));
    a.update(key('a'));
    a.update(deleted(30, note_rows(3)));
    a.update(mem_deleted(2, 1, false));
    assert_eq!(
        line(&a).as_deref(),
        Some("deleted 2 of 3 notes from engram")
    );
    assert_eq!(
        chat_of(&a).list().unwrap().visible().len(),
        1,
        "the conversation stays deleted"
    );
}

#[test]
fn a_refusal_shows_the_refusal_text() {
    let mut a = listing(true, token, 3);
    a.update(key('d'));
    a.update(key('a'));
    a.update(deleted(30, note_rows(3)));
    a.update(mem_deleted(0, 3, true));
    assert_eq!(
        line(&a).as_deref(),
        Some("could not delete notes: engram refused the delete (check ENGRAM_HTTP_TOKEN)")
    );
}

// Spec (Delete): Engram unreachable.
#[test]
fn an_unreachable_engram_says_why_and_the_list_is_usable() {
    let mut a = listing(true, token, 3);
    a.update(key('d'));
    a.update(key('a'));
    a.update(deleted(30, note_rows(3)));
    a.update(AppEvent::Memory(MemReply::Unavailable {
        pane: Some(chat_id()),
        reason: Reason::NotRunning,
    }));
    assert_eq!(
        line(&a).as_deref(),
        Some("could not delete notes: engram is not running")
    );
    a.update(key('j'));
    assert_eq!(line(&a), None, "any key clears the line");
}

// A conversation with no note rows when the delete ran asks for nothing.
#[test]
fn a_delete_that_found_no_note_rows_sends_no_request_and_shows_no_line() {
    let mut a = listing(true, token, 2);
    a.update(key('d'));
    a.update(key('a'));
    let effects = a.update(deleted(30, Vec::new()));
    assert_eq!(memory_cmds(&effects), Vec::new());
    assert_eq!(line(&a), None);
}

// Spec (Delete): Delete failure.
#[test]
fn a_failed_delete_makes_no_note_request() {
    let mut a = listing(true, token, 3);
    a.update(key('d'));
    a.update(key('a'));
    let effects = a.update(AppEvent::Store(StoreReply::DeleteFailed {
        pane: chat_id(),
    }));
    assert_eq!(memory_cmds(&effects), Vec::new());
    assert_eq!(line(&a).as_deref(), Some(DELETE_FAILED));
    // The choice does not leak into a later delete of the same conversation.
    a.update(key('d'));
    a.update(key('y'));
    let effects = a.update(deleted(30, note_rows(3)));
    assert_eq!(memory_cmds(&effects), Vec::new());
}

// A pane that went away before the answer is not an error.
#[test]
fn a_notes_result_for_a_pane_that_is_gone_changes_nothing() {
    let mut a = listing(true, token, 3);
    let effects = a.update(AppEvent::Memory(MemReply::Deleted {
        pane: PaneId::for_test(9),
        ok: 3,
        failed: 0,
        refused: false,
    }));
    assert_eq!(effects, Vec::new());
    assert_eq!(line(&a), None);
}

// ---- notes that stay --------------------------------------------------------

// Spec (Delete): No token.
#[test]
fn without_a_token_the_notes_stay_and_the_line_says_how_to_delete_them() {
    let mut a = listing(true, no_token, 3);
    a.update(key('d'));
    let effects = a.update(key('y'));
    assert_eq!(memory_cmds(&effects), Vec::new());
    let effects = a.update(deleted(30, note_rows(3)));
    assert_eq!(memory_cmds(&effects), Vec::new());
    assert_eq!(
        line(&a).as_deref(),
        Some("its 3 notes stay in engram (set ENGRAM_HTTP_TOKEN to delete them)")
    );
}

// Spec (Delete): Memory off.
#[test]
fn with_memory_off_the_notes_stay_and_the_line_says_so() {
    let mut a = listing(false, token, 2);
    a.update(key('d'));
    a.update(key('y'));
    let effects = a.update(deleted(30, note_rows(2)));
    assert_eq!(memory_cmds(&effects), Vec::new());
    assert_eq!(
        line(&a).as_deref(),
        Some("its 2 notes stay in engram (memory is off)")
    );
}

// Spec (Delete): Conversation without notes.
#[test]
fn a_conversation_without_notes_gets_no_note_line() {
    let mut a = listing(true, token, 0);
    a.update(key('d'));
    a.update(key('y'));
    a.update(deleted(30, Vec::new()));
    assert_eq!(line(&a), None);
    assert_eq!(systems(chat_of(&a)), Vec::<String>::new());
}

// The list was read before a note was saved from another pane: the prompt
// was the plain one and the notes stay, which the line says.
#[test]
fn notes_saved_after_the_list_was_read_stay_and_say_so() {
    let mut a = listing(true, token, 0);
    a.update(key('d'));
    a.update(key('y'));
    let effects = a.update(deleted(30, note_rows(1)));
    assert_eq!(memory_cmds(&effects), Vec::new());
    assert_eq!(
        line(&a).as_deref(),
        Some("its 1 note stays in engram (it was saved after the list was read)")
    );
}

// ---- eviction ---------------------------------------------------------------

// Spec (chat-memory): Eviction never deletes.
#[test]
fn eviction_never_prompts_and_never_sends_a_delete() {
    let mut a = listing(true, token, 3);
    a.update(key('n'));
    let turn = {
        type_all(&mut a, "hi");
        let effects = a.update(plain(KeyCode::Enter));
        effects
            .iter()
            .find_map(|e| match e {
                Effect::ChatSend { turn, .. } => Some(*turn),
                _ => None,
            })
            .expect("a ChatSend")
    };
    let _ = turn;
    let effects = a.update(AppEvent::Store(StoreReply::Created {
        pane: chat_id(),
        conversation: 99,
        evicted: true,
        evicted_notes: 3,
    }));
    assert_eq!(memory_cmds(&effects), Vec::new());
    assert!(
        !systems(chat_of(&a)).is_empty(),
        "the eviction line is the only news: {:?}",
        systems(chat_of(&a))
    );
    assert_eq!(prompt(&a), None);
}

fn type_all(a: &mut App, text: &str) {
    for c in text.chars() {
        a.update(key(c));
    }
}
