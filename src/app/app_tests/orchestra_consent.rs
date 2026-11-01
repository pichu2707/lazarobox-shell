//! The consent ask of a receipt-driven review (agent-orchestrator S7b, D16):
//! an inline prompt of the chat pane, a worker ask like the others (FIFO,
//! only the shown one touches the chat), answered only by the user's keys.

use super::tool_approval::{announce, live, pending, worker_decision};
use super::*;
use crate::core::ai::chat::{
    approval::{ApprovalAsk, answers_itself},
    engine::Decision,
};
use crate::core::ai::config::ApprovalMode;
use crate::core::ai::orchestra::WorkerKey;

fn consent(worker: u32, label: &str) -> ApprovalAsk {
    ApprovalAsk::Consent {
        worker: WorkerKey::for_test(1, worker),
        label: label.to_owned(),
        change: "Cargo.lock, .github/ci.yml".to_owned(),
    }
}

// Spec: Consent first. The prompt names the worker and the change, and the
// keys say what each answer does to the review.
#[test]
fn the_consent_prompt_names_the_worker_and_the_change() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, consent(1, "code claude>opus"));
    let prompt = a.approval_prompt(id2()).unwrap();
    assert!(prompt.target.contains("code claude>opus"), "{prompt:?}");
    assert!(prompt.target.contains("review"), "{prompt:?}");
    assert_eq!(prompt.command, "Cargo.lock, .github/ci.yml");
    assert!(prompt.keys.contains("y review it"), "{prompt:?}");
    assert!(prompt.keys.contains("n skip the review"), "{prompt:?}");
}

// y consents; n declines (the host then marks the change `not reviewed`).
#[test]
fn y_consents_and_n_declines_to_the_workers_session() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, consent(1, "code claude>opus"));
    assert_eq!(
        a.update(key('y')),
        vec![worker_decision(turn, 1, Decision::Sent)]
    );
    announce(&mut a, turn, 2, consent(1, "code claude>opus"));
    assert_eq!(
        a.update(key('n')),
        vec![worker_decision(
            turn,
            2,
            Decision::Denied("the user declined")
        )]
    );
    assert_eq!(pending(&a), None);
}

// No approval mode answers it: not `run`, not `auto`, not `ask`.
#[test]
fn no_approval_mode_answers_a_consent() {
    for mode in [ApprovalMode::Ask, ApprovalMode::Run, ApprovalMode::Auto] {
        assert_eq!(answers_itself(&consent(1, "code x>y"), mode), None);
    }
}

// It queues behind an ask on screen like any worker ask, and the shown one
// is the only one that touches the chat.
#[test]
fn a_consent_waits_its_turn_in_the_queue() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, consent(1, "code claude>opus"));
    assert!(announce(&mut a, turn, 2, consent(2, "code codex>gpt")).is_empty());
    let first = a.approval_prompt(id2()).unwrap();
    assert!(first.target.contains("code claude>opus"), "{first:?}");
    assert!(first.target.contains("1 waiting"), "{first:?}");
    assert_eq!(
        a.update(key('n')),
        vec![worker_decision(
            turn,
            1,
            Decision::Denied("the user declined")
        )]
    );
    let second = a.approval_prompt(id2()).unwrap();
    assert!(second.target.contains("code codex>gpt"), "{second:?}");
    assert!(!second.armed, "a promoted ask is armed afresh");
}
