//! An Ollama pull started from the ollama picker: the requests, the progress
//! in the status bar and the end, one download at a time with Hugging Face
//! (ollama-download, Pull through the local server; config-menu, Ollama
//! screens).

use std::path::PathBuf;

use super::hf_download::{cursor_label, enter, goto_pick};
use super::*;
use crate::core::ai::config::AiConfig;
use crate::core::ai::hf::End;
use crate::core::ai::ollama_lib::{Answer, Ask, Card, Licenses, Manifest};
use crate::core::ai::picker::{HF_ROW, OLLAMA_ROW};
use crate::core::ai::{CATALOG, Detection, EngineState, Status};
use crate::core::menu::Footer;

const GB: u64 = 1_000_000_000;
const LLAMA: &str = "LLAMA 3.2 COMMUNITY LICENSE AGREEMENT";

/// Ollama answers detection; llama-server is installed but stopped.
fn running_ollama() -> Detection {
    Detection {
        engines: CATALOG
            .iter()
            .map(|e| EngineState {
                binary: None,
                id: e.id,
                status: match e.id {
                    "ollama" => Status::Available,
                    "llamacpp" => Status::NotRunning,
                    _ => Status::NotInstalled,
                },
                models: if e.id == "ollama" {
                    vec!["qwen3:4b".to_owned()]
                } else {
                    Vec::new()
                },
                version: None,
                model_labels: Vec::new(),
                diagnostics: Default::default(),
            })
            .collect(),
        local_models: vec![PathBuf::from("/m/b.gguf")],
        ..Default::default()
    }
}

/// The menu open on the chat picker, at `engine`'s model level.
fn at_engine(engine: &str) -> App {
    let mut a = app();
    a.engines = Some(running_ollama());
    a.menu = Some(MenuState::open(&a.config).with_ai(AiConfig::default(), a.engines.clone()));
    a.input = InputMode::Menu;
    goto_row(&mut a, "chat");
    a.update(enter());
    goto_pick(&mut a, engine);
    let_go();
    a.update(enter());
    let_go();
    a
}

fn sizes_ask() -> Ask {
    Ask::Sizes {
        model: "llama3.2".to_owned(),
        tags: vec!["1b".to_owned(), "3b".to_owned()],
    }
}

fn licenses_ask() -> Ask {
    Ask::Licenses {
        model: "llama3.2".to_owned(),
        tag: "3b".to_owned(),
        digests: Some(vec!["sha256:l".to_owned()]),
    }
}

/// Enter on the Ollama row, on `llama3.2`, on `llama3.2:3b`, then `y`, each
/// reply arriving in turn: every effect, in order.
fn pull_llama(a: &mut App) -> Vec<Effect> {
    goto_pick(a, OLLAMA_ROW);
    let mut effects = a.update(enter());
    a.update(AppEvent::OllamaReply(
        Ask::Popular,
        Ok(Answer::Popular {
            cards: vec![Card {
                name: "llama3.2".to_owned(),
                description: String::new(),
                capabilities: vec!["tools".to_owned()],
                sizes: vec!["1b".to_owned(), "3b".to_owned()],
                pulls: "86.3M".to_owned(),
            }],
            fallback: false,
            ram: Some(8 * GB),
        }),
    ));
    let_go();
    effects.extend(a.update(enter()));
    let manifest = |size| {
        Some(Manifest {
            size,
            licenses: vec!["sha256:l".to_owned()],
        })
    };
    a.update(AppEvent::OllamaReply(
        sizes_ask(),
        Ok(Answer::Sizes {
            manifests: vec![
                ("1b".to_owned(), manifest(1_321_098_329)),
                ("3b".to_owned(), manifest(2_019_392_628)),
            ],
            ram: Some(8 * GB),
        }),
    ));
    a.update(key('j'));
    let_go();
    effects.extend(a.update(enter()));
    a.update(AppEvent::OllamaReply(
        licenses_ask(),
        Ok(Answer::Licenses(Box::new(Licenses {
            size: None,
            titles: vec![LLAMA.to_owned()],
            folder: Some(PathBuf::from("/m")),
            free: Some(50 * GB),
        }))),
    ));
    let_go();
    // The license is on screen before `y`.
    assert!(
        a.menu()
            .unwrap()
            .rows(&a.config)
            .iter()
            .any(|row| matches!(row, Row::Pick { label, .. }
                if label == "license: LLAMA 3.2 COMMUNITY LICENSE AGREEMENT")),
        "no license row"
    );
    effects.extend(a.update(key('y')));
    effects
}

fn pulls(effects: &[Effect]) -> usize {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::Pull { .. }))
        .count()
}

fn redetects(effects: &[Effect]) -> usize {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::RedetectEngines))
        .count()
}

// Scenario: Pick without typing.
#[test]
fn the_asks_then_one_pull_of_the_chosen_tag_in_order() {
    let mut a = at_engine("ollama");
    let effects = pull_llama(&mut a);
    let [
        Effect::OllamaAsk(Ask::Popular),
        Effect::OllamaAsk(sizes),
        Effect::OllamaAsk(licenses),
        Effect::Pull { pull, port: 11434 },
    ] = effects.as_slice()
    else {
        panic!("not the four effects in order: {effects:?}");
    };
    assert_eq!(sizes, &sizes_ask());
    assert_eq!(licenses, &licenses_ask());
    assert_eq!(pull.name(), "llama3.2:3b");
    assert_eq!(pull.license, LLAMA);
    assert!(
        !effects.iter().any(|e| matches!(e, Effect::SaveConfig(_))),
        "{effects:?}"
    );
    assert_eq!(a.status_download().as_deref(), Some("↓ 0%"));
    assert_eq!(
        cursor_label(&a),
        "↓ 0% llama3.2:3b · LLAMA 3.2 COMMUNITY LICENSE AGREEMENT"
    );
}

// Scenario: Progress then success.
#[test]
fn progress_shows_on_the_row_and_the_status_bar_then_success_redetects_once() {
    let mut a = at_engine("ollama");
    pull_llama(&mut a);
    let mut effects = a.update(AppEvent::DownloadProgress {
        done: 1_009_696_314,
        total: 2_019_392_628,
    });
    assert_eq!(a.status_download().as_deref(), Some("↓ 50%"));
    assert_eq!(
        cursor_label(&a),
        "↓ 50% llama3.2:3b · LLAMA 3.2 COMMUNITY LICENSE AGREEMENT"
    );
    effects.extend(a.update(AppEvent::DownloadEnd(End::Done(PathBuf::new()))));
    assert_eq!(a.status_download(), None);
    assert_eq!(a.notice(), Some("downloaded llama3.2:3b"));
    assert_eq!(cursor_label(&a), OLLAMA_ROW);
    effects.extend(a.update(AppEvent::DownloadEnd(End::Done(PathBuf::new()))));
    assert_eq!(redetects(&effects), 1, "{effects:?}");
}

// Scenario: Error line fails the pull.
#[test]
fn a_failed_pull_shows_its_reason_and_redetects_nothing() {
    let mut a = at_engine("ollama");
    pull_llama(&mut a);
    let reason = "failed: pull model manifest: file does not exist";
    let effects = a.update(AppEvent::DownloadEnd(End::Failed(reason.to_owned())));
    assert_eq!(a.notice(), Some(reason));
    assert_eq!(redetects(&effects), 0);
}

// Scenario: Cancel closes the request.
#[test]
fn enter_on_the_running_pull_asks_to_cancel() {
    let mut a = at_engine("ollama");
    pull_llama(&mut a);
    a.update(enter());
    let Footer::Hint(hint) = a.menu().unwrap().footer() else {
        panic!("not a hint");
    };
    assert_eq!(hint, "cancel the download? (y/n)");
    assert_eq!(a.update(key('y')), vec![Effect::CancelDownload]);
    a.update(AppEvent::DownloadEnd(End::Cancelled));
    assert_eq!(
        a.notice(),
        Some("download cancelled · run it again to resume")
    );
}

#[test]
fn a_running_pull_keeps_the_hugging_face_row_busy() {
    let mut a = at_engine("ollama");
    pull_llama(&mut a);
    // Back to the engines, then llamacpp's model level.
    let_go();
    a.update(esc());
    goto_pick(&mut a, "llamacpp");
    let_go();
    a.update(enter());
    goto_pick(&mut a, HF_ROW);
    let_go();
    assert_eq!(a.update(enter()), Vec::new());
    assert_eq!(
        a.menu().unwrap().footer(),
        Footer::Error("a download is already running")
    );
}

#[test]
fn a_hugging_face_download_keeps_the_ollama_row_busy() {
    let mut a = at_engine("ollama");
    a.download = Some(crate::core::ai::picker::Running {
        source: crate::core::ai::picker::Source::HuggingFace,
        file: "a.gguf".to_owned(),
        license: "mit".to_owned(),
        done: 0,
        total: 4 * GB,
    });
    a.sync_download();
    goto_pick(&mut a, OLLAMA_ROW);
    assert_eq!(a.update(enter()), Vec::new());
    assert_eq!(
        a.menu().unwrap().footer(),
        Footer::Error("a download is already running")
    );
    // A pull asked for anyway starts nothing.
    let pull = Box::new(crate::core::ai::ollama_lib::Pull {
        model: "llama3.2".to_owned(),
        tag: "3b".to_owned(),
        size: None,
        license: LLAMA.to_owned(),
    });
    assert_eq!(pulls(&a.start_pull(pull)), 0);
}

#[test]
fn an_ollama_reply_with_the_menu_closed_is_dropped() {
    let mut a = launched(80, 25);
    let effects = a.update(AppEvent::OllamaReply(
        Ask::Popular,
        Err("Ollama library request failed: boom".to_owned()),
    ));
    assert_eq!(effects, Vec::new());
    assert_eq!(a.notice(), None);
}
