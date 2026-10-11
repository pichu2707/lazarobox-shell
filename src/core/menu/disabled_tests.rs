//! Tests of the picker's disabled, saved and undetected rows at the
//! `MenuState` level.

use super::ai_tests::{assigned, ch, detection, engine, menu, press};
use super::picker_tests::{cursor_label, key, open, pick, send, status, title};
use super::*;
use crate::core::ai::{EngineState, Status};

fn enter(menu: &mut MenuState) {
    assert_eq!(send(menu, key(KeyCode::Enter)), MenuCommand::None);
}

fn only(found: &[EngineState]) -> Option<Detection> {
    Some(detection(found))
}

fn engine_at_level_two(menu: &mut MenuState, name: &str) {
    open(menu, "code");
    while cursor_label(menu) != name {
        press(menu, 'j');
    }
    enter(menu);
}

fn disabled(label: &str, mark: Mark, selected: bool, current: bool) -> Row {
    Row::Pick {
        label: label.to_owned(),
        mark,
        selected,
        enabled: false,
        current,
    }
}

// Spec: Disabled rows are reachable but inert.
#[test]
fn a_disabled_row_takes_the_cursor_and_enter_does_nothing() {
    let found = only(&[engine("ollama", Status::Available, &["llama3:8b"], None)]);
    let mut m = menu(None, None, found);
    engine_at_level_two(&mut m, "ollama");
    assert_eq!(cursor_label(&m), "llama3:8b");
    press(&mut m, 'j');
    assert_eq!(cursor_label(&m), "llama3.2:3b");
    let draft = m.ai_draft();
    enter(&mut m);
    assert_eq!(title(&m), "Model for code (ollama) 2/11");
    assert_eq!(cursor_label(&m), "llama3.2:3b");
    assert_eq!(m.ai_draft(), draft);
}

// Spec: Enter with no selectable row.
#[test]
fn a_list_with_no_selectable_row_ignores_every_key_and_esc_leaves() {
    let found = only(&[engine("lmstudio", Status::Available, &[], None)]);
    let mut m = menu(None, None, found);
    engine_at_level_two(&mut m, "lmstudio");
    let rows = m.rows(&Config::default());
    assert_eq!(
        rows[1],
        disabled("no models detected", Mark::Plain, true, false)
    );
    for k in [ch('j'), ch('l'), key(KeyCode::Enter)] {
        send(&mut m, k);
    }
    assert_eq!(m.rows(&Config::default()), rows);
    assert_eq!(m.ai_draft(), AiConfig::default());
    send(&mut m, key(KeyCode::Esc));
    assert_eq!(title(&m), "Engine for code");
}

// Spec: Saved model no longer downloaded stays.
#[test]
fn a_saved_model_that_is_gone_stays_current_and_inert() {
    let found = only(&[engine("ollama", Status::Available, &["llama3:8b"], None)]);
    let mut m = assigned(&[(Task::Code, "ollama", "old:7b")], found);
    let draft = m.ai_draft();
    open(&mut m, "code");
    enter(&mut m);
    let rows = m.rows(&Config::default());
    assert_eq!(rows[1], pick("llama3:8b", Mark::Downloaded, true, false));
    assert_eq!(
        rows[2],
        disabled("old:7b", Mark::DownloadRequired, false, true)
    );
    press(&mut m, 'j');
    enter(&mut m);
    assert_eq!(cursor_label(&m), "old:7b");
    send(&mut m, key(KeyCode::Esc));
    send(&mut m, key(KeyCode::Esc));
    assert_eq!(m.ai_draft(), draft);
}

// Spec: Undetected assigned engine listed.
#[test]
fn an_undetected_assigned_engine_is_selectable_and_reopens() {
    let mut m = assigned(&[(Task::Code, "codex", "default")], only(&[]));
    let draft = m.ai_draft();
    open(&mut m, "code");
    let rows = m.rows(&Config::default());
    assert_eq!(rows[2], pick("codex", status("(not detected)"), true, true));
    enter(&mut m);
    assert_eq!(title(&m), "Model for code (codex)");
    assert_eq!(m.ai_draft(), draft);
}

// Spec: Servers have no custom entry.
#[test]
fn servers_have_no_custom_row() {
    for id in ["ollama", "lmstudio", "llamacpp", "vllm"] {
        let found = only(&[engine(id, Status::Available, &["m"], None)]);
        let mut m = menu(None, None, found);
        engine_at_level_two(&mut m, id);
        let labels: Vec<_> = m
            .rows(&Config::default())
            .iter()
            .filter_map(|r| match r {
                Row::Pick { label, .. } => Some(label.clone()),
                _ => None,
            })
            .collect();
        assert!(!labels.iter().any(|l| l == "custom…"), "{id}");
        assert!(labels.contains(&"m".to_owned()), "{id}");
    }
}
