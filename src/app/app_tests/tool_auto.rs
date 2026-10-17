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
