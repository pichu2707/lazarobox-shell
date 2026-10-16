//! Copying the reason behind a degraded engine or a failed model listing.

use super::ai_tests::{ch, detection, engine, menu};
use super::picker_tests::{key, open, send, title};
use super::*;
use crate::core::ai::Status;

const WHY: &str = "jail: no node found to run the CLI script; install it next to the CLI or on PATH (looked in /usr/local/bin and /usr/bin)";

fn codex(models: Option<&str>, jail: Option<&str>) -> Detection {
    let mut codex = engine("codex", Status::Available, &["default"], Some("0.5.0"));
    codex.diagnostics.models = models.map(str::to_owned);
    codex.diagnostics.jail = jail.map(str::to_owned);
    detection(&[codex])
}

fn models_level(d: Detection) -> MenuState {
    let mut m = menu(Some("codex"), None, Some(d));
    open(&mut m, "general");
    assert_eq!(send(&mut m, key(KeyCode::Enter)), MenuCommand::None);
    m
}

fn is_note(m: &MenuState) -> bool {
    m.ai.as_ref()
        .and_then(AiMenu::cursor_row)
        .is_some_and(|r| matches!(r.key, PickKey::Note(_)))
}

fn go_to_note(m: &mut MenuState) {
    while !is_note(m) {
        send(m, key(KeyCode::Char('j')));
    }
}

#[test]
fn enter_and_y_on_the_note_copy_the_full_reason() {
    let full = format!("model list unavailable: {WHY}");
    for copy in [key(KeyCode::Enter), ch('y')] {
        let mut m = models_level(codex(Some(WHY), None));
        go_to_note(&mut m);
        let before = m.ai_draft();
        assert_eq!(send(&mut m, copy), MenuCommand::Copy(full.clone()));
        assert_eq!(m.ai_draft(), before, "the saved model never changes");
        // Still on the models level, on the note.
        assert!(is_note(&m));
    }
}

#[test]
fn the_footer_on_the_note_says_how_to_copy() {
    let mut m = models_level(codex(Some(WHY), None));
    go_to_note(&mut m);
    let Footer::Hint(hint) = m.footer() else {
        panic!("not a hint")
    };
    assert!(hint.contains("Enter or y copies"), "{hint}");
}

#[test]
fn y_on_a_model_row_does_nothing() {
    let mut m = models_level(codex(Some(WHY), None));
    assert!(!is_note(&m));
    assert_eq!(send(&mut m, ch('y')), MenuCommand::None);
}

#[test]
fn y_on_an_engine_row_copies_its_no_jail_reason_and_enter_still_picks_it() {
    let mut m = menu(Some("codex"), None, Some(codex(None, Some(WHY))));
    open(&mut m, "general");
    assert_eq!(send(&mut m, ch('y')), MenuCommand::Copy(WHY.to_owned()));
    let Footer::Hint(hint) = m.footer() else {
        panic!("not a hint")
    };
    assert!(hint.contains("y copies the reason"), "{hint}");
    assert_eq!(send(&mut m, key(KeyCode::Enter)), MenuCommand::None);
    assert!(title(&m).starts_with("Model for"), "{}", title(&m));
}

#[test]
fn y_outside_a_picker_is_swallowed_and_the_copy_note_clears_on_the_next_key() {
    let mut m = menu(Some("codex"), None, Some(codex(None, None)));
    assert_eq!(send(&mut m, ch('y')), MenuCommand::None);
    m.set_copied("Copied 3 characters".to_owned());
    let Footer::Hint(hint) = m.footer() else {
        panic!("not a hint")
    };
    assert_eq!(hint, "Copied 3 characters");
    send(&mut m, key(KeyCode::Char('j')));
    let Footer::Hint(hint) = m.footer() else {
        panic!("not a hint")
    };
    assert_ne!(hint, "Copied 3 characters");
}
