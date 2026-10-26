//! What the user does with a chat's batch outside a model turn: the pane
//! closes, or a batch the turn left behind (an orphan) is reviewed, applied or
//! discarded. These run on the app's thread, so each is one short hold of the
//! store, never a wait across an await. The lines returned are for the user
//! and hold counts only.

use std::{path::Path, sync::Arc};

use super::{ToolHost, files::lock};
use crate::{
    core::ai::{
        chat::review::{BatchReview, closed_notice},
        tools::stage::{BatchId, StageKey},
    },
    runtime::tools::fs::load_roots,
};

/// Shown when the batch an answer names is not there any more.
pub const GONE: &str = "that batch is no longer there";

impl ToolHost {
    /// Drops the batch of `key` because its pane closed. How many entries were
    /// lost; `None` when there was nothing worth a notice.
    pub fn close_batch(&self, key: StageKey) -> Option<usize> {
        lock(&self.stages).discard_key(key)
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

    /// Applies the orphan `id` after the user's `y`. A drifted or refused
    /// batch stays for the user to discard.
    pub fn apply_batch(&self, id: BatchId) -> String {
        let Ok(roots) = load_roots(&self.env(Path::new("/"))) else {
            return GONE.to_owned();
        };
        match lock(&self.stages).apply(id, &roots) {
            Ok(result) => {
                let text = result.summary();
                let first = text.lines().next().unwrap_or_default();
                first.trim_end_matches(':').to_owned()
            }
            Err(_) => GONE.to_owned(),
        }
    }

    /// Discards the orphan `id`; nothing is written.
    pub fn discard_batch(&self, id: BatchId) -> String {
        let mut stages = lock(&self.stages);
        let entries = (stages.batch(id.key))
            .filter(|batch| batch.id() == id)
            .map_or(0, |batch| batch.entries().len());
        match stages.discard(id) {
            Ok(()) => closed_notice(entries),
            Err(_) => GONE.to_owned(),
        }
    }
}
