//! claude's models from its own `/model` listing (S4 of
//! odd/tasks/cli-model-lists.md), and the static list when it has none.

use super::machine_tests::state;
use super::*;

const ALIASES: [&str; 9] = [
    "sonnet",
    "opus",
    "haiku",
    "fable",
    "best",
    "sonnet[1m]",
    "opus[1m]",
    "fable[1m]",
    "opusplan",
];

fn listed() -> Detection {
    let mut claude = state("claude", Status::Available, &[]);
    claude.model_labels = ALIASES
        .iter()
        .map(|a| ((*a).to_owned(), (*a).to_owned()))
        .collect();
    Detection {
        engines: vec![claude],
        ..Default::default()
    }
}

fn failed(why: &str) -> Detection {
    let mut claude = state("claude", Status::Available, &["default"]);
    claude.diagnostics.models = Some(why.to_owned());
    Detection {
        engines: vec![claude],
        ..Default::default()
    }
}

fn with_code(model: &str) -> AiConfig {
    let mut draft = AiConfig::default();
    draft.set(
        Task::Code,
        Assignment {
            engine: Some("claude".to_owned()),
            model: Some(model.to_owned()),
        },
    );
    draft
}

fn labels(rows: &[PickRow]) -> Vec<&str> {
    rows.iter().map(|r| r.label.as_str()).collect()
}

#[test]
fn a_listed_claude_shows_default_then_each_alias_as_given_then_custom() {
    let rows = model_rows("claude", Target::All, &AiConfig::default(), Some(&listed()));
    assert_eq!(
        labels(&rows),
        [
            "default (CLI picks)",
            "sonnet",
            "opus",
            "haiku",
            "fable",
            "best",
            "sonnet[1m]",
            "opus[1m]",
            "fable[1m]",
            "opusplan",
            "custom…",
        ]
    );
    for r in &rows[1..10] {
        assert_eq!(r.key, PickKey::Model(r.label.clone()), "value = alias");
        assert!(r.enabled && r.mark == Mark::Plain && r.tools.is_none());
    }
    assert_eq!(rows[0].key, PickKey::Model("default".to_owned()));
    assert_eq!(rows[10].key, PickKey::Custom);
}

#[test]
fn a_saved_alias_is_the_current_row() {
    let rows = model_rows(
        "claude",
        Target::Task(Task::Code),
        &with_code("opus[1m]"),
        Some(&listed()),
    );
    let current: Vec<&str> = rows
        .iter()
        .filter(|r| r.current)
        .map(|r| r.label.as_str())
        .collect();
    assert_eq!(current, ["opus[1m]"]);
}

#[test]
fn a_saved_model_the_list_lacks_is_kept_and_marked() {
    let draft = with_code("claude-opus-5-5");
    let d = listed();
    let rows = model_rows("claude", Target::Task(Task::Code), &draft, Some(&d));
    let kept = &rows[rows.len() - 2];
    assert_eq!(kept.label, "claude-opus-5-5");
    assert_eq!(kept.mark, Mark::NotInList);
    assert!(kept.enabled && kept.current);
    assert_eq!(
        task_label(&draft, Task::Code, Some(&d)),
        format!("claude › claude-opus-5-5 · {NOT_IN_LIST}")
    );
}

#[test]
fn without_a_list_claude_falls_back_to_the_static_list_and_says_why() {
    let draft = with_code("claude-opus-5-5");
    let d = failed("timed out after 5 s");
    let rows = model_rows("claude", Target::Task(Task::Code), &draft, Some(&d));
    assert_eq!(
        labels(&rows),
        [
            "default (CLI picks)",
            "Opus 5.5",
            "Sonnet 5.5",
            "Haiku 5.5",
            "Fable 5.1",
            "claude-opus-5-5",
            "model list unavailable: timed out after 5 s",
            "custom…",
        ]
    );
    // No list to be missing from: the saved model carries no warning.
    assert_eq!(rows[5].mark, Mark::Plain);
    assert_eq!(
        task_label(&draft, Task::Code, Some(&d)),
        "claude › claude-opus-5-5"
    );
}
