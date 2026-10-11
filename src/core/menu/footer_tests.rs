//! Tests of the footer text at the `MenuState` level.

use super::ai_tests::{machine, menu, press, selected_label};
use super::picker_tests::{cursor_label, key, open, send};
use super::*;

pub(super) fn hint(menu: &MenuState) -> String {
    match menu.footer() {
        Footer::Hint(text) => text.into_owned(),
        Footer::Error(text) => panic!("an error: {text}"),
    }
}

const SETTINGS: &str = "j/k move · h/l change · s save · Esc cancel";
const TASK: &str = "j/k move · Enter pick · r refresh · s save · Esc cancel";
const PICKER: &str = "j/k move · Enter choose · r refresh · Esc back · s save";
const CUSTOM: &str = "type a model · Enter ok · Ctrl+U clear · Esc back";

// Spec: Footer hint.
#[test]
fn a_settings_row_shows_the_settings_hint() {
    let mut m = menu(None, None, Some(machine()));
    assert_eq!(hint(&m), SETTINGS);
    press(&mut m, 'j');
    assert_eq!(hint(&m), SETTINGS);
}

// Spec: Footer hint on AI row.
#[test]
fn every_task_row_shows_the_task_hint() {
    let mut m = menu(None, None, Some(machine()));
    while selected_label(&m) != "Set all tasks" {
        press(&mut m, 'j');
    }
    for label in [
        "Set all tasks",
        "general",
        "code",
        "chat",
        "review",
        "summarize",
    ] {
        assert_eq!(selected_label(&m), label);
        assert_eq!(hint(&m), TASK, "{label}");
        press(&mut m, 'j');
    }
    assert_eq!(hint(&m), SETTINGS, "wrapped to the first Settings row");
}

// Spec: Footer hints in the picker.
#[test]
fn both_picker_levels_and_the_custom_field_have_their_hint() {
    let mut m = menu(None, None, Some(machine()));
    open(&mut m, "code");
    assert_eq!(hint(&m), PICKER, "level 1");
    press(&mut m, 'j');
    press(&mut m, 'j');
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(hint(&m), PICKER, "level 2");
    while cursor_label(&m) != "custom…" {
        press(&mut m, 'j');
    }
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(hint(&m), CUSTOM);
    send(&mut m, key(KeyCode::Esc));
    assert_eq!(hint(&m), PICKER, "back on level 2");
}

// Spec: Error replaces hint; the next press restores it.
#[test]
fn an_error_replaces_the_hint_until_the_next_press() {
    let mut m = menu(None, None, Some(machine()));
    m.set_error("save failed: disk full".to_owned());
    assert_eq!(m.footer(), Footer::Error("save failed: disk full"));
    press(&mut m, 'x');
    assert_eq!(hint(&m), SETTINGS);
    open(&mut m, "code");
    m.set_error("boom".to_owned());
    assert_eq!(m.footer(), Footer::Error("boom"), "inside the picker");
}
