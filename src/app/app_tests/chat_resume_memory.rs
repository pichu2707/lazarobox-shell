//! A resumed conversation and its engram session (engram-memory 9.6): the pair
//! the store kept is reused before the first write, a resume that writes
//! nothing ends nothing, and reasoning never reaches engram.

use super::chat_history::{chat_id, detection, listed, prompt_for, resumed, rows};
use super::chat_notes::{CWD, PROJECT, ai, ctrl, plain, type_text};
use super::*;
use crate::core::ai::{
    chat::{
        engine::{Finish, StreamEvent},
        store::{StoreCmd, StoreReply},
    },
    memory::port::{MemCmd, MemReply},
};

const STORED_SESSION: &str = "lazarobox-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const STORED_PROJECT: &str = "stored-project";

fn chat_of(a: &App) -> &ChatState {
    match a.pane_view(chat_id()) {
        Some(PaneView::Chat(chat)) => chat,
        _ => panic!("pane 2 is not a chat"),
    }
}

/// A resume of conversation 30 that carries `session`.
fn resumed_with(session: Option<(&str, &str)>) -> AppEvent {
    let AppEvent::Store(StoreReply::Resumed {
        pane,
        conversation,
        profile,
        tag,
        extras,
        messages,
        ..
    }) = resumed(chat_id(), 30, "programmer")
    else {
        unreachable!("resumed builds a Resumed reply");
    };
    AppEvent::Store(StoreReply::Resumed {
        pane,
        conversation,
        profile,
        tag,
        extras,
        messages,
        session: session.map(|(id, project)| (id.to_owned(), project.to_owned())),
    })
}

/// A chat with memory on, showing its list, before the resume.
fn listing() -> App {
    let mut a = launched(120, 30).with_ai(ai(true)).with_chat_history();
    a.update(cwd_event(CWD));
    a.update(AppEvent::EnginesDetected(detection()));
    press(&mut a, "c");
    a.update(prompt_for(chat_id()));
    listed(&mut a, chat_id(), rows());
    a
}

/// The same chat once the stored conversation is open and the project of its
/// directory is known.
fn resumed_chat(session: Option<(&str, &str)>) -> App {
    let mut a = listing();
    a.update(resumed_with(session));
    a.update(AppEvent::Memory(MemReply::Project {
        pane: chat_id(),
        result: Ok(PROJECT.into()),
    }));
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

/// Writes a note titled `title` in the open chat; returns the save effects.
fn save_note(a: &mut App, title: &str) -> Vec<Effect> {
    press(a, "n");
    type_text(a, title);
    a.update(ctrl('s'))
}

fn the_save(effects: &[Effect]) -> (Option<String>, String) {
    match memory_cmds(effects).as_slice() {
        [
            MemCmd::Save {
                session, project, ..
            },
        ] => (session.clone(), project.clone()),
        other => panic!("expected one Save, got {other:?}"),
    }
}

// 9.6: the first write reuses the stored pair, not a fresh one.
#[test]
fn the_first_note_after_a_resume_goes_into_the_stored_session_and_project() {
    let mut a = resumed_chat(Some((STORED_SESSION, STORED_PROJECT)));
    let effects = save_note(&mut a, "Again");
    assert_eq!(
        the_save(&effects),
        (Some(STORED_SESSION.to_owned()), STORED_PROJECT.to_owned()),
        "the stored pair wins over the project engram resolves now"
    );
}

// Triangulation: no stored pair, the old rule holds (a fresh session).
#[test]
fn a_resume_without_a_stored_session_saves_into_a_fresh_one() {
    let mut a = resumed_chat(None);
    let effects = save_note(&mut a, "First");
    assert_eq!(the_save(&effects), (None, PROJECT.to_owned()));
}

// The pair is already in chat.db: nothing links it again.
#[test]
fn a_hydrated_session_is_not_linked_again() {
    let mut a = resumed_chat(Some((STORED_SESSION, STORED_PROJECT)));
    save_note(&mut a, "Again");
    let (token, project) = super::chat_notes::oldest_save(&a);
    let effects = a.update(AppEvent::Memory(MemReply::Saved {
        pane: chat_id(),
        id: token,
        project,
        session: STORED_SESSION.into(),
        observation_id: 9,
        topic_key: "chat/decision/again".into(),
        title: "Again".into(),
    }));
    let links: Vec<_> = effects
        .iter()
        .filter_map(|e| match e {
            Effect::Store(cmd) => Some(cmd),
            _ => None,
        })
        .collect();
    assert!(
        links
            .iter()
            .all(|cmd| !matches!(cmd, StoreCmd::LinkSession { .. })),
        "{links:?}"
    );
    assert!(
        links.iter().any(|cmd| matches!(
            cmd,
            StoreCmd::LinkNote {
                conversation: 30,
                ..
            }
        )),
        "the note itself is linked to the conversation: {links:?}"
    );
}

fn end_of(effects: &[Effect]) -> Vec<MemCmd> {
    memory_cmds(effects)
        .into_iter()
        .filter(|cmd| matches!(cmd, MemCmd::End { .. }))
        .collect()
}

// A conversation that never wrote sends no End, resumed or not.
#[test]
fn a_resume_that_writes_nothing_ends_nothing() {
    let mut a = resumed_chat(Some((STORED_SESSION, STORED_PROJECT)));
    press(&mut a, "wq");
    let effects = a.update(key('y'));
    assert_eq!(end_of(&effects), Vec::new(), "{effects:?}");
    let mut b = resumed_chat(Some((STORED_SESSION, STORED_PROJECT)));
    press(&mut b, "q");
    assert_eq!(b.update(key('y')), vec![Effect::Quit]);
}

// Pane close, tab close and quit end the stored session once, with the titles
// of the notes written after the resume.
#[test]
fn a_resume_that_wrote_a_note_ends_the_stored_session_once() {
    let mut a = resumed_chat(Some((STORED_SESSION, STORED_PROJECT)));
    save_note(&mut a, "Again");
    let (token, project) = super::chat_notes::oldest_save(&a);
    a.update(AppEvent::Memory(MemReply::Saved {
        pane: chat_id(),
        id: token,
        project,
        session: STORED_SESSION.into(),
        observation_id: 9,
        topic_key: "chat/decision/again".into(),
        title: "Again".into(),
    }));
    press(&mut a, "q");
    let effects = a.update(key('y'));
    assert_eq!(
        end_of(&effects),
        vec![MemCmd::End {
            session: STORED_SESSION.into(),
            project: STORED_PROJECT.into(),
            summary: "Notes: Again".into(),
            cwd: None,
        }]
    );
    assert_eq!(effects.last(), Some(&Effect::Quit));
}

// Reasoning never reaches engram, in a resumed conversation either.
#[test]
fn reasoning_after_a_resume_never_reaches_engram() {
    let mut a = resumed_chat(Some((STORED_SESSION, STORED_PROJECT)));
    type_text(&mut a, "more");
    let sent = a.update(plain(KeyCode::Enter));
    let turn = sent
        .iter()
        .find_map(|e| match e {
            Effect::ChatSend { turn, .. } => Some(*turn),
            _ => None,
        })
        .expect("a ChatSend");
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::ThinkingDelta("secret plan".into()),
    ));
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Delta("the answer".into()),
    ));
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Done(Finish::Stop),
    ));
    let mut traffic = save_note(&mut a, "T");
    let (token, project) = super::chat_notes::oldest_save(&a);
    traffic.extend(a.update(AppEvent::Memory(MemReply::Saved {
        pane: chat_id(),
        id: token,
        project,
        session: STORED_SESSION.into(),
        observation_id: 9,
        topic_key: "chat/decision/t".into(),
        title: "T".into(),
    })));
    press(&mut a, "q");
    traffic.extend(a.update(key('y')));
    assert!(
        !format!("{traffic:?}").contains("secret plan"),
        "{traffic:?}"
    );
}

// A resume answered for a pane that no longer shows the list changes nothing,
// the session included.
#[test]
fn a_late_resume_does_not_hydrate_a_pane_that_left_the_list() {
    let mut a = listing();
    a.update(key('n'));
    a.update(resumed_with(Some((STORED_SESSION, STORED_PROJECT))));
    assert!(chat_of(&a).session().is_none());
}
