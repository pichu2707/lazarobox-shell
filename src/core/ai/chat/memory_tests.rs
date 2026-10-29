use super::{
    engine::{Finish, StreamEvent},
    memory::*,
    note::NOTHING_TO_SAVE,
    state_tests::{chat, sent},
    *,
};
use crate::core::ai::memory::reply::ProjectError;

/// A chat whose last turn answered `body`.
fn answered(body: &str) -> ChatState {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "q");
    state.apply(turn, StreamEvent::Delta(body.into()));
    state.apply(turn, StreamEvent::Done(Finish::Stop));
    state
}

fn saved(state: &mut ChatState, topic_key: &str, id: i64, title: &str) {
    state.record_note("proj", topic_key, id, title);
}

// 9.3: the editor opens on the last answer ----------------------------------

#[test]
fn the_editor_opens_on_the_last_answer() {
    let mut state = answered("use Axum");
    assert_eq!(state.open_note_editor(), Ok(()));
    let editor = state.note_editor().expect("open");
    assert_eq!(editor.content(), "use Axum");
}

#[test]
fn without_an_answer_nothing_opens() {
    let mut state = chat();
    assert_eq!(state.open_note_editor(), Err(NOTHING_TO_SAVE.to_owned()));
    assert!(state.note_editor().is_none());
    // A question nobody answered is not an answer.
    sent(&mut state, "hi");
    assert_eq!(state.open_note_editor(), Err(NOTHING_TO_SAVE.to_owned()));
}

#[test]
fn reasoning_never_reaches_the_editor() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "q");
    state.apply(turn, StreamEvent::ThinkingDelta("secret plan".into()));
    state.apply(turn, StreamEvent::Delta("the answer".into()));
    state.apply(turn, StreamEvent::Done(Finish::Stop));
    state.open_note_editor().expect("open");
    let editor = state.note_editor().unwrap();
    assert_eq!(editor.content(), "the answer");
    assert!(!editor.content().contains("secret plan"));
}

#[test]
fn a_cancelled_answer_can_be_saved() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "q");
    state.apply(turn, StreamEvent::Delta("half an ans".into()));
    state.cancel();
    assert_eq!(state.open_note_editor(), Ok(()));
    assert_eq!(state.note_editor().unwrap().content(), "half an ans");
}

#[test]
fn the_answer_being_written_is_not_the_source() {
    let mut state = answered("first answer");
    let (turn, _) = sent(&mut state, "again");
    state.apply(turn, StreamEvent::Delta("second, still streaming".into()));
    state.open_note_editor().expect("open");
    assert_eq!(state.note_editor().unwrap().content(), "first answer");
}

#[test]
fn the_editor_keeps_the_transcript_and_the_composer_underneath() {
    let mut state = answered("a");
    state.composer_mut().paste("draft");
    let before = state.entries().to_vec();
    state.open_note_editor().expect("open");
    assert_eq!(state.composer().text(), "draft");
    assert_eq!(state.entries(), before);
    state.close_note_editor();
    assert!(state.note_editor().is_none());
    assert_eq!(state.composer().text(), "draft");
}

#[test]
fn opening_twice_keeps_the_open_editor() {
    let mut state = answered("a");
    state.open_note_editor().expect("open");
    state.note_editor_mut().unwrap().paste("typed");
    assert_eq!(state.open_note_editor(), Ok(()));
    assert!(state.note_editor().unwrap().title().contains("typed"));
}

// readiness of the project ----------------------------------------------------

#[test]
fn readiness_starts_unknown_and_follows_the_replies() {
    let mut state = chat();
    assert_eq!(state.readiness(), &Readiness::Unknown);
    state.start_resolving(true);
    assert_eq!(state.readiness(), &Readiness::Resolving { want_note: true });
    assert!(state.project_resolved(Ok("lazarobox-shell".into())));
    assert_eq!(
        state.readiness(),
        &Readiness::Ready("lazarobox-shell".into())
    );
    assert_eq!(state.project(), Some("lazarobox-shell"));
}

#[test]
fn a_project_error_is_unavailable_with_its_reason() {
    let mut state = chat();
    state.start_resolving(false);
    assert!(!state.project_resolved(Err(ProjectError::NotGit)));
    assert_eq!(
        state.readiness(),
        &Readiness::Unavailable("memory needs a git project".into())
    );
    assert_eq!(state.project(), None);
}

#[test]
fn unavailable_only_counts_while_resolving() {
    let mut state = chat();
    state.project_resolved(Ok("p".into()));
    assert!(!state.memory_unavailable("engram did not answer"));
    assert_eq!(state.readiness(), &Readiness::Ready("p".into()));
    state.start_resolving(true);
    assert!(state.memory_unavailable("engram did not answer"));
    assert_eq!(
        state.readiness(),
        &Readiness::Unavailable("engram did not answer".into())
    );
}

// the session and the notes of a conversation -----------------------------------

#[test]
fn a_conversation_that_never_wrote_has_no_session_and_no_end() {
    let mut state = answered("a");
    assert_eq!(state.session(), None);
    assert_eq!(state.take_end(), None);
}

#[test]
fn the_end_lists_the_titles_once_and_only_once() {
    let mut state = answered("a");
    state.adopt_session("lazarobox-0123456789abcdef0123456789abcdef", "proj");
    saved(&mut state, "chat/decision/a", 1, "A");
    saved(&mut state, "chat/bugfix/b", 2, "B");
    // The same note again is an upsert, not a second title.
    saved(&mut state, "chat/decision/a", 1, "A");
    let end = state.take_end().expect("a session was registered");
    assert_eq!(
        end,
        (
            "lazarobox-0123456789abcdef0123456789abcdef".to_owned(),
            "proj".to_owned(),
            "Notes: A; B".to_owned()
        )
    );
    assert_eq!(state.take_end(), None, "the end goes out once");
}

const STORED: &str = "lazarobox-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

// 9.6: a session taken from the store has nothing to end until this view
// writes to it.
#[test]
fn a_hydrated_session_that_was_not_written_to_has_no_end() {
    let mut state = answered("a");
    state.replace_session(STORED, "stored");
    assert_eq!(state.session().map(|s| s.id.as_str()), Some(STORED));
    assert_eq!(state.take_end(), None);
    // Still no end after the failed take: a later note is not lost.
    saved(&mut state, "chat/decision/a", 1, "A");
    assert_eq!(
        state.take_end(),
        Some((
            STORED.to_owned(),
            "stored".to_owned(),
            "Notes: A".to_owned()
        ))
    );
}

#[test]
fn a_hydrated_session_with_an_approved_summary_ends_with_it() {
    let mut state = answered("a");
    state.replace_session(STORED, "stored");
    state.approve_summary("We chose Axum.");
    assert_eq!(
        state.take_end(),
        Some((
            STORED.to_owned(),
            "stored".to_owned(),
            "We chose Axum.".to_owned()
        ))
    );
}

#[test]
fn the_end_summary_is_capped_at_1000_characters() {
    let mut state = answered("a");
    state.adopt_session("lazarobox-0123456789abcdef0123456789abcdef", "proj");
    for n in 0..30 {
        saved(
            &mut state,
            &format!("chat/decision/t{n}"),
            n,
            &"é".repeat(100),
        );
    }
    let (_, _, summary) = state.take_end().unwrap();
    assert!(summary.starts_with("Notes: "));
    assert_eq!(summary.chars().count(), 1000);
}

#[test]
fn adopting_a_session_sets_it_once_and_the_stored_pair_wins() {
    let mut state = answered("a");
    assert!(state.adopt_session("lazarobox-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "p1"));
    assert!(!state.adopt_session("lazarobox-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "p1"));
    // The store answers another pair for a resumed conversation.
    state.replace_session("lazarobox-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "p2");
    let session = state.session().unwrap();
    assert_eq!(session.id, "lazarobox-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
    assert_eq!(session.project, "p2");
}

#[test]
fn notes_wait_for_the_conversation_and_link_once() {
    let mut state = answered("a");
    saved(&mut state, "chat/decision/a", 1, "A");
    assert!(!state.unlinked_notes().is_empty());
    assert_eq!(state.conversation(), None);
    state.set_conversation(7);
    let pending = state.take_unlinked_notes();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].topic_key, "chat/decision/a");
    assert_eq!(pending[0].observation_id, 1);
    assert!(state.take_unlinked_notes().is_empty());
    // An upsert of the same note re-arms its link.
    saved(&mut state, "chat/decision/a", 1, "A2");
    assert_eq!(state.take_unlinked_notes().len(), 1);
}

#[test]
fn the_not_linked_line_shows_once() {
    let mut state = answered("a");
    assert!(state.warn_not_linked_once());
    assert!(!state.warn_not_linked_once());
}

#[test]
fn a_system_line_is_added_to_the_transcript() {
    let mut state = answered("a");
    let generation = state.generation();
    state.push_system("saved note decision: A");
    assert_eq!(
        state.entries().last(),
        Some(&Entry::System("saved note decision: A".into()))
    );
    assert_ne!(state.generation(), generation);
}

// a folder change resets the memory of the chat (chat-folder D6) -------------

fn known_project(state: &mut ChatState) {
    state.start_resolving(false);
    state.project_resolved(Ok("old".into()));
    state.start_context_at(Instant::now());
    let text = "<project-memory>- [decision] **A**</project-memory>".to_owned();
    state.inject_memory(crate::core::ai::memory::context::ContextBlock {
        bytes: text.len(),
        notes: 1,
        text,
    });
    state.finish_context();
    state.degrade_once("memory unavailable".into());
}

#[test]
fn a_folder_change_forgets_the_project_the_block_and_the_degraded_line() {
    let mut state = chat();
    known_project(&mut state);
    assert_eq!(state.project(), Some("old"));
    let epoch = state.memory_epoch();
    state.reset_memory_for_folder();
    assert_eq!(state.memory_epoch(), epoch + 1);
    assert_eq!(state.readiness(), &Readiness::Unknown);
    assert!(state.memory.injected().is_none());
    assert!(
        !state
            .entries()
            .iter()
            .any(|e| matches!(e, Entry::Memory(_)))
    );
    // The context may be asked again, and the degraded line shown again.
    assert!(state.start_context_at(Instant::now()));
    assert!(state.degrade_once("memory unavailable".into()));
}

#[test]
fn a_folder_change_leaves_the_session_to_be_replaced_by_the_next_one() {
    let mut state = chat();
    state.adopt_session("lazarobox-aa", "old");
    assert!(
        state
            .take_session_link()
            .is_some_and(|(_, replace)| !replace)
    );
    state.reset_memory_for_folder();
    assert!(state.session().is_none());
    assert!(state.adopt_session("lazarobox-bb", "new"));
    let (session, replace) = state.take_session_link().expect("to link");
    assert_eq!((session.project.as_str(), replace), ("new", true));
    assert!(state.take_session_link().is_none(), "once");
}

#[test]
fn set_folder_resets_the_memory_too() {
    let mut state = chat();
    known_project(&mut state);
    state.set_folder("/w/b".into());
    assert_eq!(state.readiness(), &Readiness::Unknown);
}
