//! The review view of a proposed write batch in the app: it opens with the
//! `Batch` ask, `y` obeys the coverage gate, and `n` or Esc rejects the whole
//! batch. The keys that scroll are the ones that make rows count as seen.
//!
//! Nothing here reads the text of the batch: the answer to the model is a
//! fixed reason and the system line holds counts only.

use std::sync::Arc;

use super::{
    tool::{CANCELLED, DECLINED},
    *,
};
use crate::core::ai::chat::{
    approval::Reply,
    engine::Decision,
    review::{BatchReview, TOO_LARGE},
    review_view::{Move, ReviewView},
};

impl App {
    /// Whether chat `id` has a batch under review.
    pub(super) fn batch_pending(&self, id: PaneId) -> bool {
        self.panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .is_some_and(|chat| chat.approvals().review().is_some())
    }

    /// The size of the pane the view is drawn in, in columns and rows.
    fn review_size(&self, id: PaneId) -> (usize, usize) {
        self.panes.get(&id).map_or((80, 24), |state| {
            (usize::from(state.size.cols), usize::from(state.size.rows))
        })
    }

    /// Opens the view of `review` for the batch ask that was just requested.
    pub(super) fn open_review(&mut self, id: PaneId, review: Arc<BatchReview>) {
        let size = self.review_size(id);
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            chat.approvals_mut()
                .set_review(ReviewView::new(review, size));
        }
    }

    /// A frame was drawn: every review counts the rows that frame showed, and
    /// the first frame starts its delay. A review in a hidden pane was not
    /// drawn, so it counts nothing.
    pub fn frame_drawn(&mut self) {
        for state in self.panes.values_mut() {
            let redraw = state
                .chat_mut()
                .and_then(|chat| chat.approvals_mut().review_mut())
                .is_some_and(ReviewView::credit_drawn);
            self.dirty |= redraw;
        }
    }

    /// Asks for a frame while a countdown shows and once when it ends, so
    /// `y in 0.1s` turns into `y ready` without any other event. The event
    /// loop calls it on its own tick.
    pub fn wake_review(&mut self) {
        let now = (self.clock)();
        for state in self.panes.values_mut() {
            let wake = state
                .chat_mut()
                .and_then(|chat| chat.approvals_mut().review_mut())
                .is_some_and(|view| view.wake(now));
            self.dirty |= wake;
        }
    }

    /// Whether `n` and Esc answer the batch of chat `id` now.
    fn review_answerable(&mut self, id: PaneId) -> bool {
        let now = (self.clock)();
        self.review_mut(id).is_some_and(|view| view.answerable(now))
    }

    /// A key while a batch is under review. Scrolling keys (a held one
    /// repeats) mark the rows they bring on screen as seen; `y` approves only
    /// when the whole batch was seen and the delay is over; `n` and Esc
    /// reject; every other key is swallowed.
    pub(super) fn on_review_key(&mut self, id: PaneId, key: KeyEvent, repeat: bool) -> Vec<Effect> {
        let size = self.review_size(id);
        let by = match (key.code, key.modifiers) {
            (KeyCode::Up | KeyCode::Char('k'), KeyModifiers::NONE) => Some(Move::Up),
            (KeyCode::Down | KeyCode::Char('j'), KeyModifiers::NONE) => Some(Move::Down),
            (KeyCode::PageUp, KeyModifiers::NONE) => Some(Move::PageUp),
            (KeyCode::PageDown, KeyModifiers::NONE) => Some(Move::PageDown),
            (KeyCode::Home, KeyModifiers::NONE) => Some(Move::Top),
            (KeyCode::End, KeyModifiers::NONE) => Some(Move::Bottom),
            _ => None,
        };
        if let Some(by) = by {
            if let Some(view) = self.review_mut(id) {
                view.scroll(by, size);
                self.dirty = true;
            }
            return Vec::new();
        }
        let answers = matches!(
            (key.code, key.modifiers),
            (KeyCode::Char('y' | 'n') | KeyCode::Esc, KeyModifiers::NONE)
        );
        if repeat || (answers && !self.review_answerable(id)) {
            return Vec::new();
        }
        match (key.code, key.modifiers) {
            (KeyCode::Char('y'), KeyModifiers::NONE) => self.approve_batch(id, size),
            (KeyCode::Char('n') | KeyCode::Esc, KeyModifiers::NONE) => self.reject_batch(id),
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                let mut effects = self.decline(id, CANCELLED);
                effects.extend(self.chat_cancel_key(id));
                effects
            }
            _ => Vec::new(),
        }
    }

    /// The pane of chat `id` changed size: the rows on screen are counted at
    /// the new size.
    pub(super) fn refit_review(&mut self, id: PaneId) {
        let size = self.review_size(id);
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            chat.approvals_mut().fit_review(size);
        }
    }

    fn review_mut(&mut self, id: PaneId) -> Option<&mut ReviewView> {
        (self.panes.get_mut(&id)?.chat_mut()?)
            .approvals_mut()
            .review_mut()
    }

    /// `y`: the batch goes ahead only if the gate is open for the pane as it
    /// is now. Otherwise nothing happens and the view says what is missing.
    fn approve_batch(&mut self, id: PaneId, size: (usize, usize)) -> Vec<Effect> {
        let now = (self.clock)();
        let open = self.review_mut(id).is_some_and(|view| {
            view.fit(size);
            view.can_approve(now)
        });
        if !open {
            return Vec::new();
        }
        self.close_approval(id, Reply::Approve)
            .and_then(|approval| Self::answer(id, &approval, Decision::Sent))
            .into_iter()
            .collect()
    }

    /// `n` and Esc: the whole batch is rejected. The model is told why in
    /// fixed words (too large, or declined), and the transcript gets a line
    /// with the counts.
    fn reject_batch(&mut self, id: PaneId) -> Vec<Effect> {
        let Some(view) = self.review_mut(id) else {
            return Vec::new();
        };
        let reason = if view.reviewable() {
            DECLINED
        } else {
            TOO_LARGE
        };
        let counts = view.review().counts();
        let effects = self.decline(id, reason);
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            chat.push_system_line(format!("batch rejected: {counts}"));
        }
        effects
    }
}
