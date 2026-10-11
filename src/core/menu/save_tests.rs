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
    while selected_label(&m) != "code" {
        press(&mut m, 'j');
    }
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
    assert_eq!(cursor_label(&m), "default");
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
