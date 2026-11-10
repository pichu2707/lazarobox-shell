//! Deleting a downloaded model from the picker: the key, the effect, the
//! reply and what is refused (config-menu, Delete a model; ai-task-models).

use ratatui::crossterm::event::{KeyEvent as Key, KeyEventKind};

use super::chat_live::{ready_app, type_text};
use super::hf_download::{cursor_label, enter, goto_pick};
use super::*;
use crate::core::ai::config::AiConfig;
use crate::core::ai::picker::{DeleteAsk, DeleteTarget};
use crate::core::ai::{CATALOG, Detection, EngineState, Status};
use crate::core::menu::Footer;

const GB: u64 = 1_000_000_000;

/// Ollama answers detection and lists two models; llama-server is stopped.
pub(super) fn machine() -> Detection {
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
                    vec!["gemma:2b".to_owned(), "qwen3:4b".to_owned()]
                } else {
                    Vec::new()
                },
                version: None,
                model_labels: Vec::new(),
                diagnostics: Default::default(),
            })
            .collect(),
        ollama_sizes: vec![("qwen3:4b".to_owned(), 2 * GB + 400_000_000)],
        ..Default::default()
    }
}

/// The menu open on `chat`'s picker, at ollama's model level, on `model`.
fn at_ollama(ai: AiConfig, model: &str) -> App {
    let mut a = app();
    a.engines = Some(machine());
    a.menu = Some(MenuState::open(&a.config).with_ai(ai, a.engines.clone()));
    a.input = InputMode::Menu;
    open_picker(&mut a, model);
    a
}

pub(super) fn open_picker(a: &mut App, model: &str) {
    goto_row(a, "chat");
    a.update(enter());
    let_go();
    goto_pick(a, "ollama");
    let_go();
    a.update(enter());
    let_go();
    goto_pick(a, model);
    let_go();
}

/// `d`, then a frame, so the confirmation accepts `y`.
pub(super) fn confirming(a: &mut App) {
    assert_eq!(a.update(key('d')), vec![]);
    let_go();
    a.frame_drawn();
}

fn footer(a: &App) -> String {
    match a.menu().unwrap().footer() {
        Footer::Hint(text) => text.into_owned(),
        Footer::Error(text) => text.to_owned(),
    }
}

fn notice(a: &App) -> String {
    a.notice().unwrap_or_default().to_owned()
}

fn the_ask(effects: &[Effect]) -> (DeleteAsk, u16) {
    match effects {
        [Effect::DeleteModel { ask, port }] => (ask.clone(), *port),
        other => panic!("expected one delete: {other:?}"),
    }
}

// Spec: `y` on the confirmation asks the runtime to delete the model.
#[test]
fn d_then_y_asks_the_runtime_with_the_target_and_the_ollama_port() {
    let mut a = at_ollama(AiConfig::default(), "qwen3:4b");
    confirming(&mut a);
    assert!(footer(&a).contains("y delete"), "{}", footer(&a));
    let (ask, port) = the_ask(&a.update(key('y')));
    assert_eq!(
        ask.target,
        DeleteTarget::Ollama {
            name: "qwen3:4b".to_owned(),
            size: Some(2 * GB + 400_000_000),
        }
    );
    assert_eq!(port, a.ai.servers().port("ollama").unwrap());
    assert!(footer(&a).contains("deleting"), "{}", footer(&a));
}

// Spec: `d` before the frame, or a key held from before, answers nothing.
#[test]
fn y_before_the_confirmation_was_drawn_deletes_nothing() {
    let mut a = at_ollama(AiConfig::default(), "qwen3:4b");
    assert_eq!(a.update(key('d')), vec![]);
    let_go();
    assert_eq!(a.update(key('y')), vec![]);
    let_go();
    // The screen still waits: once drawn, `y` works.
    a.frame_drawn();
    the_ask(&a.update(key('y')));
}

#[test]
fn a_held_y_does_not_confirm() {
    let mut a = at_ollama(AiConfig::default(), "qwen3:4b");
    confirming(&mut a);
    let repeat = Key::new_with_kind(KeyCode::Char('y'), KeyModifiers::NONE, KeyEventKind::Repeat);
    assert_eq!(a.update(AppEvent::Key(repeat)), vec![]);
    assert!(footer(&a).contains("y delete"), "{}", footer(&a));
}

// Spec: only an unmodified `y` deletes; any other key goes back.
#[test]
fn any_other_key_goes_back_and_deletes_nothing() {
    let others = [
        Key::new(KeyCode::Char('n'), KeyModifiers::NONE),
        Key::new(KeyCode::Esc, KeyModifiers::NONE),
        Key::new(KeyCode::Enter, KeyModifiers::NONE),
        Key::new(KeyCode::Char('d'), KeyModifiers::NONE),
        Key::new(KeyCode::Char('y'), KeyModifiers::CONTROL),
        Key::new(KeyCode::Char('y'), KeyModifiers::SHIFT),
        Key::new(KeyCode::Char('Y'), KeyModifiers::SHIFT),
    ];
    for other in others {
        let mut a = at_ollama(AiConfig::default(), "qwen3:4b");
        confirming(&mut a);
        assert_eq!(a.update(AppEvent::Key(other)), vec![], "{other:?}");
        assert_eq!(cursor_label(&a), "qwen3:4b", "{other:?}");
        let_go();
        // Not armed: the next `d` opens the screen again and deletes nothing.
        assert_eq!(a.update(key('y')), vec![], "{other:?}");
    }
}

// Spec: the keys that work on a model row keep working there.
#[test]
fn y_r_and_s_keep_their_answers_on_a_deletable_row() {
    let mut a = at_ollama(AiConfig::default(), "qwen3:4b");
    assert_eq!(a.update(key('y')), vec![]);
    assert_eq!(cursor_label(&a), "qwen3:4b");
    let_go();
    assert_eq!(a.update(key('r')), vec![Effect::RedetectEngines]);
    let_go();
    a.update(key('s'));
    assert!(a.menu().is_none(), "s saves and closes the menu");
}

// Spec: the footer offers `d` only on a row it can delete.
#[test]
fn the_footer_shows_d_delete_on_a_deletable_row_only() {
    let mut a = at_ollama(AiConfig::default(), "qwen3:4b");
    assert_eq!(
        footer(&a),
        "j/k move · Enter choose · d delete · Esc back · s save"
    );
    let curated = crate::core::ai::catalog::curated("ollama")
        .iter()
        .find(|m| !["gemma:2b", "qwen3:4b"].contains(m))
        .expect("a curated model that is not downloaded");
    goto_pick(&mut a, curated);
    assert!(!footer(&a).contains("d delete"), "{}", footer(&a));
    let_go();
    goto_pick(&mut a, crate::core::ai::picker::OLLAMA_ROW);
    assert!(!footer(&a).contains("d delete"), "{}", footer(&a));
}

#[test]
fn the_engines_level_keeps_its_hint() {
    let mut a = app();
    a.engines = Some(machine());
    a.menu = Some(MenuState::open(&a.config).with_ai(AiConfig::default(), a.engines.clone()));
    a.input = InputMode::Menu;
    goto_row(&mut a, "chat");
    a.update(enter());
    let_go();
    assert_eq!(
        footer(&a),
        "j/k move · Enter choose · r refresh · Esc back · s save"
    );
}

#[test]
fn unsaved_changes_and_the_delete_key_share_the_footer() {
    let mut a = at_ollama(AiConfig::default(), "gemma:2b");
    // Choosing a model changes the draft and closes the picker.
    a.update(enter());
    let_go();
    assert!(footer(&a).contains("unsaved"), "{}", footer(&a));
    open_picker(&mut a, "gemma:2b");
    assert_eq!(
        footer(&a),
        "unsaved changes · d delete · s to save · Esc back"
    );
}

// Spec (D3): a task of the draft or the saved config refuses the delete.
#[test]
fn a_task_assignment_refuses_and_names_it_and_emits_nothing() {
    let mut saved = AiConfig::default();
    saved.set(
        crate::core::ai::config::Task::Chat,
        crate::core::ai::config::Assignment {
            engine: Some("ollama".to_owned()),
            model: Some("qwen3:4b".to_owned()),
        },
    );
    let mut a = at_ollama(saved, "qwen3:4b");
    assert_eq!(a.update(key('d')), vec![]);
    assert!(footer(&a).contains("used by task chat"), "{}", footer(&a));
    assert!(footer(&a).contains("qwen3:4b"), "{}", footer(&a));
    assert_eq!(cursor_label(&a), "qwen3:4b", "still on the model level");
}

// Spec (D3): a running turn on the model refuses the delete.
#[test]
fn a_chat_turn_running_on_the_model_refuses_it_and_names_it() {
    let mut a = ready_app();
    type_text(&mut a, "hi");
    let effects = a.update(AppEvent::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )));
    assert!(
        effects.iter().any(|e| matches!(e, Effect::ChatSend { .. })),
        "{effects:?}"
    );
    a.engines = Some(machine_with("llama3:8b"));
    a.menu = Some(MenuState::open(&a.config).with_ai(AiConfig::default(), a.engines.clone()));
    a.input = InputMode::Menu;
    open_picker(&mut a, "llama3:8b");
    assert_eq!(a.update(key('d')), vec![]);
    assert!(footer(&a).contains("running chat turn"), "{}", footer(&a));
    assert!(footer(&a).contains("llama3:8b"), "{}", footer(&a));
}

fn machine_with(model: &str) -> Detection {
    let mut d = machine();
    for e in d.engines.iter_mut().filter(|e| e.id == "ollama") {
        e.models = vec![model.to_owned()];
    }
    d
}

// Spec: refused while a download runs, as Outcome::Busy does.
#[test]
fn a_running_download_refuses_it_with_the_busy_message() {
    let mut a = at_ollama(AiConfig::default(), "qwen3:4b");
    a.download = Some(crate::core::ai::picker::Running {
        source: crate::core::ai::picker::Source::Ollama,
        file: "other:1b".to_owned(),
        license: String::new(),
        done: 0,
        total: 100,
    });
    a.sync_download();
    assert_eq!(a.update(key('d')), vec![]);
    assert_eq!(footer(&a), crate::core::ai::picker::BUSY);
}

// Spec: the check runs again on `y`.
#[test]
fn a_download_that_started_after_d_refuses_y_and_emits_nothing() {
    let mut a = at_ollama(AiConfig::default(), "qwen3:4b");
    confirming(&mut a);
    a.download = Some(crate::core::ai::picker::Running {
        source: crate::core::ai::picker::Source::Ollama,
        file: "other:1b".to_owned(),
        license: String::new(),
        done: 0,
        total: 100,
    });
    a.sync_download();
    assert_eq!(a.update(key('y')), vec![]);
    assert_eq!(footer(&a), crate::core::ai::picker::BUSY);
}

// Spec: a success detects the engines again and says what was freed.
#[test]
fn a_success_reply_detects_again_and_notices_what_was_freed() {
    let mut a = at_ollama(AiConfig::default(), "qwen3:4b");
    confirming(&mut a);
    let (ask, _) = the_ask(&a.update(key('y')));
    let after = a.update(AppEvent::DeleteReply {
        ask: ask.clone(),
        answer: Ok(()),
    });
    assert!(after.contains(&Effect::RedetectEngines), "{after:?}");
    assert_eq!(notice(&a), "deleted qwen3:4b · freed up to 2.4 GB");
    // Back on the model level, on the same row.
    assert_eq!(cursor_label(&a), "qwen3:4b");
    assert!(!footer(&a).contains("deleting"), "{}", footer(&a));
}

// Spec: a failure says why, and the list is read again.
#[test]
fn a_failed_reply_says_why_and_detects_again() {
    let mut a = at_ollama(AiConfig::default(), "qwen3:4b");
    confirming(&mut a);
    let (ask, _) = the_ask(&a.update(key('y')));
    let why = "Ollama has no model with that name";
    let after = a.update(AppEvent::DeleteReply {
        ask,
        answer: Err(why.to_owned()),
    });
    assert!(after.contains(&Effect::RedetectEngines), "{after:?}");
    assert_eq!(notice(&a), format!("could not delete qwen3:4b: {why}"));
    assert_eq!(cursor_label(&a), "qwen3:4b");
}

// Spec: a stale reply never answers a later screen.
#[test]
fn a_reply_with_another_id_leaves_the_screen_open() {
    let mut a = at_ollama(AiConfig::default(), "qwen3:4b");
    confirming(&mut a);
    let (ask, _) = the_ask(&a.update(key('y')));
    let stale = DeleteAsk {
        id: ask.id + 7,
        target: ask.target.clone(),
    };
    a.update(AppEvent::DeleteReply {
        ask: stale,
        answer: Ok(()),
    });
    assert!(footer(&a).contains("deleting"), "{}", footer(&a));
}

const FILE: &str = "/h/.local/share/llama.cpp/models/qwen.gguf";

/// llama-server is stopped and the model scan found one file.
fn with_a_file() -> Detection {
    let mut d = machine();
    d.local_models = vec![std::path::PathBuf::from(FILE)];
    d
}

/// The menu open on `chat`'s picker, at llamacpp's model level, on the file.
fn at_file(ai: AiConfig) -> App {
    let mut a = app();
    a.engines = Some(with_a_file());
    a.menu = Some(MenuState::open(&a.config).with_ai(ai, a.engines.clone()));
    a.input = InputMode::Menu;
    goto_row(&mut a, "chat");
    a.update(enter());
    let_go();
    goto_pick(&mut a, "llamacpp");
    let_go();
    a.update(enter());
    let_go();
    goto_pick(&mut a, "qwen.gguf");
    let_go();
    a
}

fn file_ask(effects: &[Effect], measure: bool) -> DeleteAsk {
    match effects {
        [Effect::MeasureModel(ask)] if measure => ask.clone(),
        [Effect::DeleteFiles(ask)] if !measure => ask.clone(),
        other => panic!("expected one file effect: {other:?}"),
    }
}

/// `ask` as the runtime answers a measure: `size` bytes in one part.
fn sized(mut ask: DeleteAsk, size: u64) -> DeleteAsk {
    if let DeleteTarget::Gguf(file) = &mut ask.target {
        file.size = Some(size);
    }
    ask
}

// Spec: `d` on a file measures it, `y` deletes it through the runtime's
// checks, and the reply detects again and says what was freed (D4).
#[test]
fn a_file_is_measured_on_d_deleted_on_y_and_the_notice_says_what_was_freed() {
    let mut a = at_file(AiConfig::default());
    assert!(footer(&a).contains("d delete"), "{}", footer(&a));
    let ask = file_ask(&a.update(key('d')), true);
    let_go();
    a.frame_drawn();
    assert!(footer(&a).contains("measuring"), "{}", footer(&a));
    assert_eq!(a.update(key('y')), vec![], "y waits for the size");
    let_go();
    a.update(AppEvent::DeleteMeasured {
        ask: sized(ask, 5 * GB),
        answer: Ok(()),
    });
    assert!(footer(&a).contains("y delete"), "{}", footer(&a));
    let asked = file_ask(&a.update(key('y')), false);
    let after = a.update(AppEvent::DeleteReply {
        ask: sized(asked, 4 * GB),
        answer: Ok(()),
    });
    assert!(after.contains(&Effect::RedetectEngines), "{after:?}");
    assert_eq!(notice(&a), "deleted qwen.gguf · freed 4.0 GB");
    assert_eq!(cursor_label(&a), "qwen.gguf");
}

#[test]
fn a_file_that_fails_its_check_closes_the_screen_and_says_why() {
    let mut a = at_file(AiConfig::default());
    let ask = file_ask(&a.update(key('d')), true);
    let_go();
    let after = a.update(AppEvent::DeleteMeasured {
        ask,
        answer: Err("it is outside the model folders".to_owned()),
    });
    assert_eq!(after, vec![]);
    assert_eq!(
        footer(&a),
        "could not delete qwen.gguf: it is outside the model folders"
    );
    assert_eq!(cursor_label(&a), "qwen.gguf");
    assert_eq!(a.update(key('y')), vec![], "no screen left to answer");
}

// Spec: a partial failure says what was removed and what was not.
#[test]
fn a_partial_failure_is_noticed_and_detects_again() {
    let mut a = at_file(AiConfig::default());
    let ask = file_ask(&a.update(key('d')), true);
    let_go();
    a.frame_drawn();
    a.update(AppEvent::DeleteMeasured {
        ask: sized(ask, 3),
        answer: Ok(()),
    });
    let asked = file_ask(&a.update(key('y')), false);
    let why = "removed qwen.gguf (freed 0 B); not removed the blob of qwen.gguf: denied";
    let after = a.update(AppEvent::DeleteReply {
        ask: asked,
        answer: Err(why.to_owned()),
    });
    assert!(after.contains(&Effect::RedetectEngines), "{after:?}");
    assert_eq!(notice(&a), format!("could not delete qwen.gguf: {why}"));
}

// Spec (D3): the saved managed model is refused, named, and nothing is asked.
#[test]
fn the_managed_model_refuses_d_and_emits_nothing() {
    let mut saved = AiConfig::default();
    saved.manage(std::path::PathBuf::from(FILE));
    let mut a = at_file(saved);
    assert_eq!(a.update(key('d')), vec![]);
    assert!(
        footer(&a).contains("qwen.gguf is the model of the managed llama-server"),
        "{}",
        footer(&a)
    );
}
