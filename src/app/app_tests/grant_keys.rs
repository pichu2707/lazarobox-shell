//! `Ctrl+Space x` and `Ctrl+Space f`: the keys that open the `/exec` and
//! `/write` dialogs (orchestrator-worker-flow W6c). They open the same dialog
//! as the command and never grant: only the user's `y` in the dialog does.

use super::chat_live::{composer, type_text};
use super::exec_grant::{dialog_open, granted, has_line};
use super::github_grant::tools_app;
use super::run_pane::chat;
use super::tool_approval::pending;
use super::write_grant::{granted as write_granted, lines, wait_for_arming};
use super::*;
use crate::core::ai::{
    chat::approval::{ApprovalAsk, EXEC_DECLINED_LINE, EXEC_GRANTED_LINE, Origin},
    config::ToolToggle,
};

fn write_dialog_open(a: &App) -> bool {
    matches!(
        pending(a).map(|(ask, _)| ask),
        Some(ApprovalAsk::ToolGrant {
            toggle: ToolToggle::Write,
            ..
        })
    )
}

#[test]
fn the_exec_key_opens_the_dialog_and_grants_nothing() {
    let mut a = tools_app();
    assert_eq!(press(&mut a, "x"), vec![]);
    assert!(dialog_open(&a), "{:?}", lines(&a));
    assert!(!granted(&a), "opening grants nothing");
    assert!(!a.ai.toggle(ToolToggle::Exec));
}

#[test]
fn y_after_the_exec_key_grants_and_n_or_esc_refuse() {
    let mut a = tools_app();
    press(&mut a, "x");
    wait_for_arming();
    a.update(key('y'));
    assert!(granted(&a));
    assert!(has_line(&a, EXEC_GRANTED_LINE), "{:?}", lines(&a));
    for decline in [key('n'), super::chat_live::plain(KeyCode::Esc)] {
        let mut a = tools_app();
        press(&mut a, "x");
        wait_for_arming();
        a.update(decline);
        assert!(!granted(&a));
        assert_eq!(pending(&a), None);
        assert!(has_line(&a, EXEC_DECLINED_LINE), "{:?}", lines(&a));
    }
}

#[test]
fn the_key_keeps_the_draft_in_the_composer() {
    let mut a = tools_app();
    type_text(&mut a, "half a thought");
    press(&mut a, "x");
    assert!(dialog_open(&a));
    assert_eq!(composer(&a), "half a thought");
}

#[test]
fn the_write_key_opens_the_write_dialog_and_only_y_grants() {
    let mut a = tools_app();
    assert_eq!(press(&mut a, "f"), vec![]);
    assert!(write_dialog_open(&a), "{:?}", lines(&a));
    assert!(!write_granted(&a));
    wait_for_arming();
    a.update(key('y'));
    assert!(write_granted(&a));
    assert!(!granted(&a), "Exec is not granted");
}

// Each refusal is the line the command gives, visible in the chat.
#[test]
fn each_refusal_is_the_line_the_command_gives() {
    let refused = |a: &mut App, key_: &str, command: &str, why: &str| {
        assert_eq!(press(a, key_), vec![], "{why}");
        let line = format!("{command} refused: {why}");
        assert!(has_line(a, &line), "{why}: {:?}", lines(a));
        assert_eq!(pending(a), None, "{why}");
    };
    for (key_, command, toggle) in [
        ("x", "/exec", ToolToggle::Exec),
        ("f", "/write", ToolToggle::Write),
    ] {
        let mut a = tools_app();
        a.ai.set_tools_enabled(false);
        refused(&mut a, key_, command, "tools are off");
        let mut a = tools_app();
        a.ai.set_toggle(toggle, true);
        let name = if toggle == ToolToggle::Exec {
            "Exec"
        } else {
            "Write"
        };
        refused(&mut a, key_, command, &format!("{name} is already on"));
        let mut a = tools_app();
        *a.panes
            .get_mut(&id2())
            .and_then(PaneState::chat_mut)
            .unwrap() = crate::core::ai::chat::ChatState::new(None);
        refused(&mut a, key_, command, "this chat has no folder");
    }
    let mut a = super::github_command::list_app();
    press(&mut a, "x");
    assert!(has_line(&a, "/exec refused: the conversation list is open"));
}

#[test]
fn another_pending_confirm_keeps_the_keys_and_says_so() {
    let mut a = tools_app();
    a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap()
        .approvals_mut()
        .request(ApprovalAsk::read("a.rs"), Origin::Block)
        .unwrap();
    press(&mut a, "x");
    assert!(has_line(&a, "Answer the pending confirm first"));
    assert!(!dialog_open(&a));
    assert!(chat(&a, id2()).approvals().pending().is_some());
}

#[test]
fn outside_a_chat_the_keys_say_so() {
    for (key_, what) in [("x", "Exec"), ("f", "Write")] {
        let mut a = launched(80, 25);
        press(&mut a, key_);
        let notice = a.notice.clone().expect("a notice");
        assert!(notice.starts_with(what), "{notice}");
        assert!(notice.contains("chat pane"), "{notice}");
    }
}
