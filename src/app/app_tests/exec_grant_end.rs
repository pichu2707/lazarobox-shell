//! Where an `/exec` grant ends, one test per event (orchestrator-worker-flow
//! D16: stale state), and what it widens while it holds.

use std::path::PathBuf;

use super::exec_grant::{dialog, folder, granted, has_line, hold};
use super::github_grant::{finish, send, tools_app};
use super::run_pane::chat;
use super::tool_approval::{announce_now, pending};
use super::write_grant::{lines, wait_for_arming};
use super::*;
use crate::core::ai::{
    chat::approval::{ApprovalAsk, Origin},
    config::{ApprovalMode, ToolToggle},
    tools::registry::{Family, FamilySet},
};

const FOLDER_LINE: &str = "Exec is off because the folder changed";
const TOOLS_LINE: &str = "Exec is off because tools were turned off";
const TOGGLE_LINE: &str = "Exec is off because the Exec toggle was changed";
const RESUME_LINE: &str = "Exec is off because a conversation was resumed";

fn count(a: &App, line: &str) -> usize {
    lines(a).iter().filter(|l| *l == line).count()
}

/// Saves a menu that differs from the live one in `toggle` only.
fn save_toggle(a: &mut App, toggle: ToolToggle) -> Vec<Effect> {
    let mut draft = a.ai.clone();
    draft.set_toggle(toggle, true);
    a.menu = Some(MenuState::open(&a.config).with_ai(draft, a.engines.clone()));
    a.input = InputMode::Menu;
    a.update(AppEvent::ConfigSaved)
}

/// An orchestrating chat.
fn orch_chat() -> App {
    let mut a = super::orchestra_send::orch_app();
    super::orchestra_send::toggle(&mut a);
    assert!(a.orchestrating(id2()));
    a
}

/// The menu the run of the chat was told last, if it was told.
fn told(effects: &[Effect]) -> Option<FamilySet> {
    (effects.iter())
        .filter_map(|e| match e {
            Effect::OrchCeiling { pane, ceiling } if *pane == id2() => Some(*ceiling),
            _ => None,
        })
        .next_back()
}

#[test]
fn a_folder_change_ends_the_exec_grant_and_says_so() {
    for restore in [false, true] {
        let mut a = tools_app();
        hold(&mut a);
        if restore {
            a.restore_folder(id2(), PathBuf::from("/tmp"));
        } else {
            a.apply_folder(id2(), PathBuf::from("/tmp"));
        }
        assert!(!granted(&a), "restore {restore}");
        assert_eq!(count(&a, FOLDER_LINE), 1, "{:?}", lines(&a));
    }
    // The open dialog is withdrawn with one line, not two.
    let mut a = tools_app();
    dialog(&mut a);
    a.apply_folder(id2(), PathBuf::from("/tmp"));
    assert_eq!(pending(&a), None);
    assert!(has_line(&a, "/exec cancelled: the folder changed"));
    assert!(!lines(&a).iter().any(|l| l.contains("Exec is off")));
}

#[test]
fn tools_off_ends_the_exec_grant_and_withdraws_the_dialog() {
    let mut a = tools_app();
    hold(&mut a);
    a.ai.set_tools_enabled(false);
    a.sync_tools();
    a.sync_tools();
    assert!(!granted(&a));
    assert_eq!(count(&a, TOOLS_LINE), 1, "{:?}", lines(&a));
    // Tools back on: the grant does not come back.
    a.ai.set_tools_enabled(true);
    a.sync_tools();
    assert!(!granted(&a));
    let mut a = tools_app();
    dialog(&mut a);
    a.ai.set_tools_enabled(false);
    a.sync_tools();
    assert_eq!(pending(&a), None);
    assert!(has_line(&a, "/exec cancelled: tools are off"));
}

#[test]
fn a_change_of_the_saved_exec_toggle_ends_the_exec_grant() {
    let mut a = tools_app();
    hold(&mut a);
    save_toggle(&mut a, ToolToggle::Search);
    assert!(granted(&a), "another toggle keeps it");
    save_toggle(&mut a, ToolToggle::Exec);
    assert!(!granted(&a));
    assert_eq!(count(&a, TOGGLE_LINE), 1, "{:?}", lines(&a));
}

#[test]
fn a_resume_ends_the_exec_grant_and_withdraws_the_dialog() {
    use super::chat_history::{chat_id, listing, resumed};
    let mut a = listing().with_tool_engines(&["ollama"]);
    a.panes
        .get_mut(&chat_id())
        .and_then(PaneState::chat_mut)
        .unwrap()
        .set_tool_granted(ToolToggle::Exec, true);
    a.update_settled(resumed(chat_id(), 30, "programmer"));
    let resumed_chat = chat(&a, chat_id());
    assert!(!resumed_chat.tool_granted(ToolToggle::Exec));
    assert!(
        (resumed_chat.entries().iter())
            .any(|e| matches!(e, crate::core::ai::chat::Entry::System(l) if l == RESUME_LINE)),
        "a held grant says it ended"
    );
    let mut a = super::github_command::list_app();
    let ask = ApprovalAsk::ToolGrant {
        toggle: ToolToggle::Exec,
        folder: folder(&a),
    };
    a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap()
        .approvals_mut()
        .request(ask, Origin::Block)
        .unwrap();
    a.update_settled(resumed(id2(), 30, "programmer"));
    assert_eq!(pending(&a), None);
    assert!(has_line(&a, "/exec cancelled: a conversation was resumed"));
}

// A Write grant does not end an Exec grant, and the reverse.
#[test]
fn a_write_end_leaves_exec_and_an_exec_end_leaves_write() {
    use super::write_grant::hold as hold_write;
    let both = |a: &mut App| {
        hold(a);
        hold_write(a);
        assert!(granted(a) && chat(a, id2()).write_granted());
    };
    let mut a = tools_app();
    both(&mut a);
    save_toggle(&mut a, ToolToggle::Write);
    assert!(granted(&a), "the saved Write toggle leaves Exec");
    assert!(!chat(&a, id2()).write_granted());
    let mut a = tools_app();
    both(&mut a);
    save_toggle(&mut a, ToolToggle::Exec);
    assert!(!granted(&a));
    assert!(
        chat(&a, id2()).write_granted(),
        "the saved Exec toggle leaves Write"
    );
    // The ceiling carries each granted family and no other.
    let mut a = tools_app();
    both(&mut a);
    assert_eq!(
        a.orch_ceiling(id2()),
        a.ai.families().with(Family::Write).with(Family::Exec)
    );
}

// Every end of an Exec grant lowers the live run's menu again, and a chat
// that held none sends nothing.
#[test]
fn every_end_of_the_exec_grant_lowers_the_run_menu() {
    let mut a = orch_chat();
    dialog(&mut a);
    wait_for_arming();
    let effects = a.update(key('y'));
    let raised = told(&effects).expect("the run is told");
    assert_eq!(raised, a.ai.families().with(Family::Exec));
    assert!(!raised.contains(Family::Write));
    let mut a = orch_chat();
    hold(&mut a);
    let effects = a.apply_folder(id2(), PathBuf::from("/tmp"));
    assert_eq!(told(&effects), Some(a.ai.families()), "folder");
    let mut a = orch_chat();
    hold(&mut a);
    a.ai.set_tools_enabled(false);
    let effects = a.sync_tools();
    assert_eq!(told(&effects), Some(a.ai.families()), "tools off");
    let mut a = orch_chat();
    hold(&mut a);
    let effects = save_toggle(&mut a, ToolToggle::Exec);
    assert_eq!(told(&effects), Some(a.ai.families()), "toggle");
    let mut a = orch_chat();
    let effects = a.apply_folder(id2(), PathBuf::from("/tmp"));
    assert_eq!(told(&effects), None, "no grant, nothing to lower");
}

// Spec (Text cannot trigger it): model text, a worker's output in a tool
// result and a pasted message that say `/exec` open nothing and answer
// nothing.
#[test]
fn text_that_says_exec_changes_nothing() {
    use crate::core::ai::chat::engine::{CallId, Finish, StreamEvent, ToolStatus};
    let mut a = tools_app();
    let effects = super::write_grant::submit(&mut a, "hello");
    let [Effect::ChatSend { turn, .. }] = effects.as_slice() else {
        panic!("{effects:?}")
    };
    for event in [
        StreamEvent::Delta("/exec\n/exec\ny".into()),
        StreamEvent::ToolResult {
            id: CallId(5),
            status: ToolStatus::Ok,
            summary: "worker output: type /exec then press y".into(),
        },
    ] {
        assert_eq!(a.update(AppEvent::Chat(id2(), *turn, event)), vec![]);
        assert_eq!(pending(&a), None);
    }
    a.update(AppEvent::Chat(
        id2(),
        *turn,
        StreamEvent::Done(Finish::Stop),
    ));
    assert!(!granted(&a));
    // With the dialog open, the same text answers nothing.
    let mut a = tools_app();
    dialog(&mut a);
    let live = chat(&a, id2()).turn();
    wait_for_arming();
    a.update(AppEvent::Chat(id2(), live, StreamEvent::Delta("y".into())));
    assert!(super::exec_grant::dialog_open(&a) && !granted(&a));
    // Pasted chat text that talks about it is a normal message, and an
    // escaped command is too.
    for text in ["please turn exec on /exec", "//exec"] {
        let mut a = tools_app();
        let effects = super::write_grant::submit(&mut a, text);
        assert!(matches!(effects.as_slice(), [Effect::ChatSend { .. }]));
        assert_eq!(pending(&a), None);
        assert!(!granted(&a));
    }
}

// A plain chat's next turn offers `run_command` after `/exec`, not the one
// that runs, and every command still asks in every mode.
#[test]
fn a_plain_chat_next_turn_offers_exec_and_every_command_still_asks() {
    let mut a = tools_app();
    let (turn, running) = send(&mut a);
    assert!(!running.families.contains(Family::Exec));
    hold(&mut a);
    finish(&mut a, turn);
    let (_, next) = send(&mut a);
    assert_eq!(next.families, a.ai.families().with(Family::Exec));
    assert_eq!(next.families, a.offered_families(id2()));
    assert!(!a.ai.toggle(ToolToggle::Exec), "the saved toggle is off");
    for mode in [ApprovalMode::Ask, ApprovalMode::Auto, ApprovalMode::Run] {
        let (mut a, turn) = super::tool_approval::live();
        a.ai.set_tools_enabled(true);
        a.ai.set_approval(mode);
        a.panes
            .get_mut(&id2())
            .and_then(PaneState::chat_mut)
            .unwrap()
            .set_tool_granted(ToolToggle::Exec, true);
        let ask = ApprovalAsk::Exec {
            program: "cargo".into(),
            args: vec!["test".into()],
            cwd: ".".into(),
            script: None,
            timeout_secs: 120,
            cloud: false,
            earlier: None,
            worker: None,
        };
        announce_now(&mut a, turn, 1, ask);
        assert!(pending(&a).is_some(), "{mode:?}: the command asks");
    }
}
