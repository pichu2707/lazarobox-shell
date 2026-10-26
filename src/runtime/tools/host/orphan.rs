//! What the user does with a chat's batch outside a model turn: the pane
//! closes, or a batch the turn left behind (an orphan) is reviewed, applied or
//! discarded. These run on the app's thread, so each is one short hold of the
//! store, never a wait across an await. The lines returned are for the user
//! and hold counts only.

use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, TryLockError},
};

use super::{ToolHost, files::lock};
use crate::{
    core::{
        ai::{
            chat::review::{Badge, BatchEvent, BatchReview, ReviewDoc, TOO_LARGE, closed_notice},
            tools::stage::{BatchId, StageKey},
        },
        layout::PaneId,
    },
    runtime::tools::fs::load_roots,
};

/// Shown when the batch an answer names is not there any more.
pub const GONE: &str = "that batch is no longer there";

/// Shown when an apply arrives while a turn of the chat still runs: then the
/// batch is the model's, not an orphan.
pub const LIVE: &str = "a turn of this chat is still running";

impl ToolHost {
    /// Drops the batch of `key` because its pane closed. How many entries were
    /// lost; `None` when there was nothing worth a notice.
    pub fn close_batch(&self, key: StageKey) -> Option<usize> {
        lock(&self.stages).discard_key(key)
    }

    /// What the panes show of their batches (see `Stages::badges`). It never
    /// waits: a store held by an apply answers on the next poll.
    pub fn badges(&self, live: impl Fn(PaneId) -> bool) -> HashMap<PaneId, Badge> {
        match self.stages.try_lock() {
            Ok(stages) => stages.badges(live),
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner().badges(live),
            Err(TryLockError::WouldBlock) => HashMap::new(),
        }
    }

    /// The orphan of `key` for the user to review. A batch the turn never
    /// proposed is sealed now, so the model can no longer add to it.
    /// `turn_live`: a turn of the chat still runs, so the batch is its own.
    pub fn review_batch(
        &self,
        key: StageKey,
        turn_live: bool,
    ) -> Option<(BatchId, Arc<BatchReview>)> {
        let (id, review) = lock(&self.stages).review(key, turn_live).ok()?;
        Some((id, Arc::new(review)))
    }

    /// Applies the orphan `id` after the user's `y`. The host is the trust
    /// boundary, so it refuses what the user could not have approved: a batch
    /// of a pane whose turn is live, or one that is not reviewable. A drifted
    /// or refused batch stays for the user to discard, and the model is told
    /// only when the batch is closed.
    pub fn apply_batch(&self, id: BatchId, turn_live: bool) -> BatchEvent {
        if turn_live {
            return refused(LIVE);
        }
        let Ok(roots) = load_roots(&self.env(Path::new("/"))) else {
            return gone();
        };
        let mut stages = lock(&self.stages);
        let Some(batch) = stages.batch(id.key).filter(|batch| batch.id() == id) else {
            return gone();
        };
        if !ReviewDoc::build(batch).reviewable() {
            return refused(TOO_LARGE);
        }
        let Ok(result) = stages.apply(id, &roots) else {
            return gone();
        };
        let text = result.summary();
        let first = text.lines().next().unwrap_or_default();
        let shown = first.trim_end_matches(':').to_owned();
        let closed = stages.batch(id.key).is_none();
        BatchEvent::Done {
            shown,
            told: closed.then(|| result.told()),
        }
    }

    /// Discards the orphan `id`; nothing is written.
    pub fn discard_batch(&self, id: BatchId) -> BatchEvent {
        let mut stages = lock(&self.stages);
        let entries = (stages.batch(id.key))
            .filter(|batch| batch.id() == id)
            .map_or(0, |batch| batch.entries().len());
        if stages.discard(id).is_err() {
            return gone();
        }
        let s = if entries == 1 { "y" } else { "ies" };
        BatchEvent::Done {
            shown: closed_notice(entries),
            told: Some(format!(
                "The user discarded the batch you left staged ({entries} entr{s})."
            )),
        }
    }
}

/// The batch an answer names is not there any more: nothing to tell the model.
fn gone() -> BatchEvent {
    BatchEvent::Done {
        shown: GONE.to_owned(),
        told: None,
    }
}

/// Nothing was applied and the model hears nothing: the batch is still there.
fn refused(why: &str) -> BatchEvent {
    BatchEvent::Done {
        shown: why.to_owned(),
        told: None,
    }
}
