//! Memory in a chat view (engram-memory M6): what a view asks engram when it
//! opens, and how the answers become the memory line and the block the turns
//! carry.

use super::*;
use crate::core::ai::{
    chat::memory::{Readiness, project_error_line},
    memory::{context::ContextBlock, port::Reason, reply::ProjectError},
};

impl App {
    /// A conversation view opened (or was resumed): with memory active, asks
    /// engram for the project of its directory. The context follows when the
    /// project is known (see `on_project`). A view without a directory has
    /// no project to look for, and says so at once.
    pub(super) fn begin_memory(&mut self, id: PaneId) -> Vec<Effect> {
        if !self.ai.memory.active() {
            return Vec::new();
        }
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        if !matches!(chat.readiness(), Readiness::Unknown) {
            return Vec::new();
        }
        let Some(cwd) = chat.origin_cwd().map(|p| p.to_string_lossy().into_owned()) else {
            chat.set_unavailable(&Reason::NotGit.text());
            chat.degrade_once(Reason::NotGit.line());
            self.dirty = true;
            return Vec::new();
        };
        chat.start_resolving(false);
        vec![Effect::Memory(MemCmd::Resolve { pane: id, cwd })]
    }

    /// The project of chat `id` is known or is not usable. Returns the context
    /// request when the view has none yet: exactly one per view.
    pub(super) fn on_project(
        &mut self,
        id: PaneId,
        result: &Result<String, ProjectError>,
    ) -> Vec<Effect> {
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        match result {
            Ok(project) => chat
                .start_context()
                .then(|| {
                    Effect::Memory(MemCmd::Context {
                        pane: id,
                        project: project.clone(),
                    })
                })
                .into_iter()
                .collect(),
            Err(error) => {
                chat.degrade_once(project_error_line(error));
                Vec::new()
            }
        }
    }

    /// The context reply of chat `id`: the block the next turns carry and the
    /// line above the transcript. A reply nobody asked for (no fetch is
    /// pending: it already came, or never went out) is ignored.
    pub(super) fn on_context(&mut self, id: PaneId, block: &Option<ContextBlock>) {
        let width = self
            .panes
            .get(&id)
            .map_or(0, |pane| usize::from(pane.size.cols));
        let rows = self.chat_viewport(id);
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return;
        };
        if !chat.finish_context() {
            return;
        }
        if let Some(block) = block {
            chat.inject_memory_at(block.clone(), width, rows);
        }
    }
}
