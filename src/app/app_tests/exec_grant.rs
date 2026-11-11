//! `/exec` and its dialog (orchestrator-worker-flow D16: the same path as
//! `/write`). Only a human keypress grants, the grant lives in the chat and in
//! memory, and every command still asks in every mode.

use std::path::PathBuf;

use super::chat_live::composer;
use super::github_grant::tools_app;
use super::run_pane::chat;
use super::tool_approval::{announce_now, pending};
use super::write_grant::{lines, submit, wait_for_arming};
use super::*;
use crate::core::ai::{
    chat::{
        ChatState,
        approval::{
            ApprovalAsk, EXEC_DECLINED_LINE, EXEC_GRANT_DENIED, EXEC_GRANTED_LINE, Origin,
            TOOL_GRANT_KEYS,
        },
        engine::{CallId, Decision},
    },
    config::{ApprovalMode, ToolToggle},
};

pub(super) fn has_line(a: &App, line: &str) -> bool {
    lines(a).iter().any(|l| l == line)
}

pub(super) fn granted(a: &App) -> bool {
    chat(a, id2()).tool_granted(ToolToggle::Exec)
}

pub(super) fn folder(a: &App) -> PathBuf {
    chat(a, id2()).origin_cwd().expect("a folder").to_path_buf()
}

pub(super) fn dialog_open(a: &App) -> bool {
    matches!(
        pending(a).map(|(ask, _)| ask),
        Some(ApprovalAsk::ToolGrant {
            toggle: ToolToggle::Exec,
            ..
        })
    )
}

/// The dialog is open for the chat folder.
pub(super) fn dialog(a: &mut App) {
    assert_eq!(submit(a, "/exec"), vec![]);
    assert!(dialog_open(a), "{:?}", lines(a));
}

/// The chat holds the Exec grant, as after `y`.
pub(super) fn hold(a: &mut App) {
    dialog(a);
    wait_for_arming();
    a.update(key('y'));
    assert!(granted(a), "{:?}", lines(a));
}

// Spec (D16): the dialog names Exec (not the config key), the folder, and says
// that every command still asks.
#[test]
fn the_command_opens_a_dialog_naming_exec_and_the_approval_rule() {
    let mut a = tools_app();
    assert_eq!(submit(&mut a, "/exec"), vec![]);
    assert_eq!(composer(&a), "");
    let (ask, _) = pending(&a).expect("the dialog");
    assert_eq!(
        ask,
        ApprovalAsk::ToolGrant {
            toggle: ToolToggle::Exec,
            folder: folder(&a)
        }
    );
    let prompt = a.approval_prompt(id2()).expect("the prompt");
    assert!(prompt.target.contains("Exec"), "{}", prompt.target);
    assert!(!prompt.target.contains("exec on"), "{}", prompt.target);
    assert_eq!(prompt.command, folder(&a).to_string_lossy());
    assert_eq!(prompt.keys, TOOL_GRANT_KEYS);
    let notice = prompt.notice.expect("a notice");
    assert!(notice.contains("every command still asks"), "{notice}");
    assert!(!notice.contains("file changes"), "{notice}");
    assert!(!granted(&a), "opening grants nothing");
}

#[test]
fn y_grants_this_chat_only_and_saves_nothing() {
    let mut a = tools_app();
    let config = a.config;
    dialog(&mut a);
    wait_for_arming();
    assert_eq!(a.update(key('y')), vec![]);
    assert!(granted(&a));
    assert!(!chat(&a, id2()).write_granted(), "Write is not granted");
    assert!(has_line(&a, EXEC_GRANTED_LINE), "{:?}", lines(&a));
    assert_eq!(pending(&a), None);
    assert!(!a.ai.toggle(ToolToggle::Exec), "the saved toggle is off");
    assert_eq!(a.config, config);
    press(&mut a, "c");
    let other = a.focused();
    assert!(!chat(&a, other).tool_granted(ToolToggle::Exec));
}

#[test]
fn n_and_esc_say_exec_stays_off() {
    for decline in [key('n'), super::chat_live::plain(KeyCode::Esc)] {
        let mut a = tools_app();
        dialog(&mut a);
        wait_for_arming();
        a.update(decline);
        assert!(!granted(&a));
        assert_eq!(pending(&a), None);
        assert!(has_line(&a, EXEC_DECLINED_LINE), "{:?}", lines(&a));
        assert!(!has_line(&a, EXEC_GRANTED_LINE));
    }
}

#[test]
fn arguments_are_refused_with_a_usage_line() {
    let mut a = tools_app();
    assert_eq!(submit(&mut a, "/exec x"), vec![]);
    assert!(has_line(&a, "/exec takes no arguments"), "{:?}", lines(&a));
    assert_eq!(composer(&a), "/exec x");
    assert_eq!(pending(&a), None);
}

#[test]
fn an_escaped_command_is_a_message() {
    let mut a = tools_app();
    let effects = submit(&mut a, "//exec");
    assert!(
        matches!(effects.as_slice(), [Effect::ChatSend { .. }]),
        "{effects:?}"
    );
    assert_eq!(pending(&a), None);
    assert!(!granted(&a));
}

// A refusal is visible with its reason and the `/write` wording.
#[test]
fn the_command_says_why_when_it_cannot_apply() {
    let refused = |a: &mut App, why: &str| {
        assert_eq!(submit(a, "/exec"), vec![], "{why}");
        let line = format!("/exec refused: {why}");
        assert!(has_line(a, &line), "{why}: {:?}", lines(a));
        assert_eq!(composer(a), "", "{why}");
        assert_eq!(pending(a), None, "{why}");
    };
    let mut a = tools_app();
    a.ai.set_tools_enabled(false);
    refused(&mut a, "tools are off");
    // Exec already on: by the saved toggle and by an earlier grant.
    let mut a = tools_app();
    a.ai.set_toggle(ToolToggle::Exec, true);
    refused(&mut a, "Exec is already on");
    let mut a = tools_app();
    hold(&mut a);
    refused(&mut a, "Exec is already on");
    // Write on does not make Exec "already on".
    let mut a = tools_app();
    a.ai.set_toggle(ToolToggle::Write, true);
    dialog(&mut a);
    let mut a = tools_app();
    *a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap() = ChatState::new(None);
    refused(&mut a, "this chat has no folder");
    let mut a = super::github_command::list_app();
    assert_eq!(a.exec_submit(id2()), vec![]);
    assert!(has_line(&a, "/exec refused: the conversation list is open"));
    assert_eq!(pending(&a), None);
}

#[test]
fn a_dialog_already_open_keeps_the_command_in_the_composer() {
    let mut a = tools_app();
    super::chat_live::type_text(&mut a, "/exec");
    a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap()
        .approvals_mut()
        .request(ApprovalAsk::read("a.rs"), Origin::Block)
        .unwrap();
    assert_eq!(a.chat_submit(id2()), vec![]);
    assert_eq!(composer(&a), "/exec");
    assert!(has_line(&a, "Answer the pending confirm first"));
    assert!(!dialog_open(&a));
}

// Spec (Text cannot trigger it): a tool call that carries the Exec grant is
// denied in every approval mode and opens nothing.
#[test]
fn a_tool_call_carrying_the_grant_is_denied_in_every_mode() {
    for mode in [ApprovalMode::Ask, ApprovalMode::Auto, ApprovalMode::Run] {
        let (mut a, turn) = super::tool_approval::live();
        a.ai.set_tools_enabled(true);
        a.ai.set_approval(mode);
        let effects = announce_now(
            &mut a,
            turn,
            1,
            ApprovalAsk::ToolGrant {
                toggle: ToolToggle::Exec,
                folder: PathBuf::new(),
            },
        );
        assert_eq!(effects.len(), 1, "{mode:?}: {effects:?}");
        assert!(
            matches!(
                &effects[0],
                Effect::ToolDecision {
                    call: CallId(1),
                    decision: Decision::Denied(why),
                    ..
                } if *why == EXEC_GRANT_DENIED
            ),
            "{mode:?}: {effects:?}"
        );
        assert_eq!(pending(&a), None, "{mode:?}");
        assert!(!granted(&a), "{mode:?}");
    }
}
