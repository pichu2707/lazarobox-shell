use super::machine_tests::{assigned, at_models, code_with, det, index_of};
use super::*;

fn all_assigned(engine: &str, model: &str) -> AiConfig {
    let mut config = AiConfig::default();
    for task in Task::ALL {
        config.set(task, assigned(engine, model));
    }
    config
}

#[test]
fn assigning_one_task_leaves_the_others() {
    let mut draft = AiConfig::default();
    draft.set(Task::Chat, assigned("ollama", "llama3:8b"));
    Target::Task(Task::Code).apply(&mut draft, assigned("claude", "opus"));
    assert_eq!(draft.get(Task::Code), &assigned("claude", "opus"));
    assert_eq!(draft.get(Task::Chat), &assigned("ollama", "llama3:8b"));
    assert_eq!(draft.get(Task::General), &Assignment::default());
    assert_eq!(draft.get(Task::Review), &Assignment::default());
    assert_eq!(draft.get(Task::Summarize), &Assignment::default());
}

#[test]
fn set_all_writes_all_five_tasks() {
    let mut draft = AiConfig::default();
    draft.set(Task::Code, assigned("ollama", "llama3:8b"));
    draft.set(Task::Chat, assigned("codex", "default"));
    Target::All.apply(&mut draft, assigned("claude", "sonnet"));
    for task in Task::SET_ALL {
        assert_eq!(draft.get(task), &assigned("claude", "sonnet"));
    }
    assert_eq!(draft.aggregate(), "claude › sonnet");
}

#[test]
fn set_all_none_clears_all_five() {
    let mut draft = all_assigned("claude", "opus");
    Target::All.apply(&mut draft, Assignment::default());
    for task in Task::SET_ALL {
        assert_eq!(draft.get(task), &Assignment::default());
    }
    assert_eq!(draft.label(Task::General), "(not set)");
    assert_eq!(draft.label(Task::Code), "(default)");
    assert_eq!(draft.aggregate(), "(not set)");
}

#[test]
fn unassigning_a_task_reads_default() {
    let mut draft = code_with("claude", "opus");
    Target::Task(Task::Code).apply(&mut draft, Assignment::default());
    assert_eq!(draft.get(Task::Code), &Assignment::default());
    assert_eq!(draft.label(Task::Code), "(default)");
}

#[test]
fn clearing_general_leaves_the_other_tasks() {
    let mut draft = all_assigned("claude", "opus");
    Target::Task(Task::General).apply(&mut draft, Assignment::default());
    assert_eq!(draft.get(Task::General), &Assignment::default());
    assert_eq!(draft.get(Task::Code), &assigned("claude", "opus"));
}

#[test]
fn a_full_walk_through_the_picker_assigns_set_all() {
    let d = det(&[]);
    let mut draft = AiConfig::default();
    let mut p = Picker::open(Target::All, &draft, Some(&d));
    let rows = p.rows(&draft, Some(&d));
    p.cursor = index_of(&rows, PickKey::Engine("claude".to_owned()));
    assert_eq!(p.enter(&draft, Some(&d)), Outcome::Stay);
    let rows = p.rows(&draft, Some(&d));
    p.cursor = index_of(&rows, PickKey::Model("sonnet".to_owned()));
    let Outcome::Commit(a) = p.enter(&draft, Some(&d)) else {
        panic!("expected a commit");
    };
    p.target.apply(&mut draft, a);
    for task in Task::SET_ALL {
        assert_eq!(draft.get(task), &assigned("claude", "sonnet"));
    }
}

#[test]
fn every_key_on_a_disabled_row_has_no_effect() {
    let d = det(&["llama3:8b"]);
    let draft = code_with("ollama", "old:7b");
    let mut p = at_models(&draft, &d, "ollama");
    let rows = p.rows(&draft, Some(&d));
    for (i, row) in rows.iter().enumerate().filter(|(_, r)| !r.enabled) {
        p.cursor = i;
        p.offset = 0;
        p.follow();
        let before = p.clone();
        assert_eq!(p.enter(&draft, Some(&d)), Outcome::Stay);
        p.type_char('a');
        p.backspace();
        p.clear_text();
        p.paste("abc");
        assert_eq!(p, before, "row {}", row.label);
    }
}

#[test]
fn an_info_row_is_inert() {
    let d = det(&[]);
    let draft = AiConfig::default();
    // lmstudio is not running, so its level shows only "engine not detected".
    let mut p = Picker::open(Target::Task(Task::Code), &draft, Some(&d));
    p.level = Level::Models {
        engine: "lmstudio".to_owned(),
    };
    let rows = p.rows(&draft, Some(&d));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].key, PickKey::Info);
    p.cursor = 0;
    let before = p.clone();
    assert_eq!(p.enter(&draft, Some(&d)), Outcome::Stay);
    assert_eq!(p, before);
}

// Spec (ai-task-models): Set all leaves the orchestrator alone.
#[test]
fn set_all_leaves_orchestrate_alone() {
    let mut draft = AiConfig::default();
    draft.set(Task::Orchestrate, assigned("claude", "opus"));
    Target::All.apply(&mut draft, assigned("codex", "default"));
    for task in Task::SET_ALL {
        assert_eq!(draft.get(task), &assigned("codex", "default"));
    }
    assert_eq!(draft.get(Task::Orchestrate), &assigned("claude", "opus"));
    assert_eq!(draft.aggregate(), "codex › default");
    // Clearing the five does not clear the orchestrator either.
    Target::All.apply(&mut draft, Assignment::default());
    assert_eq!(draft.get(Task::Orchestrate), &assigned("claude", "opus"));
}

// The shared-assignment mark of Set all ignores the orchestrator row.
#[test]
fn set_all_marks_ignore_the_orchestrator_row() {
    let d = det(&[]);
    let mut draft = all_assigned("claude", "opus");
    draft.set(Task::Orchestrate, assigned("codex", "default"));
    let rows = engine_rows(Target::All, &draft, Some(&d));
    let claude = rows
        .iter()
        .find(|r| r.key == PickKey::Engine("claude".to_owned()))
        .expect("claude row");
    assert!(claude.current);
}
