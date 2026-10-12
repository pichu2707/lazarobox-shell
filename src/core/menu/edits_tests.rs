//! Tests of `edits()` with an AI draft: Settings first, then the changed
//! tasks in `Task::ALL` order, both keys of each.

use super::ai_tests::{machine, menu, press};
use super::picker_tests::{cursor_label, key, open, send};
use super::*;
use crate::core::ai::config::Assignment;

/// Picks the first model of claude for the task row `label`.
fn assign_claude(m: &mut MenuState, label: &str) {
    open(m, label);
    press(m, 'j');
    press(m, 'j');
    send(m, key(KeyCode::Enter));
    send(m, key(KeyCode::Enter));
}

fn edit(table: &'static str, key: &'static str, value: &'static str) -> ConfigEdit {
    ConfigEdit {
        table,
        key,
        value: value.into(),
    }
}

// Spec: a changed task writes both keys under its own table.
#[test]
fn a_changed_task_writes_engine_and_model_under_its_table() {
    let mut m = menu(None, None, Some(machine()));
    assign_claude(&mut m, "code");
    assert_eq!(
        m.edits(&Config::default()),
        vec![
            edit("ai.tasks.code", "engine", "claude"),
            edit("ai.tasks.code", "model", "default"),
        ]
    );
}

// Spec: Settings first, then the changed tasks in `Task::ALL` order.
#[test]
fn settings_come_first_then_the_changed_tasks_in_task_order() {
    let mut m = menu(None, None, Some(machine()));
    assign_claude(&mut m, "summarize");
    assign_claude(&mut m, "chat");
    let mut live = Config::default();
    live.bars.statusline = BarPosition::Top;
    let tables: Vec<_> = m.edits(&live).iter().map(|e| (e.table, e.key)).collect();
    assert_eq!(
        tables,
        vec![
            ("statusline", "position"),
            ("ai.tasks.chat", "engine"),
            ("ai.tasks.chat", "model"),
            ("ai.tasks.summarize", "engine"),
            ("ai.tasks.summarize", "model"),
        ]
    );
}

// Spec: a task set back to unset is written as empty strings.
#[test]
fn a_task_cleared_from_a_saved_value_writes_empty_strings() {
    let mut ai = AiConfig::default();
    ai.set(
        Task::Code,
        Assignment {
            engine: Some("claude".into()),
            model: Some("default".into()),
        },
    );
    let mut m = MenuState::open(&Config::default()).with_ai(ai, Some(machine()));
    open(&mut m, "code");
    while cursor_label(&m) != "(default · use general)" {
        press(&mut m, 'k');
    }
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(
        m.edits(&Config::default()),
        vec![
            edit("ai.tasks.code", "engine", ""),
            edit("ai.tasks.code", "model", ""),
        ]
    );
}
