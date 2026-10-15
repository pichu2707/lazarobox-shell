//! Tests of the picker wiring at the `MenuState` level.

use super::ai_tests::{
    assigned, ch, detection, engine, machine, menu, press, selected_label, value_of,
};
use super::*;

pub(super) fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

pub(super) fn send(menu: &mut MenuState, event: KeyEvent) -> MenuCommand {
    menu.on_key(&event, false, &mut Config::default())
}

/// Moves the selection to the task row `label` and presses Enter.
pub(super) fn open(menu: &mut MenuState, label: &str) {
    while selected_label(menu) != label {
        press(menu, 'j');
    }
    assert_eq!(send(menu, key(KeyCode::Enter)), MenuCommand::None);
}

pub(super) fn pick(label: &str, mark: Mark, selected: bool, current: bool) -> Row {
    Row::Pick {
        label: label.to_owned(),
        mark,
        selected,
        enabled: true,
        current,
    }
}

pub(super) fn status(text: &str) -> Mark {
    Mark::Status(text.to_owned())
}

pub(super) fn title(menu: &MenuState) -> String {
    match &menu.rows(&Config::default())[0] {
        Row::Title(title) => title.to_string(),
        other => panic!("not a title: {other:?}"),
    }
}

pub(super) fn cursor_label(menu: &MenuState) -> String {
    menu.rows(&Config::default())
        .iter()
        .find_map(|row| match row {
            Row::Pick {
                label,
                selected: true,
                ..
            } => Some(label.clone()),
            _ => None,
        })
        .expect("a picker cursor")
}

// Spec: Open level 1.
#[test]
fn enter_on_a_task_row_opens_level_one_on_the_first_entry() {
    let mut m = menu(None, None, Some(machine()));
    open(&mut m, "code");
    assert_eq!(
        m.rows(&Config::default()),
        vec![
            Row::Title("Engine for code".into()),
            pick("(default · use general)", Mark::Plain, true, true),
            pick("ollama", status("available · 2 models"), false, false),
            pick("claude", status("available · 2.1.0"), false, false),
            pick("codex", status("available · 0.5.0"), false, false),
        ]
    );
}

// Spec: CLI model catalog. A labeled gemini row saves its explicit id.
#[test]
fn choosing_a_labeled_gemini_row_saves_the_explicit_id() {
    let found = detection(&[engine(
        "gemini",
        crate::core::ai::Status::Available,
        &[],
        None,
    )]);
    let mut m = menu(None, None, Some(found));
    open(&mut m, "code");
    while cursor_label(&m) != "gemini" {
        press(&mut m, 'j');
    }
    assert_eq!(send(&mut m, key(KeyCode::Enter)), MenuCommand::None);
    assert_eq!(title(&m), "Model for code (gemini)");
    press(&mut m, 'j');
    assert_eq!(cursor_label(&m), "Flash 3.8");
    assert_eq!(send(&mut m, key(KeyCode::Enter)), MenuCommand::None);
    assert_eq!(value_of(&m, "code"), "gemini › gemini-3.8-flash");
}

// Spec: Assign a CLI model.
#[test]
fn choosing_a_cli_model_assigns_it_and_returns_to_the_task_row() {
    let mut m = menu(None, None, Some(machine()));
    open(&mut m, "code");
    press(&mut m, 'j');
    press(&mut m, 'j');
    assert_eq!(send(&mut m, key(KeyCode::Enter)), MenuCommand::None);
    assert_eq!(title(&m), "Model for code (claude)");
    assert_eq!(cursor_label(&m), "default (CLI picks)");
    press(&mut m, 'j');
    assert_eq!(cursor_label(&m), "Opus 5.5");
    assert_eq!(send(&mut m, key(KeyCode::Enter)), MenuCommand::None);
    assert_eq!(selected_label(&m), "code");
    assert_eq!(value_of(&m, "code"), "claude › opus");
    assert_eq!(value_of(&m, "chat"), "(default)");
}

// Spec: Unassign entry.
#[test]
fn the_first_entry_unassigns_a_task() {
    let mut m = assigned(&[(Task::Code, "ollama", "llama3:8b")], Some(machine()));
    open(&mut m, "code");
    assert_eq!(cursor_label(&m), "ollama", "on the current engine");
    press(&mut m, 'k');
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(value_of(&m, "code"), "(default)");
    assert_eq!(selected_label(&m), "code");
}

// Spec: General and Set all use none.
#[test]
fn general_and_set_all_use_none_to_clear() {
    let entries = [
        (Task::General, "claude", "opus"),
        (Task::Code, "ollama", "llama3:8b"),
    ];
    let mut m = assigned(&entries, Some(machine()));
    open(&mut m, "general");
    press(&mut m, 'k');
    press(&mut m, 'k');
    assert_eq!(cursor_label(&m), "none");
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(value_of(&m, "general"), "(not set)");
    assert_eq!(value_of(&m, "code"), "ollama › llama3:8b", "only general");

    let mut all = assigned(&entries, Some(machine()));
    open(&mut all, "Set all tasks");
    assert_eq!(cursor_label(&all), "none", "mixed: first entry");
    send(&mut all, key(KeyCode::Enter));
    assert_eq!(value_of(&all, "general"), "(not set)");
    assert_eq!(value_of(&all, "code"), "(default)");
    assert_eq!(value_of(&all, "Set all tasks"), "(not set)");
}

// Spec: Set all assigns five.
#[test]
fn set_all_assigns_the_five_tasks() {
    let mut m = menu(None, None, Some(machine()));
    open(&mut m, "Set all tasks");
    press(&mut m, 'j');
    press(&mut m, 'j');
    send(&mut m, key(KeyCode::Enter));
    press(&mut m, 'j');
    send(&mut m, key(KeyCode::Enter));
    for label in ["Set all tasks", "general", "code", "chat", "review"] {
        assert_eq!(value_of(&m, label), "claude › opus", "{label}");
    }
    assert_eq!(value_of(&m, "summarize"), "claude › opus");
    assert_eq!(selected_label(&m), "Set all tasks");
}

// Spec: Esc goes back one level and never cancels while the picker is open.
#[test]
fn esc_goes_back_one_level_and_the_picker_reopens_on_level_one() {
    let mut m = menu(None, None, Some(machine()));
    open(&mut m, "code");
    press(&mut m, 'j');
    press(&mut m, 'j');
    send(&mut m, key(KeyCode::Enter));
    let draft = m.ai_draft();
    assert_eq!(send(&mut m, key(KeyCode::Esc)), MenuCommand::None);
    assert_eq!(title(&m), "Engine for code");
    assert_eq!(cursor_label(&m), "claude");
    assert_eq!(send(&mut m, key(KeyCode::Esc)), MenuCommand::None);
    assert_eq!(selected_label(&m), "code");
    assert_eq!(m.ai_draft(), draft);
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(title(&m), "Engine for code");
    assert_eq!(cursor_label(&m), "(default · use general)");
    assert_eq!(send(&mut m, ch('s')), MenuCommand::Save);
    assert_eq!(title(&m), "Engine for code", "the picker stays open");
}
