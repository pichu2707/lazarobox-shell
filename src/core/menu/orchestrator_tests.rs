//! Tests of the `orchestrate` row and the `Orchestrator` gate row of the AI
//! tab, at the `MenuState` level.

use super::ai_tests::{assigned, detection, engine, machine, menu, press, value_of};
use super::picker_tests::{cursor_label, key, open, pick, send, status, title};
use super::*;
use crate::core::ai::Status;

fn on_gate() -> MenuState {
    let mut m = menu(None, None, Some(machine()));
    m.goto("Orchestrator");
    m
}

/// Moves the picker cursor to `label` and presses Enter on it.
fn choose(m: &mut MenuState, label: &str) {
    for _ in 0..12 {
        if cursor_label(m) == label {
            break;
        }
        press(m, 'j');
    }
    assert_eq!(cursor_label(m), label);
    send(m, key(KeyCode::Enter));
}

// Spec (config-menu): Task rows rendered, Nothing assigned.
#[test]
fn the_orchestrate_and_gate_rows_read_their_defaults() {
    let m = menu(Some("claude"), Some("sonnet"), Some(machine()));
    assert_eq!(value_of(&m, "orchestrate"), "(default)");
    assert_eq!(value_of(&m, "Orchestrator"), "off");
    let labels: Vec<_> = MODELS_SECTION.items.iter().map(|i| i.label).collect();
    let at = labels.iter().position(|l| *l == "orchestrate").unwrap();
    assert_eq!(labels[at - 1], "summarize");
    assert_eq!(labels[at + 1], "Hugging Face");
    let labels: Vec<_> = TOOLS_SECTION.items.iter().map(|i| i.label).collect();
    let gate = labels.iter().position(|l| *l == "Orchestrator").unwrap();
    assert_eq!(labels[gate - 1], "Approval");
    assert_eq!(labels.last(), Some(&"Orchestrator"));
}

// Spec (config-menu): Orchestrator row is the global gate.
#[test]
fn the_gate_row_cycles_off_and_on_and_saves_the_flag() {
    let mut m = on_gate();
    assert!(!m.ai_draft().orchestrator.enabled);
    press(&mut m, 'l');
    assert_eq!(value_of(&m, "Orchestrator"), "on");
    assert!(m.ai_draft().orchestrator.enabled);
    assert_eq!(
        m.edits(&Config::default()),
        vec![ConfigEdit::orchestrator_flag("enabled", true)]
    );
    press(&mut m, 'h');
    assert_eq!(value_of(&m, "Orchestrator"), "off");
    assert_eq!(m.edits(&Config::default()), vec![]);
    // Enter cycles like the other on/off rows and opens no picker.
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(value_of(&m, "Orchestrator"), "on");
    assert!(!m.in_sub_level());
}

#[test]
fn the_gate_row_is_a_draft_row_without_a_refresh_or_a_picker() {
    let mut m = on_gate();
    assert_eq!(press(&mut m, 'r'), MenuCommand::None);
    assert!(!m.on_ai_row());
    // A held Enter does not toggle again.
    let before = m.ai_draft();
    m.on_key(&key(KeyCode::Enter), true, &mut Config::default());
    assert_eq!(m.ai_draft(), before);
}

// Spec (config-menu): accepted notice rides the next save; unchanged adds nothing.
#[test]
fn the_notice_acceptance_in_the_draft_is_an_edit_and_nothing_else_is() {
    let mut m = menu(None, None, Some(machine()));
    assert_eq!(m.edits(&Config::default()), vec![]);
    m.ai.as_mut().unwrap().accept_notice();
    assert_eq!(
        m.edits(&Config::default()),
        vec![ConfigEdit::orchestrator_flag("acknowledged", true)]
    );
}

// Spec (config-menu): Picker first entry.
#[test]
fn enter_on_orchestrate_opens_the_picker_with_the_default_entry_first() {
    let mut m = menu(Some("claude"), Some("sonnet"), Some(machine()));
    open(&mut m, "orchestrate");
    assert_eq!(title(&m), "Engine for orchestrate");
    let rows = m.rows(&Config::default());
    assert_eq!(
        rows[1],
        pick("(default · use general)", Mark::Plain, true, true)
    );
}

// Spec (config-menu): Only orchestrate changes.
#[test]
fn choosing_for_orchestrate_changes_only_orchestrate() {
    let mut m = assigned(&[(Task::General, "codex", "default")], Some(machine()));
    let before = m.ai_draft();
    open(&mut m, "orchestrate");
    // claude, then its first model.
    choose(&mut m, "claude");
    send(&mut m, key(KeyCode::Enter));
    let draft = m.ai_draft();
    assert_eq!(
        draft.get(Task::Orchestrate).engine.as_deref(),
        Some("claude")
    );
    for task in Task::SET_ALL {
        assert_eq!(draft.get(task), before.get(task), "{}", task.id());
    }
    let edits = m.edits(&Config::default());
    assert!(!edits.is_empty());
    assert!(
        edits.iter().all(|e| e.table == "ai.tasks.orchestrate"),
        "{edits:?}"
    );
}

// Spec (ai-task-models): Ollama marked not recommended, others unmarked.
#[test]
fn a_server_engine_is_marked_not_recommended_on_the_orchestrate_row_only() {
    let m = assigned(
        &[
            (Task::Code, "ollama", "llama3:8b"),
            (Task::Orchestrate, "ollama", "llama3:8b"),
        ],
        Some(machine()),
    );
    assert_eq!(
        value_of(&m, "orchestrate"),
        "ollama › llama3:8b · not recommended"
    );
    assert_eq!(value_of(&m, "code"), "ollama › llama3:8b");
    // A CLI engine on the orchestrate row is not marked.
    let m = assigned(&[(Task::Orchestrate, "claude", "opus")], Some(machine()));
    assert_eq!(value_of(&m, "orchestrate"), "claude › opus");
}

// Follow-up from the S1a review (1d.6): the mark follows the effective engine.
#[test]
fn an_unassigned_orchestrate_row_falling_back_to_a_server_is_marked() {
    let m = assigned(&[(Task::General, "ollama", "llama3:8b")], Some(machine()));
    assert_eq!(value_of(&m, "orchestrate"), "(default) · not recommended");
    assert_eq!(value_of(&m, "general"), "ollama › llama3:8b");
    assert_eq!(value_of(&m, "code"), "(default)");
    let m = assigned(&[(Task::General, "claude", "opus")], Some(machine()));
    assert_eq!(value_of(&m, "orchestrate"), "(default)");
    // An own CLI assignment wins over a server in general.
    let m = assigned(
        &[
            (Task::General, "ollama", "llama3:8b"),
            (Task::Orchestrate, "claude", "opus"),
        ],
        Some(machine()),
    );
    assert_eq!(value_of(&m, "orchestrate"), "claude › opus");
}

#[test]
fn the_engine_list_of_orchestrate_marks_the_servers_and_keeps_them_selectable() {
    let found = detection(&[
        engine("ollama", Status::Available, &["llama3:8b"], None),
        engine("claude", Status::Available, &["default"], Some("2.1.302")),
    ]);
    let rows_of = |label: &str| {
        let mut m = menu(None, None, Some(found.clone()));
        open(&mut m, label);
        m.rows(&Config::default())
    };
    let ollama_of = |rows: &[Row]| {
        rows.iter()
            .find_map(|r| match r {
                Row::Pick {
                    label,
                    mark,
                    enabled,
                    ..
                } if label == "ollama" => Some((mark.clone(), *enabled)),
                _ => None,
            })
            .expect("an ollama row")
    };
    assert_eq!(
        ollama_of(&rows_of("orchestrate")),
        (status("available · 1 model · not recommended"), true)
    );
    assert_eq!(
        ollama_of(&rows_of("code")),
        (status("available · 1 model"), true)
    );
}

// Spec (ai-task-models): Set all leaves the orchestrator alone (menu level).
#[test]
fn set_all_from_the_menu_leaves_orchestrate_alone() {
    let mut m = assigned(&[(Task::Orchestrate, "claude", "opus")], Some(machine()));
    open(&mut m, "Set all tasks");
    choose(&mut m, "codex");
    send(&mut m, key(KeyCode::Enter));
    let draft = m.ai_draft();
    assert_eq!(
        draft.get(Task::Orchestrate).engine.as_deref(),
        Some("claude")
    );
    assert_eq!(draft.get(Task::Chat).engine.as_deref(), Some("codex"));
    assert_eq!(value_of(&m, "Set all tasks"), "codex › default");
}
