//! The window an Ollama turn is budgeted against: the model's own window held
//! to `num_ctx_cap`, the same value the request body carries.

use super::chat_notes::{ai, plain, type_text};
use super::*;
use crate::core::ai::{
    EngineState, ModelFacts, Status,
    chat::{Entry, engine::Tools},
    config::OllamaOptions,
    marks::Tools as Marks,
};

const MODEL: &str = "llama3:8b";

fn chat_id() -> PaneId {
    id2()
}

fn chat_of(a: &App) -> &ChatState {
    match a.pane_view(chat_id()) {
        Some(PaneView::Chat(chat)) => chat,
        _ => panic!("pane 2 is not a chat"),
    }
}

/// A chat on Ollama whose model reports `window`, with `cap` as the user's
/// `num_ctx_cap`.
fn chat_with(window: Option<usize>, cap: usize) -> App {
    let mut config = ai(false);
    config = config.with_ollama(OllamaOptions {
        num_ctx_cap: cap,
        keep_alive_minutes: 30,
    });
    let mut a = launched(80, 25).with_ai(config);
    a.update(AppEvent::EnginesDetected(Detection {
        engines: vec![EngineState {
            binary: None,
            id: "ollama",
            status: Status::Available,
            models: vec![MODEL.to_owned()],
            version: None,
            model_labels: Vec::new(),
            diagnostics: Default::default(),
        }],
        model_facts: vec![ModelFacts {
            engine: "ollama",
            name: MODEL.to_owned(),
            params: None,
            digest: None,
            tools: Marks::Yes,
            window,
        }],
        ..Default::default()
    }));
    a.open_chat_tab(None);
    a.update(AppEvent::PromptReady(
        chat_id(),
        crate::core::ai::chat::prompts::Resolved {
            text: "system".into(),
            notices: Vec::new(),
        },
    ));
    a
}

fn send(a: &mut App, text: &str) {
    type_text(a, text);
    match a.update(plain(KeyCode::Enter)).as_slice() {
        [Effect::ChatSend { .. }] => {}
        other => panic!("expected one ChatSend, got {other:?}"),
    }
}

fn percent(a: &App) -> Option<u8> {
    chat_of(a).context_percent("ollama", Some(MODEL))
}

#[test]
fn the_cap_sizes_the_budget_of_a_model_with_a_big_window() {
    let text = "x".repeat(4_000);
    let mut capped = chat_with(Some(131_072), 16_384);
    send(&mut capped, &text);
    let mut loose = chat_with(Some(131_072), 100_000);
    send(&mut loose, &text);
    let (capped, loose) = (percent(&capped).unwrap(), percent(&loose).unwrap());
    assert!(capped > loose, "{capped} should be above {loose}");
}

#[test]
fn a_model_smaller_than_the_cap_keeps_its_own_window() {
    let text = "x".repeat(4_000);
    let mut small = chat_with(Some(4_096), 16_384);
    send(&mut small, &text);
    let mut capped = chat_with(Some(131_072), 16_384);
    send(&mut capped, &text);
    assert!(percent(&small).unwrap() > percent(&capped).unwrap());
}

#[test]
fn an_unknown_window_has_no_fill() {
    let mut a = chat_with(None, 16_384);
    send(&mut a, "hi");
    assert_eq!(percent(&a), None);
    let _ = (Entry::System(String::new()), Tools::Off);
}
