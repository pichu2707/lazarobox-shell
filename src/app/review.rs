//! The review view of a proposed write batch in the app: it opens with the
//! `Batch` ask, `y` obeys the coverage gate, and `n` or Esc rejects the whole
//! batch. The keys that scroll bring rows on screen; a row counts as seen
//! only once a frame has drawn it (`App::frame_drawn`).
//!
//! Nothing here reads the text of the batch: the answer to the model is a
//! fixed reason and the system line holds counts only.

use std::{collections::HashMap, sync::Arc};

use super::{
    tool::{CANCELLED, DECLINED},
    *,
};
use crate::core::ai::chat::{
    approval::Reply,
    approval::{ApprovalAsk, Origin},
    engine::Decision,
    review::{ARM_DELAY, Badge, BatchEvent, BatchReview, NO_ORPHAN, TOO_LARGE, closed_notice},
    review_view::{Move, ReviewView},
};
use crate::core::ai::tools::stage::BatchId;

impl App {
    /// What the runtime reports about a batch outside a model turn. A closed
    /// chat and an answered orphan show a line; a review opens the batch view.
    pub(super) fn on_batch_event(&mut self, pane: PaneId, event: BatchEvent) -> Vec<Effect> {
        match event {
            BatchEvent::Closed(entries) => self.notice = Some(closed_notice(entries)),
            BatchEvent::Done { shown, told } => {
                self.notice = Some(shown);
                // The model hears of it with the chat's next request.
                if let (Some(line), Some(chat)) = (
                    told,
                    self.panes.get_mut(&pane).and_then(PaneState::chat_mut),
                ) {
                    chat.tell(line);
                }
            }
            BatchEvent::Review(batch, review) => self.open_orphan_review(pane, batch, review),
        }
        self.dirty = true;
        Vec::new()
    }

    /// The batches the runtime holds, by pane (it polls on its tick): each
    /// chat wears the badge of its own, or none. Redraws only on a change.
    /// A batch that turns into an orphan when the last turn ended with
    /// `Done` (the model staged it and never proposed it) has its review
    /// asked for at once, unless the chat already waits on an approval
    /// (local-model-writes D1). A cancelled or failed turn leaves the orphan
    /// to Ctrl+G.
    pub fn sync_batches(&mut self, badges: &HashMap<PaneId, Badge>) -> Vec<Effect> {
        let mut changed = false;
        let mut effects = Vec::new();
        for (id, state) in &mut self.panes {
            let Some(chat) = state.chat_mut() else {
                continue;
            };
            let badge = badges.get(id).copied();
            if chat.batch_badge() != badge {
                let was_orphan = matches!(chat.batch_badge(), Some(Badge::Orphan(_)));
                chat.set_batch_badge(badge);
                changed = true;
                if matches!(badge, Some(Badge::Orphan(_)))
                    && !was_orphan
                    && chat.take_clean_end()
                    && chat.approvals().pending().is_none()
                {
                    effects.push(Effect::BatchReview(*id));
                }
            }
        }
        self.dirty |= changed;
        effects
    }

    /// Ctrl+G: asks the runtime for the review of the orphan chat `id` wears.
    /// A batch the model still stages is not the user's to review.
    pub(super) fn chat_review_orphan(&mut self, id: PaneId) -> Vec<Effect> {
        let orphan = (self.panes.get(&id).and_then(PaneState::chat_state))
            .is_some_and(|chat| matches!(chat.batch_badge(), Some(Badge::Orphan(_))));
        if orphan {
            return vec![Effect::BatchReview(id)];
        }
        self.notice = Some(NO_ORPHAN.to_owned());
        self.dirty = true;
        Vec::new()
    }

    /// Opens the review of the orphan `batch` as a batch ask without a call:
    /// the same view and gate, but its answer goes to the batch by its id.
    fn open_orphan_review(&mut self, id: PaneId, batch: BatchId, review: Arc<BatchReview>) {
        let armed_at = (self.clock)() + ARM_DELAY;
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return;
        };
        let ask = ApprovalAsk::Batch(review.clone());
        match chat.approvals_mut().request(ask, Origin::Block) {
            Ok(_) => {
                chat.set_armed_at(Some(armed_at));
                chat.set_orphan(Some(batch));
                self.open_review(id, review);
            }
            Err(refusal) => self.notice = Some(refusal.reason().to_owned()),
        }
    }

    /// The orphan chat `id` is reviewing, when the pending batch ask is
    /// that review and not a model's call.
    fn reviewed_orphan(&self, id: PaneId) -> Option<BatchId> {
        let chat = self.panes.get(&id)?.chat_state()?;
        chat.approvals().pending()?.call().is_none().then_some(())?;
        chat.orphan()
    }

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

    /// A frame was drawn: an install prompt becomes answerable, every review
    /// counts the rows that frame showed, and the first frame starts its
    /// delay. A review in a hidden pane was not drawn, so it counts nothing.
    pub fn frame_drawn(&mut self) {
        // The install wizard's prompts accept `y`/`n` only once drawn.
        if let Some(menu) = self.menu.as_mut() {
            menu.install_arm();
        }
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
    /// repeats) bring rows on screen; `y` approves only when the whole batch
    /// was seen and the delay is over; `n` and Esc reject after the same
    /// delay (at once when the batch cannot be reviewed); every other key is
    /// swallowed.
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
            self.scroll_review(id, by);
            return Vec::new();
        }
        let answers = matches!(
            (key.code, key.modifiers),
            (KeyCode::Char('y' | 'n') | KeyCode::Esc, KeyModifiers::NONE)
        );
        // A `y` the gate will refuse says why; the gate itself is untouched.
        if !repeat && key.code == KeyCode::Char('y') && key.modifiers == KeyModifiers::NONE {
            let now = (self.clock)();
            if let Some(view) = self.review_mut(id) {
                view.y_pressed(now);
                self.dirty = true;
            }
        }
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

    /// Scrolls the review of chat `id`; the rows count once a frame draws them.
    pub(super) fn scroll_review(&mut self, id: PaneId, by: Move) {
        let size = self.review_size(id);
        if let Some(view) = self.review_mut(id) {
            view.scroll(by, size);
            self.dirty = true;
        }
    }

    /// The pane of chat `id` changed size: the rows are laid out again; the
    /// next frame counts what it shows.
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
        let orphan = self.reviewed_orphan(id);
        let closed = self.close_approval(id, Reply::Approve);
        if let Some(batch) = orphan {
            return vec![Effect::BatchApply {
                pane: id,
                id: batch,
            }];
        }
        closed
            .and_then(|approval| Self::answer(id, &approval, Decision::Sent))
            .into_iter()
            .collect()
    }

    /// `n` and Esc: the whole batch is rejected. The model is told why in
    /// fixed words (too large, or declined), and the transcript gets a line
    /// with the counts.
    fn reject_batch(&mut self, id: PaneId) -> Vec<Effect> {
        if let Some(batch) = self.reviewed_orphan(id) {
            self.close_approval(id, Reply::Decline);
            return vec![Effect::BatchDiscard {
                pane: id,
                id: batch,
            }];
        }
        let commit = (self.panes.get(&id))
            .and_then(PaneState::chat_state)
            .and_then(|chat| chat.approvals().pending())
            .is_some_and(|approval| matches!(approval.ask(), ApprovalAsk::Commit(_)));
        let Some(view) = self.review_mut(id) else {
            return Vec::new();
        };
        let reason = if view.reviewable() {
            DECLINED
        } else {
            TOO_LARGE
        };
        let counts = view.review().counts();
        let what = if commit { "commit" } else { "batch" };
        let effects = self.decline(id, reason);
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            chat.push_system_line(format!("{what} rejected: {counts}"));
        }
        effects
    }
}
