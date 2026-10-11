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

/// Selects the Model row directly.
fn on_model(menu: &mut MenuState) {
    menu.selected = Some(3);
}

fn pick_engine(menu: &mut MenuState, id: &str) {
    on_engine(menu);
    for _ in 0..8 {
        if shown(menu).0 == id {
            return;
        }
        press(menu, 'l');
    }
    panic!("engine {id} is not offered");
}

// Spec: Status follows the draft engine.
#[test]
fn status_text_follows_the_draft_engine() {
    let status =
        |engine: Option<&str>, found: Option<Detection>| shown(&menu(engine, None, found)).2;
    assert_eq!(status(Some("lmstudio"), Some(machine())), "not running");
    assert_eq!(status(None, Some(machine())), "-");
    assert_eq!(status(Some("claude"), Some(machine())), "available · 2.1.0");
    assert_eq!(
        status(Some("ollama"), Some(machine())),
        "available · 2 models"
    );
    assert_eq!(status(Some("claude"), None), "detecting…");
    assert_eq!(status(Some("future-ai"), Some(machine())), "not detected");
    assert_eq!(
        status(Some("claude"), Some(Detection::default())),
        "unknown"
    );
}

#[test]
fn status_moves_with_the_engine_row() {
    let mut m = menu(Some("ollama"), None, Some(machine()));
    on_engine(&mut m);
    press(&mut m, 'l');
    assert_eq!(shown(&m).2, "available · 2.1.0");
}

// Spec: AI rows rendered (row part).
#[test]
fn the_ai_section_is_three_fixed_rows_with_a_muted_status() {
    let expected = |engine: &str, model: &str, status: &str| {
        vec![
            Row::Title("AI"),
            Row::Item {
                label: "Engine",
                value: Some(engine.to_owned().into()),
                selected: false,
                enabled: true,
            },
            Row::Item {
                label: "Model",
                value: Some(model.to_owned().into()),
                selected: false,
                enabled: true,
            },
            Row::Item {
                label: "Status",
                value: Some(status.to_owned().into()),
                selected: false,
                enabled: false,
            },
        ]
    };
    let mut m = menu(Some("ollama"), Some("llama3:8b"), None);
    assert_eq!(
        m.rows(&Config::default())[4..],
        expected("ollama (checking)", "llama3:8b · not found", "detecting…")
    );
    m.set_engines(Some(machine()));
    assert_eq!(
        m.rows(&Config::default())[4..],
        expected("ollama", "llama3:8b", "available · 2 models")
    );
    assert_eq!(
        MenuState::open(&Config::default())
            .rows(&Config::default())
            .len(),
        4
    );
}

// Spec: Engine change resets model.
#[test]
fn changing_the_engine_resets_the_model_to_its_first_model() {
    let mut m = menu(Some("ollama"), Some("qwen2:7b"), Some(machine()));
    assert_eq!(shown(&m).1, "qwen2:7b");
    pick_engine(&mut m, "claude");
    assert_eq!(shown(&m).1, "default");
    pick_engine(&mut m, "ollama");
    assert_eq!(shown(&m).1, "llama3:8b");
    assert_eq!(m.ai_draft(), saved(Some("ollama"), Some("llama3:8b")));
}

// Spec: Engine none clears model.
#[test]
fn engine_none_clears_engine_and_model() {
    let mut m = menu(Some("ollama"), Some("llama3:8b"), Some(machine()));
    pick_engine(&mut m, "none");
    assert_eq!(shown(&m).1, "none");
    assert_eq!(m.ai_draft(), saved(None, None));
}

// Spec: Server without models.
#[test]
fn an_engine_with_no_models_shows_none_and_cannot_cycle() {
    let found = detection(&[engine("ollama", Status::Available, &[], None)]);
    let mut m = menu(Some("ollama"), None, Some(found));
    on_model(&mut m);
    assert_eq!(shown(&m).1, "none");
    assert_eq!(press(&mut m, 'l'), MenuCommand::None);
    assert_eq!(press(&mut m, 'h'), MenuCommand::None);
    assert_eq!(shown(&m).1, "none");
    assert_eq!(m.ai_draft(), saved(Some("ollama"), None));
}

// Spec: Saved model no longer listed.
#[test]
fn a_saved_model_that_is_gone_is_shown_and_kept() {
    let mut m = menu(Some("ollama"), Some("old:7b"), Some(machine()));
    on_model(&mut m);
    assert_eq!(shown(&m).1, "old:7b · not found");
    assert_eq!(m.ai_draft(), saved(Some("ollama"), Some("old:7b")));
    press(&mut m, 'l');
    assert_eq!(shown(&m).1, "llama3:8b");
    // Once left, the missing model is gone from the choices.
    press(&mut m, 'h');
    assert_eq!(shown(&m).1, "qwen2:7b");
}

#[test]
fn the_model_row_cycles_the_engines_models() {
    let mut m = menu(Some("ollama"), Some("llama3:8b"), Some(machine()));
    on_model(&mut m);
    press(&mut m, 'l');
    assert_eq!(shown(&m).1, "qwen2:7b");
    press(&mut m, 'l');
    assert_eq!(shown(&m).1, "llama3:8b");
    press(&mut m, 'h');
    assert_eq!(shown(&m).1, "qwen2:7b");
}
