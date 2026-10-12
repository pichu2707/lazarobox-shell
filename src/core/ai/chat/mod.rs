//! The AI chat core. Pure: no IO lives here.
//!
//! C1 holds only what a chat pane carries before the conversation exists: the
//! cwd it was opened from.

use std::path::{Path, PathBuf};

/// The state of one chat pane.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ChatState {
    origin_cwd: Option<PathBuf>,
}

impl ChatState {
    /// A chat opened from a pane whose cwd was `origin_cwd`.
    pub fn new(origin_cwd: Option<PathBuf>) -> Self {
        Self { origin_cwd }
    }

    /// The cwd of the pane the chat was opened from.
    pub fn origin_cwd(&self) -> Option<&Path> {
        self.origin_cwd.as_deref()
    }
}
