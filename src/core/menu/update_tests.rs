//! The update note of the menu: shown on the task list only.

use super::ai_tests::machine;
use super::*;
use crate::core::update::Fallback;

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
        "lazarobox 0.16.0 is available. Go to Updates and press Enter."
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
    assert!(m.update_note().is_some(), "on the Settings tab");
    m.goto("code");
    assert_eq!(m.update_note(), None, "gone on the AI tab");
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
    m.goto("Updates");
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

fn press_key(m: &mut MenuState, code: KeyCode) -> MenuCommand {
    m.on_key(
        &KeyEvent::new(code, KeyModifiers::NONE),
        false,
        &mut Config::default(),
    )
}

fn on_updates(update: Option<&str>) -> MenuState {
    let mut m = menu().with_update(update.map(str::to_owned));
    go_to_updates(&mut m);
    m
}

fn footer_text(m: &MenuState) -> String {
    match m.footer() {
        Footer::Hint(text) => text.into_owned(),
        Footer::Error(text) => text.to_owned(),
    }
}

#[test]
fn enter_without_an_update_does_nothing_and_the_footer_says_up_to_date() {
    let mut m = on_updates(None);
    assert!(
        footer_text(&m).starts_with("up to date"),
        "{}",
        footer_text(&m)
    );
    let before = m.clone();
    assert_eq!(press_key(&mut m, KeyCode::Enter), MenuCommand::None);
    assert_eq!(press_key(&mut m, KeyCode::Char('y')), MenuCommand::None);
    assert_eq!(m, before);
}

#[test]
fn enter_with_an_update_asks_the_app_to_update() {
    let mut m = on_updates(Some("0.15.4"));
    assert!(
        footer_text(&m).starts_with("Enter updates"),
        "{}",
        footer_text(&m)
    );
    assert_eq!(press_key(&mut m, KeyCode::Enter), MenuCommand::Update);
}

#[test]
fn a_held_enter_does_not_ask_twice() {
    let mut m = on_updates(Some("0.15.4"));
    let held = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(
        m.on_key(&held, true, &mut Config::default()),
        MenuCommand::None
    );
}

#[test]
fn y_on_the_row_does_not_update_before_the_command_is_shown() {
    let mut m = on_updates(Some("0.15.4"));
    assert_eq!(press_key(&mut m, KeyCode::Char('y')), MenuCommand::None);
}

#[test]
fn once_shown_enter_and_y_copy_the_command() {
    for code in [KeyCode::Enter, KeyCode::Char('y')] {
        let mut m = on_updates(Some("0.15.4"));
        m.show_update_command(Fallback::NotCargoInstall);
        assert_eq!(
            footer_text(&m),
            "Enter or y copies it \u{b7} not installed with cargo"
        );
        assert_eq!(
            press_key(&mut m, code),
            MenuCommand::Copy("cargo install lazarobox-shell --locked".to_owned()),
            "{code:?}"
        );
        // Still shown: a second copy is possible.
        assert_eq!(
            press_key(&mut m, code),
            MenuCommand::Copy("cargo install lazarobox-shell --locked".to_owned())
        );
    }
}

#[test]
fn the_shown_command_says_why_it_was_not_run_on_one_line() {
    let cases = [
        (Fallback::NotCargoInstall, "not installed with cargo"),
        (Fallback::NoCargo, "cargo is not installed"),
        (Fallback::UntrustedCargo, "cargo folder not safe"),
    ];
    for (why, reason) in cases {
        let mut m = on_updates(Some("0.15.4"));
        m.show_update_command(why);
        let text = footer_text(&m);
        // The action first, the reason after it.
        assert_eq!(text, format!("Enter or y copies it \u{b7} {reason}"));
        // The popup is 51 columns with its borders and a leading space.
        assert!(text.chars().count() <= 48, "{text}");
        // Two lines above the footer say it cannot update itself, and give
        // the command to copy.
        let note = m.update_note().unwrap();
        assert_eq!(note[0], "lazarobox cannot update itself here. Run:");
        assert_eq!(note[1], "cargo install lazarobox-shell --locked");
    }
    // Any key hides the explanation and brings the usual note back.
    let mut m = on_updates(Some("0.15.4"));
    m.show_update_command(Fallback::NoCargo);
    press_key(&mut m, KeyCode::Char('k'));
    assert_eq!(
        m.update_note().unwrap().join(" "),
        "lazarobox 0.15.4 is available. Go to Updates and press Enter."
    );
}

#[test]
fn moving_away_hides_the_shown_command() {
    let mut m = on_updates(Some("0.15.4"));
    m.show_update_command(Fallback::NotCargoInstall);
    press_key(&mut m, KeyCode::Char('k'));
    press_key(&mut m, KeyCode::Char('j'));
    assert!(footer_text(&m).starts_with("Enter updates"));
    assert_eq!(press_key(&mut m, KeyCode::Enter), MenuCommand::Update);
}

#[test]
fn after_an_install_the_row_reads_installed_until_restart() {
    let mut m = on_updates(Some("0.15.4"));
    m.set_installed(Some("0.15.4".into()));
    m.set_update(None);
    assert_eq!(
        updates_row(&m),
        ("Updates", "0.15.4 installed: restart".to_owned())
    );
    assert_eq!(press_key(&mut m, KeyCode::Enter), MenuCommand::None);
    assert!(
        footer_text(&m).starts_with("Updated to 0.15.4"),
        "{}",
        footer_text(&m)
    );
}

#[test]
fn a_menu_opened_after_the_install_starts_with_the_installed_value() {
    let m = menu().with_installed(Some("0.15.4".into()));
    assert_eq!(
        updates_row(&m),
        ("Updates", "0.15.4 installed: restart".to_owned())
    );
}
