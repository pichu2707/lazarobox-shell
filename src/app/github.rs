//! The GitHub tools of a chat pane: the families a turn offers, and where the
//! grant of `/github` ends (github-tools, The chat grant).

use super::*;
use crate::core::ai::tools::registry::{Family, FamilySet};

/// The system line of a grant that a folder change ended.
const FOLDER_ENDED: &str = "GitHub tools are off because the folder changed";
/// The system line of a grant that tools turned off ended.
pub(super) const TOOLS_ENDED: &str = "GitHub tools are off because tools were turned off";
/// The system line of a grant that the saved toggle going off ended.
pub(super) const TOGGLE_ENDED: &str =
    "GitHub tools are off because the GitHub toggle was turned off";

impl App {
    /// The families chat `id` offers its next turn: the saved toggles plus the
    /// GitHub family when this chat holds a grant and tools are on. The one
    /// source of the offered tools and of the context budget.
    pub(super) fn offered_families(&self, id: PaneId) -> FamilySet {
        let saved = self.ai.families();
        let granted = self.ai.tools_enabled()
            && (self.panes.get(&id).and_then(PaneState::chat_state))
                .is_some_and(ChatState::github_granted);
        if granted {
            saved.with(Family::Github)
        } else {
            saved
        }
    }

    /// The folder of chat `id` changed: a grant was for the old repository.
    pub(super) fn end_grant_for_folder(&mut self, id: PaneId) {
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut)
            && chat.github_granted()
        {
            chat.set_github_granted(false);
            chat.push_system_line(FOLDER_ENDED.to_owned());
        }
    }

    /// Every grant ends (tools turned off, or the saved toggle turned off):
    /// each chat that held one says why, `line`.
    pub(super) fn end_all_grants(&mut self, line: &str) {
        for chat in self.panes.values_mut().filter_map(PaneState::chat_mut) {
            if chat.github_granted() {
                chat.set_github_granted(false);
                chat.push_system_line(line.to_owned());
            }
        }
        self.dirty = true;
    }
}
