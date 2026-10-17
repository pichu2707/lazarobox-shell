//! Tests of the `Approval` row of the AI tab, at the `MenuState` level.

use super::ai_tests::{machine, press, selected_label};
use super::footer_tests::hint;
use super::*;
use crate::core::ai::config::ApprovalMode;

const SETTINGS_HINT: &str = "j/k move · h/l change · s save · Esc cancel";

fn open_with(mode: ApprovalMode) -> MenuState {
    let mut ai = AiConfig::default();
    ai.set_approval(mode);
    MenuState::open(&Config::default()).with_ai(ai, Some(machine()))
}

fn on_approval(mode: ApprovalMode) -> MenuState {
    let mut m = open_with(mode);
    m.goto("Approval");
    m
}

fn send(menu: &mut MenuState, code: KeyCode, repeat: bool) -> MenuCommand {
    let key = KeyEvent::new(code, KeyModifiers::NONE);
    menu.on_key(&key, repeat, &mut Config::default())
}

fn value_of(menu: &MenuState, wanted: &str) -> Option<String> {
    menu.all_rows(&Config::default())
        .into_iter()
        .find_map(|row| match row {
            Row::Item {
                label,
                value: Some(value),
                ..
            } if label == wanted => Some(value.into_owned()),
            _ => None,
        })
}

fn approval_edit(value: &'static str) -> ConfigEdit {
    ConfigEdit {
        table: "ai.tools",
        key: "approval",
        value: value.into(),
    }
}

// Spec (agent-tools, config menu): the row is the last of the AI tab, right
// after Tools, and is found by its kind.
#[test]
fn the_approval_row_follows_tools_and_is_found_by_kind() {
    let items = AI_SECTION.items;
    let last = items.last().unwrap();
    assert_eq!(last.label, "Approval");
    assert!(matches!(last.kind, ItemKind::Approval));
    assert_eq!(items[items.len() - 2].label, "Tools");
    let count = items
        .iter()
        .filter(|i| matches!(i.kind, ItemKind::Approval))
        .count();
    assert_eq!(count, 1);
}

#[test]
fn the_ai_tab_holds_eleven_rows() {
    assert_eq!(AI_SECTION.items.len(), 11);
}

#[test]
fn navigation_goes_tools_approval_then_wraps_to_the_first_ai_row() {
    let mut m = on_approval(ApprovalMode::Ask);
    press(&mut m, 'j');
    assert_eq!(
        selected_label(&m),
        "Set all tasks",
        "wraps inside the AI tab"
    );
    press(&mut m, 'k');
    assert_eq!(selected_label(&m), "Approval");
    press(&mut m, 'k');
    assert_eq!(selected_label(&m), "Tools");
    press(&mut m, 'j');
    assert_eq!(selected_label(&m), "Approval");
    assert_eq!(m.active_tab(), 1);
}

#[test]
fn the_row_reads_ask_or_auto_and_needs_the_ai_section() {
    assert_eq!(
        value_of(&open_with(ApprovalMode::Ask), "Approval").as_deref(),
        Some("ask")
    );
    assert_eq!(
        value_of(&open_with(ApprovalMode::Auto), "Approval").as_deref(),
        Some("auto")
    );
    let plain_menu = MenuState::open(&Config::default());
    assert_eq!(value_of(&plain_menu, "Approval"), None, "no AI, no row");
}

#[test]
fn every_cycle_key_flips_the_draft_and_emits_nothing() {
    for code in [
        KeyCode::Char('l'),
        KeyCode::Right,
        KeyCode::Char('h'),
        KeyCode::Left,
    ] {
        let mut m = on_approval(ApprovalMode::Ask);
        assert_eq!(send(&mut m, code, false), MenuCommand::None, "{code:?}");
        assert_eq!(m.ai_draft().approval(), ApprovalMode::Auto, "{code:?}");
        assert_eq!(value_of(&m, "Approval").as_deref(), Some("auto"));
        send(&mut m, code, false);
        assert_eq!(m.ai_draft().approval(), ApprovalMode::Ask, "back {code:?}");
    }
}

#[test]
fn enter_cycles_but_a_repeated_enter_is_ignored() {
    let mut m = on_approval(ApprovalMode::Ask);
    assert_eq!(send(&mut m, KeyCode::Enter, false), MenuCommand::None);
    assert_eq!(m.ai_draft().approval(), ApprovalMode::Auto);
    assert_eq!(send(&mut m, KeyCode::Enter, true), MenuCommand::None);
    assert_eq!(
        m.ai_draft().approval(),
        ApprovalMode::Auto,
        "repeat ignored"
    );
}

#[test]
fn cycling_touches_neither_the_live_config_nor_the_other_drafts() {
    let mut m = on_approval(ApprovalMode::Ask);
    let mut config = Config::default();
    m.on_key(&KeyEvent::from(KeyCode::Char('l')), false, &mut config);
    assert_eq!(config, Config::default());
    assert!(m.ai_draft().tools_enabled());
    assert!(m.ai_draft().context());
    assert!(!m.ai_draft().memory.enabled);
}

#[test]
fn the_footer_is_the_settings_hint_and_s_saves() {
    let mut m = on_approval(ApprovalMode::Ask);
    assert_eq!(hint(&m), SETTINGS_HINT);
    assert_eq!(send(&mut m, KeyCode::Char('s'), false), MenuCommand::Save);
    assert_eq!(send(&mut m, KeyCode::Esc, false), MenuCommand::Cancel);
}

// Spec (config-persistence): only a changed value is an edit, written
// explicitly even when it equals the default, and never `ai.mcp.*`.
#[test]
fn the_edit_is_the_approval_key_alone_and_only_when_changed() {
    let live = Config::default();
    let mut m = on_approval(ApprovalMode::Ask);
    assert_eq!(m.edits(&live), vec![]);
    press(&mut m, 'l');
    assert_eq!(m.edits(&live), vec![approval_edit("auto")]);
    press(&mut m, 'l');
    assert_eq!(m.edits(&live), vec![], "back to the open value");
    let mut auto = on_approval(ApprovalMode::Auto);
    press(&mut auto, 'l');
    assert_eq!(
        auto.edits(&live),
        vec![approval_edit("ask")],
        "the default is written explicitly"
    );
}

#[test]
fn the_approval_edit_comes_right_after_the_tools_edit() {
    let mut m = on_approval(ApprovalMode::Ask);
    press(&mut m, 'l');
    press(&mut m, 'k');
    assert_eq!(selected_label(&m), "Tools");
    press(&mut m, 'l');
    let edits = m.edits(&Config::default());
    assert_eq!(edits.len(), 2);
    assert_eq!(edits[0].key, "enabled");
    assert_eq!(edits[1], approval_edit("auto"));
}
