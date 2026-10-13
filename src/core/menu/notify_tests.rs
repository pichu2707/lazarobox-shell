//! Tests of the `Notify` row of the AI section, at the `MenuState` level.

use super::ai_tests::{ch, machine, press, selected_label};
use super::footer_tests::hint;
use super::*;
use crate::core::ai::config::Profile;

const SETTINGS_HINT: &str = "j/k move · h/l change · s save · Esc cancel";

fn open_with(notify: bool) -> MenuState {
    let mut ai = AiConfig::default();
    ai.set_notify(notify);
    MenuState::open(&Config::default()).with_ai(ai, Some(machine()))
}

fn on_notify(notify: bool) -> MenuState {
    let mut m = open_with(notify);
    while selected_label(&m) != "Notify" {
        press(&mut m, 'j');
    }
    m
}

fn send(menu: &mut MenuState, code: KeyCode, repeat: bool) -> MenuCommand {
    let key = KeyEvent::new(code, KeyModifiers::NONE);
    menu.on_key(&key, repeat, &mut Config::default())
}

fn value_of(menu: &MenuState, wanted: &str) -> Option<String> {
    menu.rows(&Config::default())
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

fn notify_edit(value: &'static str) -> ConfigEdit {
    ConfigEdit {
        table: "ai",
        key: "notify",
        value: value.into(),
    }
}

// Spec (Notify row): the row is last in the AI section and found by kind.
#[test]
fn the_notify_row_is_last_and_found_by_kind() {
    let last = AI_SECTION.items.last().unwrap();
    assert_eq!(last.label, "Notify");
    assert!(matches!(last.kind, ItemKind::Notify));
    let count = AI_SECTION
        .items
        .iter()
        .filter(|i| matches!(i.kind, ItemKind::Notify))
        .count();
    assert_eq!(count, 1);
}

#[test]
fn the_popup_body_grows_with_the_row() {
    // 3 settings sections' rows + their titles + the AI title + the AI rows.
    let settings: usize = SECTIONS.iter().map(|s| s.items.len()).sum();
    assert_eq!(
        AI_BODY,
        SECTIONS.len() + 1 + settings + AI_SECTION.items.len()
    );
    assert_eq!(AI_SECTION.items.len(), 10);
}

#[test]
fn the_row_reads_on_or_off_and_needs_the_ai_section() {
    assert_eq!(
        value_of(&open_with(false), "Notify").as_deref(),
        Some("off")
    );
    assert_eq!(value_of(&open_with(true), "Notify").as_deref(), Some("on"));
    let plain_menu = MenuState::open(&Config::default());
    assert_eq!(value_of(&plain_menu, "Notify"), None, "no AI, no row");
}

#[test]
fn every_cycle_key_toggles_the_draft_and_emits_nothing() {
    for code in [
        KeyCode::Char('l'),
        KeyCode::Right,
        KeyCode::Char(' '),
        KeyCode::Enter,
        KeyCode::Char('h'),
        KeyCode::Left,
    ] {
        let mut m = on_notify(false);
        assert_eq!(send(&mut m, code, false), MenuCommand::None, "{code:?}");
        assert!(m.ai_draft().notify(), "{code:?}");
        assert_eq!(value_of(&m, "Notify").as_deref(), Some("on"));
        send(&mut m, code, false);
        assert!(!m.ai_draft().notify(), "toggles back {code:?}");
    }
}

#[test]
fn a_repeat_toggles_except_on_enter() {
    let mut m = on_notify(false);
    send(&mut m, KeyCode::Char('l'), true);
    assert!(m.ai_draft().notify());
    let mut m = on_notify(false);
    assert_eq!(send(&mut m, KeyCode::Enter, true), MenuCommand::None);
    assert!(!m.ai_draft().notify(), "Enter repeat is ignored");
}

#[test]
fn toggling_touches_neither_the_live_config_nor_the_other_drafts() {
    let mut m = on_notify(false);
    let mut config = Config::default();
    m.on_key(&ch('l'), false, &mut config);
    assert_eq!(config, Config::default());
    assert_eq!(m.ai_draft().profile(), Profile::Programmer);
    assert!(m.ai_draft().context(), "context keeps its default");
    assert!(!m.ai_draft().memory.enabled);
}

#[test]
fn the_footer_is_the_settings_hint_and_r_does_nothing() {
    let mut m = on_notify(false);
    assert_eq!(hint(&m), SETTINGS_HINT);
    let before = m.clone();
    assert_eq!(
        m.on_key(&ch('r'), false, &mut Config::default()),
        MenuCommand::None
    );
    assert_eq!(m, before);
}

#[test]
fn s_saves_and_esc_cancels_on_the_notify_row() {
    let mut m = on_notify(false);
    assert_eq!(send(&mut m, KeyCode::Char('s'), false), MenuCommand::Save);
    assert_eq!(send(&mut m, KeyCode::Esc, false), MenuCommand::Cancel);
}

// Spec (config-persistence): only a changed value is an edit.
#[test]
fn the_edit_is_the_notify_key_alone_and_only_when_changed() {
    let live = Config::default();
    let mut m = on_notify(false);
    assert_eq!(m.edits(&live), vec![]);
    press(&mut m, 'l');
    assert_eq!(m.edits(&live), vec![notify_edit("true")]);
    press(&mut m, 'l');
    assert_eq!(m.edits(&live), vec![], "back to the open value");
    let mut on = on_notify(true);
    press(&mut on, 'l');
    assert_eq!(on.edits(&live), vec![notify_edit("false")]);
}

#[test]
fn the_notify_edit_comes_after_the_context_and_memory_edits() {
    let mut m = on_notify(false);
    press(&mut m, 'l');
    press(&mut m, 'k');
    assert_eq!(selected_label(&m), "Memory");
    press(&mut m, 'k');
    assert_eq!(selected_label(&m), "Context");
    press(&mut m, 'l');
    let edits = m.edits(&Config::default());
    assert_eq!(edits.len(), 2);
    assert_eq!(edits[0].key, "context");
    assert_eq!(edits[1], notify_edit("true"));
}
