//! What the user does with a chat's batch outside a model turn: the pane
//! closes, or a batch the turn left behind (an orphan) is reviewed, applied or
//! discarded. These run on the app's thread, so each is one short hold of the
//! store, never a wait across an await. The lines returned are for the user
//! and hold counts only.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, TryLockError},
};

use super::{ToolHost, files::lock};
use crate::{
    core::{
        ai::{
            chat::review::{
                Badge, BatchEvent, BatchReview, RUN_ENDED, ReviewDoc, TOO_LARGE, closed_notice,
            },
            orchestra::RunId,
            tools::stage::{BatchId, StageKey, State},
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
    /// Drops every batch of `pane` because it closed. How many entries were
    /// lost; `None` when there was nothing worth a notice.
    pub fn close_batch(&self, pane: PaneId) -> Option<usize> {
        lock(&self.stages).discard_pane(pane)
    }

    /// What the panes show of their batches (see `Stages::badges`). It never
    /// waits: `None` while an apply holds the store, so the caller keeps what
    /// it showed and asks again on the next poll.
    pub fn badges(&self, live: impl Fn(PaneId) -> bool) -> Option<HashMap<PaneId, Badge>> {
        match self.stages.try_lock() {
            Ok(stages) => Some(stages.badges(live)),
            Err(TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner().badges(live)),
            Err(TryLockError::WouldBlock) => None,
        }
    }

    /// Run `run` of `pane` ended: the batches its workers proposed were
    /// denied with it, so they are dropped and never turn into an orphan a
    /// later `y` could write. How many were dropped.
    pub fn end_run_batches(&self, pane: PaneId, run: RunId) -> usize {
        lock(&self.stages).discard_asked(pane, run)
    }

    /// The id of the batch of `key`, for a test that answers it.
    #[cfg(all(test, target_os = "linux"))]
    pub(crate) fn batch_id_for_test(&self, key: StageKey) -> Option<BatchId> {
        lock(&self.stages).batch(key).map(|batch| batch.id())
    }

    /// Whether the batch of `key` holds entries and was never proposed: its
    /// writer is gone or still staging, and nobody asked the user yet. Each
    /// writer of a run has its own key, so a sibling's files or proposal never
    /// change the answer.
    pub fn unproposed(&self, key: StageKey) -> bool {
        lock(&self.stages)
            .batch(key)
            .is_some_and(|batch| batch.state() == State::Staging && !batch.entries().is_empty())
    }

    /// The oldest orphan of `pane` for the user to review, one batch at a
    /// time. A batch the turn never proposed is sealed now, so the model can
    /// no longer add to it. `turn_live`: a turn of the chat still runs, so
    /// the batches are its own.
    pub fn review_batch(
        &self,
        pane: PaneId,
        turn_live: bool,
    ) -> Option<(BatchId, Arc<BatchReview>)> {
        let (id, review) = lock(&self.stages).review_pane(pane, turn_live).ok()?;
        Some((id, Arc::new(review)))
    }

    /// Applies the orphan `id` after the user's `y`. The host is the trust
    /// boundary, so it refuses what the user could not have approved: a batch
    /// of a pane whose turn is live, or one that is not reviewable. A drifted
    /// or refused batch stays for the user to discard, and the model is told
    /// only when the batch is closed. `drive_grant` is the Windows drive
    /// folder the chat may write in now (openspec `wsl-drive-access`): an
    /// entry on a drive outside it is refused again here.
    pub fn apply_batch(
        &self,
        id: BatchId,
        turn_live: bool,
        drive_grant: Option<PathBuf>,
    ) -> BatchEvent {
        self.apply_batch_of(id, turn_live, drive_grant, true)
    }

    /// [`ToolHost::apply_batch`] told whether the run of the worker that
    /// proposed the batch is still live: a batch of an ended run is refused.
    pub fn apply_batch_of(
        &self,
        id: BatchId,
        turn_live: bool,
        drive_grant: Option<PathBuf>,
        run_live: bool,
    ) -> BatchEvent {
        if turn_live {
            return refused(LIVE);
        }
        // A batch a worker proposed belongs to its run: once the run is over
        // it was denied, and no `y` writes it, whatever view is still open.
        let denied = lock(&self.stages)
            .batch(id.key)
            .is_some_and(|batch| batch.asked() && matches!(id.key, StageKey::Worker(..)));
        if denied && !run_live {
            let _ = lock(&self.stages).discard(id);
            return refused(RUN_ENDED);
        }
        let mut env = self.env(Path::new("/"));
        env.drive_grant = drive_grant;
        let Ok(roots) = load_roots(&env) else {
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
