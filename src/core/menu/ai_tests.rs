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

/// The label of the selected row.
fn selected_label(menu: &MenuState) -> &'static str {
    let rows = menu.rows(&Config::default());
    let labels: Vec<_> = rows
        .iter()
        .filter_map(|row| match row {
            Row::Item {
                label,
                selected: true,
                ..
            } => Some(*label),
            _ => None,
        })
        .collect();
    assert_eq!(labels.len(), 1, "exactly one selected row");
    labels[0]
}

// Spec: Move down and up, Wrap, Status row is skipped.
#[test]
fn navigation_visits_settings_engine_and_model_and_skips_status_and_mouse() {
    let mut m = menu(Some("ollama"), None, Some(machine()));
    let mut visited = vec![selected_label(&m)];
    for _ in 0..4 {
        press(&mut m, 'j');
        visited.push(selected_label(&m));
    }
    assert_eq!(
        visited,
        [
            "Statusline position",
            "Tab bar position",
            "Engine",
            "Model",
            "Statusline position"
        ]
    );
    let mut back = Vec::new();
    for _ in 0..4 {
        press(&mut m, 'k');
        back.push(selected_label(&m));
    }
    assert_eq!(
        back,
        ["Model", "Engine", "Tab bar position", "Statusline position"]
    );
}

// Spec: Repeat moves.
#[test]
fn repeat_moves_into_the_ai_rows_like_a_press() {
    let mut m = menu(Some("ollama"), None, Some(machine()));
    m.on_key(&ch('j'), true, &mut Config::default());
    m.on_key(&ch('j'), true, &mut Config::default());
    assert_eq!(selected_label(&m), "Engine");
}

// Spec: Result arrives while open (menu part). Characterization: the
// replacement was written with the stub, so this passes at once.
#[test]
fn a_result_updates_in_place_and_keeps_selection_and_draft() {
    let mut m = menu(Some("ollama"), Some("llama3:8b"), None);
    on_model(&mut m);
    assert_eq!(shown(&m).2, "detecting…");
    m.set_engines(Some(machine()));
    assert_eq!(
        shown(&m),
        (
            "ollama".into(),
            "llama3:8b".into(),
            "available · 2 models".into()
        )
    );
    assert_eq!(selected_label(&m), "Model");
    assert_eq!(m.ai_draft(), saved(Some("ollama"), Some("llama3:8b")));
}

// Spec: Engine disappears while open (menu part). Characterization.
#[test]
fn a_vanished_engine_stays_in_the_draft_as_not_detected() {
    let mut m = menu(Some("ollama"), Some("llama3:8b"), Some(machine()));
    m.set_engines(Some(detection(&[engine(
        "ollama",
        Status::NotRunning,
        &[],
        None,
    )])));
    assert_eq!(
        shown(&m),
        (
            "ollama (not detected)".into(),
            "llama3:8b · not found".into(),
            "not running".into()
        )
    );
    assert_eq!(m.ai_draft(), saved(Some("ollama"), Some("llama3:8b")));
}

// Spec: Refresh on AI row (menu part).
#[test]
fn r_on_the_engine_or_model_row_asks_for_a_refresh() {
    let mut m = menu(Some("ollama"), Some("llama3:8b"), Some(machine()));
    on_engine(&mut m);
    assert_eq!(press(&mut m, 'r'), MenuCommand::Refresh);
    on_model(&mut m);
    assert_eq!(press(&mut m, 'r'), MenuCommand::Refresh);
    assert_eq!(m.ai_draft(), saved(Some("ollama"), Some("llama3:8b")));
}

// Spec: Refresh elsewhere or repeated (menu part).
#[test]
fn r_elsewhere_repeated_or_shifted_gives_no_command() {
    let mut m = menu(Some("ollama"), None, Some(machine()));
    assert_eq!(press(&mut m, 'r'), MenuCommand::None, "Statusline row");
    on_engine(&mut m);
    assert_eq!(
        m.on_key(&ch('r'), true, &mut Config::default()),
        MenuCommand::None,
        "repeat"
    );
    assert_eq!(press(&mut m, 'R'), MenuCommand::None);
    let plain = MenuState::open(&Config::default());
    let mut plain = plain;
    assert_eq!(press(&mut plain, 'r'), MenuCommand::None, "no AI section");
}

fn ai_edit(key: &'static str, value: &str) -> ConfigEdit {
    ConfigEdit {
        table: "ai",
        key,
        value: value.to_owned().into(),
    }
}

// Spec: Save writes both AI keys (edit part).
#[test]
fn a_changed_ai_draft_writes_both_keys_after_the_settings() {
    let mut m = menu(Some("ollama"), Some("llama3:8b"), Some(machine()));
    pick_engine(&mut m, "claude");
    assert_eq!(
        m.edits(&Config::default()),
        vec![ai_edit("engine", "claude"), ai_edit("model", "default")]
    );
    let mut live = Config::default();
    live.bars.tabbar = BarPosition::Bottom;
    assert_eq!(
        m.edits(&live),
        vec![
            ConfigEdit {
                table: "tabbar",
                key: "position",
                value: "bottom".into()
            },
            ai_edit("engine", "claude"),
            ai_edit("model", "default"),
        ]
    );
}

#[test]
fn a_model_only_change_still_writes_both_keys() {
    let mut m = menu(Some("ollama"), Some("llama3:8b"), Some(machine()));
    on_model(&mut m);
    press(&mut m, 'l');
    assert_eq!(
        m.edits(&Config::default()),
        vec![ai_edit("engine", "ollama"), ai_edit("model", "qwen2:7b")]
    );
}

// Spec: engine none is written as empty strings.
#[test]
fn none_is_written_as_empty_strings() {
    let mut m = menu(Some("ollama"), Some("llama3:8b"), Some(machine()));
    pick_engine(&mut m, "none");
    assert_eq!(
        m.edits(&Config::default()),
        vec![ai_edit("engine", ""), ai_edit("model", "")]
    );
}

// Spec: Unchanged AI draft, Undetected saved engine survives (edit part).
#[test]
fn an_unchanged_ai_draft_adds_no_edits() {
    for m in [
        menu(Some("ollama"), Some("llama3:8b"), Some(machine())),
        menu(Some("codex"), Some("gone"), Some(detection(&[]))),
        menu(Some("claude"), None, None),
        menu(None, None, Some(machine())),
    ] {
        assert_eq!(m.edits(&Config::default()), vec![]);
        let mut live = Config::default();
        live.bars.statusline = BarPosition::Top;
        assert_eq!(
            m.edits(&live),
            vec![ConfigEdit {
                table: "statusline",
                key: "position",
                value: "top".into()
            }]
        );
    }
}

// Spec: Esc reverts AI (menu part).
#[test]
fn reopening_shows_the_saved_values_again() {
    let mut m = menu(Some("ollama"), Some("llama3:8b"), Some(machine()));
    pick_engine(&mut m, "claude");
    assert_eq!(m.ai_draft(), saved(Some("claude"), Some("default")));
    let again = menu(Some("ollama"), Some("llama3:8b"), Some(machine()));
    assert_eq!(again.ai_draft(), saved(Some("ollama"), Some("llama3:8b")));
    assert_eq!(
        MenuState::open(&Config::default()).ai_draft(),
        saved(None, None)
    );
}

// Spec: Descriptor integrity.
#[test]
fn the_ai_descriptors_are_well_formed() {
    let mut paths: Vec<(&str, &str)> = SECTIONS
        .iter()
        .flat_map(|s| s.items)
        .filter_map(|item| match &item.kind {
            ItemKind::Choice(c) => Some((c.table, c.key)),
            _ => None,
        })
        .collect();
    assert_eq!(AI_SECTION.title, "AI");
    assert_eq!(AI_SECTION.items.len(), 3);
    for item in AI_SECTION.items {
        assert!(!item.label.is_empty());
        match &item.kind {
            ItemKind::Dynamic(field) => paths.push(field.key_path()),
            ItemKind::Derived(_) => {}
            _ => panic!("{}: not an AI item", item.label),
        }
    }
    let derived = AI_SECTION
        .items
        .iter()
        .filter(|i| matches!(i.kind, ItemKind::Derived(_)))
        .count();
    assert_eq!(derived, 1, "only Status is display-only");
    let mut unique = paths.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), paths.len(), "duplicate key path");
    assert!(paths.contains(&("ai", "engine")) && paths.contains(&("ai", "model")));
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
    // The missing model stays offered, last, for as long as the menu is open.
    press(&mut m, 'h');
    assert_eq!(shown(&m).1, "old:7b · not found");
    press(&mut m, 'h');
    assert_eq!(shown(&m).1, "qwen2:7b");
}

// Spec: Saved model no longer listed (the model stays reachable).
#[test]
fn a_saved_model_that_is_gone_is_the_last_choice_while_open() {
    let mut m = menu(Some("ollama"), Some("old:7b"), Some(machine()));
    on_model(&mut m);
    press(&mut m, 'h');
    assert_eq!(shown(&m).1, "qwen2:7b");
    press(&mut m, 'l');
    assert_eq!(shown(&m).1, "old:7b · not found");
    assert_eq!(m.ai_draft(), saved(Some("ollama"), Some("old:7b")));
}

fn with_models(models: &[&str]) -> Option<Detection> {
    Some(detection(&[engine(
        "ollama",
        Status::Available,
        models,
        None,
    )]))
}

#[test]
fn duplicate_model_names_do_not_trap_cycling() {
    let mut m = menu(Some("ollama"), Some("a"), with_models(&["a", "a", "b"]));
    on_model(&mut m);
    press(&mut m, 'l');
    assert_eq!(shown(&m).1, "b");
    press(&mut m, 'l');
    assert_eq!(shown(&m).1, "a");
}

#[test]
fn model_cycles_backward_through_three_choices() {
    let mut m = menu(Some("ollama"), Some("b"), with_models(&["a", "b", "c"]));
    on_model(&mut m);
    for want in ["a", "c", "b"] {
        press(&mut m, 'h');
        assert_eq!(shown(&m).1, want);
    }
    press(&mut m, 'l');
    assert_eq!(shown(&m).1, "c");
}

#[test]
fn an_unset_model_starts_at_the_first_forward_and_the_last_backward() {
    let mut m = menu(Some("ollama"), None, with_models(&["a", "b", "c"]));
    on_model(&mut m);
    press(&mut m, 'l');
    assert_eq!(shown(&m).1, "a");
    let mut m = menu(Some("ollama"), None, with_models(&["a", "b", "c"]));
    on_model(&mut m);
    press(&mut m, 'h');
    assert_eq!(shown(&m).1, "c");
}

#[test]
fn a_result_keeps_a_draft_that_differs_from_the_saved_choice() {
    let mut m = menu(Some("ollama"), Some("llama3:8b"), Some(machine()));
    on_model(&mut m);
    press(&mut m, 'l');
    m.set_engines(Some(machine()));
    assert_eq!(m.ai_draft(), saved(Some("ollama"), Some("qwen2:7b")));
    assert_eq!(shown(&m).1, "qwen2:7b");
}

#[test]
fn a_draft_engine_that_vanishes_stays_a_choice_until_left() {
    let mut m = menu(Some("ollama"), Some("llama3:8b"), Some(machine()));
    pick_engine(&mut m, "codex");
    let only_ollama = detection(&[engine("ollama", Status::Available, &["llama3:8b"], None)]);
    m.set_engines(Some(only_ollama));
    assert_eq!(shown(&m).0, "codex (not detected)");
    press(&mut m, 'l');
    assert_eq!(shown(&m).0, "none");
    // Only the saved engine is kept once the draft has moved away.
    press(&mut m, 'h');
    assert_eq!(shown(&m).0, "ollama");
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

#[test]
fn backward_from_a_vanished_draft_engine_reaches_the_one_before_it() {
    let mut m = menu(Some("ollama"), Some("llama3:8b"), Some(machine()));
    pick_engine(&mut m, "codex");
    m.set_engines(Some(detection(&[engine(
        "ollama",
        Status::Available,
        &["llama3:8b"],
        None,
    )])));
    press(&mut m, 'h');
    assert_eq!(shown(&m).0, "ollama");
}

#[test]
fn the_saved_model_is_not_offered_to_another_engine() {
    let mut m = menu(Some("ollama"), Some("old:7b"), Some(machine()));
    pick_engine(&mut m, "claude");
    on_model(&mut m);
    press(&mut m, 'l');
    assert_eq!(shown(&m).1, "default");
}
