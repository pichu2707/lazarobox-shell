//! The update note of the menu: shown on the task list only.

use super::ai_tests::{machine, press, selected_label};
use super::*;

fn menu() -> MenuState {
    MenuState::open(&Config::default()).with_ai(AiConfig::default(), Some(machine()))
}

#[test]
fn there_is_no_note_without_an_update() {
    assert_eq!(menu().update_note(), None);
    assert_eq!(menu().with_update(None).update_note(), None);
}

#[test]
fn a_known_update_gives_the_two_note_lines() {
    let note = menu()
        .with_update(Some("0.16.0".into()))
        .update_note()
        .unwrap();
    assert_eq!(
        note.join(" "),
        "update 0.16.0 available \u{2014} run: cargo install lazarobox-shell"
    );
}

#[test]
fn the_note_is_not_a_selectable_row_and_changes_no_edit() {
    let plain = menu();
    let noted = menu().with_update(Some("0.16.0".into()));
    assert_eq!(
        plain.rows(&Config::default()),
        noted.rows(&Config::default())
    );
    assert_eq!(
        plain.edits(&Config::default()),
        noted.edits(&Config::default())
    );
}

#[test]
fn a_late_result_reaches_an_open_menu() {
    let mut m = menu();
    m.set_update(Some("0.16.0".into()));
    assert!(m.update_note().is_some());
    m.set_update(None);
    assert!(m.update_note().is_none());
}

#[test]
fn the_note_hides_while_a_picker_is_open() {
    let mut m = menu().with_update(Some("0.16.0".into()));
    while selected_label(&m) != "code" {
        press(&mut m, 'j');
    }
    assert!(m.update_note().is_some());
    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    m.on_key(&enter, false, &mut Config::default());
    assert!(
        m.rows(&Config::default())
            .iter()
            .any(|r| matches!(r, Row::Pick { .. })),
        "the picker is open"
    );
    assert_eq!(m.update_note(), None);
}
