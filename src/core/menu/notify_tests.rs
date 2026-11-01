//! Tests of the `Notify` row of the AI section, at the `MenuState` level.

use super::ai_tests::{ch, machine, press, selected_label};
use super::footer_tests::hint;
use super::*;
use crate::core::ai::config::{NotifyMode, Profile};

const NOTIFY_HINT: &str = "j/k move · h/l change · t test · s save · Esc cancel";

fn open_with(notify: NotifyMode) -> MenuState {
    let mut ai = AiConfig::default();
    ai.set_notify(notify);
    MenuState::open(&Config::default()).with_ai(ai, Some(machine()))
}

fn on_notify(notify: NotifyMode) -> MenuState {
    let mut m = open_with(notify);
    m.goto("Notify");
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

// Spec (Notify row): the rows sit in Settings, after Mouse, found by kind.
#[test]
fn the_notify_rows_follow_mouse_in_settings_and_leave_the_ai_section() {
    let items = SECTIONS[0].items;
    let labels: Vec<_> = items.iter().map(|i| i.label).collect();
    assert_eq!(
        labels,
        [
            "Statusline position",
            "Tab bar position",
            "Mouse",
            "Notify",
            "Sound",
            "llama-server idle",
            "Updates",
            "Feedback"
        ]
    );
    assert!(matches!(items[3].kind, ItemKind::Notify));
    assert!(matches!(&items[4].kind, ItemKind::Draft(c) if c.test));
    assert!(
        [&MODELS_SECTION, &TOOLS_SECTION, &CHAT_SECTION]
            .iter()
            .flat_map(|s| s.items.iter())
            .all(|i| !matches!(&i.kind, ItemKind::Notify)
                && !matches!(&i.kind, ItemKind::Draft(c) if c.test))
    );
}

#[test]
fn the_tabs_hold_eight_settings_rows_and_nineteen_ai_rows() {
    let ai: usize = [&MODELS_SECTION, &TOOLS_SECTION, &CHAT_SECTION]
        .iter()
        .map(|s| s.items.len())
        .sum();
    assert_eq!(ai, 19);
    assert_eq!(SECTIONS[0].items.len(), 8);
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
    assert_eq!(hint(&m), NOTIFY_HINT);
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
    m.goto("Context");
    press(&mut m, 'l');
    let edits = m.edits(&Config::default());
    assert_eq!(edits.len(), 2);
    assert_eq!(edits[0].key, "context");
    assert_eq!(edits[1], notify_edit("auto"));
}

// Sound row: an on/off draft flag, written as a boolean.
fn on_sound(sound: bool) -> MenuState {
    let mut ai = AiConfig::default();
    ai.set_notify_sound(sound);
    let mut m = MenuState::open(&Config::default()).with_ai(ai, Some(machine()));
    m.goto("Sound");
    m
}

fn sound_edit(value: &'static str) -> ConfigEdit {
    ConfigEdit {
        table: "ai",
        key: "notify_sound",
        value: value.into(),
    }
}

#[test]
fn the_sound_row_reads_on_or_off_and_toggles_the_draft() {
    let mut m = on_sound(true);
    assert_eq!(value_of(&m, "Sound").as_deref(), Some("on"));
    press(&mut m, 'l');
    assert_eq!(value_of(&m, "Sound").as_deref(), Some("off"));
    assert!(!m.ai_draft().notify_sound());
    press(&mut m, 'h');
    assert!(m.ai_draft().notify_sound());
}

#[test]
fn the_sound_edit_is_a_boolean_and_only_when_changed() {
    let live = Config::default();
    let mut m = on_sound(true);
    assert_eq!(m.edits(&live), vec![]);
    press(&mut m, 'l');
    assert_eq!(m.edits(&live), vec![sound_edit("false")]);
    assert!(m.edits(&live)[0].is_flag());
    press(&mut m, 'l');
    assert_eq!(m.edits(&live), vec![]);
}

// Test action: `t` on the Notify row fires the mode shown in the row.
#[test]
fn t_on_the_notify_row_tests_the_draft_mode_without_saving() {
    let mut m = on_notify(NotifyMode::Off);
    press(&mut m, 'l');
    assert_eq!(
        send(&mut m, KeyCode::Char('t'), false),
        MenuCommand::TestNotify {
            mode: NotifyMode::Auto,
            sound: true
        }
    );
    assert_eq!(
        m.ai_draft().notify(),
        NotifyMode::Auto,
        "the draft is untouched"
    );
}

#[test]
fn t_carries_the_sound_flag_and_works_from_the_sound_row_too() {
    let mut m = on_notify(NotifyMode::Desktop);
    m.goto("Sound");
    press(&mut m, 'l');
    assert_eq!(
        send(&mut m, KeyCode::Char('t'), false),
        MenuCommand::TestNotify {
            mode: NotifyMode::Desktop,
            sound: false
        }
    );
}

#[test]
fn t_elsewhere_or_on_repeat_does_nothing() {
    let mut m = on_notify(NotifyMode::Auto);
    assert_eq!(send(&mut m, KeyCode::Char('t'), true), MenuCommand::None);
    press(&mut m, 'k');
    assert_ne!(selected_label(&m), "Notify");
    assert_eq!(send(&mut m, KeyCode::Char('t'), false), MenuCommand::None);
    // Nor on the Updates row, which sits right after Sound.
    m.goto("Updates");
    assert_eq!(send(&mut m, KeyCode::Char('t'), false), MenuCommand::None);
}

#[test]
fn the_notify_rows_hint_names_the_test_key() {
    let m = on_notify(NotifyMode::Auto);
    assert_eq!(
        hint(&m),
        "j/k move · h/l change · t test · s save · Esc cancel"
    );
}

#[test]
fn the_test_report_has_its_own_field_and_any_press_clears_it() {
    let mut m = on_notify(NotifyMode::Auto);
    assert_eq!(m.test_report(), None);
    m.set_test_report("sent: bell".to_owned());
    assert_eq!(hint(&m), "sent: bell");
    press(&mut m, 'j');
    assert_eq!(m.test_report(), None, "cleared like the copy notice");
    assert_eq!(hint(&m), NOTIFY_HINT, "the row hint is back");
    // A copy notice is a different field and wins while both are set.
    let mut m = on_notify(NotifyMode::Auto);
    m.set_test_report("sent: bell".to_owned());
    m.set_copied("Copied 3 characters".to_owned());
    assert_eq!(hint(&m), "Copied 3 characters");
}

#[test]
fn a_reopened_menu_has_no_test_report() {
    let mut m = on_notify(NotifyMode::Auto);
    m.set_test_report("sent: bell".to_owned());
    assert_eq!(on_notify(NotifyMode::Auto).test_report(), None);
}
