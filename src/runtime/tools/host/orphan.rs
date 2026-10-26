//! What the user does with a chat's batch outside a model turn: the pane
//! closes, or a batch the turn left behind (an orphan) is reviewed, applied or
//! discarded. These run on the app's thread, so each is one short hold of the
//! store, never a wait across an await.

use super::{ToolHost, files::lock};
use crate::core::ai::tools::stage::StageKey;

impl ToolHost {
    /// Drops the batch of `key` because its pane closed. How many entries were
    /// lost; `None` when there was nothing worth a notice.
    pub fn close_batch(&self, key: StageKey) -> Option<usize> {
        lock(&self.stages).discard_key(key)
    }
}
