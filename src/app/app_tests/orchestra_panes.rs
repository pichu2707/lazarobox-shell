//! The coordinator's pane reads in the app (agent-tools, "Toolsets by session
//! role" and "read_pane" [W11]): an orchestrator turn offers `list_panes` and
//! `read_pane`, never `run_in_pane`, and their asks open the same prompts as a
//! chat's, which no approval mode answers.

use super::chat_live::chat_id;
use super::orchestra_run::running;
use super::orchestra_send::{orch_app, orchestrated, toggle};
use super::tool_approval::{decision, pending, shell_output};
use super::*;
use crate::core::ai::{
    chat::{
        approval::ApprovalAsk,
        engine::{CallId, Decision},
    },
    config::ApprovalMode,
    tools::registry_with,
};

/// The ask of call `call` of the coordinator's turn `turn` arrives.
fn coordinator_asks(a: &mut App, turn: u64, call: u64, ask: ApprovalAsk) -> Vec<Effect> {
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::ToolCall {
            id: CallId(call),
            name: "tool".into(),
            summary: "args".into(),
            approval: Some(Box::new(ask)),
        },
    ))
}

fn set_mode(a: &mut App, mode: ApprovalMode) {
    (a.panes.get_mut(&chat_id()).and_then(PaneState::chat_mut))
        .expect("the chat")
        .set_approval_mode(mode);
}

fn screen_ask(pane: u32) -> ApprovalAsk {
    ApprovalAsk::Screen {
        pane,
        scrollback: None,
        review: None,
    }
}

// The orchestrator turn's request offers the two pane reads and never
// `run_in_pane`: what the engine is offered is what the host accepts.
#[test]
fn an_orchestrator_turn_offers_the_pane_reads_but_not_run_in_pane() {
    let mut a = orch_app();
    toggle(&mut a);
    let effects = super::chat_live::ask(&mut a, "what is on my screen?");
    let (_, _, request, _) = orchestrated(&effects).expect("an orchestrator turn");
    let offered = registry_with(&[], request.families);
    let names: Vec<_> = offered.iter().map(|tool| tool.name.as_str()).collect();
    assert!(names.contains(&"list_panes"), "{names:?}");
    assert!(names.contains(&"read_pane"), "{names:?}");
    assert!(!names.contains(&"run_in_pane"), "{names:?}");
}

// A cloud coordinator's `list_panes` opens the prompt with the exact text,
// as a chat's does, in every approval mode: no mode answers it.
#[test]
fn the_coordinators_list_panes_ask_is_answered_by_no_mode() {
    for mode in [ApprovalMode::Ask, ApprovalMode::Auto, ApprovalMode::Run] {
        let (mut a, turn) = running();
        set_mode(&mut a, mode);
        let ask = ApprovalAsk::Panes {
            text: "pane 1 (focused) cwd: ~/app program: nvim prueba.rs".into(),
        };
        assert_eq!(
            coordinator_asks(&mut a, turn, 1, ask.clone()),
            vec![],
            "{mode:?}"
        );
        assert_eq!(
            pending(&a),
            Some((ask, Some((turn, CallId(1))))),
            "{mode:?}"
        );
    }
}

// The coordinator's `read_pane` freezes the screen of another pane and opens
// its review in every mode; the chat's own pane is never read.
#[test]
fn the_coordinator_reads_another_pane_only_through_the_review() {
    for mode in [ApprovalMode::Ask, ApprovalMode::Auto, ApprovalMode::Run] {
        let (mut a, turn) = running();
        set_mode(&mut a, mode);
        shell_output(&mut a, b"$ nvim prueba.rs\r\n");
        assert_eq!(
            coordinator_asks(&mut a, turn, 1, screen_ask(chat_id().number())),
            vec![decision(turn, 1, Decision::Denied("the pane is not open"))],
            "{mode:?}: the chat's own pane"
        );
        assert_eq!(pending(&a), None, "{mode:?}");
        assert_eq!(
            coordinator_asks(&mut a, turn, 2, screen_ask(PaneId::FIRST.number())),
            vec![],
            "{mode:?}"
        );
        match pending(&a) {
            Some((
                ApprovalAsk::Screen {
                    review: Some(review),
                    ..
                },
                call,
            )) => {
                assert_eq!(call, Some((turn, CallId(2))), "{mode:?}");
                assert!(review.text().contains("nvim prueba.rs"), "{mode:?}");
            }
            other => panic!("{mode:?}: no screen review pending: {other:?}"),
        }
    }
}
