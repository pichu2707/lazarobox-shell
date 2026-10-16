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
    let without_updates = |m: &MenuState| {
        let mut rows = m.rows(&Config::default());
        rows.retain(|r| !matches!(r, Row::Item { label, .. } if label.ends_with("Updates")));
        rows
    };
    assert_eq!(without_updates(&plain), without_updates(&noted));
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

fn updates_row(m: &MenuState) -> (&'static str, String) {
    m.rows(&Config::default())
        .into_iter()
        .find_map(|row| match row {
            Row::Item { label, value, .. } if label.ends_with("Updates") => {
                Some((label, value.unwrap().into_owned()))
            }
            _ => None,
        })
        .expect("the Updates row")
}

fn go_to_updates(m: &mut MenuState) {
    while !selected_label(m).ends_with("Updates") {
        press(m, 'j');
    }
}

#[test]
fn the_updates_row_shows_the_running_version() {
    assert_eq!(updates_row(&menu()), ("Updates", "0.15.3".to_owned()));
    let other = menu().with_current("1.2.3");
    assert_eq!(updates_row(&other), ("Updates", "1.2.3".to_owned()));
}

#[test]
fn a_known_update_stars_the_name_and_replaces_the_value() {
    let m = menu().with_update(Some("0.15.4".into()));
    assert_eq!(
        updates_row(&m),
        ("* Updates", "0.15.4 available".to_owned())
    );
}

#[test]
fn the_row_follows_the_late_result_of_the_check() {
    let mut m = menu();
    m.set_update(Some("0.15.4".into()));
    assert_eq!(updates_row(&m).0, "* Updates");
    m.set_update(None);
    assert_eq!(updates_row(&m).0, "Updates");
}

#[test]
fn the_row_has_no_toggle_and_no_edit() {
    let mut m = menu().with_update(Some("0.15.4".into()));
    go_to_updates(&mut m);
    let before = m.clone();
    let mut live = Config::default();
    for k in ['h', 'l', ' '] {
        assert_eq!(
            m.on_key(
                &KeyEvent::new(KeyCode::Char(k), KeyModifiers::NONE),
                false,
                &mut live
            ),
            MenuCommand::None
        );
    }
    assert_eq!(m, before);
    assert_eq!(live, Config::default());
    assert!(m.edits(&live).is_empty());
}
