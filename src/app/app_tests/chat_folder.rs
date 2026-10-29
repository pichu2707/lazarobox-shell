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
fn a_chat_that_is_gone_is_not_busy() {
    let a = one_shell_and_a_chat("hello");
    assert_eq!(a.folder_busy(PaneId::for_test(99)), None);
}

// ---- the /cd command (chat-folder A2) --------------------------------------

use super::chat_history::{ask, plain, systems, type_text};
use super::chat_live::ollama;
use crate::core::ai::chat::folder::{Probed, Purpose, Refusal, Verdict};

fn composer(a: &App) -> String {
    super::run_pane::chat(a, id2()).composer().text()
}

fn messages(a: &App) -> usize {
    super::run_pane::chat(a, id2()).messages().count()
}

fn lines(a: &App) -> Vec<String> {
    systems(super::run_pane::chat(a, id2()))
}

fn probes(effects: &[Effect]) -> Vec<(PathBuf, Purpose)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::ProbeFolder { path, purpose, .. } => Some((path.clone(), *purpose)),
            _ => None,
        })
        .collect()
}

fn has_send(effects: &[Effect]) -> bool {
    effects.iter().any(|e| matches!(e, Effect::ChatSend { .. }))
}

/// A chat whose folder is `/w/a`, idle.
fn in_folder() -> App {
    let mut a = one_shell_and_a_chat("hello");
    a.apply_folder(id2(), "/w/a".into());
    // The prompt is prepared again for the new folder.
    a.update(AppEvent::PromptReady(
        id2(),
        crate::core::ai::chat::prompts::Resolved {
            text: "system".to_owned(),
            notices: Vec::new(),
        },
    ));
    a
}

fn probed(a: &mut App, result: Result<Probed, Refusal>) -> Vec<Effect> {
    a.update(AppEvent::FolderProbed {
        pane: id2(),
        purpose: Purpose::Cd,
        result,
    })
}

fn concrete(path: &str) -> Result<Probed, Refusal> {
    Ok(Probed {
        path: path.into(),
        verdict: Verdict::Concrete,
    })
}

#[test]
fn cd_is_not_a_message_and_asks_for_a_probe() {
    let mut a = in_folder();
    let before = messages(&a);
    let effects = ask(&mut a, "/cd /w/b");
    assert!(!has_send(&effects), "{effects:?}");
    assert_eq!(probes(&effects), vec![(PathBuf::from("/w/b"), Purpose::Cd)]);
    assert_eq!(messages(&a), before);
    assert_eq!(composer(&a), "/cd /w/b");
}

#[test]
fn bare_cd_shows_the_usage_and_keeps_the_composer() {
    let mut a = in_folder();
    let effects = ask(&mut a, "/cd");
    assert!(probes(&effects).is_empty() && !has_send(&effects));
    assert!(lines(&a).contains(&"usage: /cd <folder>".to_owned()));
    assert_eq!(composer(&a), "/cd");
}

#[test]
fn a_relative_path_is_read_against_the_chat_folder() {
    let mut a = in_folder();
    let effects = ask(&mut a, "/cd src");
    assert_eq!(probes(&effects)[0].0, PathBuf::from("/w/a/src"));
    let mut a = in_folder();
    let effects = ask(&mut a, "/cd ..");
    assert_eq!(probes(&effects)[0].0, PathBuf::from("/w"));
}

#[test]
fn other_slash_text_is_a_normal_message() {
    for text in ["/cdx foo", "/etc/passwd explain this"] {
        let mut a = in_folder();
        let effects = ask(&mut a, text);
        assert!(has_send(&effects), "{text}: {effects:?}");
        assert!(probes(&effects).is_empty());
    }
}

#[test]
fn a_double_slash_cd_is_sent_as_cd() {
    let mut a = in_folder();
    let effects = ask(&mut a, "//cd /w/b");
    assert!(has_send(&effects));
    assert!(probes(&effects).is_empty());
    let last = super::run_pane::chat(&a, id2()).messages().last().cloned();
    assert_eq!(last.map(|m| m.text), Some("/cd /w/b".to_owned()));
}

#[test]
fn cd_works_while_the_engine_state_is_unknown() {
    let mut a = launched(80, 25).with_ai(ollama());
    press(&mut a, "c");
    let effects = ask(&mut a, "/cd /w/b");
    assert_eq!(probes(&effects).len(), 1, "{effects:?}");
}

#[test]
fn a_busy_chat_refuses_cd_with_the_reason_and_keeps_the_text() {
    let (mut a, turn) = run_live();
    announce(&mut a, turn, 1, run("cd ~/dev"));
    type_text(&mut a, "/cd /w/b");
    let effects = a.update(plain(KeyCode::Enter));
    assert!(probes(&effects).is_empty());
    assert!(lines(&a).contains(&"/cd refused: a command is running".to_owned()));
    assert_eq!(composer(&a), "/cd /w/b");
}

#[test]
fn a_streaming_chat_refuses_cd() {
    let mut a = in_folder();
    ask(&mut a, "again");
    type_text(&mut a, "/cd /w/b");
    let effects = a.update(plain(KeyCode::Enter));
    assert!(probes(&effects).is_empty());
    assert!(lines(&a).contains(&"/cd refused: the model is answering".to_owned()));
}

#[test]
fn a_refused_probe_shows_its_reason_and_keeps_the_text() {
    for (refusal, text) in [
        (Refusal::NotFound, "folder not found"),
        (Refusal::NotAFolder, "not a folder"),
        (Refusal::NotAllowed, "not allowed"),
        (Refusal::Root, "/ is not a valid chat folder"),
    ] {
        let mut a = in_folder();
        ask(&mut a, "/cd /x");
        let effects = probed(&mut a, Err(refusal));
        assert!(effects.is_empty());
        assert!(
            lines(&a).contains(&format!("/cd refused: {text}")),
            "{:?}",
            lines(&a)
        );
        assert_eq!(composer(&a), "/cd /x");
        assert_eq!(folder_of(&a), Some(PathBuf::from("/w/a")));
    }
}

#[test]
fn a_typed_outside_folder_is_not_allowed() {
    let mut a = in_folder();
    ask(&mut a, "/cd /usr");
    probed(
        &mut a,
        Ok(Probed {
            path: "/usr".into(),
            verdict: Verdict::Outside,
        }),
    );
    assert!(lines(&a).contains(&"/cd refused: not allowed".to_owned()));
}

#[test]
fn the_same_folder_says_so_and_opens_nothing() {
    let mut a = in_folder();
    ask(&mut a, "/cd /w/a/");
    probed(&mut a, concrete("/w/a"));
    assert!(lines(&a).contains(&"already in /w/a".to_owned()));
    assert!(
        super::run_pane::chat(&a, id2())
            .approvals()
            .pending()
            .is_none()
    );
}
