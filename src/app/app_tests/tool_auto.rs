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
