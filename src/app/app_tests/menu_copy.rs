//! Copying the reason behind a failed model listing or a degraded engine
//! from the AI picker.

use super::*;
use crate::core::{
    ai::{CATALOG, Detection, Diagnostics, EngineState, Status, config::AiConfig},
    copy::osc52,
    menu::Footer,
};

const WHY: &str =
    "jail: no node found to run the CLI script; install it next to the CLI or on PATH";

fn enter() -> AppEvent {
    AppEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
}

fn codex(models: Option<&str>, jail: Option<&str>) -> Detection {
    Detection {
        engines: CATALOG
            .iter()
            .map(|e| EngineState {
                binary: None,
                id: e.id,
                status: if e.id == "codex" {
                    Status::Available
                } else {
                    Status::NotInstalled
                },
                models: if e.id == "codex" {
                    vec!["default".to_owned()]
                } else {
                    Vec::new()
                },
                version: None,
                model_labels: Vec::new(),
                diagnostics: Diagnostics {
                    models: models.filter(|_| e.id == "codex").map(str::to_owned),
                    jail: jail.filter(|_| e.id == "codex").map(str::to_owned),
                    ..Default::default()
                },
            })
            .collect(),
        ..Default::default()
    }
}

/// The menu open on the general task's engine list (codex assigned).
fn picker_app(d: Detection) -> App {
    let mut a = app();
    a.menu =
        Some(MenuState::open(&a.config).with_ai(AiConfig::general(Some("codex"), None), Some(d)));
    a.input = InputMode::Menu;
    goto_row(&mut a, "general");
    a.update(enter());
    a
}

#[test]
fn y_on_an_engine_row_with_a_no_jail_reason_sets_the_clipboard_and_says_so() {
    let mut a = picker_app(codex(None, Some(WHY)));
    let effects = a.update(key('y'));
    assert_eq!(effects, vec![Effect::SetClipboard(osc52(WHY).unwrap())]);
    let hint = a.menu().unwrap().footer();
    let Footer::Hint(hint) = hint else {
        panic!("not a hint")
    };
    assert_eq!(hint, format!("Copied {} characters", WHY.chars().count()));
}

#[test]
fn enter_on_the_listing_note_copies_the_full_reason_and_keeps_the_menu_open() {
    let mut a = picker_app(codex(Some(WHY), None));
    a.update(enter()); // codex -> its models
    for _ in 0..5 {
        let effects = a.update(key('j'));
        assert!(effects.is_empty());
    }
    let full = format!("model list unavailable: {WHY}");
    let mut copied = false;
    for _ in 0..6 {
        let effects = a.update(enter());
        if effects == vec![Effect::SetClipboard(osc52(&full).unwrap())] {
            copied = true;
            break;
        }
        a.update(key('j'));
    }
    assert!(copied, "no row copied the note");
    assert_eq!(a.input(), InputMode::Menu);
}

#[test]
fn a_reason_too_large_for_osc52_says_so_and_sets_nothing() {
    let mut a = picker_app(codex(None, Some(&"x".repeat(80_000))));
    assert_eq!(a.update(key('y')), Vec::new());
    assert_eq!(
        a.menu().unwrap().footer(),
        Footer::Error("Too large to copy")
    );
}
