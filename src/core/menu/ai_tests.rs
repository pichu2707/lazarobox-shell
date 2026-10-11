//! Tests of the AI section of the menu, at the `MenuState` level.

use super::*;
use crate::core::ai::{CATALOG, EngineState, Status};

fn engine(id: &'static str, status: Status, models: &[&str], version: Option<&str>) -> EngineState {
    EngineState {
        id,
        status,
        models: models.iter().map(|m| (*m).to_owned()).collect(),
        version: version.map(str::to_owned),
    }
}

/// All eight catalog engines in order; the ones not listed are
/// `NotInstalled`.
fn detection(found: &[EngineState]) -> Detection {
    Detection {
        engines: CATALOG
            .iter()
            .map(|e| {
                found
                    .iter()
                    .find(|s| s.id == e.id)
                    .cloned()
                    .unwrap_or_else(|| engine(e.id, Status::NotInstalled, &[], None))
            })
            .collect(),
    }
}

fn machine() -> Detection {
    detection(&[
        engine(
            "ollama",
            Status::Available,
            &["llama3:8b", "qwen2:7b"],
            None,
        ),
        engine("lmstudio", Status::NotRunning, &[], None),
        engine("claude", Status::Available, &["default"], Some("2.1.0")),
        engine("codex", Status::Available, &["default"], Some("0.5.0")),
    ])
}

fn saved(engine: Option<&str>, model: Option<&str>) -> AiConfig {
    AiConfig {
        engine: engine.map(str::to_owned),
        model: model.map(str::to_owned),
    }
}

fn menu(engine: Option<&str>, model: Option<&str>, found: Option<Detection>) -> MenuState {
    MenuState::open(&Config::default()).with_ai(saved(engine, model), found)
}

fn ch(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn press(menu: &mut MenuState, c: char) -> MenuCommand {
    menu.on_key(&ch(c), false, &mut Config::default())
}

fn text(row: &Row) -> (&'static str, Option<String>) {
    match row {
        Row::Item { label, value, .. } => (label, value.as_deref().map(str::to_owned)),
        Row::Title(title) => (title, None),
    }
}

/// `(engine, model, status)` as shown.
fn shown(menu: &MenuState) -> (String, String, String) {
    let rows = menu.rows(&Config::default());
    let value = |i: usize| text(&rows[i]).1.unwrap();
    (value(5), value(6), value(7))
}

/// Selects the Engine row directly (navigation has its own tests).
fn on_engine(menu: &mut MenuState) {
    menu.selected = Some(2);
}

// Spec: Cycle engines.
#[test]
fn engine_cycles_none_then_available_engines_in_catalog_order() {
    let mut m = menu(Some("ollama"), None, Some(machine()));
    on_engine(&mut m);
    let mut seen = Vec::new();
    for _ in 0..4 {
        assert_eq!(press(&mut m, 'l'), MenuCommand::None);
        seen.push(shown(&m).0);
    }
    assert_eq!(seen, ["claude", "codex", "none", "ollama"]);
    press(&mut m, 'h');
    assert_eq!(shown(&m).0, "none");
}

// Spec: Saved engine not available.
#[test]
fn a_saved_engine_that_is_not_available_is_still_offered() {
    let found = detection(&[engine("ollama", Status::Available, &["m"], None)]);
    let mut m = menu(Some("codex"), None, Some(found));
    on_engine(&mut m);
    assert_eq!(shown(&m).0, "codex (not detected)");
    press(&mut m, 'l');
    assert_eq!(shown(&m).0, "none");
    press(&mut m, 'h');
    assert_eq!(shown(&m).0, "codex (not detected)");
    press(&mut m, 'h');
    assert_eq!(shown(&m).0, "ollama");
}

// Spec: Pending detection.
#[test]
fn a_saved_engine_is_checking_until_the_first_result() {
    let mut m = menu(Some("claude"), None, None);
    on_engine(&mut m);
    assert_eq!(shown(&m).0, "claude (checking)");
    assert_eq!(press(&mut m, 'l'), MenuCommand::None);
    assert_eq!(shown(&m).0, "none");
}

// Spec: No engines at all.
#[test]
fn with_no_engines_and_nothing_saved_the_row_is_none_and_fixed() {
    let mut m = menu(None, None, Some(detection(&[])));
    on_engine(&mut m);
    assert_eq!(shown(&m).0, "none");
    assert_eq!(press(&mut m, 'l'), MenuCommand::None);
    assert_eq!(press(&mut m, 'h'), MenuCommand::None);
    assert_eq!(shown(&m).0, "none");
}
