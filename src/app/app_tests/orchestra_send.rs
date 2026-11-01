//! The send path of an orchestrating pane (agent-orchestrator D5): which
//! task, engine and model a turn goes to, and the roster it carries. State
//! asserts only.

use super::chat_live::{ask, chat_id, chat_of, composer, ctrl, finish};
use super::*;
use crate::core::ai::{
    EngineState, Status,
    chat::engine::{Availability, OrchSend, Tools},
    config::{Assignment, Profile, Task},
    tools::registry::Family,
};

fn assign(ai: &mut AiConfig, task: Task, engine: &str, model: Option<&str>) {
    ai.set(
        task,
        Assignment {
            engine: Some(engine.into()),
            model: model.map(Into::into),
        },
    );
}

/// `chat` on ollama / llama3:8b, `orchestrate` on claude / opus, tools on and
/// the global gate on.
pub(super) fn orch_ai() -> AiConfig {
    let mut ai = AiConfig::default();
    assign(&mut ai, Task::Chat, "ollama", Some("llama3:8b"));
    assign(&mut ai, Task::Orchestrate, "claude", Some("opus"));
    ai.set_tools_enabled(true);
    ai.orchestrator.enabled = true;
    ai
}

fn engine(id: &'static str, status: Status, models: &[&str]) -> EngineState {
    EngineState {
        binary: None,
        id,
        status,
        models: models.iter().map(|m| (*m).to_owned()).collect(),
        version: None,
        model_labels: Vec::new(),
        diagnostics: Default::default(),
    }
}

/// ollama with llama3:8b and claude both available; codex not installed.
pub(super) fn detected() -> AppEvent {
    AppEvent::EnginesDetected(Detection {
        engines: vec![
            engine("ollama", Status::Available, &["llama3:8b"]),
            engine("claude", Status::Available, &[]),
            engine("codex", Status::NotInstalled, &[]),
        ],
        ..Default::default()
    })
}

pub(super) fn prompt_ready(id: PaneId) -> AppEvent {
    AppEvent::PromptReady(
        id,
        crate::core::ai::chat::prompts::Resolved {
            text: "system".into(),
            notices: Vec::new(),
        },
    )
}

/// A ready chat (pane 2) on `ai`, both engines offering tools, on the test
/// clock.
pub(super) fn orch_app_with(ai: AiConfig) -> App {
    let mut a = launched(80, 25)
        .with_ai(ai)
        .with_tool_engines(&["claude", "ollama"])
        .with_clock(mock_now);
    a.update(detected());
    press(&mut a, "c");
    a.update(prompt_ready(chat_id()));
    a
}

pub(super) fn orch_app() -> App {
    orch_app_with(orch_ai())
}

/// Ctrl+O on the focused chat, a second after the last one so it is never
/// taken for a hold.
pub(super) fn toggle(a: &mut App) -> Vec<Effect> {
    let later = NOW_MS.with(|n| n.get()) + 1000;
    at(a, later, ctrl('o'))
}

/// The orchestrator turn among `effects`.
fn orchestrated(effects: &[Effect]) -> Option<(PaneId, u64, &ChatRequest, &OrchSend)> {
    effects.iter().find_map(|e| match e {
        Effect::OrchestrateSend {
            pane,
            turn,
            request,
            orch,
        } => Some((*pane, *turn, request, orch.as_ref())),
        _ => None,
    })
}

/// The chat turn among `effects`.
fn chatted(effects: &[Effect]) -> Option<(PaneId, u64, &ChatRequest)> {
    effects.iter().find_map(|e| match e {
        Effect::ChatSend {
            pane,
            turn,
            request,
        } => Some((*pane, *turn, request)),
        _ => None,
    })
}

fn sends(effects: &[Effect]) -> usize {
    let send = |e: &&Effect| matches!(e, Effect::ChatSend { .. } | Effect::OrchestrateSend { .. });
    effects.iter().filter(send).count()
}

fn ready(engine: &str, model: Option<&str>) -> Availability {
    Availability::Ready {
        engine: engine.into(),
        model: model.map(Into::into),
    }
}

/// A normal chat turn on `chat`: ollama, no orchestration tool offered.
fn assert_chat_turn(effects: &[Effect], pane: PaneId) {
    assert_eq!(sends(effects), 1, "{effects:?}");
    let (to, _, request) = chatted(effects).expect("a chat turn");
    assert_eq!(to, pane);
    assert_eq!(request.engine, "ollama");
    assert_eq!(request.model.as_deref(), Some("llama3:8b"));
    assert!(!request.families.contains(Family::Orchestra));
}

/// An orchestrator turn on `orchestrate`: claude with opus, offering the
/// orchestration tools.
fn assert_orch_turn(effects: &[Effect], pane: PaneId) {
    assert_eq!(sends(effects), 1, "{effects:?}");
    let (to, _, request, _) = orchestrated(effects).expect("an orchestrator turn");
    assert_eq!(to, pane);
    assert_eq!(request.engine, "claude");
    assert_eq!(request.model.as_deref(), Some("opus"));
    assert!(request.families.contains(Family::Orchestra));
    assert!(
        !request.families.contains(Family::Pane),
        "never run_in_pane"
    );
}

// agent-orchestrator / Orchestrate engine runs the turn; tasks.md 1e1.3: the
// turn carries the roster of every task, as the menu resolves it.
#[test]
fn an_orchestrating_pane_sends_on_the_orchestrate_engine_with_the_roster() {
    let mut a = orch_app();
    toggle(&mut a);
    let effects = ask(&mut a, "plan it");
    assert_orch_turn(&effects, chat_id());
    let (_, turn, request, orch) = orchestrated(&effects).unwrap();
    assert_eq!(turn, chat_of(&a).turn());
    assert!(request.tools != Tools::Off, "the session is opened");
    assert_eq!(
        orch.roster.get(Task::Orchestrate),
        &ready("claude", Some("opus"))
    );
    assert_eq!(
        orch.roster.get(Task::Chat),
        &ready("ollama", Some("llama3:8b"))
    );
    for task in Task::ALL {
        assert_ne!(orch.roster.get(task), &Availability::Checking, "{task:?}");
    }
    assert_eq!(orch.profile, Profile::default());
    assert_eq!(orch.deadline, Duration::from_secs(600));
    assert_eq!(composer(&a), "");
}

// agent-orchestrator / Unusable orchestrator engine.
#[test]
fn an_unusable_orchestrate_engine_sends_nothing_and_shows_its_guidance() {
    let mut ai = orch_ai();
    assign(&mut ai, Task::Orchestrate, "codex", None);
    let mut a = orch_app_with(ai.clone());
    toggle(&mut a);
    let effects = ask(&mut a, "plan it");
    assert_eq!(sends(&effects), 0, "{effects:?}");
    let text = a.chat_guidance(chat_id()).expect("the engine guidance");
    assert!(
        text.contains("codex") && text.contains("not installed"),
        "{text}"
    );
    assert_eq!(composer(&a), "plan it", "the text is kept");
    assert_eq!(chat_of(&a).messages().count(), 0);
    assert_eq!(a.ai(), &ai, "assignments unchanged");
}

// Tools off, or an orchestrate engine whose tools are not verified: the turn
// could not offer the orchestration tools, so it is refused (parent decision
// 2026-10-29).
#[test]
fn an_orchestrator_turn_without_tools_is_refused_with_guidance() {
    let mut off = orch_ai();
    off.set_tools_enabled(false);
    let unverified = orch_app().with_tool_engines(&["ollama"]);
    for (mut a, says) in [(orch_app_with(off), "tools"), (unverified, "claude")] {
        let before = a.ai().clone();
        toggle(&mut a);
        let effects = ask(&mut a, "plan it");
        assert_eq!(sends(&effects), 0, "{effects:?}");
        let text = a.chat_guidance(chat_id()).expect("guidance");
        assert!(text.contains(says), "{text}");
        assert_eq!(composer(&a), "plan it");
        assert_eq!(chat_of(&a).messages().count(), 0);
        assert_eq!(a.ai(), &before);
    }
}

// agent-orchestrator / Enabled alone does not orchestrate.
#[test]
fn the_gate_alone_sends_a_chat_turn() {
    let mut a = orch_app();
    let effects = ask(&mut a, "hello");
    assert_chat_turn(&effects, chat_id());
}

// agent-orchestrator / Toggle applies to the next message.
#[test]
fn the_toggle_applies_to_the_next_message() {
    let mut a = orch_app();
    let first = ask(&mut a, "one");
    assert_chat_turn(&first, chat_id());
    let (_, turn, _) = chatted(&first).unwrap();
    // Only the one-time question about interrupted runs (S9) leaves.
    let recover = Effect::Store(StoreCmd::RecoverRuns { pane: chat_id() });
    assert_eq!(
        toggle(&mut a),
        vec![recover],
        "nothing reaches the running turn"
    );
    assert!(a.orchestrating(chat_id()));
    a.update(finish(turn));
    let next = ask(&mut a, "two");
    assert_orch_turn(&next, chat_id());
}

// agent-orchestrator / Panes are independent.
#[test]
fn panes_are_independent() {
    let mut a = orch_app();
    toggle(&mut a);
    press(&mut a, "c");
    let b = a.focused();
    a.update(prompt_ready(b));
    let in_b = ask(&mut a, "in b");
    assert_chat_turn(&in_b, b);
    a.set_focus(chat_id());
    let in_a = ask(&mut a, "in a");
    assert_orch_turn(&in_a, chat_id());
}

// agent-orchestrator / Gate off then on.
#[test]
fn gate_off_then_on() {
    let mut a = orch_app();
    toggle(&mut a);
    a.ai.orchestrator.enabled = false;
    let off = ask(&mut a, "one");
    assert_chat_turn(&off, chat_id());
    a.update(finish(chatted(&off).unwrap().1));
    a.ai.orchestrator.enabled = true;
    let on = ask(&mut a, "two");
    assert_orch_turn(&on, chat_id());
}

// S5: with the gate off a pane whose mode is on sends exactly what a pane
// with the mode off sends.
#[test]
fn with_the_gate_off_the_chat_send_is_unchanged() {
    let mut ai = orch_ai();
    ai.orchestrator.enabled = false;
    let mut plain = orch_app_with(ai.clone());
    let mut flagged = orch_app_with(ai);
    let id = chat_id();
    flagged
        .panes
        .get_mut(&id)
        .and_then(PaneState::chat_mut)
        .unwrap()
        .set_orch(true);
    let expected = ask(&mut plain, "hello");
    assert_chat_turn(&expected, id);
    assert_eq!(ask(&mut flagged, "hello"), expected);
}

// A send that waited for the first detection and is then refused (tools off)
// goes nowhere and keeps its text in the composer, like a direct send
// (review R3/R4, 2026-10-29).
#[test]
fn a_waiting_orchestrator_send_refused_after_detection_keeps_its_text() {
    let mut ai = orch_ai();
    ai.set_tools_enabled(false);
    let mut a = launched(80, 25)
        .with_ai(ai)
        .with_tool_engines(&["claude", "ollama"])
        .with_clock(mock_now);
    press(&mut a, "c");
    a.update(prompt_ready(chat_id()));
    toggle(&mut a);
    assert_eq!(sends(&ask(&mut a, "plan it")), 0, "the send waits");
    let effects = a.update(detected());
    assert_eq!(sends(&effects), 0, "{effects:?}");
    assert_eq!(composer(&a), "plan it", "the text is kept");
    assert_eq!(chat_of(&a).messages().count(), 0);
    let text = a.chat_guidance(chat_id()).expect("the refusal");
    assert!(text.contains("tools"), "{text}");
}
