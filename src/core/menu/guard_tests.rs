//! Tests of the key guards of the picker: repeats and modifiers.
//! Characterization: the guards already held, these pin them.

use super::ai_tests::{machine, menu, press};
use super::picker_tests::{cursor_label, key, open, send, title};
use super::*;

fn repeat(menu: &mut MenuState, code: KeyCode) -> MenuCommand {
    menu.on_key(&key(code), true, &mut Config::default())
}

fn with(menu: &mut MenuState, code: KeyCode, modifiers: KeyModifiers) -> MenuCommand {
    menu.on_key(
        &KeyEvent::new(code, modifiers),
        false,
        &mut Config::default(),
    )
}

/// The picker for `code` at the models of claude.
fn level_two() -> MenuState {
    let mut m = menu(None, None, Some(machine()));
    open(&mut m, "code");
    press(&mut m, 'j');
    press(&mut m, 'j');
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(title(&m), "Model for code (claude)");
    m
}

fn custom_field() -> MenuState {
    let mut m = level_two();
    while cursor_label(&m) != "custom…" {
        press(&mut m, 'j');
    }
    send(&mut m, key(KeyCode::Enter));
    for c in "abc".chars() {
        press(&mut m, c);
    }
    m
}

fn buffer(menu: &MenuState) -> String {
    match menu.rows(&Config::default()).last() {
        Some(Row::Field(text)) => text.clone(),
        other => panic!("not the field: {other:?}"),
    }
}

// Spec: Repeat types and deletes, but Enter and Esc need a press.
#[test]
fn a_held_enter_or_esc_does_nothing_in_the_lists() {
    let mut m = menu(None, None, Some(machine()));
    open(&mut m, "code");
    press(&mut m, 'j');
    press(&mut m, 'j');
    let before = m.clone();
    for code in [KeyCode::Enter, KeyCode::Esc] {
        assert_eq!(repeat(&mut m, code), MenuCommand::None, "{code:?}");
        assert_eq!(m, before, "level 1, {code:?}");
    }
    send(&mut m, key(KeyCode::Enter));
    let before = m.clone();
    for code in [KeyCode::Enter, KeyCode::Esc] {
        assert_eq!(repeat(&mut m, code), MenuCommand::None, "{code:?}");
        assert_eq!(m, before, "level 2, {code:?}");
    }
    assert_eq!(m.ai_draft(), AiConfig::default(), "nothing committed");
}

#[test]
fn a_held_enter_or_esc_does_nothing_in_the_custom_field() {
    let mut m = custom_field();
    let before = m.clone();
    for code in [KeyCode::Enter, KeyCode::Esc] {
        assert_eq!(repeat(&mut m, code), MenuCommand::None, "{code:?}");
        assert_eq!(m, before, "{code:?}");
    }
    assert_eq!(buffer(&m), "abc");
    assert_eq!(m.ai_draft(), AiConfig::default());
}

// Spec: modified keys are swallowed outside the custom field.
#[test]
fn modified_keys_do_nothing_in_the_picker_lists() {
    let ctrl = KeyModifiers::CONTROL;
    let alt = KeyModifiers::ALT;
    for mut m in [
        {
            let mut m = menu(None, None, Some(machine()));
            open(&mut m, "code");
            m
        },
        level_two(),
    ] {
        let before = m.clone();
        for (code, modifiers) in [
            (KeyCode::Char('j'), ctrl),
            (KeyCode::Char('j'), alt),
            (KeyCode::Char('k'), ctrl),
            (KeyCode::Enter, ctrl),
            (KeyCode::Enter, alt),
            (KeyCode::Esc, ctrl),
            (KeyCode::Char('r'), ctrl),
            (KeyCode::Char('s'), ctrl),
        ] {
            assert_eq!(
                with(&mut m, code, modifiers),
                MenuCommand::None,
                "{code:?} {modifiers:?}"
            );
            assert_eq!(m, before, "{code:?} {modifiers:?}");
        }
    }
}

// Spec: Ctrl+U clears; other combinations do not.
#[test]
fn only_exactly_ctrl_u_clears_the_custom_buffer() {
    let mut m = custom_field();
    for modifiers in [KeyModifiers::ALT, KeyModifiers::CONTROL | KeyModifiers::ALT] {
        with(&mut m, KeyCode::Char('u'), modifiers);
        assert_eq!(buffer(&m), "abc", "{modifiers:?}");
    }
    with(&mut m, KeyCode::Char('u'), KeyModifiers::CONTROL);
    assert_eq!(buffer(&m), "");
}

// Spec: Task picker title for `Set all tasks`.
#[test]
fn the_set_all_picker_is_titled_set_all_tasks() {
    let mut m = menu(None, None, Some(machine()));
    open(&mut m, "Set all tasks");
    assert_eq!(title(&m), "Engine for Set all tasks");
}
