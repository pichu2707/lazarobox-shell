//! Tests of the `Tools` row of the AI section, at the `MenuState` level.

use super::ai_tests::{ch, machine, press, selected_label};
use super::footer_tests::hint;
use super::*;

const SETTINGS_HINT: &str = "j/k move · h/l change · s save · Esc cancel";

fn open_with(tools: bool) -> MenuState {
    let mut ai = AiConfig::default();
    ai.set_tools_enabled(tools);
    MenuState::open(&Config::default()).with_ai(ai, Some(machine()))
}

fn on_tools(tools: bool) -> MenuState {
    let mut m = open_with(tools);
    m.goto("Tools");
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

fn tools_edit(value: &'static str) -> ConfigEdit {
    ConfigEdit {
        table: "ai.tools",
        key: "enabled",
        value: value.into(),
    }
}

// Spec (agent-tools, config menu): the row is right after Memory; the five
// family rows and then Approval follow it.
#[test]
fn the_tools_row_follows_memory_and_is_found_by_kind() {
    let items = AI_SECTION.items;
    let tools = &items[items.len() - 7];
    assert_eq!(tools.label, "Tools");
    assert!(matches!(tools.kind, ItemKind::Tools));
    assert_eq!(items[items.len() - 8].label, "Memory");
    let count = items
        .iter()
        .filter(|i| matches!(i.kind, ItemKind::Tools))
        .count();
    assert_eq!(count, 1);
}

#[test]
fn navigation_goes_memory_tools_search() {
    let mut m = on_tools(true);
    press(&mut m, 'j');
    assert_eq!(selected_label(&m), "Search");
    press(&mut m, 'k');
    assert_eq!(selected_label(&m), "Tools");
    press(&mut m, 'k');
    assert_eq!(selected_label(&m), "Memory");
}

#[test]
fn the_row_reads_on_or_off_and_needs_the_ai_section() {
    assert_eq!(value_of(&open_with(true), "Tools").as_deref(), Some("on"));
    assert_eq!(value_of(&open_with(false), "Tools").as_deref(), Some("off"));
    let plain_menu = MenuState::open(&Config::default());
    assert_eq!(value_of(&plain_menu, "Tools"), None, "no AI, no row");
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
        let mut m = on_tools(true);
        assert_eq!(send(&mut m, code, false), MenuCommand::None, "{code:?}");
        assert!(!m.ai_draft().tools_enabled(), "{code:?}");
        assert_eq!(value_of(&m, "Tools").as_deref(), Some("off"));
        send(&mut m, code, false);
        assert!(m.ai_draft().tools_enabled(), "toggles back {code:?}");
    }
}

#[test]
fn toggling_touches_neither_the_live_config_nor_the_other_drafts() {
    let mut m = on_tools(true);
    let mut config = Config::default();
    m.on_key(&ch('l'), false, &mut config);
    assert_eq!(config, Config::default());
    assert_eq!(m.ai_draft().notify(), NotifyMode::Off);
    assert!(m.ai_draft().context());
    assert!(!m.ai_draft().memory.enabled);
}

#[test]
fn the_footer_is_the_settings_hint_and_s_saves() {
    let mut m = on_tools(true);
    assert_eq!(hint(&m), SETTINGS_HINT);
    assert_eq!(send(&mut m, KeyCode::Char('s'), false), MenuCommand::Save);
    assert_eq!(send(&mut m, KeyCode::Esc, false), MenuCommand::Cancel);
}

// Spec (config-persistence): only a changed value is an edit, and it is the
// tools key alone: never `ai.mcp.*`.
#[test]
fn the_edit_is_the_tools_key_alone_and_only_when_changed() {
    let live = Config::default();
    let mut m = on_tools(true);
    assert_eq!(m.edits(&live), vec![]);
    press(&mut m, 'l');
    assert_eq!(m.edits(&live), vec![tools_edit("false")]);
    press(&mut m, 'l');
    assert_eq!(m.edits(&live), vec![], "back to the open value");
    let mut off = on_tools(false);
    press(&mut off, 'l');
    assert_eq!(off.edits(&live), vec![tools_edit("true")]);
    assert!(
        off.edits(&live)
            .iter()
            .all(|e| !e.table.starts_with("ai.mcp"))
    );
}

#[test]
fn the_tools_edit_comes_last_after_notify_and_sound() {
    let mut m = on_tools(true);
    press(&mut m, 'l');
    m.goto("Notify");
    press(&mut m, 'l');
    press(&mut m, 'j');
    assert_eq!(selected_label(&m), "Sound");
    press(&mut m, 'l');
    let edits = m.edits(&Config::default());
    assert_eq!(edits.len(), 3);
    assert_eq!(edits[0].key, "notify");
    assert_eq!(edits[1].key, "notify_sound");
    assert_eq!(edits[2], tools_edit("false"));
}
