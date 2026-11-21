//! Tests of the `ODD + RDD roles` section of the Models tab (acceptance-judges
//! S3): its title, the `Set all ODD + RDD roles` row and the two judge rows.

use super::ai_tests::{
    assigned, detection, engine, machine, menu, press, selected_label, value_of,
};
use super::picker_tests::{cursor_label, key, open, pick, send, title};
use super::*;
use crate::core::ai::Status;

const ROLES: &str = "Set all ODD + RDD roles";

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

// Spec (config-menu): the section sits after `simple`, before Hugging Face.
#[test]
fn the_roles_section_follows_simple_and_leads_hugging_face() {
    let mut m = menu(Some("claude"), Some("sonnet"), Some(machine()));
    m.goto("simple");
    let rows = m.rows(&Config::default());
    let at = rows
        .iter()
        .position(|row| {
            matches!(
                row,
                Row::Item {
                    label: "simple",
                    ..
                }
            )
        })
        .unwrap();
    let shown: Vec<String> = rows[at..]
        .iter()
        .map(|row| match row {
            Row::Title(text) => format!("title {text}"),
            Row::Item { label, .. } => (*label).to_owned(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        shown,
        [
            "simple",
            "title ODD + RDD roles",
            ROLES,
            "judge-a",
            "judge-b",
            "Hugging Face"
        ]
    );
}

// Spec (config-menu): the title is not selectable; `j` passes over it.
#[test]
fn navigation_passes_over_the_title() {
    let mut m = menu(Some("claude"), Some("sonnet"), Some(machine()));
    m.goto("simple");
    for want in [ROLES, "judge-a", "judge-b", "Hugging Face"] {
        press(&mut m, 'j');
        assert_eq!(selected_label(&m), want);
    }
    press(&mut m, 'k');
    assert_eq!(selected_label(&m), "judge-b");
}

// Spec (config-menu): unassigned rows read `(default)`.
#[test]
fn the_judge_rows_read_their_default() {
    let m = menu(Some("claude"), Some("sonnet"), Some(machine()));
    assert_eq!(value_of(&m, "judge-a"), "(default)");
    assert_eq!(value_of(&m, "judge-b"), "(default)");
    assert_eq!(value_of(&m, ROLES), "(not set)");
}

#[test]
fn enter_on_a_judge_opens_the_picker_with_the_default_entry_first() {
    let mut m = menu(Some("claude"), Some("sonnet"), Some(machine()));
    open(&mut m, "judge-a");
    assert_eq!(title(&m), "Engine for judge-a");
    let rows = m.rows(&Config::default());
    assert_eq!(rows[1], pick("(default)", Mark::Plain, true, true));
    let mut m = menu(Some("claude"), Some("sonnet"), Some(machine()));
    open(&mut m, ROLES);
    assert_eq!(title(&m), format!("Engine for {ROLES}"));
}

// Spec (ai-task-models): a server engine is not marked on a judge row.
#[test]
fn a_server_engine_is_not_marked_on_a_judge_picker() {
    let found = detection(&[engine("ollama", Status::Available, &["llama3:8b"], None)]);
    let mut m = menu(None, None, Some(found));
    open(&mut m, "judge-b");
    let marked = m.rows(&Config::default()).into_iter().any(|r| {
        matches!(r, Row::Pick { mark: Mark::Status(ref t), .. } if t.contains("not recommended"))
    });
    assert!(!marked);
}

#[test]
fn choosing_for_a_judge_changes_only_that_judge() {
    let mut m = assigned(&[(Task::General, "codex", "default")], Some(machine()));
    let before = m.ai_draft();
    open(&mut m, "judge-a");
    choose(&mut m, "claude");
    send(&mut m, key(KeyCode::Enter));
    let draft = m.ai_draft();
    assert_eq!(draft.get(Task::JudgeA).engine.as_deref(), Some("claude"));
    assert_eq!(draft.get(Task::JudgeB), before.get(Task::JudgeB));
    let edits = m.edits(&Config::default());
    assert!(!edits.is_empty());
    assert!(
        edits.iter().all(|e| e.table == "ai.tasks.judge-a"),
        "{edits:?}"
    );
}

// Spec (ai-task-models): the roles row writes only the judges.
#[test]
fn set_all_roles_writes_only_the_judges() {
    let mut m = assigned(&[(Task::General, "codex", "default")], Some(machine()));
    let before = m.ai_draft();
    open(&mut m, ROLES);
    choose(&mut m, "claude");
    send(&mut m, key(KeyCode::Enter));
    let draft = m.ai_draft();
    for task in Task::ROLES {
        assert_eq!(draft.get(task).engine.as_deref(), Some("claude"));
    }
    for task in Task::SET_ALL
        .into_iter()
        .chain([Task::Orchestrate, Task::Simple])
    {
        assert_eq!(draft.get(task), before.get(task), "{}", task.id());
    }
    assert_eq!(value_of(&m, ROLES), "claude › default");
}

// Spec (ai-task-models): `Set all tasks` writes no judge.
#[test]
fn set_all_tasks_leaves_the_judges_alone() {
    let mut m = assigned(&[(Task::JudgeA, "ollama", "llama3:8b")], Some(machine()));
    open(&mut m, "Set all tasks");
    choose(&mut m, "codex");
    send(&mut m, key(KeyCode::Enter));
    let draft = m.ai_draft();
    assert_eq!(draft.get(Task::JudgeA).engine.as_deref(), Some("ollama"));
    assert_eq!(draft.get(Task::JudgeB).engine, None);
    assert_eq!(value_of(&m, "Set all tasks"), "codex › default");
}

// Spec (config-menu): the popup keeps its size with the section.
#[test]
fn the_models_tab_with_its_section_fits_the_popup_body() {
    let m = menu(Some("claude"), Some("sonnet"), Some(machine()));
    let mut m = m;
    m.goto("judge-a");
    assert!(m.rows(&Config::default()).len() <= VIEW_BODY);
}
