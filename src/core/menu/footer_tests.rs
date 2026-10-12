//! Tests of the footer text at the `MenuState` level.

use super::ai_tests::{assigned, engine, machine, menu, press, selected_label};
use super::disabled_tests::{engine_at_level_two, only};
use super::picker_tests::{cursor_label, key, open, send};
use super::*;
use crate::core::ai::Status;

pub(super) fn hint(menu: &MenuState) -> String {
    match menu.footer() {
        Footer::Hint(text) => text.into_owned(),
        Footer::Error(text) => panic!("an error: {text}"),
    }
}

const SETTINGS: &str = "j/k move · h/l change · s save · Esc cancel";
const TASK: &str = "j/k move · Enter pick · r refresh · s save · Esc cancel";
const PICKER: &str = "j/k move · Enter choose · r refresh · Esc back · s save";
const CUSTOM: &str = "type a model · Enter ok · Ctrl+U clear · Esc back";

// Spec: Footer hint.
#[test]
fn a_settings_row_shows_the_settings_hint() {
    let mut m = menu(None, None, Some(machine()));
    assert_eq!(hint(&m), SETTINGS);
    press(&mut m, 'j');
    assert_eq!(hint(&m), SETTINGS);
}

// Spec: Footer hint on AI row.
#[test]
fn every_task_row_shows_the_task_hint() {
    let mut m = menu(None, None, Some(machine()));
    while selected_label(&m) != "Set all tasks" {
        press(&mut m, 'j');
    }
    for label in [
        "Set all tasks",
        "general",
        "code",
        "chat",
        "review",
        "summarize",
    ] {
        assert_eq!(selected_label(&m), label);
        assert_eq!(hint(&m), TASK, "{label}");
        press(&mut m, 'j');
    }
    assert_eq!(hint(&m), SETTINGS, "wrapped to the first Settings row");
}

// Spec: Footer hints in the picker.
#[test]
fn both_picker_levels_and_the_custom_field_have_their_hint() {
    let mut m = menu(None, None, Some(machine()));
    open(&mut m, "code");
    assert_eq!(hint(&m), PICKER, "level 1");
    press(&mut m, 'j');
    press(&mut m, 'j');
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(hint(&m), PICKER, "level 2");
    while cursor_label(&m) != "custom…" {
        press(&mut m, 'j');
    }
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(hint(&m), CUSTOM);
    send(&mut m, key(KeyCode::Esc));
    assert_eq!(hint(&m), PICKER, "back on level 2");
}

// Spec: Error replaces hint; the next press restores it.
#[test]
fn an_error_replaces_the_hint_until_the_next_press() {
    let mut m = menu(None, None, Some(machine()));
    m.set_error("save failed: disk full".to_owned());
    assert_eq!(m.footer(), Footer::Error("save failed: disk full"));
    press(&mut m, 'x');
    assert_eq!(hint(&m), SETTINGS);
    open(&mut m, "code");
    m.set_error("boom".to_owned());
    assert_eq!(m.footer(), Footer::Error("boom"), "inside the picker");
}

fn on_code_in_the_picker(m: &mut MenuState, levels: usize) {
    open(m, "code");
    if levels == 2 {
        press(m, 'j');
        press(m, 'j');
        send(m, key(KeyCode::Enter));
    }
}

// Spec: Before the first result.
#[test]
fn before_the_first_result_the_ai_views_show_the_checking_cue() {
    let mut m = assigned(&[(Task::Code, "codex", "default")], None);
    assert_eq!(hint(&m), SETTINGS, "no cue on a Settings row");
    while selected_label(&m) != "code" {
        press(&mut m, 'j');
    }
    assert_eq!(hint(&m), "checking engines… · s save · Esc cancel");
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(hint(&m), "checking engines… · s save · Esc back", "level 1");
    assert_eq!(cursor_label(&m), "codex");
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(hint(&m), "checking engines… · s save · Esc back", "level 2");
}

// Spec: Refreshing keeps last known engines.
#[test]
fn a_run_in_flight_shows_the_refreshing_cue_with_the_engines_kept() {
    let mut m = menu(None, None, Some(machine()));
    m.set_checking(true);
    assert_eq!(hint(&m), SETTINGS, "no cue on a Settings row");
    while selected_label(&m) != "code" {
        press(&mut m, 'j');
    }
    assert_eq!(hint(&m), "refreshing… · s save · Esc cancel");
    on_code_in_the_picker(&mut m, 1);
    assert_eq!(hint(&m), "refreshing… · s save · Esc back", "level 1");
    assert_eq!(cursor_label(&m), "(default · use general)");
    press(&mut m, 'j');
    assert_eq!(cursor_label(&m), "ollama", "the engines stay listed");
    press(&mut m, 'j');
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(hint(&m), "refreshing… · s save · Esc back", "level 2");
}

// Spec: Cue clears.
#[test]
fn a_result_or_a_finished_run_clears_the_cue() {
    let mut m = menu(None, None, None);
    while selected_label(&m) != "code" {
        press(&mut m, 'j');
    }
    m.set_engines(Some(machine()));
    assert_eq!(hint(&m), TASK);
    m.set_checking(true);
    assert_eq!(hint(&m), "refreshing… · s save · Esc cancel");
    m.set_checking(false);
    assert_eq!(hint(&m), TASK);
    m.set_checking(true);
    m.set_engines(Some(machine()));
    assert_eq!(hint(&m), TASK, "a result ends the run");
}

// Spec: Footer precedence, error > disabled row > cue > hint.
#[test]
fn the_cue_yields_to_an_error_a_disabled_row_and_the_custom_field() {
    let mut m = assigned(&[(Task::Code, "ollama", "old:7b")], None);
    open(&mut m, "code");
    assert_eq!(hint(&m), "checking engines… · s save · Esc back");
    while cursor_label(&m) != "ollama" {
        press(&mut m, 'j');
    }
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(hint(&m), not_selectable("checking…"), "disabled row");
    m.set_error("boom".to_owned());
    assert_eq!(m.footer(), Footer::Error("boom"));

    let mut c = assigned(&[(Task::Code, "codex", "default")], None);
    open(&mut c, "code");
    send(&mut c, key(KeyCode::Enter));
    while cursor_label(&c) != "custom…" {
        press(&mut c, 'j');
    }
    send(&mut c, key(KeyCode::Enter));
    c.set_checking(true);
    assert_eq!(hint(&c), CUSTOM);
}

fn not_selectable(reason: &str) -> String {
    format!("{reason} · not selectable")
}

// Spec: Footer on a disabled row (download required).
#[test]
fn a_model_to_download_reads_download_required() {
    let found = only(&[engine("ollama", Status::Available, &["llama3:8b"], None)]);
    let mut m = menu(None, None, found);
    engine_at_level_two(&mut m, "ollama");
    assert_eq!(hint(&m), PICKER, "the downloaded model");
    press(&mut m, 'j');
    assert_eq!(cursor_label(&m), "llama3.2:3b");
    assert_eq!(hint(&m), not_selectable("download required"));
    press(&mut m, 'k');
    assert_eq!(hint(&m), PICKER, "back on an enabled row");
}

// Spec: Footer on a disabled row (no models detected).
#[test]
fn an_empty_server_reads_no_models_detected() {
    let found = only(&[engine("lmstudio", Status::Available, &[], None)]);
    let mut m = menu(None, None, found);
    engine_at_level_two(&mut m, "lmstudio");
    assert_eq!(hint(&m), not_selectable("no models detected"));
}

// Spec: Footer on a disabled row (engine not detected).
#[test]
fn an_engine_that_is_gone_reads_engine_not_detected_on_both_rows() {
    let found = only(&[engine("ollama", Status::NotRunning, &[], None)]);
    let mut m = assigned(&[(Task::Code, "ollama", "old:7b")], found);
    open(&mut m, "code");
    while cursor_label(&m) != "ollama" {
        press(&mut m, 'j');
    }
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(cursor_label(&m), "old:7b");
    assert_eq!(hint(&m), not_selectable("engine not detected"));
    press(&mut m, 'j');
    assert_eq!(cursor_label(&m), "engine not detected");
    assert_eq!(hint(&m), not_selectable("engine not detected"));
}

// Spec: Footer on a disabled row (checking…).
#[test]
fn a_server_before_the_first_result_reads_checking() {
    let mut m = assigned(&[(Task::Code, "ollama", "old:7b")], None);
    open(&mut m, "code");
    while cursor_label(&m) != "ollama" {
        press(&mut m, 'j');
    }
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(cursor_label(&m), "old:7b");
    assert_eq!(hint(&m), not_selectable("checking…"));
}
