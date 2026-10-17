//! Tests of the save key and of the Enter key at the `MenuState` level.

use super::ai_tests::{ch, machine, menu, press, selected_label};
use super::picker_tests::{cursor_label, key, send, title};
use super::*;

// Spec: Save on s. Every view saves; nothing closes the picker on its own.
#[test]
fn s_saves_from_the_settings_the_task_list_and_both_picker_levels() {
    let mut m = menu(None, None, Some(machine()));
    assert_eq!(selected_label(&m), "Statusline position");
    assert_eq!(send(&mut m, ch('s')), MenuCommand::Save, "settings row");
    m.goto("code");
    assert_eq!(send(&mut m, ch('s')), MenuCommand::Save, "task row");
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(send(&mut m, ch('s')), MenuCommand::Save, "level 1");
    assert_eq!(title(&m), "Engine for code", "a save keeps the picker open");
    press(&mut m, 'j');
    press(&mut m, 'j');
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(title(&m), "Model for code (claude)");
    assert_eq!(send(&mut m, ch('s')), MenuCommand::Save, "level 2");
    assert_eq!(title(&m), "Model for code (claude)");
    assert_eq!(cursor_label(&m), "default (CLI picks)");
}

// Spec: Save on s. The key is a press: not a repeat, not Ctrl+S, not `S`.
#[test]
fn only_a_plain_press_of_s_saves() {
    let mut m = menu(None, None, Some(machine()));
    let mut config = Config::default();
    assert_eq!(m.on_key(&ch('s'), true, &mut config), MenuCommand::None);
    let ctrl_s = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL);
    assert_eq!(m.on_key(&ctrl_s, false, &mut config), MenuCommand::None);
    let upper = KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT);
    assert_eq!(m.on_key(&upper, false, &mut config), MenuCommand::None);
    let mut release = ch('s');
    release.kind = KeyEventKind::Release;
    assert_eq!(m.on_key(&release, false, &mut config), MenuCommand::None);
}

fn on_task_row(label: &str) -> MenuState {
    let mut m = menu(None, None, Some(machine()));
    m.goto(label);
    m
}

// Spec: Enter cycles a Settings row forward, like `l`, and never saves.
#[test]
fn enter_cycles_a_settings_row_forward_and_never_saves() {
    let mut m = menu(None, None, Some(machine()));
    let mut config = Config::default();
    let enter = key(KeyCode::Enter);
    assert_eq!(m.on_key(&enter, false, &mut config), MenuCommand::Changed);
    assert_eq!(config.bars.statusline, BarPosition::Top);
    assert_eq!(
        m.on_key(&enter, true, &mut config),
        MenuCommand::Changed,
        "a repeat cycles like a press"
    );
    assert_eq!(config.bars.statusline, BarPosition::Bottom);
    press(&mut m, 'j');
    assert_eq!(m.on_key(&enter, false, &mut config), MenuCommand::Changed);
    assert_eq!(config.bars.tabbar, BarPosition::Bottom);
}

// Spec: Enter on a task row opens the picker and never saves.
#[test]
fn enter_on_a_task_row_opens_the_picker_and_never_saves() {
    let mut m = on_task_row("code");
    assert_eq!(send(&mut m, key(KeyCode::Enter)), MenuCommand::None);
    assert_eq!(title(&m), "Engine for code");
}

// Spec: Cycling keys do nothing on task rows.
#[test]
fn cycling_keys_do_nothing_on_task_rows() {
    let mut m = on_task_row("code");
    let before = m.clone();
    for k in [
        ch('h'),
        ch('l'),
        ch(' '),
        key(KeyCode::Left),
        key(KeyCode::Right),
    ] {
        assert_eq!(send(&mut m, k), MenuCommand::None, "{k:?}");
    }
    assert_eq!(m, before);
}

// Spec: Unknown key. `r` only acts on a task row press.
#[test]
fn r_acts_only_on_a_task_row_press_and_other_keys_are_swallowed() {
    let mut m = menu(None, None, Some(machine()));
    assert_eq!(press(&mut m, 'r'), MenuCommand::None, "settings row");
    for k in [ch('R'), ch('S'), ch('x'), key(KeyCode::Tab)] {
        assert_eq!(send(&mut m, k), MenuCommand::None, "{k:?}");
    }
    let mut m = on_task_row("code");
    assert_eq!(press(&mut m, 'r'), MenuCommand::Refresh);
    let mut config = Config::default();
    assert_eq!(m.on_key(&ch('r'), true, &mut config), MenuCommand::None);
}
