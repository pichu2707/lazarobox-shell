use super::*;
use crate::core::ai::{EngineState, Status};

fn assigned(engine: &str, model: &str) -> Assignment {
    Assignment {
        engine: Some(engine.to_owned()),
        model: Some(model.to_owned()),
    }
}

fn engine(id: &'static str) -> EngineState {
    EngineState {
        id,
        status: Status::Available,
        models: Vec::new(),
        model_labels: Vec::new(),
        version: None,
        diagnostics: Default::default(),
        binary: None,
    }
}

fn detection() -> Detection {
    Detection {
        engines: vec![engine("claude"), engine("codex")],
        ..Default::default()
    }
}

/// A `review` worker that failed on `claude` / `opus`, the menu saying `codex`.
fn picker(change: Change) -> FailurePicker {
    let mut menu = AiConfig::default();
    menu.set(Task::Review, assigned("codex", "default"));
    FailurePicker::open(
        Task::Review,
        &menu,
        ("claude", Some("opus")),
        Some(detection()),
        change,
    )
}

// Spec (config-menu, Change model opens level 2): the title names the engine
// of the failed worker and the cursor is on its current model.
#[test]
fn change_model_opens_the_models_of_the_engine_in_use() {
    let picker = picker(Change::Model);
    assert_eq!(picker.title(), "Model for review (claude)");
    assert!(picker.view().contains("[Opus 5.5]"), "{}", picker.view());
}

// Spec (config-menu, Change provider opens level 1).
#[test]
fn change_provider_opens_the_engines_with_the_one_in_use_selected() {
    let picker = picker(Change::Provider);
    assert_eq!(picker.title(), "Engine for review");
    assert!(picker.view().contains("[claude]"), "{}", picker.view());
}

// Spec (agent-orchestrator, Change provider relaunches on the new one): a
// model is chosen in two steps for a provider, one for change model.
#[test]
fn a_choice_gives_the_engine_and_the_model() {
    let mut picker = picker(Change::Provider);
    picker.step(true);
    assert_eq!(picker.enter(), Picked::Open, "an engine opens its models");
    assert_eq!(picker.title(), "Model for review (codex)");
    let Picked::Chosen(choice) = picker.enter() else {
        panic!("a model row was under the cursor");
    };
    assert_eq!(choice.engine.as_deref(), Some("codex"));
    assert!(choice.model.is_some());
}

#[test]
fn a_model_choice_keeps_the_engine_in_use() {
    let mut picker = picker(Change::Model);
    picker.step(true);
    let Picked::Chosen(choice) = picker.enter() else {
        panic!("a model row was under the cursor");
    };
    assert_eq!(choice.engine.as_deref(), Some("claude"));
    assert_ne!(choice.model.as_deref(), Some("opus"));
}

// Never fail silently: a choice that needs a screen the failure picker
// does not have says so and the picker stays usable.
#[test]
fn a_custom_model_is_not_offered_here_and_the_picker_stays_usable() {
    let mut picker = picker(Change::Model);
    // The custom entry is the last model row.
    for _ in 0..20 {
        picker.step(true);
        if picker.view().contains("[custom…]") {
            break;
        }
    }
    assert!(picker.view().contains("[custom…]"), "{}", picker.view());
    assert_eq!(picker.enter(), Picked::Open);
    assert!(picker.view().contains(NOT_HERE));
    assert_eq!(picker.title(), "Model for review (claude)");
}

#[test]
fn a_clear_choice_gives_the_default_assignment() {
    let mut picker = picker(Change::Provider);
    picker.step(false);
    picker.step(true);
    // The first row is "(default · use general)".
    picker.step(false);
    assert!(picker.view().contains("[(default"), "{}", picker.view());
    assert_eq!(picker.enter(), Picked::Chosen(Assignment::default()));
}

// Spec (agent-orchestrator, Override reaches later workers): an override sits
// on top of the menu for its task only, and never changes the menu config.
#[test]
fn an_override_replaces_its_task_on_a_copy_of_the_config() {
    let mut menu = AiConfig::default();
    menu.set(Task::Review, assigned("codex", "default"));
    menu.set(Task::Code, assigned("codex", "default"));
    let before = menu.clone();
    let mut over = RunOverrides::default();
    over.set(Task::Review, assigned("claude", "sonnet"));
    let applied = over.apply(&menu);
    assert_eq!(applied.get(Task::Review), &assigned("claude", "sonnet"));
    assert_eq!(applied.get(Task::Code), &assigned("codex", "default"));
    assert_eq!(menu, before, "the menu draft is not touched");
}

#[test]
fn a_second_choice_for_a_task_replaces_the_first() {
    let mut over = RunOverrides::default();
    over.set(Task::Review, assigned("claude", "sonnet"));
    over.set(Task::Review, assigned("gemini", "flash"));
    let applied = over.apply(&AiConfig::default());
    assert_eq!(applied.get(Task::Review), &assigned("gemini", "flash"));
    assert_eq!(applied.get(Task::Code), &Assignment::default());
}

// Spec (agent-orchestrator, Override shown): the marker follows the engine
// (and model, when picked) of the row; an engine-only pick that is the menu's
// own engine, and a "use general" pick, mark nothing.
#[test]
fn a_row_is_on_the_override_when_it_runs_the_pick() {
    let mut over = RunOverrides::default();
    over.set(Task::Review, assigned("claude", "sonnet"));
    over.set(Task::Code, Assignment::default());
    let on = |task, engine, model, menu| over.is_on(task, engine, model, menu);
    assert!(on(Task::Review, "claude", Some("sonnet"), Some("codex")));
    assert!(!on(Task::Review, "claude", Some("opus"), Some("codex")));
    assert!(!on(Task::Review, "codex", Some("sonnet"), None));
    assert!(!on(Task::Code, "codex", None, None), "use general");
    let mut over = RunOverrides::default();
    let engine_only = Assignment {
        engine: Some("claude".into()),
        model: None,
    };
    over.set(Task::Review, engine_only);
    assert!(over.is_on(Task::Review, "claude", Some("opus"), Some("codex")));
    assert!(!over.is_on(Task::Review, "claude", Some("opus"), Some("claude")));
}
