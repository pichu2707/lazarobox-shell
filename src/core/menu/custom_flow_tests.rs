//! Tests of the custom model field and of the refresh key in the picker.

use super::ai_tests::{ch, machine, menu, press};
use super::picker_tests::{cursor_label, key, open, send, title};
use super::*;

/// The picker for `code` with the custom field of `claude` focused.
fn field() -> MenuState {
    let mut m = menu(None, None, Some(machine()));
    open(&mut m, "code");
    press(&mut m, 'j');
    press(&mut m, 'j');
    send(&mut m, key(KeyCode::Enter));
    while cursor_label(&m) != "custom…" {
        press(&mut m, 'j');
    }
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(buffer(&m), "");
    m
}

fn buffer(menu: &MenuState) -> String {
    match menu.rows(&Config::default()).last() {
        Some(Row::Field(text)) => text.clone(),
        other => panic!("not the field: {other:?}"),
    }
}

fn type_all(menu: &mut MenuState, text: &str) {
    for c in text.chars() {
        assert_eq!(send(menu, ch(c)), MenuCommand::None, "{c}");
    }
}

// Spec: Type and accept; `s` is a letter in the field.
#[test]
fn typing_in_the_field_including_shortcut_letters_then_enter_assigns() {
    let mut m = field();
    assert_eq!(title(&m), "Model for code (claude)");
    type_all(&mut m, "ms-1");
    assert_eq!(buffer(&m), "ms-1");
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(m.ai_draft().label(Task::Code), "claude › ms-1");
    assert_eq!(selected_task(&m), "code");
}

fn selected_task(menu: &MenuState) -> &'static str {
    super::ai_tests::selected_label(menu)
}

#[test]
fn every_shortcut_letter_is_typed_and_backspace_deletes() {
    let mut m = field();
    type_all(&mut m, "jkhlrs");
    assert_eq!(buffer(&m), "jkhlrs");
    send(&mut m, key(KeyCode::Backspace));
    assert_eq!(buffer(&m), "jkhlr");
    assert_eq!(
        m.ai_draft(),
        AiConfig::default(),
        "nothing saved or assigned"
    );
}

// A terminal that sends ^H for Backspace delivers Ctrl+H.
#[test]
fn ctrl_h_deletes_like_backspace_in_the_field() {
    let mut m = field();
    type_all(&mut m, "ab");
    send(
        &mut m,
        KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL),
    );
    assert_eq!(buffer(&m), "a");
}

// Spec: Ctrl+U clears; Ctrl+Space and other modified keys are swallowed.
#[test]
fn ctrl_u_clears_and_other_modified_keys_are_swallowed() {
    let mut m = field();
    type_all(&mut m, "abc");
    let ctrl = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
    send(&mut m, ctrl(' '));
    send(&mut m, ctrl('x'));
    send(&mut m, KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT));
    assert_eq!(buffer(&m), "abc");
    send(&mut m, ctrl('u'));
    assert_eq!(buffer(&m), "");
}

// Spec: Blank entry refused.
#[test]
fn a_blank_entry_is_refused_and_space_is_not_typed() {
    let mut m = field();
    type_all(&mut m, "   ");
    assert_eq!(buffer(&m), "");
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(buffer(&m), "", "the field stays focused");
    assert_eq!(m.ai_draft(), AiConfig::default());
}

// Spec: Esc leaves the field.
#[test]
fn esc_leaves_the_field_on_custom_and_reopening_shows_it_empty() {
    let mut m = field();
    type_all(&mut m, "abc");
    assert_eq!(send(&mut m, key(KeyCode::Esc)), MenuCommand::None);
    assert_eq!(title(&m), "Model for code (claude)");
    assert_eq!(cursor_label(&m), "custom…");
    assert_eq!(m.ai_draft(), AiConfig::default());
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(buffer(&m), "");
}

// Spec: Paste goes to the field; paste elsewhere ignored.
#[test]
fn a_paste_goes_to_the_field_only() {
    let mut m = field();
    type_all(&mut m, "ab");
    m.on_paste("cd\nef");
    assert_eq!(buffer(&m), "abcd");

    let mut list = menu(None, None, Some(machine()));
    let before = list.clone();
    list.on_paste("x");
    assert_eq!(list, before, "task list");
    open(&mut list, "code");
    let before = list.clone();
    list.on_paste("x");
    assert_eq!(list, before, "level 1");
}

// Spec: Refresh key at levels 1 and 2, typed in the field.
#[test]
fn r_refreshes_at_levels_one_and_two_but_is_typed_in_the_field() {
    let mut m = menu(None, None, Some(machine()));
    open(&mut m, "code");
    assert_eq!(press(&mut m, 'r'), MenuCommand::Refresh, "level 1");
    assert_eq!(
        m.on_key(&ch('r'), true, &mut Config::default()),
        MenuCommand::None,
        "repeat"
    );
    press(&mut m, 'j');
    press(&mut m, 'j');
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(press(&mut m, 'r'), MenuCommand::Refresh, "level 2");
    assert_eq!(title(&m), "Model for code (claude)");
    let mut typed = field();
    assert_eq!(press(&mut typed, 'r'), MenuCommand::None);
    assert_eq!(buffer(&typed), "r");
}
