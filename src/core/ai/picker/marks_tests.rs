use super::machine_tests::{at_models, code_with, state};
use super::*;
use crate::core::ai::ModelFacts;
use crate::core::ai::marks::Tools;

fn facts(name: &str, params: Option<u64>, tools: Tools) -> ModelFacts {
    ModelFacts {
        engine: "ollama",
        name: name.to_owned(),
        params,
        digest: Some(format!("digest-{name}")),
        tools,
    }
}

fn marked(models: Vec<ModelFacts>) -> Detection {
    let names: Vec<&str> = models.iter().map(|f| f.name.as_str()).collect();
    Detection {
        engines: vec![state("ollama", Status::Available, &names)],
        model_facts: models,
        ..Default::default()
    }
}

fn mark_of(rows: &[PickRow], label: &str) -> Option<ToolMark> {
    rows.iter().find(|r| r.label == label).unwrap().tools
}

fn rows_of(d: &Detection, draft: &AiConfig) -> Vec<PickRow> {
    model_rows("ollama", Target::Task(Task::Code), draft, Some(d))
}

#[test]
fn installed_models_carry_the_mark_their_facts_earn() {
    let d = marked(vec![
        facts("llama3:8b", Some(8_000_000_000), Tools::Yes),
        facts("tiny:1b", Some(1_000_000_000), Tools::Yes),
        facts("mid:3b", Some(3_000_000_000), Tools::Yes),
    ]);
    let rows = rows_of(&d, &AiConfig::default());
    assert_eq!(mark_of(&rows, "llama3:8b"), Some(ToolMark::Capable));
    assert_eq!(mark_of(&rows, "tiny:1b"), Some(ToolMark::Unreliable));
    assert_eq!(mark_of(&rows, "mid:3b"), Some(ToolMark::Capable));
    let llama = rows.iter().find(|r| r.label == "llama3:8b").unwrap();
    assert_eq!(llama.mark, Mark::Downloaded);
}

#[test]
fn no_tools_unknown_tools_and_unknown_size_carry_no_mark() {
    let d = marked(vec![
        facts("plain:8b", Some(8_000_000_000), Tools::No),
        facts("probe:8b", Some(8_000_000_000), Tools::Unknown),
        facts("sizeless", None, Tools::Yes),
    ]);
    let rows = rows_of(&d, &AiConfig::default());
    assert_eq!(mark_of(&rows, "plain:8b"), None);
    assert_eq!(mark_of(&rows, "probe:8b"), None);
    assert_eq!(mark_of(&rows, "sizeless"), None);
}

#[test]
fn a_model_without_facts_carries_no_mark() {
    let d = Detection {
        engines: vec![state("ollama", Status::Available, &["bare:7b"])],
        ..Default::default()
    };
    assert_eq!(mark_of(&rows_of(&d, &AiConfig::default()), "bare:7b"), None);
}

#[test]
fn the_marks_show_even_with_tools_switched_off() {
    let d = marked(vec![facts("llama3:8b", Some(8_000_000_000), Tools::Yes)]);
    let mut draft = AiConfig::default();
    draft.set_tools_enabled(false);
    assert_eq!(
        mark_of(&rows_of(&d, &draft), "llama3:8b"),
        Some(ToolMark::Capable)
    );
}

#[test]
fn rows_not_detected_carry_only_download_required() {
    let d = marked(vec![facts("llama3:8b", Some(8_000_000_000), Tools::Yes)]);
    let mut draft = AiConfig::default();
    draft.set(
        Task::Code,
        Assignment {
            engine: Some("ollama".to_owned()),
            model: Some("old:7b".to_owned()),
        },
    );
    let rows = rows_of(&d, &draft);
    let old = rows.iter().find(|r| r.label == "old:7b").unwrap();
    assert_eq!(old.mark, Mark::DownloadRequired);
    assert_eq!(old.tools, None);
    let mistral = rows.iter().find(|r| r.label == "mistral:7b").unwrap();
    assert_eq!(mistral.mark, Mark::DownloadRequired);
    assert_eq!(mistral.tools, None);
}

#[test]
fn enter_on_a_marked_row_assigns_like_an_unmarked_one() {
    let marked_d = marked(vec![facts("llama3:8b", Some(8_000_000_000), Tools::Yes)]);
    let plain_d = marked(vec![facts("llama3:8b", Some(8_000_000_000), Tools::No)]);
    let draft = code_with("claude", "opus");
    let commit = |d: &Detection| {
        let mut p = at_models(&draft, d, "ollama");
        let rows = p.rows(&draft, Some(d));
        p.cursor = rows.iter().position(|r| r.label == "llama3:8b").unwrap();
        p.enter(&draft, Some(d))
    };
    let expected = Outcome::Commit(Assignment {
        engine: Some("ollama".to_owned()),
        model: Some("llama3:8b".to_owned()),
    });
    assert_eq!(commit(&marked_d), expected);
    assert_eq!(commit(&plain_d), expected);
}
