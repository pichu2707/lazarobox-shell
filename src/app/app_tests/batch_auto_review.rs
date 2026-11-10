//! local-model-writes D1: a turn that ends with `Done` while its pane holds a
//! staged batch the model never proposed opens that batch's review, through
//! the orphan path (`Effect::BatchReview`). A cancelled or failed turn, or a
//! review already pending, leaves the orphan badge and opens nothing.

use super::chat_live::ctrl;
use super::tool_approval::live;
use super::*;
use crate::core::ai::{
    chat::{
        engine::{ChatError, Finish},
        review::{Badge, BatchEvent},
        review_view::testing::numbered,
    },
    tools::stage::{BatchId, StageKey},
};
use std::collections::HashMap;

fn badges(badge: Option<Badge>) -> HashMap<PaneId, Badge> {
    badge.map(|b| (id2(), b)).into_iter().collect()
}

fn poll(a: &mut App, badge: Option<Badge>) -> Vec<Effect> {
    a.update(AppEvent::Badges(badges(badge)))
}

fn end(a: &mut App, turn: u64, event: StreamEvent) {
    a.update(AppEvent::Chat(id2(), turn, event));
}

// D1: the poll that sees the batch left behind by a clean end opens it.
#[test]
fn a_turn_that_ends_with_a_staged_batch_opens_its_review() {
    let (mut a, turn) = live();
    assert_eq!(poll(&mut a, Some(Badge::Staged(2))), vec![]);
    end(&mut a, turn, StreamEvent::Done(Finish::Stop));
    assert_eq!(
        poll(&mut a, Some(Badge::Orphan(2))),
        vec![Effect::BatchReview(id2())]
    );
    assert_eq!(
        poll(&mut a, Some(Badge::Orphan(2))),
        vec![],
        "once per turn"
    );
}

// D1: a turn short enough to stage and end between two polls counts too.
#[test]
fn a_batch_first_seen_as_an_orphan_after_a_clean_end_opens_its_review() {
    let (mut a, turn) = live();
    end(&mut a, turn, StreamEvent::Done(Finish::Stop));
    assert_eq!(
        poll(&mut a, Some(Badge::Orphan(1))),
        vec![Effect::BatchReview(id2())]
    );
}

// D1: a turn that ends with no batch opens nothing.
#[test]
fn a_clean_end_with_no_batch_opens_nothing() {
    let (mut a, turn) = live();
    end(&mut a, turn, StreamEvent::Done(Finish::Stop));
    assert_eq!(poll(&mut a, None), vec![]);
}

// D1: the user stopped the turn, or it failed: the orphan waits for Ctrl+G.
#[test]
fn a_cancelled_or_failed_turn_keeps_the_orphan() {
    let (mut a, _) = live();
    poll(&mut a, Some(Badge::Staged(2)));
    a.update(ctrl('c'));
    assert_eq!(poll(&mut a, Some(Badge::Orphan(2))), vec![]);
    assert_eq!(
        super::tool_approval::chat(&a, id2()).batch_badge(),
        Some(Badge::Orphan(2))
    );

    let (mut a, turn) = live();
    poll(&mut a, Some(Badge::Staged(2)));
    end(
        &mut a,
        turn,
        StreamEvent::Error(ChatError::Failed("x".into())),
    );
    assert_eq!(poll(&mut a, Some(Badge::Orphan(2))), vec![]);
}

// D1: a clean end of an earlier turn does not open the orphan of a later,
// cancelled one.
#[test]
fn only_the_last_turn_ending_cleanly_opens_the_review() {
    let (mut a, turn) = live();
    end(&mut a, turn, StreamEvent::Done(Finish::Stop));
    poll(&mut a, None);
    a.update(key('q'));
    super::run_pane::enter_turn(&mut a);
    poll(&mut a, Some(Badge::Staged(1)));
    a.update(ctrl('c'));
    assert_eq!(poll(&mut a, Some(Badge::Orphan(1))), vec![]);
}

// D1: a review that is already open is not asked for twice.
#[test]
fn a_pending_review_is_not_asked_for_again() {
    let (mut a, turn) = live();
    end(&mut a, turn, StreamEvent::Done(Finish::Stop));
    a.update(AppEvent::Batch(
        id2(),
        BatchEvent::Review(
            BatchId {
                key: StageKey::Pane(id2()),
                generation: 7,
            },
            numbered(1),
        ),
    ));
    assert!(super::tool_approval::pending(&a).is_some());
    assert_eq!(poll(&mut a, Some(Badge::Orphan(1))), vec![]);
}
