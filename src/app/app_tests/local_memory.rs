//! The local project memory in a chat view (chat-memory-local S1a): the load
//! at project entry, the config switch and one visible line per outcome.

use super::chat_notes::{CWD, ai, systems};
use super::*;
use crate::core::ai::{
    EngineState, Status,
    chat::prompts::Resolved,
    memory::local::{
        gitignore::GitignoreOutcome,
        port::{Failure, LocalCmd, LocalReply, Root},
    },
};

fn chat_id() -> PaneId {
    id2()
}

/// An app whose chat engine is ready, with a chat opened from `/srv/app`.
/// Engram stays off: the local memory does not depend on it.
fn opened(local: bool) -> (App, Vec<Effect>) {
    let mut config = ai(false);
    config.memory.local = local;
    let mut a = launched(80, 25).with_ai(config);
    a.update(cwd_event(CWD));
    a.update(AppEvent::EnginesDetected(Detection {
        engines: vec![EngineState {
            binary: None,
            id: "ollama",
            status: Status::Available,
            models: vec!["llama3:8b".to_owned()],
            version: None,
            model_labels: Vec::new(),
            diagnostics: Default::default(),
        }],
        ..Default::default()
    }));
    let effects = press(&mut a, "c");
    a.update(AppEvent::PromptReady(
        chat_id(),
        Resolved {
            text: "system".into(),
            notices: Vec::new(),
        },
    ));
    (a, effects)
}

fn local_cmds(effects: &[Effect]) -> Vec<LocalCmd> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::LocalMemory(cmd) => Some(cmd.clone()),
            _ => None,
        })
        .collect()
}

fn loaded(epoch: u32, root: bool, foreign: usize, unreadable: usize) -> AppEvent {
    AppEvent::LocalMemory(LocalReply::Loaded {
        pane: chat_id(),
        epoch,
        root: root.then(|| Root {
            store: "/srv/app".into(),
            area: ".".into(),
        }),
        notes: Vec::new(),
        foreign,
        unreadable,
        newer: 0,
        ledger: None,
    })
}

#[test]
fn opening_a_chat_loads_the_local_memory_without_engram() {
    let (_, effects) = opened(true);
    assert_eq!(
        local_cmds(&effects),
        [LocalCmd::Load {
            pane: chat_id(),
            epoch: 0,
            cwd: CWD.into()
        }]
    );
}

#[test]
fn with_the_local_switch_off_nothing_is_asked() {
    let (_, effects) = opened(false);
    assert_eq!(local_cmds(&effects), []);
}

#[test]
fn every_outcome_of_a_load_is_a_visible_system_line() {
    let (mut a, _) = opened(true);
    assert_eq!(a.update(loaded(0, true, 5, 2)), vec![]);
    assert_eq!(
        systems(&a),
        [
            "memory: 5 notes from others ignored (not created on this machine)",
            "memory: skipped 2 unreadable notes in .lzr/",
        ]
    );
    assert!(a.take_dirty());
}

#[test]
fn a_folder_without_git_says_so() {
    let (mut a, _) = opened(true);
    a.update(loaded(0, false, 0, 0));
    assert_eq!(
        systems(&a),
        ["memory: no git repository here; nothing is remembered for this folder"]
    );
}

#[test]
fn a_clean_load_adds_no_line() {
    let (mut a, _) = opened(true);
    a.update(loaded(0, true, 0, 0));
    assert_eq!(systems(&a), Vec::<String>::new());
}

#[test]
fn a_load_for_another_epoch_or_a_closed_pane_is_ignored() {
    let (mut a, _) = opened(true);
    a.update(loaded(9, true, 5, 0));
    assert_eq!(systems(&a), Vec::<String>::new());
    let gone = AppEvent::LocalMemory(LocalReply::Failed {
        pane: PaneId::FIRST,
        failure: Failure::Io("x".into()),
    });
    assert_eq!(a.update(gone), vec![]);
}

#[test]
fn a_save_outcome_shows_its_lines_and_never_reaches_the_model() {
    let (mut a, _) = opened(true);
    a.update(AppEvent::LocalMemory(LocalReply::Saved {
        pane: chat_id(),
        id: "n-1".into(),
        cut: false,
        gitignore: Some(GitignoreOutcome::Added),
    }));
    a.update(AppEvent::LocalMemory(LocalReply::Failed {
        pane: chat_id(),
        failure: Failure::ReadOnly("/srv/app".into()),
    }));
    assert_eq!(
        systems(&a),
        [
            "added .lzr/ to .gitignore (remove the entry to share memory through git)",
            "memory not saved: /srv/app is read-only",
        ]
    );
}

#[test]
fn a_chat_without_a_folder_says_there_is_no_git_project() {
    let mut a = launched(80, 25).with_ai(ai(false));
    a.open_chat(None);
    assert_eq!(a.begin_local_memory(chat_id()), vec![]);
    assert_eq!(
        systems(&a),
        ["memory: no git repository here; nothing is remembered for this folder"]
    );
}
