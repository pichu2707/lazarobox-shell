use super::*;
use crate::core::ai::EngineState;

pub(super) fn state(id: &'static str, status: Status, models: &[&str]) -> EngineState {
    EngineState {
        id,
        status,
        models: models.iter().map(|m| m.to_string()).collect(),
        version: None,
        model_labels: Vec::new(),
        diagnostics: Default::default(),
    }
}

pub(super) fn det(ollama_models: &[&str]) -> Detection {
    Detection {
        engines: vec![
            state("ollama", Status::Available, ollama_models),
            state("lmstudio", Status::NotRunning, &[]),
            state("claude", Status::Available, &[]),
        ],
    }
}

pub(super) fn assigned(engine: &str, model: &str) -> Assignment {
    Assignment {
        engine: Some(engine.to_owned()),
        model: Some(model.to_owned()),
    }
}

pub(super) fn code_with(engine: &str, model: &str) -> AiConfig {
    let mut config = AiConfig::default();
    config.set(Task::Code, assigned(engine, model));
    config
}

pub(super) fn index_of(rows: &[PickRow], key: PickKey) -> usize {
    rows.iter().position(|r| r.key == key).unwrap()
}

pub(super) fn engine_key(id: &str) -> PickKey {
    PickKey::Engine(id.to_owned())
}

/// A picker for code, moved to the Models level of `engine`.
pub(super) fn at_models(draft: &AiConfig, d: &Detection, engine: &str) -> Picker {
    let mut p = Picker::open(Target::Task(Task::Code), draft, Some(d));
    let rows = p.rows(draft, Some(d));
    p.cursor = index_of(&rows, engine_key(engine));
    assert_eq!(p.enter(draft, Some(d)), Outcome::Stay);
    p
}

#[test]
fn open_puts_the_cursor_on_the_current_engine() {
    let d = det(&["llama3:8b"]);
    let draft = code_with("claude", "opus");
    let p = Picker::open(Target::Task(Task::Code), &draft, Some(&d));
    let rows = p.rows(&draft, Some(&d));
    assert_eq!(p.level, Level::Engines);
    assert_eq!(p.cursor(), index_of(&rows, engine_key("claude")));
    assert_eq!(p.offset(), 0);
}

#[test]
fn open_puts_the_cursor_on_the_first_row_when_unassigned() {
    let d = det(&["llama3:8b"]);
    let draft = AiConfig::default();
    let p = Picker::open(Target::Task(Task::Code), &draft, Some(&d));
    assert_eq!(p.cursor(), 0);
}

#[test]
fn step_wraps_over_every_row() {
    let d = det(&["llama3:8b"]);
    let draft = AiConfig::default();
    let mut p = Picker::open(Target::Task(Task::Code), &draft, Some(&d));
    let last = p.rows(&draft, Some(&d)).len() - 1;
    p.step(false, &draft, Some(&d));
    assert_eq!(p.cursor(), last);
    p.step(true, &draft, Some(&d));
    assert_eq!(p.cursor(), 0);
    p.step(true, &draft, Some(&d));
    assert_eq!(p.cursor(), 1);
}

#[test]
fn step_lands_on_disabled_rows() {
    let d = det(&["llama3:8b"]);
    let draft = AiConfig::default();
    let mut p = at_models(&draft, &d, "ollama");
    let rows = p.rows(&draft, Some(&d));
    assert!(rows[0].enabled && !rows[1].enabled);
    p.step(true, &draft, Some(&d));
    assert_eq!(p.cursor(), 1);
    p.step(false, &draft, Some(&d));
    p.step(false, &draft, Some(&d));
    assert_eq!(p.cursor(), rows.len() - 1);
    assert!(!rows[p.cursor()].enabled);
}

#[test]
fn enter_on_the_first_engine_row_clears() {
    let d = det(&[]);
    let draft = code_with("claude", "opus");
    let mut p = Picker::open(Target::Task(Task::Code), &draft, Some(&d));
    p.cursor = 0;
    assert_eq!(
        p.enter(&draft, Some(&d)),
        Outcome::Commit(Assignment::default())
    );
}

#[test]
fn enter_on_an_engine_opens_its_models() {
    let d = det(&["llama3:8b"]);
    let draft = AiConfig::default();
    let p = at_models(&draft, &d, "ollama");
    assert_eq!(
        p.level,
        Level::Models {
            engine: "ollama".to_owned()
        }
    );
    assert_eq!(p.cursor(), 0);
}

#[test]
fn models_open_on_the_current_model_when_selectable() {
    let d = det(&["a:1b", "b:1b"]);
    let draft = code_with("ollama", "b:1b");
    let p = at_models(&draft, &d, "ollama");
    assert_eq!(p.cursor(), 1);
}

#[test]
fn models_open_on_the_first_selectable_when_current_is_disabled() {
    let d = det(&["a:1b", "b:1b"]);
    let draft = code_with("ollama", "old:7b");
    let p = at_models(&draft, &d, "ollama");
    assert_eq!(p.cursor(), 0);
}

#[test]
fn models_open_on_the_first_row_when_nothing_is_selectable() {
    let d = det(&[]);
    let draft = AiConfig::default();
    let mut p = at_models(&draft, &d, "ollama");
    let rows = p.rows(&draft, Some(&d));
    assert!(rows.iter().all(|r| !r.enabled));
    assert_eq!(p.cursor(), 0);
    assert_eq!(p.enter(&draft, Some(&d)), Outcome::Stay);
}

#[test]
fn enter_on_a_selectable_model_commits_engine_and_model() {
    let d = det(&["llama3:8b"]);
    let draft = AiConfig::default();
    let mut p = at_models(&draft, &d, "ollama");
    assert_eq!(
        p.enter(&draft, Some(&d)),
        Outcome::Commit(assigned("ollama", "llama3:8b"))
    );
}

#[test]
fn enter_on_a_disabled_model_changes_nothing() {
    let d = det(&["llama3:8b"]);
    let draft = AiConfig::default();
    let mut p = at_models(&draft, &d, "ollama");
    p.step(true, &draft, Some(&d));
    let before = p.clone();
    assert_eq!(p.enter(&draft, Some(&d)), Outcome::Stay);
    assert_eq!(p, before);
}

#[test]
fn enter_on_custom_opens_an_empty_buffer() {
    let d = det(&[]);
    let draft = AiConfig::default();
    let mut p = at_models(&draft, &d, "claude");
    let rows = p.rows(&draft, Some(&d));
    p.cursor = index_of(&rows, PickKey::Custom);
    assert_eq!(p.enter(&draft, Some(&d)), Outcome::Stay);
    assert_eq!(
        p.level,
        Level::Custom {
            engine: "claude".to_owned(),
            text: String::new()
        }
    );
}

#[test]
fn back_goes_up_one_level_at_a_time() {
    let d = det(&[]);
    let draft = AiConfig::default();
    let mut p = at_models(&draft, &d, "claude");
    let rows = p.rows(&draft, Some(&d));
    let custom = index_of(&rows, PickKey::Custom);
    p.cursor = custom;
    p.enter(&draft, Some(&d));
    assert_eq!(p.back(&draft, Some(&d)), Outcome::Stay);
    assert_eq!(
        p.level,
        Level::Models {
            engine: "claude".to_owned()
        }
    );
    assert_eq!(p.cursor(), custom);
    assert_eq!(p.back(&draft, Some(&d)), Outcome::Stay);
    assert_eq!(p.level, Level::Engines);
    let engines = p.rows(&draft, Some(&d));
    assert_eq!(p.cursor(), index_of(&engines, engine_key("claude")));
    assert_eq!(p.back(&draft, Some(&d)), Outcome::Close);
}

#[test]
fn refresh_is_requested_at_both_lists_but_typed_in_custom() {
    let d = det(&[]);
    let draft = AiConfig::default();
    let p = Picker::open(Target::Task(Task::Code), &draft, Some(&d));
    assert_eq!(p.refresh(), Outcome::Refresh);
    let mut p = at_models(&draft, &d, "claude");
    assert_eq!(p.refresh(), Outcome::Refresh);
    let rows = p.rows(&draft, Some(&d));
    p.cursor = index_of(&rows, PickKey::Custom);
    p.enter(&draft, Some(&d));
    assert_eq!(p.refresh(), Outcome::Stay);
}

#[test]
fn enter_on_an_engine_without_a_chat_adapter_stays_on_the_engines_level() {
    let d = Detection {
        engines: vec![
            state("ollama", Status::Available, &["a"]),
            state("opencode", Status::Available, &[]),
        ],
    };
    let draft = AiConfig::default();
    let mut p = Picker::open(Target::Task(Task::Chat), &draft, Some(&d));
    let rows = p.rows(&draft, Some(&d));
    p.cursor = index_of(&rows, engine_key("opencode"));
    assert_eq!(p.enter(&draft, Some(&d)), Outcome::Stay);
    assert_eq!(p.level, Level::Engines);
    assert_eq!(p.cursor, index_of(&rows, engine_key("opencode")));
}

#[test]
fn a_server_that_is_not_running_opens_its_level_and_enter_copies_the_start_note() {
    let d = Detection {
        engines: vec![state("vllm", Status::NotRunning, &[])],
    };
    let draft = AiConfig::default();
    let mut p = Picker::open(Target::Task(Task::Chat), &draft, Some(&d));
    let rows = p.rows(&draft, Some(&d));
    p.cursor = index_of(&rows, engine_key("vllm"));
    assert_eq!(p.enter(&draft, Some(&d)), Outcome::Stay);
    assert_eq!(
        p.level,
        Level::Models {
            engine: "vllm".to_owned()
        }
    );
    let rows = p.rows(&draft, Some(&d));
    p.cursor = 0;
    let text = "vllm not running on 127.0.0.1:8000 · start it, then press r: \
                vllm serve <model> --host 127.0.0.1 --port 8000";
    assert!(matches!(&rows[0].key, PickKey::Note { text: t, .. } if t == text));
    assert_eq!(p.enter(&draft, Some(&d)), Outcome::Copy(text.to_owned()));
}
