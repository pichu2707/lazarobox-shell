//! Tests of the `Context` row of the AI section, at the `MenuState` level.

use super::ai_tests::{ch, machine, press, selected_label};
use super::footer_tests::hint;
use super::*;
use crate::core::ai::config::Profile;

const SETTINGS_HINT: &str = "j/k move · h/l change · s save · Esc cancel";

fn open_with(context: bool) -> MenuState {
    let mut ai = AiConfig::default();
    ai.set_context(context);
    MenuState::open(&Config::default()).with_ai(ai, Some(machine()))
}

fn on_context(context: bool) -> MenuState {
    let mut m = open_with(context);
    while selected_label(&m) != "Context" {
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

fn context_edit(value: &'static str) -> ConfigEdit {
    ConfigEdit {
        table: "ai",
        key: "context",
        value: value.into(),
    }
}

// Spec (Context toggle): the row comes right after Profile and is found by kind.
#[test]
fn the_context_row_follows_profile_and_precedes_memory() {
    let labels: Vec<&str> = AI_SECTION.items.iter().map(|i| i.label).collect();
    assert_eq!(
        labels,
        [
            "Set all tasks",
            "general",
            "code",
            "chat",
            "review",
            "summarize",
            "Profile",
            "Context",
            "Memory",
            "Notify"
        ]
    );
    let context = &AI_SECTION.items[AI_SECTION.items.len() - 3];
    assert!(matches!(context.kind, ItemKind::Context));
}

#[test]
fn context_sits_between_profile_and_memory_and_navigation_wraps_through_them() {
    let mut m = open_with(true);
    for _ in 0..9 {
        press(&mut m, 'j');
    }
    assert_eq!(selected_label(&m), "Context");
    press(&mut m, 'j');
    assert_eq!(selected_label(&m), "Memory");
    press(&mut m, 'j');
    assert_eq!(selected_label(&m), "Notify");
    press(&mut m, 'j');
    assert_eq!(selected_label(&m), "Statusline position");
    press(&mut m, 'k');
    assert_eq!(selected_label(&m), "Notify");
    press(&mut m, 'k');
    assert_eq!(selected_label(&m), "Memory");
    press(&mut m, 'k');
    assert_eq!(selected_label(&m), "Context");
    press(&mut m, 'k');
    assert_eq!(selected_label(&m), "Profile");
}

#[test]
fn the_row_reads_on_or_off() {
    assert_eq!(value_of(&open_with(true), "Context").as_deref(), Some("on"));
    assert_eq!(
        value_of(&open_with(false), "Context").as_deref(),
        Some("off")
    );
    let plain_menu = MenuState::open(&Config::default());
    assert_eq!(value_of(&plain_menu, "Context"), None, "no AI, no row");
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
        let mut m = on_context(true);
        assert_eq!(send(&mut m, code, false), MenuCommand::None, "{code:?}");
        assert!(!m.ai_draft().context(), "{code:?}");
        assert_eq!(value_of(&m, "Context").as_deref(), Some("off"));
        send(&mut m, code, false);
        assert!(m.ai_draft().context(), "toggles back {code:?}");
    }
}

#[test]
fn a_repeat_toggles_except_on_enter() {
    let mut m = on_context(true);
    send(&mut m, KeyCode::Char('l'), true);
    assert!(!m.ai_draft().context());
    let mut m = on_context(true);
    assert_eq!(send(&mut m, KeyCode::Enter, true), MenuCommand::None);
    assert!(m.ai_draft().context(), "Enter repeat is ignored");
}

#[test]
fn the_footer_is_the_settings_hint_and_r_does_nothing() {
    let mut m = on_context(true);
    assert_eq!(hint(&m), SETTINGS_HINT);
    let before = m.clone();
    assert_eq!(
        m.on_key(&ch('r'), false, &mut Config::default()),
        MenuCommand::None
    );
    assert_eq!(m, before);
}

#[test]
fn cycling_touches_neither_the_live_config_nor_the_profile() {
    let mut m = on_context(true);
    let mut config = Config::default();
    m.on_key(&ch('l'), false, &mut config);
    assert_eq!(config, Config::default());
    assert_eq!(m.ai_draft().profile(), Profile::Programmer);
}

#[test]
fn s_saves_and_esc_cancels_on_the_context_row() {
    let mut m = on_context(true);
    assert_eq!(send(&mut m, KeyCode::Char('s'), false), MenuCommand::Save);
    assert_eq!(send(&mut m, KeyCode::Esc, false), MenuCommand::Cancel);
}

// Spec (Context toggle, config-persistence): only a changed value is an
// edit, written explicitly, even when it goes back to the default.
#[test]
fn the_edit_is_the_context_key_alone_and_only_when_changed() {
    let live = Config::default();
    let mut m = on_context(true);
    assert_eq!(m.edits(&live), vec![]);
    press(&mut m, 'l');
    assert_eq!(m.edits(&live), vec![context_edit("false")]);
    press(&mut m, 'l');
    assert_eq!(m.edits(&live), vec![], "back to the open value");
    let mut off = on_context(false);
    press(&mut off, 'l');
    assert_eq!(off.edits(&live), vec![context_edit("true")]);
}

#[test]
fn the_context_edit_follows_the_profile_edit() {
    let mut m = on_context(true);
    press(&mut m, 'l');
    press(&mut m, 'k');
    assert_eq!(selected_label(&m), "Profile");
    press(&mut m, 'l');
    let edits = m.edits(&Config::default());
    assert_eq!(edits.len(), 2);
    assert_eq!(edits[0].key, "profile");
    assert_eq!(edits[1], context_edit("false"));
}
