//! Tests of a detection result arriving while the picker is open.

use super::ai_tests::{assigned, detection, engine, machine, menu, press};
use super::picker_tests::{cursor_label, key, open, send, title};
use super::*;
use crate::core::ai::Status;

fn ollama(models: &[&str]) -> Option<Detection> {
    Some(detection(&[engine(
        "ollama",
        Status::Available,
        models,
        None,
    )]))
}

// Spec: Result arrives in the picker.
#[test]
fn the_cursor_stays_on_a_row_that_became_disabled() {
    let mut m = assigned(
        &[(Task::Code, "ollama", "llama3:8b")],
        ollama(&["x:1", "llama3:8b"]),
    );
    open(&mut m, "code");
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(cursor_label(&m), "llama3:8b");
    let draft = m.ai_draft();
    m.set_engines(ollama(&["x:1", "y:2"]));
    assert_eq!(title(&m), "Model for code (ollama) 3/13");
    assert_eq!(cursor_label(&m), "llama3:8b");
    let rows = m.rows(&Config::default());
    assert!(
        rows.iter().any(|r| matches!(
            r,
            Row::Pick { label, selected: true, enabled: false, mark: Mark::DownloadRequired, .. }
                if label == "llama3:8b"
        )),
        "{rows:?}"
    );
    assert_eq!(m.ai_draft(), draft);
}

// Spec: Cursor entry removed.
#[test]
fn the_cursor_is_clamped_when_its_row_is_gone() {
    let mut m = menu(None, None, Some(machine()));
    open(&mut m, "code");
    press(&mut m, 'j');
    press(&mut m, 'j');
    assert_eq!(cursor_label(&m), "claude");
    m.set_engines(ollama(&["llama3:8b"]));
    assert_eq!(title(&m), "Engine for code");
    assert_eq!(cursor_label(&m), "ollama", "the last row left");
    assert_eq!(m.ai_draft(), AiConfig::default());
}

// A choice changes the draft and nothing else.
#[test]
fn choosing_changes_only_the_ai_draft() {
    let mut m = menu(None, None, Some(machine()));
    let mut live = Config::default();
    open(&mut m, "code");
    press(&mut m, 'j');
    for _ in 0..2 {
        assert_eq!(
            m.on_key(&key(KeyCode::Enter), false, &mut live),
            MenuCommand::None
        );
    }
    assert_eq!(m.ai_draft().label(Task::Code), "ollama › llama3:8b");
    assert_eq!(live, Config::default());
    let tables: Vec<_> = m.edits(&live).iter().map(|e| e.table).collect();
    assert_eq!(
        tables, ["ai.tasks.code"; 2],
        "only the AI draft produced edits"
    );
}
