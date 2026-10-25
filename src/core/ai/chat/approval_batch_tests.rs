//! The batch ask: routing, the view it carries and what never leaves it.

use std::time::Instant;

use super::approval::*;
use super::engine::CallId;
use super::review::TOO_LARGE;
use super::review_view::{
    ReviewView,
    testing::{edit_of, numbered},
};
use crate::core::ai::config::ApprovalMode;

const FREE: AutoCtx = AutoCtx {
    typed_this_turn: 0,
    line_pending: false,
    target_focused: false,
    tab_shown: true,
    executing: false,
    cwd_known: true,
    paused: false,
    dirty: false,
    utf8: true,
};

fn batch() -> ApprovalAsk {
    ApprovalAsk::Batch(numbered(3))
}

fn view_of(ask: &ApprovalAsk) -> ReviewView {
    let ApprovalAsk::Batch(review) = ask else {
        panic!("a batch ask");
    };
    ReviewView::new(review.clone(), Instant::now(), (40, 12))
}

#[test]
fn a_batch_prompts_in_every_mode_and_for_every_origin() {
    let ask = batch();
    for mode in [ApprovalMode::Ask, ApprovalMode::Auto, ApprovalMode::Run] {
        for origin in [Origin::Tool, Origin::Block] {
            assert_eq!(
                route(&ask, origin, mode, FREE),
                Route::Prompt(None),
                "{mode:?} {origin:?}"
            );
        }
    }
}

#[test]
fn the_view_lives_as_long_as_the_approval() {
    let mut approvals = Approvals::default();
    let ask = batch();
    assert!(approvals.review().is_none());
    let id = approvals
        .request_call(ask.clone(), 1, CallId(1), None)
        .unwrap();
    approvals.set_review(view_of(&ask));
    assert!(approvals.review().is_some());
    approvals.answer(id, Reply::Decline, Press::Press).unwrap();
    assert!(approvals.review().is_none(), "answered");

    let id = approvals
        .request_call(ask.clone(), 1, CallId(2), None)
        .unwrap();
    approvals.set_review(view_of(&ask));
    approvals.withdraw(id).unwrap();
    assert!(approvals.review().is_none(), "withdrawn");
}

#[test]
fn a_repeat_or_a_stale_id_leaves_the_view_alone() {
    let mut approvals = Approvals::default();
    let ask = batch();
    let id = approvals
        .request_call(ask.clone(), 1, CallId(1), None)
        .unwrap();
    approvals.set_review(view_of(&ask));
    assert!(
        approvals
            .answer(id, Reply::Approve, Press::Repeat)
            .is_none()
    );
    assert!(approvals.withdraw(id + 1).is_none());
    assert!(approvals.review().is_some());
}

// Review-only invariant: the text of a diff may hold a secret the model never
// saw. Nothing built for the model, a log or an error carries it.
#[test]
fn a_secret_in_the_base_or_the_new_text_reaches_nothing_but_the_renderer() {
    let secrets = ["sk-ant-api03-SECRETSECRETSECRET", "hunter2"];
    let review = edit_of(
        ".env.example",
        "API_KEY=sk-ant-api03-SECRETSECRETSECRET\nkeep\n",
        "API_KEY=sk-ant-api03-SECRETSECRETSECRET",
        "password = hunter2",
    );
    // The rows do hold the text: the test is not vacuous.
    let view = ReviewView::new(review.clone(), Instant::now(), (80, 20));
    let shown: String = view
        .visible(20)
        .iter()
        .map(|row| row.text_for_review_renderer_only())
        .collect();
    assert!(secrets.iter().all(|s| shown.contains(s)), "{shown}");

    let ask = ApprovalAsk::Batch(review.clone());
    let mut approvals = Approvals::default();
    approvals
        .request_call(ask.clone(), 1, CallId(1), None)
        .unwrap();
    approvals.set_review(view.clone());
    let model_facing = [
        review.counts(),
        TOO_LARGE.to_owned(),
        format!("{review:?}"),
        format!("{:?}", review.doc()),
        format!("{ask:?}"),
        format!("{view:?}"),
        format!("{approvals:?}"),
    ];
    for text in &model_facing {
        for secret in secrets {
            assert!(!text.contains(secret), "{secret} in {text}");
        }
    }
    assert_eq!(review.counts(), "1 file, +1 -1");
}
