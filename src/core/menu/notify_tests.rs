//! Tests of the `Notify` row of the AI section, at the `MenuState` level.

use super::ai_tests::{ch, machine, press, selected_label};
use super::footer_tests::hint;
use super::*;
use crate::core::ai::config::{NotifyMode, Profile};

const SETTINGS_HINT: &str = "j/k move · h/l change · s save · Esc cancel";

fn open_with(notify: NotifyMode) -> MenuState {
    let mut ai = AiConfig::default();
    ai.set_notify(notify);
    MenuState::open(&Config::default()).with_ai(ai, Some(machine()))
}

fn on_notify(notify: NotifyMode) -> MenuState {
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
fn the_row_reads_the_mode_and_needs_the_ai_section() {
    for mode in NotifyMode::ALL {
        assert_eq!(
            value_of(&open_with(mode), "Notify").as_deref(),
            Some(mode.id())
        );
    }
    let plain_menu = MenuState::open(&Config::default());
    assert_eq!(value_of(&plain_menu, "Notify"), None, "no AI, no row");
}

#[test]
fn forward_keys_step_the_mode_and_emit_nothing() {
    for code in [
        KeyCode::Char('l'),
        KeyCode::Right,
        KeyCode::Char(' '),
        KeyCode::Enter,
    ] {
        let mut m = on_notify(NotifyMode::Off);
        assert_eq!(send(&mut m, code, false), MenuCommand::None, "{code:?}");
        assert_eq!(m.ai_draft().notify(), NotifyMode::Auto, "{code:?}");
        assert_eq!(value_of(&m, "Notify").as_deref(), Some("auto"));
        send(&mut m, code, false);
        assert_eq!(m.ai_draft().notify(), NotifyMode::Desktop, "{code:?}");
    }
}

#[test]
fn backward_keys_step_the_other_way() {
    for code in [KeyCode::Char('h'), KeyCode::Left] {
        let mut m = on_notify(NotifyMode::Off);
        send(&mut m, code, false);
        assert_eq!(m.ai_draft().notify(), NotifyMode::Bell, "{code:?}");
    }
}

#[test]
fn a_repeat_steps_except_on_enter() {
    let mut m = on_notify(NotifyMode::Off);
    send(&mut m, KeyCode::Char('l'), true);
    assert_eq!(m.ai_draft().notify(), NotifyMode::Auto);
    let mut m = on_notify(NotifyMode::Off);
    assert_eq!(send(&mut m, KeyCode::Enter, true), MenuCommand::None);
    assert_eq!(
        m.ai_draft().notify(),
        NotifyMode::Off,
        "Enter repeat is ignored"
    );
}

#[test]
fn toggling_touches_neither_the_live_config_nor_the_other_drafts() {
    let mut m = on_notify(NotifyMode::Off);
    let mut config = Config::default();
    m.on_key(&ch('l'), false, &mut config);
    assert_eq!(config, Config::default());
    assert_eq!(m.ai_draft().profile(), Profile::Programmer);
    assert!(m.ai_draft().context(), "context keeps its default");
    assert!(!m.ai_draft().memory.enabled);
}

#[test]
fn the_footer_is_the_settings_hint_and_r_does_nothing() {
    let mut m = on_notify(NotifyMode::Off);
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
    let mut m = on_notify(NotifyMode::Off);
    assert_eq!(send(&mut m, KeyCode::Char('s'), false), MenuCommand::Save);
    assert_eq!(send(&mut m, KeyCode::Esc, false), MenuCommand::Cancel);
}

// Spec (config-persistence): only a changed value is an edit.
#[test]
fn the_edit_is_the_notify_key_alone_and_only_when_changed() {
    let live = Config::default();
    let mut m = on_notify(NotifyMode::Off);
    assert_eq!(m.edits(&live), vec![]);
    press(&mut m, 'l');
    assert_eq!(m.edits(&live), vec![notify_edit("auto")]);
    for _ in 0..3 {
        press(&mut m, 'l');
    }
    assert_eq!(m.edits(&live), vec![], "back to the open value");
    let mut on = on_notify(NotifyMode::Auto);
    press(&mut on, 'l');
    assert_eq!(on.edits(&live), vec![notify_edit("desktop")]);
}

#[test]
fn the_notify_edit_comes_after_the_context_and_memory_edits() {
    let mut m = on_notify(NotifyMode::Off);
    press(&mut m, 'l');
    press(&mut m, 'k');
    assert_eq!(selected_label(&m), "Memory");
    press(&mut m, 'k');
    assert_eq!(selected_label(&m), "Context");
    press(&mut m, 'l');
    let edits = m.edits(&Config::default());
    assert_eq!(edits.len(), 2);
    assert_eq!(edits[0].key, "context");
    assert_eq!(edits[1], notify_edit("auto"));
}
