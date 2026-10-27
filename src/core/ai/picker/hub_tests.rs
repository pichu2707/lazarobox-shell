//! The `download from Hugging Face…` row of the llamacpp model level
//! (config-menu, Hugging Face row in the llamacpp picker).

use std::path::PathBuf;

use super::machine_tests::state;
use super::*;

fn llamacpp(status: Status, models: &[&str]) -> Detection {
    Detection {
        engines: vec![state("llamacpp", status, &[])],
        local_models: models.iter().map(PathBuf::from).collect(),
    }
}

fn hf_row(label: &str) -> PickRow {
    PickRow {
        key: PickKey::HfDownload,
        label: label.to_owned(),
        mark: Mark::Plain,
        enabled: true,
        current: false,
    }
}

fn rows(d: &Detection) -> Vec<PickRow> {
    model_rows(
        "llamacpp",
        Target::Task(Task::Chat),
        &AiConfig::default(),
        Some(d),
    )
}

// Scenario: The row is last.
#[test]
fn the_row_is_last_after_the_local_models() {
    let rows = rows(&llamacpp(
        Status::NotRunning,
        &["/m/a-Q4_K_M.gguf", "/m/b.gguf"],
    ));
    assert_eq!(rows.len(), 3, "{rows:?}");
    assert!(matches!(rows[0].key, PickKey::Model(_)));
    assert!(matches!(rows[1].key, PickKey::Model(_)));
    assert_eq!(rows[2], hf_row(HF_ROW));
}

#[test]
fn the_row_is_shown_with_no_local_models_and_with_a_running_server() {
    let stopped = rows(&llamacpp(Status::NotRunning, &[]));
    assert_eq!(stopped.last(), Some(&hf_row(HF_ROW)), "{stopped:?}");
    let running = rows(&Detection {
        engines: vec![state("llamacpp", Status::Available, &["m.gguf"])],
        ..Default::default()
    });
    assert_eq!(running.last(), Some(&hf_row(HF_ROW)), "{running:?}");
}

#[test]
fn the_row_is_not_shown_for_another_engine() {
    // Without llama-server it is still there, with a note (config-menu S1).
    let missing = rows(&llamacpp(Status::NotInstalled, &[]));
    assert_eq!(
        missing.last().map(|r| &r.mark),
        Some(&Mark::Status(HF_NEEDS_SERVER.to_owned())),
        "{missing:?}"
    );
    let ollama = model_rows(
        "ollama",
        Target::Task(Task::Chat),
        &AiConfig::default(),
        Some(&Detection {
            engines: vec![state("ollama", Status::Available, &["m"])],
            ..Default::default()
        }),
    );
    assert!(!ollama.iter().any(|r| r.key == PickKey::HfDownload));
}

// Scenario: The row shows a running download.
#[test]
fn the_row_shows_a_running_download() {
    let d = llamacpp(Status::NotRunning, &["/m/b.gguf"]);
    let draft = AiConfig::default();
    let mut p = Picker::open(Target::Task(Task::Chat), &draft, Some(&d));
    p.level = Level::Models {
        engine: "llamacpp".to_owned(),
    };
    p.set_download(Some(Running {
        file: "a-Q4_K_M.gguf".to_owned(),
        license: "apache-2.0".to_owned(),
        done: 42,
        total: 100,
    }));
    let rows = p.rows(&draft, Some(&d));
    assert_eq!(
        rows.last(),
        Some(&hf_row("↓ 42% a-Q4_K_M.gguf · apache-2.0"))
    );
}

#[test]
fn hub_labels_draw_control_and_format_characters_as_question_marks() {
    assert_eq!(shown("a\u{1b}[31mb\u{202e}c\td"), "a?[31mb?c?d");
}
