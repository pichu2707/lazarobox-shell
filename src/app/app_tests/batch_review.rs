//! The review of a proposed write batch: a full-pane view that opens for a
//! `Batch` ask in every approval mode, a coverage gate on `y`, and `n` or Esc
//! that rejects the whole batch.

use super::chat_live::{ctrl, plain};
use super::tool_approval::{announce_now, chat, decision, live, pending, result};
use super::*;
use crate::app::tool::DECLINED;
use crate::core::ai::chat::{
    approval::ApprovalAsk,
    engine::{CallId, Decision, ToolStatus},
    review::{ARM_DELAY, TOO_LARGE},
    review_view::{ReviewView, testing::numbered},
};
use crate::core::ai::config::ApprovalMode;

fn batch(n: usize) -> ApprovalAsk {
    ApprovalAsk::Batch(numbered(n))
}

fn review(a: &App) -> &ReviewView {
    chat(a, id2()).approvals().review().expect("a review view")
}

fn ms(d: std::time::Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap()
}

/// A batch of `n` rows asked at t = 0.
fn asked(n: usize) -> (App, u64) {
    let (mut a, turn) = live();
    announce_now(&mut a, turn, 1, batch(n));
    (a, turn)
}

/// Pages down at `t` until nothing is unseen or the end is on screen.
fn read_all(a: &mut App, t: u64) {
    for _ in 0..500 {
        if review(a).remaining() == 0 {
            return;
        }
        at(a, t, plain(KeyCode::PageDown));
    }
}

fn system_lines(a: &App) -> Vec<String> {
    chat(a, id2())
        .lines(80)
        .iter()
        .map(|l| l.text.clone())
        .filter(|t| t.contains("batch"))
        .collect()
}

#[test]
fn a_batch_opens_the_review_in_every_approval_mode_and_never_runs_by_itself() {
    for mode in [ApprovalMode::Ask, ApprovalMode::Auto, ApprovalMode::Run] {
        let (mut a, turn) = live();
        a.panes
            .get_mut(&id2())
            .and_then(PaneState::chat_mut)
            .unwrap()
            .set_approval_mode(mode);
        assert_eq!(announce_now(&mut a, turn, 1, batch(5)), vec![], "{mode:?}");
        assert_eq!(pending(&a).map(|p| p.1), Some(Some((turn, CallId(1)))));
        assert!(review(&a).total() > 0, "{mode:?}");
        // The overlay of the other asks is not drawn for it.
        assert_eq!(a.approval_prompt(id2()), None);
    }
}

#[test]
fn y_does_nothing_while_lines_are_unseen_however_late() {
    let (mut a, _) = asked(200);
    assert!(review(&a).remaining() > 0);
    assert_eq!(at(&mut a, 60_000, key('y')), vec![]);
    assert!(pending(&a).is_some(), "still waiting");
}

#[test]
fn y_arms_at_full_coverage_plus_the_review_delay() {
    let (mut a, turn) = asked(60);
    read_all(&mut a, 100);
    assert_eq!(review(&a).remaining(), 0);
    assert_eq!(at(&mut a, ms(ARM_DELAY) - 1, key('y')), vec![]);
    assert_eq!(
        at(&mut a, ms(ARM_DELAY), key('y')),
        vec![decision(turn, 1, Decision::Sent)]
    );
    assert_eq!(pending(&a), None);
    assert!(chat(&a, id2()).approvals().review().is_none());
}

#[test]
fn a_held_y_never_approves() {
    let (mut a, _) = asked(2);
    read_all(&mut a, 100);
    let held = super::chat_live::repeat(KeyCode::Char('y'), KeyModifiers::NONE);
    assert_eq!(at(&mut a, 5_000, held), vec![]);
    assert!(pending(&a).is_some());
}

#[test]
fn scrolling_keys_mark_rows_seen_and_other_keys_do_nothing() {
    let (mut a, _) = asked(100);
    let before = review(&a).remaining();
    at(&mut a, 1, plain(KeyCode::Down));
    assert_eq!((review(&a).top(), review(&a).remaining()), (1, before - 1));
    at(&mut a, 2, key('j'));
    assert_eq!(review(&a).top(), 2);
    at(&mut a, 3, key('k'));
    at(&mut a, 4, plain(KeyCode::Up));
    assert_eq!(review(&a).top(), 0);
    at(&mut a, 5, plain(KeyCode::PageDown));
    assert!(review(&a).top() > 2);
    at(&mut a, 6, plain(KeyCode::End));
    let end = review(&a).top();
    at(&mut a, 7, plain(KeyCode::Home));
    assert_eq!(review(&a).top(), 0);
    assert!(end > 50);
    for other in [key('a'), plain(KeyCode::Enter), key('Y'), key(' ')] {
        assert_eq!(at(&mut a, 8, other), vec![]);
    }
    assert_eq!(review(&a).top(), 0);
    // A held arrow keeps scrolling.
    let held = super::chat_live::repeat(KeyCode::Down, KeyModifiers::NONE);
    at(&mut a, 9, held);
    assert_eq!(review(&a).top(), 1);
}

#[test]
fn n_and_esc_reject_the_whole_batch_with_a_system_line() {
    for rejecting in [key('n'), plain(KeyCode::Esc)] {
        let (mut a, turn) = asked(5);
        assert_eq!(
            at(&mut a, ms(ARM_DELAY), rejecting),
            vec![decision(turn, 1, Decision::Denied(DECLINED))]
        );
        assert_eq!(pending(&a), None);
        assert_eq!(system_lines(&a), ["• batch rejected: 1 file, +5 -0"]);
    }
}

#[test]
fn ctrl_c_cancels_the_turn_and_denies_the_batch() {
    let (mut a, turn) = asked(5);
    let effects = at(&mut a, 0, ctrl('c'));
    assert_eq!(
        effects,
        vec![
            decision(turn, 1, Decision::Denied("cancelled")),
            Effect::ChatCancel(id2())
        ]
    );
}

#[test]
fn a_batch_too_large_to_review_never_arms_and_tells_the_model_to_split() {
    let (mut a, turn) = asked(2100);
    assert!(!review(&a).reviewable());
    read_all(&mut a, 100);
    assert_eq!(review(&a).remaining(), 0);
    assert_eq!(at(&mut a, 600_000, key('y')), vec![]);
    assert!(pending(&a).is_some());
    assert_eq!(
        at(&mut a, 600_001, key('n')),
        vec![decision(turn, 1, Decision::Denied(TOO_LARGE))]
    );
    assert!(TOO_LARGE.contains("too large") && TOO_LARGE.contains("smaller batches"));
}

#[test]
fn an_empty_batch_is_rejected_with_the_same_answer() {
    let (mut a, turn) = live();
    let empty = ApprovalAsk::Batch(std::sync::Arc::new(
        crate::core::ai::chat::review::BatchReview::new(
            crate::core::ai::chat::review::ReviewDoc::build(
                &crate::core::ai::tools::stage::Batch::new(
                    crate::core::ai::tools::stage::BatchId {
                        key: crate::core::ai::tools::stage::StageKey::Pane(id2()),
                        generation: 1,
                    },
                ),
            ),
        ),
    ));
    announce_now(&mut a, turn, 1, empty);
    assert_eq!(at(&mut a, 600_000, key('y')), vec![]);
    assert_eq!(
        at(&mut a, 600_001, key('n')),
        vec![decision(turn, 1, Decision::Denied(TOO_LARGE))]
    );
}

#[test]
fn a_call_that_ends_takes_the_review_with_it() {
    let (mut a, turn) = asked(5);
    result(&mut a, turn, 1, ToolStatus::Denied);
    assert_eq!(pending(&a), None);
    assert!(chat(&a, id2()).approvals().review().is_none());
}

#[test]
fn a_resize_keeps_what_was_seen_and_counts_the_new_screen() {
    let (mut a, _) = asked(60);
    read_all(&mut a, 100);
    assert_eq!(review(&a).remaining(), 0);
    a.update(AppEvent::Resize { cols: 50, rows: 12 });
    assert_eq!(review(&a).remaining(), 0, "no line is lost");
}
