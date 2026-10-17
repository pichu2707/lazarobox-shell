//! Tests of the `Profile` row of the AI section, at the `MenuState` level.

use super::ai_tests::{ch, machine, menu, press, selected_label};
use super::footer_tests::hint;
use super::*;
use crate::core::ai::config::{Assignment, Profile};

const SETTINGS_HINT: &str = "j/k move · h/l change · s save · Esc cancel";

fn open_with(profile: Profile, found: Option<Detection>) -> MenuState {
    let mut ai = AiConfig::default();
    ai.set_profile(profile);
    MenuState::open(&Config::default()).with_ai(ai, found)
}

fn on_profile(profile: Profile, found: Option<Detection>) -> MenuState {
    let mut m = open_with(profile, found);
    m.goto("Profile");
    m
}

fn send(menu: &mut MenuState, key: KeyEvent, repeat: bool) -> MenuCommand {
    menu.on_key(&key, repeat, &mut Config::default())
}

fn plain(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
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

fn profile_edit(value: &'static str) -> ConfigEdit {
    ConfigEdit {
        table: "ai",
        key: "profile",
        value: value.into(),
    }
}

// Spec (config-menu): Descriptor integrity; the row is found by kind.
#[test]
fn the_profile_row_follows_the_task_rows() {
    let row = &AI_SECTION.items[6];
    assert_eq!(row.label, "Profile");
    assert!(matches!(row.kind, ItemKind::Profile));
    let kinds = AI_SECTION
        .items
        .iter()
        .filter(|i| matches!(i.kind, ItemKind::Profile))
        .count();
    assert_eq!(kinds, 1);
    assert_eq!(AI_SECTION.items.len(), 10);
}

// Spec (config-menu): Task rows in order, Wrap.
#[test]
fn profile_sits_between_summarize_and_context_in_navigation() {
    let mut m = menu(None, None, Some(machine()));
    m.goto("summarize");
    press(&mut m, 'j');
    assert_eq!(selected_label(&m), "Profile", "after summarize");
    press(&mut m, 'j');
    assert_eq!(selected_label(&m), "Context", "then Context");
    press(&mut m, 'k');
    assert_eq!(selected_label(&m), "Profile");
    press(&mut m, 'k');
    assert_eq!(selected_label(&m), "summarize");
}

// Spec (config-menu): AI section rows.
#[test]
fn the_row_shows_the_profile_id() {
    let m = open_with(Profile::Programmer, None);
    assert_eq!(value_of(&m, "Profile").as_deref(), Some("programmer"));
    let m = open_with(Profile::Analyst, Some(machine()));
    assert_eq!(value_of(&m, "Profile").as_deref(), Some("analyst"));
    let plain_menu = MenuState::open(&Config::default());
    assert_eq!(value_of(&plain_menu, "Profile"), None, "no AI, no row");
}

// Spec (config-menu): Profile row cycles with wrap.
#[test]
fn every_cycle_key_toggles_the_draft_and_emits_nothing() {
    let keys = [
        plain(KeyCode::Char('l')),
        plain(KeyCode::Right),
        plain(KeyCode::Char(' ')),
        plain(KeyCode::Enter),
        plain(KeyCode::Char('h')),
        plain(KeyCode::Left),
    ];
    for key in keys {
        let mut m = on_profile(Profile::Programmer, Some(machine()));
        assert_eq!(send(&mut m, key, false), MenuCommand::None, "{key:?}");
        assert_eq!(m.ai_draft().profile(), Profile::Analyst, "{key:?}");
        assert_eq!(send(&mut m, key, false), MenuCommand::None, "{key:?}");
        assert_eq!(m.ai_draft().profile(), Profile::Programmer, "wraps {key:?}");
        assert_eq!(value_of(&m, "Profile").as_deref(), Some("programmer"));
    }
}

// Spec (config-menu): Repeat is handled like Press except on Enter.
#[test]
fn a_repeat_cycles_except_on_enter() {
    for code in [
        KeyCode::Char('l'),
        KeyCode::Char('h'),
        KeyCode::Right,
        KeyCode::Left,
    ] {
        let mut m = on_profile(Profile::Programmer, Some(machine()));
        send(&mut m, plain(code), true);
        assert_eq!(m.ai_draft().profile(), Profile::Analyst, "{code:?}");
    }
    let mut m = on_profile(Profile::Programmer, Some(machine()));
    assert_eq!(send(&mut m, plain(KeyCode::Enter), true), MenuCommand::None);
    assert_eq!(m.ai_draft().profile(), Profile::Programmer, "Enter repeat");
}

// Spec (config-menu): Footer on the Profile row.
#[test]
fn the_footer_is_the_settings_hint_whatever_the_detection_state() {
    let mut waiting = on_profile(Profile::Programmer, None);
    assert_eq!(hint(&waiting), SETTINGS_HINT, "detection in flight");
    waiting.set_checking(true);
    assert_eq!(hint(&waiting), SETTINGS_HINT, "refreshing");
    let done = on_profile(Profile::Analyst, Some(machine()));
    assert_eq!(hint(&done), SETTINGS_HINT);
}

// Spec (config-menu): r has no effect on the Profile row.
#[test]
fn r_does_nothing_on_the_profile_row() {
    let mut m = on_profile(Profile::Analyst, Some(machine()));
    let before = m.clone();
    assert_eq!(send(&mut m, ch('r'), false), MenuCommand::None);
    assert_eq!(m, before);
}

// Spec (config-menu): no live effect, no side effects on the tasks.
#[test]
fn cycling_touches_no_task_and_the_live_config() {
    let mut m = on_profile(Profile::Programmer, Some(machine()));
    let mut config = Config::default();
    m.on_key(&ch('l'), false, &mut config);
    assert_eq!(config, Config::default());
    let draft = m.ai_draft();
    for task in Task::ALL {
        assert_eq!(draft.get(task), &Assignment::default(), "{}", task.id());
    }
    assert!(m.ai.as_ref().unwrap().picker().is_none());
}

// Spec (config-menu): s saves; Esc cancels.
#[test]
fn s_saves_and_esc_cancels_on_the_profile_row() {
    let mut m = on_profile(Profile::Programmer, Some(machine()));
    assert_eq!(send(&mut m, ch('s'), false), MenuCommand::Save);
    assert_eq!(
        send(&mut m, plain(KeyCode::Esc), false),
        MenuCommand::Cancel
    );
}

// Spec (config-menu, config-persistence): only a changed profile is an edit,
// and nothing else is written with it.
#[test]
fn the_edit_is_the_profile_key_alone_and_only_when_changed() {
    let live = Config::default();
    let mut m = on_profile(Profile::Programmer, Some(machine()));
    assert_eq!(m.edits(&live), vec![]);
    press(&mut m, 'l');
    assert_eq!(m.edits(&live), vec![profile_edit("analyst")]);
    press(&mut m, 'l');
    assert_eq!(m.edits(&live), vec![], "back to the open value");
}

// Spec (config-persistence): the default is written explicitly.
#[test]
fn changing_back_to_the_default_is_written_explicitly() {
    let mut m = on_profile(Profile::Analyst, Some(machine()));
    press(&mut m, 'l');
    assert_eq!(
        m.edits(&Config::default()),
        vec![profile_edit("programmer")]
    );
}

#[test]
fn the_profile_edit_joins_the_position_edits() {
    let mut m = on_profile(Profile::Programmer, Some(machine()));
    press(&mut m, 'l');
    let mut live = Config::default();
    live.bars.statusline = BarPosition::Top;
    assert_eq!(
        m.edits(&live),
        vec![
            ConfigEdit {
                table: "statusline",
                key: "position",
                value: "top".into()
            },
            profile_edit("analyst"),
        ]
    );
}
