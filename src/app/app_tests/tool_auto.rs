//! Chat approval mode (agent-tools, D18): where the mode of a chat comes from
//! and when it resets.

use super::chat_history::{chat_of, history_app, listed, opened, prompt_for, resumed, rows};
use super::*;
use crate::core::ai::config::ApprovalMode;

fn auto_config() -> AiConfig {
    let mut ai = chat_live::ollama();
    ai.set_approval(ApprovalMode::Auto);
    ai
}

// 2e-3.3: a chat takes the live config value when its pane opens.
#[test]
fn a_chat_pane_opens_in_the_configured_mode() {
    let mut a = launched(80, 25).with_ai(chat_live::ollama());
    press(&mut a, "c");
    assert_eq!(chat_of_focus(&a), ApprovalMode::Ask);

    let mut b = launched(80, 25).with_ai(auto_config());
    press(&mut b, "c");
    assert_eq!(chat_of_focus(&b), ApprovalMode::Auto);
}

fn chat_of_focus(a: &App) -> ApprovalMode {
    run_pane::chat(a, a.focused()).approval_mode()
}

fn set_mode(a: &mut App, mode: ApprovalMode) {
    let id = a.focused();
    a.panes
        .get_mut(&id)
        .and_then(PaneState::chat_mut)
        .unwrap()
        .set_approval_mode(mode);
}

// A config change reaches later opens and switches, never the open
// conversation.
#[test]
fn a_config_change_does_not_touch_an_open_chat() {
    let mut a = launched(80, 25).with_ai(chat_live::ollama());
    press(&mut a, "c");
    a.ai.set_approval(ApprovalMode::Auto);
    assert_eq!(chat_of_focus(&a), ApprovalMode::Ask);
}

// The mode chosen in a chat lasts for its conversation: starting a new one
// from the list goes back to the config value.
#[test]
fn starting_a_new_conversation_resets_the_mode_to_the_config() {
    let mut a = history_app();
    a.ai.set_approval(ApprovalMode::Auto);
    press(&mut a, "c");
    let id = a.focused();
    a.update(prompt_for(id));
    listed(&mut a, id, rows());
    set_mode(&mut a, ApprovalMode::Ask);
    a.update(key('n'));
    assert_eq!(chat_of(&a).approval_mode(), ApprovalMode::Auto);
}

#[test]
fn resuming_a_conversation_resets_the_mode_to_the_config() {
    let (mut a, _) = opened();
    let id = a.focused();
    a.update(prompt_for(id));
    listed(&mut a, id, rows());
    a.ai.set_approval(ApprovalMode::Auto);
    a.update(resumed(id, 30, "programmer"));
    assert_eq!(chat_of(&a).approval_mode(), ApprovalMode::Auto);

    // And the other way: the toggled chat goes back to ask.
    a.ai.set_approval(ApprovalMode::Ask);
    set_mode(&mut a, ApprovalMode::Auto);
    a.focused_state_mut().chat_mut().unwrap().open_list();
    listed(&mut a, id, rows());
    a.update(resumed(id, 10, "analyst"));
    assert_eq!(chat_of(&a).approval_mode(), ApprovalMode::Ask);
}

// 2e-3.4: auto-type -----------------------------------------------------------

use super::tool_approval::{announce, announce_now, decision, live, pending, run, shell_output};
use crate::core::ai::chat::{
    approval::ANOTHER_PENDING,
    engine::{CallId, Decision},
};

/// `live()` with the chat in `auto` mode.
fn auto_live() -> (App, u64) {
    let (mut a, turn) = live();
    set_mode(&mut a, ApprovalMode::Auto);
    (a, turn)
}

fn typed(turn: u64, call: u64, bytes: &[u8]) -> Effect {
    Effect::AutoType {
        chat: id2(),
        target: PaneId::FIRST,
        bytes: bytes.to_vec(),
        turn,
        call: CallId(call),
    }
}

fn auto_typed(a: &App) -> u8 {
    run_pane::chat(a, id2()).auto_typed()
}

#[test]
fn an_auto_run_is_typed_without_a_prompt_and_without_enter() {
    let (mut a, turn) = auto_live();
    assert_eq!(
        announce(&mut a, turn, 1, run("cd x")),
        vec![typed(turn, 1, b"cd x")]
    );
    assert_eq!(pending(&a), None);
    assert_eq!(auto_typed(&a), 1);
}

#[test]
fn ask_mode_still_prompts() {
    let (mut a, turn) = live();
    assert_eq!(announce(&mut a, turn, 1, run("cd x")), vec![]);
    assert!(pending(&a).is_some());
    assert_eq!(auto_typed(&a), 0);
}

// Sent means typed, by the same function as `y`: a paste-framed multi-line
// command has the same bytes both ways.
#[test]
fn the_bytes_are_the_ones_y_would_write() {
    let command = "echo a\necho b";
    let (mut asking, turn) = live();
    announce(&mut asking, turn, 1, run(command));
    shell_output(&mut asking, b"\x1b[?2004h");
    let effects = asking.update(key('y'));
    let Some(Effect::WritePty(_, want)) = effects.first() else {
        panic!("{effects:?}")
    };

    let (mut a, turn) = auto_live();
    shell_output(&mut a, b"\x1b[?2004h");
    let effects = announce(&mut a, turn, 1, run(command));
    assert_eq!(effects, vec![typed(turn, 1, want)]);
    assert!(!want.contains(&b'\r'));
}

#[test]
fn a_failed_recheck_denies_the_call_with_the_reason_and_writes_nothing() {
    let (mut a, turn) = auto_live();
    shell_output(&mut a, b"\x1b[?1049h");
    assert_eq!(
        announce(&mut a, turn, 1, run("cd x")),
        vec![decision(
            turn,
            1,
            Decision::Denied("the pane runs a full-screen app")
        )]
    );
    assert_eq!(auto_typed(&a), 0);

    // A multi-line command needs paste mode.
    let (mut a, turn) = auto_live();
    assert_eq!(
        announce(&mut a, turn, 1, run("echo a\necho b")),
        vec![decision(
            turn,
            1,
            Decision::Denied("the shell cannot take a multi-line command safely")
        )]
    );

    // An escape in the command is refused by the validator.
    let (mut a, turn) = auto_live();
    assert_eq!(
        announce(&mut a, turn, 1, run("echo \x1b[2J")),
        vec![decision(
            turn,
            1,
            Decision::Denied("the command was refused")
        )]
    );
    assert_eq!(auto_typed(&a), 0);
}

#[test]
fn a_pending_approval_denies_the_auto_call() {
    let (mut a, turn) = auto_live();
    a.focused_state_mut()
        .chat_mut()
        .unwrap()
        .approvals_mut()
        .request(run("mine"), crate::core::ai::chat::approval::Origin::Block)
        .unwrap();
    assert_eq!(
        announce(&mut a, turn, 1, run("cd x")),
        vec![decision(turn, 1, Decision::Denied(ANOTHER_PENDING))]
    );
    assert_eq!(auto_typed(&a), 0);
}

#[test]
fn a_focused_target_or_a_hidden_tab_falls_back_to_the_prompt() {
    let (mut a, turn) = auto_live();
    a.tab_mut().focus = PaneId::FIRST;
    assert_eq!(announce(&mut a, turn, 1, run("cd x")), vec![]);
    assert!(pending(&a).is_some());
    assert_eq!(auto_typed(&a), 0);

    let (mut a, turn) = auto_live();
    press(&mut a, "tn");
    assert_ne!(a.active, 0, "another tab is shown");
    assert_eq!(announce_now(&mut a, turn, 1, run("cd x")), vec![]);
    assert!(pending(&a).is_some());
}

// A zoomed tab shows only the focused pane: the user cannot see the typing in
// the target, so the call prompts.
#[test]
fn a_zoomed_chat_hides_the_target_so_the_call_prompts() {
    let (mut a, turn) = auto_live();
    assert_eq!(a.tab().focus, id2());
    a.tab_mut().zoom = true;
    assert_eq!(announce(&mut a, turn, 1, run("cd x")), vec![]);
    assert!(pending(&a).is_some());
    assert_eq!(auto_typed(&a), 0);
}

// One batch, one pane: the first is typed, the second prompts (the line is
// pending as soon as the effect is emitted).
#[test]
fn two_calls_to_one_pane_in_a_batch_type_one_and_prompt_the_other() {
    let (mut a, turn) = auto_live();
    assert_eq!(
        announce_now(&mut a, turn, 1, run("cd x")),
        vec![typed(turn, 1, b"cd x")]
    );
    assert_eq!(announce_now(&mut a, turn, 2, run("ls")), vec![]);
    assert_eq!(
        pending(&a).map(|(ask, _)| ask),
        Some(run("ls")),
        "the second one waits for the user"
    );
}

// 2e-3.4b: the line-pending mark ----------------------------------------------

use super::tool_approval::result;
use crate::core::ai::chat::engine::ToolStatus;

/// Types one command so pane 1 holds an unsent line, then gives the focus to
/// the shell, as when the user goes to look at it.
fn marked() -> (App, u64) {
    let (mut a, turn) = auto_live();
    assert_eq!(
        announce(&mut a, turn, 1, run("cd x")),
        vec![typed(turn, 1, b"cd x")]
    );
    (a, turn)
}

fn prompts(a: &mut App, turn: u64, call: u64) -> bool {
    let effects = announce(a, turn, call, run("ls"));
    let waits = pending(a).is_some_and(|(ask, _)| ask == run("ls"));
    if waits {
        // Leave the chat as it was for the next probe.
        a.update(key('n'));
        assert_eq!(effects, vec![]);
    }
    waits
}

fn user_in_the_shell(a: &mut App) {
    a.tab_mut().focus = PaneId::FIRST;
}

#[test]
fn a_later_auto_call_to_a_marked_pane_prompts() {
    let (mut a, turn) = marked();
    assert!(prompts(&mut a, turn, 2), "same turn");
}

#[test]
fn the_mark_survives_into_a_later_turn() {
    let (mut a, turn) = marked();
    a.tab_mut().focus = id2();
    a.update(AppEvent::Chat(
        id2(),
        turn,
        StreamEvent::Done(crate::core::ai::chat::engine::Finish::Stop),
    ));
    a.update(key('q'));
    let next = super::run_pane::enter_turn(&mut a);
    assert!(prompts(&mut a, next, 2), "later turn");
}

#[test]
fn enter_ctrl_c_and_ctrl_u_in_the_shell_clear_the_mark() {
    for send in [
        chat_live::plain(KeyCode::Enter),
        chat_live::ctrl('c'),
        chat_live::ctrl('u'),
    ] {
        let (mut a, turn) = marked();
        user_in_the_shell(&mut a);
        a.update(send);
        a.tab_mut().focus = id2();
        assert!(!prompts(&mut a, turn, 2), "cleared");
    }
}

// The rule reads key events, not the bytes the child would see.
#[test]
fn enter_clears_the_mark_also_with_an_extended_keyboard_protocol() {
    let (mut a, turn) = marked();
    shell_output(&mut a, b"\x1b[>1u");
    user_in_the_shell(&mut a);
    a.update(chat_live::plain(KeyCode::Enter));
    a.tab_mut().focus = id2();
    assert!(!prompts(&mut a, turn, 2));
}

#[test]
fn other_keys_leave_the_mark_set() {
    let (mut a, turn) = marked();
    user_in_the_shell(&mut a);
    for other in [
        chat_live::plain(KeyCode::Backspace),
        key('x'),
        chat_live::plain(KeyCode::Left),
        chat_live::ctrl('a'),
    ] {
        a.update(other);
    }
    a.tab_mut().focus = id2();
    assert!(prompts(&mut a, turn, 2));
}

// Without bracketed paste a newline of the paste is turned into `\r` and runs
// the line.
#[test]
fn a_paste_with_a_newline_clears_the_mark_when_paste_mode_is_off() {
    for newline in ["\r", "\n", "\r\n"] {
        let (mut a, turn) = marked();
        user_in_the_shell(&mut a);
        a.update(AppEvent::Paste("abc".into()));
        a.tab_mut().focus = id2();
        assert!(prompts(&mut a, turn, 2), "no newline: still pending");
        user_in_the_shell(&mut a);
        a.update(AppEvent::Paste(format!("abc{newline}def")));
        a.tab_mut().focus = id2();
        assert!(!prompts(&mut a, turn, 3), "{newline:?}");
    }
}

// Inside the paste frame a newline executes nothing: the line is still there.
#[test]
fn a_bracketed_paste_never_clears_the_mark() {
    let (mut a, turn) = marked();
    shell_output(&mut a, b"\x1b[?2004h");
    user_in_the_shell(&mut a);
    for text in ["abc\rdef", "abc\ndef", "abc\r\ndef"] {
        a.update(AppEvent::Paste(text.into()));
    }
    a.tab_mut().focus = id2();
    assert!(prompts(&mut a, turn, 2));
}

#[test]
fn a_denied_result_of_the_marked_call_clears_the_mark() {
    let (mut a, turn) = marked();
    // The runtime refused the write: nothing sits on the line.
    result(&mut a, turn, 1, ToolStatus::Denied);
    assert!(!prompts(&mut a, turn, 2));
}

#[test]
fn an_ok_result_leaves_the_mark() {
    let (mut a, turn) = marked();
    result(&mut a, turn, 1, ToolStatus::Ok);
    assert!(prompts(&mut a, turn, 2));
}

#[test]
fn a_mark_on_one_pane_never_affects_another() {
    let (mut a, turn) = marked();
    press(&mut a, "wv");
    let second = a.focused();
    assert_ne!(second, PaneId::FIRST);
    a.tab_mut().focus = id2();
    a.tab_mut().last_shell = Some(second);
    let effects = announce(&mut a, turn, 2, run("ls"));
    let [Effect::AutoType { target, .. }] = effects.as_slice() else {
        panic!("{effects:?}")
    };
    assert_eq!(*target, second);
}

// Enter after each command lets five be typed and the sixth prompts.
#[test]
fn five_commands_a_turn_with_enter_between_and_the_sixth_prompts() {
    let (mut a, turn) = auto_live();
    for call in 1..=5 {
        let effects = announce(&mut a, turn, call, run("ls"));
        assert!(
            matches!(effects.as_slice(), [Effect::AutoType { .. }]),
            "{call}"
        );
        user_in_the_shell(&mut a);
        a.update(chat_live::plain(KeyCode::Enter));
        a.tab_mut().focus = id2();
    }
    assert_eq!(auto_typed(&a), 5);
    assert!(prompts(&mut a, turn, 6));
}

// 2e-3.6: Prefix a ------------------------------------------------------------

fn lines(a: &App) -> Vec<String> {
    use crate::core::ai::chat::Entry;
    run_pane::chat(a, id2())
        .entries()
        .iter()
        .filter_map(|e| match e {
            Entry::System(line) => Some(line.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn prefix_a_flips_the_mode_of_the_focused_chat_and_says_so() {
    let (mut a, _) = live();
    assert_eq!(press(&mut a, "a"), vec![]);
    assert_eq!(chat_of_focus(&a), ApprovalMode::Auto);
    assert!(
        lines(&a).contains(&"approval: auto (commands are typed, never run)".to_owned()),
        "{:?}",
        lines(&a)
    );
    press(&mut a, "a");
    assert_eq!(chat_of_focus(&a), ApprovalMode::Ask);
    assert_eq!(lines(&a).last().map(String::as_str), Some("approval: ask"));
}

// The toggle flips only the focused chat.
#[test]
fn the_toggle_leaves_the_other_chats_alone() {
    let (mut a, _) = live();
    press(&mut a, "c");
    let second = a.focused();
    assert_ne!(second, id2());
    press(&mut a, "a");
    assert_eq!(
        run_pane::chat(&a, second).approval_mode(),
        ApprovalMode::Auto
    );
    assert_eq!(run_pane::chat(&a, id2()).approval_mode(), ApprovalMode::Ask);
}

// A pending approval is never answered by the mode.
#[test]
fn toggling_to_auto_leaves_a_pending_confirm_pending() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, run("cd x"));
    assert_eq!(press(&mut a, "a"), vec![]);
    assert_eq!(chat_of_focus(&a), ApprovalMode::Auto);
    assert_eq!(pending(&a), Some((run("cd x"), Some((turn, CallId(1))))));
}

#[test]
fn a_shell_pane_or_tools_off_only_shows_a_notice() {
    let mut a = launched(80, 25).with_ai(chat_live::ollama());
    assert_eq!(press(&mut a, "a"), vec![]);
    assert!(
        a.notice().is_some_and(|n| n.contains("chat")),
        "{:?}",
        a.notice()
    );

    let mut ai = chat_live::ollama();
    ai.set_tools_enabled(false);
    let mut a = launched(80, 25).with_ai(ai);
    press(&mut a, "c");
    assert_eq!(press(&mut a, "a"), vec![]);
    assert!(
        a.notice().is_some_and(|n| n.contains("Tools")),
        "{:?}",
        a.notice()
    );
    assert_eq!(chat_of_focus(&a), ApprovalMode::Ask);
    assert!(lines(&a).is_empty() || !lines(&a).iter().any(|l| l.starts_with("approval")));
}

#[test]
fn a_inside_a_group_toggles_nothing() {
    let (mut a, _) = live();
    press(&mut a, "wa");
    assert_eq!(chat_of_focus(&a), ApprovalMode::Ask);
}
