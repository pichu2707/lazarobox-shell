//! The chat folder (chat-folder): when it may change, and what a change does.

use super::run_pane::one_shell_and_a_chat;
use super::tool_approval::{announce, run};
use super::tool_run::run_live;
use super::*;

#[test]
fn an_idle_chat_may_change_folder() {
    let a = one_shell_and_a_chat("hello");
    assert_eq!(a.folder_busy(id2()), None);
}

#[test]
fn a_command_running_for_the_chat_blocks_the_change() {
    let (mut a, turn) = run_live();
    announce(&mut a, turn, 1, run("cd ~/dev"));
    assert_eq!(a.folder_busy(id2()), Some("a command is running"));
}

fn folder_of(a: &App) -> Option<PathBuf> {
    super::run_pane::chat(a, id2())
        .origin_cwd()
        .map(Path::to_path_buf)
}

#[test]
fn the_change_moves_the_chat_and_prepares_its_prompt_again() {
    let mut a = one_shell_and_a_chat("hello");
    let effects = a.apply_folder(id2(), "/w/b".into());
    assert_eq!(folder_of(&a), Some(PathBuf::from("/w/b")));
    let prepared: Vec<_> = effects
        .iter()
        .filter_map(|e| match e {
            Effect::PreparePrompt { pane, origin, .. } => Some((*pane, origin.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(prepared, vec![(id2(), Some(PathBuf::from("/w/b")))]);
    assert!(a.take_dirty());
}

#[test]
fn the_change_ends_the_tool_session_of_the_old_folder_once() {
    let mut a = one_shell_and_a_chat("hello");
    a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap()
        .set_tool_session(true);
    let first = a.apply_folder(id2(), "/w/b".into());
    assert!(first.contains(&Effect::EndToolSession(id2())));
    let second = a.apply_folder(id2(), "/w/c".into());
    assert!(!second.contains(&Effect::EndToolSession(id2())));
}

#[test]
fn a_parked_send_blocks_the_change() {
    let mut a = one_shell_and_a_chat("hello");
    a.waiting_sends.push((id2(), "hi".into()));
    assert_eq!(
        a.folder_busy(id2()),
        Some("a message is waiting to be sent")
    );
}

#[test]
fn an_open_note_editor_blocks_the_change() {
    let a = super::chat_notes::ready("body");
    assert_eq!(a.folder_busy(id2()), Some("a note or summary is open"));
}

#[test]
fn a_chat_that_is_gone_is_not_busy() {
    let a = one_shell_and_a_chat("hello");
    assert_eq!(a.folder_busy(PaneId::for_test(99)), None);
}
