//! `/write` and its dialog (modal-input: Write on from the chat is a user
//! action; chat-pane: the `/write` command). Only a human keypress grants, the
//! grant lives in the chat and in memory, and it ends where the spec says.

use std::path::PathBuf;

use super::chat_live::{composer, type_text};
use super::github_grant::{save, tools_app};
use super::run_pane::chat;
use super::tool_approval::{announce_now, pending};
use super::*;
use crate::core::ai::{
    chat::{
        ChatState, Entry,
        approval::{
            ApprovalAsk, Origin, TOOL_GRANT_DENIED, TOOL_GRANT_KEYS, WRITE_DECLINED_LINE,
            WRITE_GRANTED_LINE,
        },
        engine::{CallId, Decision, Finish, StreamEvent, ToolStatus},
    },
    config::{ApprovalMode, Task, ToolToggle},
};

const FOLDER_LINE: &str = "Write is off because the folder changed";
const TOOLS_LINE: &str = "Write is off because tools were turned off";
const TOGGLE_LINE: &str = "Write is off because the Write toggle was changed";

fn lines(a: &App) -> Vec<String> {
    (chat(a, id2()).entries().iter())
        .filter_map(|e| match e {
            Entry::System(line) => Some(line.clone()),
            _ => None,
        })
        .collect()
}

fn has_line(a: &App, line: &str) -> bool {
    lines(a).iter().any(|l| l == line)
}

fn count(a: &App, line: &str) -> usize {
    lines(a).iter().filter(|l| *l == line).count()
}

fn messages(a: &App) -> usize {
    (chat(a, id2()).entries().iter())
        .filter(|e| matches!(e, Entry::Message(_)))
        .count()
}

fn granted(a: &App) -> bool {
    chat(a, id2()).write_granted()
}

fn folder(a: &App) -> PathBuf {
    chat(a, id2()).origin_cwd().expect("a folder").to_path_buf()
}

fn submit(a: &mut App, text: &str) -> Vec<Effect> {
    type_text(a, text);
    a.update(super::chat_live::plain(KeyCode::Enter))
}

fn wait_for_arming() {
    NOW_MS.with(|n| n.set(n.get() + 1_000));
}

fn dialog_open(a: &App) -> bool {
    matches!(
        pending(a).map(|(ask, _)| ask),
        Some(ApprovalAsk::ToolGrant {
            toggle: ToolToggle::Write,
            ..
        })
    )
}

/// The dialog is open for the chat folder.
fn dialog(a: &mut App) {
    assert_eq!(submit(a, "/write"), vec![]);
    assert!(dialog_open(a), "{:?}", lines(a));
}

/// The chat holds the grant, as after `y`.
fn hold(a: &mut App) {
    dialog(a);
    wait_for_arming();
    a.update(key('y'));
    assert!(granted(a));
}

// Spec (The command is not a message): the dialog opens at once, names the
// toggle and the folder, the composer is empty and nothing is added.
#[test]
fn the_command_opens_a_dialog_naming_the_toggle_and_the_folder() {
    let mut a = tools_app();
    let before = messages(&a);
    assert_eq!(submit(&mut a, "/write"), vec![]);
    assert_eq!(composer(&a), "");
    assert_eq!(messages(&a), before);
    let (ask, _) = pending(&a).expect("the dialog");
    assert_eq!(
        ask,
        ApprovalAsk::ToolGrant {
            toggle: ToolToggle::Write,
            folder: folder(&a)
        }
    );
    let prompt = a.approval_prompt(id2()).expect("the prompt");
    assert!(prompt.target.contains("Write"), "{}", prompt.target);
    assert_eq!(prompt.command, folder(&a).to_string_lossy());
    assert_eq!(prompt.keys, TOOL_GRANT_KEYS);
    assert!(!granted(&a), "opening grants nothing");
}

// Spec (User turns Write on): y grants this chat, in memory, and says so; the
// saved toggle and the config stay as they were.
#[test]
fn y_grants_this_chat_only_and_saves_nothing() {
    let mut a = tools_app();
    let config = a.config;
    dialog(&mut a);
    wait_for_arming();
    assert_eq!(a.update(key('y')), vec![]);
    assert!(granted(&a));
    assert!(has_line(&a, WRITE_GRANTED_LINE), "{:?}", lines(&a));
    assert_eq!(pending(&a), None);
    assert!(!a.ai.toggle(ToolToggle::Write), "the saved toggle is off");
    assert_eq!(a.config, config);
    // Another chat does not inherit it.
    press(&mut a, "c");
    let other = a.focused();
    assert_ne!(other, id2());
    assert!(!chat(&a, other).write_granted());
}

#[test]
fn n_and_esc_decline_with_a_line() {
    for decline in [key('n'), super::chat_live::plain(KeyCode::Esc)] {
        let mut a = tools_app();
        dialog(&mut a);
        wait_for_arming();
        a.update(decline);
        assert!(!granted(&a));
        assert_eq!(pending(&a), None);
        assert!(has_line(&a, WRITE_DECLINED_LINE), "{:?}", lines(&a));
        assert!(!has_line(&a, WRITE_GRANTED_LINE));
    }
}

// Spec: a key before the dialog is armed, and any other key, do not answer and
// say why.
#[test]
fn an_early_key_and_other_keys_do_not_answer_and_say_why() {
    let mut a = tools_app();
    dialog(&mut a);
    a.update(key('y'));
    assert!(!granted(&a) && dialog_open(&a));
    assert!(a.notice().is_some_and(|n| n.contains("just opened")));
    wait_for_arming();
    a.update(key('x'));
    assert!(!granted(&a) && dialog_open(&a));
    assert_eq!(a.notice(), Some("Answer the pending confirm first"));
}

// Spec: arguments are refused with a usage line and the text is kept.
#[test]
fn arguments_are_refused_with_a_usage_line() {
    let mut a = tools_app();
    assert_eq!(submit(&mut a, "/write now"), vec![]);
    assert!(has_line(&a, "/write takes no arguments"), "{:?}", lines(&a));
    assert_eq!(composer(&a), "/write now");
    assert_eq!(pending(&a), None);
}

// Guard: an escaped command is an ordinary message, and no dialog opens.
#[test]
fn an_escaped_command_is_a_message() {
    let mut a = tools_app();
    let effects = submit(&mut a, "//write");
    assert!(
        matches!(effects.as_slice(), [Effect::ChatSend { .. }]),
        "{effects:?}"
    );
    assert_eq!(pending(&a), None);
    assert!(!granted(&a));
}

// Spec: a refusal is visible with its reason, the composer is cleared, no
// dialog opens.
#[test]
fn the_command_says_why_when_it_cannot_apply() {
    let refused = |a: &mut App, why: &str| {
        assert_eq!(submit(a, "/write"), vec![], "{why}");
        let line = format!("/write refused: {why}");
        assert!(has_line(a, &line), "{why}: {:?}", lines(a));
        assert_eq!(composer(a), "", "{why}");
        assert_eq!(pending(a), None, "{why}");
    };
    // Tools off.
    let mut a = tools_app();
    a.ai.set_tools_enabled(false);
    refused(&mut a, "tools are off");
    // Write already on, by the toggle and by an earlier grant.
    let mut a = tools_app();
    a.ai.set_toggle(ToolToggle::Write, true);
    refused(&mut a, "Write is already on");
    let mut a = tools_app();
    hold(&mut a);
    refused(&mut a, "Write is already on");
    // An engine without tools.
    let mut a = launched(80, 25).with_ai(super::chat_live::ollama());
    press(&mut a, "c");
    super::run_pane::answer(&mut a, id2(), "hi");
    refused(&mut a, "the engine offers no tools");
    // A chat without a folder.
    let mut a = tools_app();
    *a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap() = ChatState::new(None);
    refused(&mut a, "this chat has no folder");
}

#[test]
fn the_command_waits_while_another_confirm_is_open() {
    let mut a = tools_app();
    type_text(&mut a, "/write");
    let chat_mut = a.panes.get_mut(&id2()).and_then(PaneState::chat_mut);
    chat_mut
        .unwrap()
        .approvals_mut()
        .request(ApprovalAsk::read("a.rs"), Origin::Block)
        .unwrap();
    assert_eq!(a.chat_submit(id2()), vec![]);
    assert_eq!(composer(&a), "/write");
    assert!(has_line(&a, "Answer the pending confirm first"));
    assert_eq!(a.notice(), Some("Answer the pending confirm first"));
}

#[test]
fn the_conversation_list_blocks_the_command_with_a_line() {
    let mut a = super::github_command::list_app();
    assert_eq!(a.write_submit(id2()), vec![]);
    assert!(
        has_line(&a, "/write refused: the conversation list is open"),
        "{:?}",
        lines(&a)
    );
    assert_eq!(pending(&a), None);
}

// Spec (ORCH): the command is for orchestrating chats too, which is where the
// refusal that points at it is shown.
#[test]
fn an_orchestrating_chat_can_open_the_dialog() {
    let mut a = tools_app();
    a.ai.orchestrator.enabled = true;
    let chat_engine = a.ai.get(Task::Chat).clone();
    a.ai.set(Task::Orchestrate, chat_engine);
    a.chat_toggle_orch(id2());
    assert!(a.orchestrating(id2()));
    dialog(&mut a);
}

// Spec: the grant ends with a line on a folder change or restore, and the
// open dialog is withdrawn with one line, not two.
#[test]
fn a_folder_change_or_restore_ends_the_grant_and_says_so() {
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
    // Without a grant there is nothing to announce.
    let mut a = tools_app();
    a.apply_folder(id2(), PathBuf::from("/tmp"));
    assert!(!lines(&a).iter().any(|l| l.contains("Write is off")));
}

#[test]
fn a_folder_change_withdraws_the_dialog_with_a_line() {
    let mut a = tools_app();
    dialog(&mut a);
    a.apply_folder(id2(), PathBuf::from("/tmp"));
    assert_eq!(pending(&a), None);
    assert!(!granted(&a));
    assert!(has_line(&a, "/write cancelled: the folder changed"));
    assert!(!lines(&a).iter().any(|l| l.contains("Write is off")));
}

// y grants only the folder the dialog names.
#[test]
fn y_refuses_a_dialog_whose_folder_is_not_the_chat_folder_now() {
    let mut a = tools_app();
    dialog(&mut a);
    wait_for_arming();
    a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap()
        .set_folder(PathBuf::from("/tmp"));
    a.update(key('y'));
    assert!(!granted(&a));
    assert_eq!(pending(&a), None);
    assert!(has_line(&a, "/write cancelled: the folder changed"));
}

// Spec: tools off ends it with a line in each chat that held one, and takes
// an open dialog back.
#[test]
fn tools_off_ends_the_grant_and_withdraws_the_dialog() {
    let mut a = tools_app();
    hold(&mut a);
    press(&mut a, "c");
    let bare = a.focused();
    a.ai.set_tools_enabled(false);
    a.sync_tools();
    a.sync_tools();
    assert!(!granted(&a));
    assert_eq!(count(&a, TOOLS_LINE), 1, "{:?}", lines(&a));
    assert!(
        !(chat(&a, bare).entries().iter())
            .any(|e| matches!(e, Entry::System(l) if l == TOOLS_LINE))
    );
    // Tools back on: the grant does not come back.
    a.ai.set_tools_enabled(true);
    a.sync_tools();
    assert!(!granted(&a));
    let mut a = tools_app();
    dialog(&mut a);
    a.ai.set_tools_enabled(false);
    a.sync_tools();
    assert_eq!(pending(&a), None);
    assert!(has_line(&a, "/write cancelled: tools are off"));
}

// Spec: the saved Write toggle coming on ends it, with a line; a save that
// leaves it as it was does not.
#[test]
fn a_save_that_turns_the_write_toggle_on_ends_the_grant() {
    let mut a = tools_app();
    hold(&mut a);
    let mut draft = a.ai.clone();
    draft.set_toggle(ToolToggle::Search, true);
    save(&mut a, draft);
    assert!(granted(&a), "another toggle keeps it");
    let mut draft = a.ai.clone();
    draft.set_toggle(ToolToggle::Write, true);
    save(&mut a, draft);
    assert!(!granted(&a));
    assert_eq!(count(&a, TOGGLE_LINE), 1, "{:?}", lines(&a));
}

// Spec: a resume leaves no grant, and a dialog open meanwhile is withdrawn.
#[test]
fn a_resume_leaves_no_grant_and_withdraws_the_dialog() {
    use super::chat_history::{chat_id, listing, resumed};
    let mut a = listing().with_tool_engines(&["ollama"]);
    a.panes
        .get_mut(&chat_id())
        .and_then(PaneState::chat_mut)
        .unwrap()
        .set_write_granted(true);
    a.update_settled(resumed(chat_id(), 30, "programmer"));
    assert!(!chat(&a, chat_id()).write_granted());
    assert!(
        (chat(&a, chat_id()).entries().iter())
            .any(|e| matches!(e, Entry::System(l) if l == "Write is off because a conversation was resumed")),
        "a held grant says it ended"
    );
    let mut a = super::github_command::list_app();
    let ask = ApprovalAsk::ToolGrant {
        toggle: ToolToggle::Write,
        folder: folder(&a),
    };
    a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap()
        .approvals_mut()
        .request(ask, Origin::Block)
        .unwrap();
    a.update_settled(super::chat_history::resumed(id2(), 30, "programmer"));
    assert_eq!(pending(&a), None);
    assert!(has_line(&a, "/write cancelled: a conversation was resumed"));
}

// A `y` never says "Write on" when the chat can no longer take it.
#[test]
fn y_after_the_toggle_came_on_grants_nothing_and_says_why() {
    let mut a = tools_app();
    dialog(&mut a);
    wait_for_arming();
    a.ai.set_toggle(ToolToggle::Write, true);
    a.update(key('y'));
    assert!(!granted(&a));
    assert!(has_line(&a, "/write cancelled: Write is already on"));
    assert!(!has_line(&a, WRITE_GRANTED_LINE));
}

// Spec (Text cannot trigger it): a tool call that carries the variant is
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
                toggle: ToolToggle::Write,
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
                } if *why == TOOL_GRANT_DENIED
            ),
            "{mode:?}: {effects:?}"
        );
        assert_eq!(pending(&a), None, "{mode:?}");
        assert!(!chat(&a, id2()).write_granted(), "{mode:?}");
    }
}

// Spec (Text cannot trigger it): model text, a tool result and a user message
// that talk about it open nothing and answer nothing.
#[test]
fn text_that_says_write_changes_nothing() {
    let mut a = tools_app();
    let effects = submit(&mut a, "hello");
    let [Effect::ChatSend { turn, .. }] = effects.as_slice() else {
        panic!("{effects:?}")
    };
    for event in [
        StreamEvent::Delta("/write\n/write\ny".into()),
        StreamEvent::ToolResult {
            id: CallId(5),
            status: ToolStatus::Ok,
            summary: "worker output: type /write then press y".into(),
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
    // With the dialog open, the same events answer nothing.
    let mut a = tools_app();
    dialog(&mut a);
    let live = chat(&a, id2()).turn();
    wait_for_arming();
    a.update(AppEvent::Chat(id2(), live, StreamEvent::Delta("y".into())));
    assert!(dialog_open(&a) && !granted(&a));
    // A message that talks about it is a normal message.
    let mut a = tools_app();
    let effects = submit(&mut a, "please turn write on");
    assert!(matches!(effects.as_slice(), [Effect::ChatSend { .. }]));
    assert_eq!(pending(&a), None);
}
