//! Memory in a chat view (engram-memory M6): what a view asks engram when it
//! opens, and how the answers become the memory line and the block the turns
//! carry.

use super::*;
use crate::core::ai::{
    chat::{
        adapters::carries_memory,
        memory::{Readiness, project_error_line},
        note,
    },
    config::Task,
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
        let now = (self.clock)();
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
        chat.start_resolving_at(false, now);
        let epoch = chat.memory_epoch();
        vec![Effect::Memory(MemCmd::Resolve {
            pane: id,
            cwd,
            epoch,
        })]
    }

    /// The project of chat `id` is known or is not usable. Returns the context
    /// request when the view has none yet: exactly one per view.
    pub(super) fn on_project(
        &mut self,
        id: PaneId,
        result: &Result<String, ProjectError>,
    ) -> Vec<Effect> {
        if !self.ai.memory.active() {
            return Vec::new();
        }
        let now = (self.clock)();
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        let epoch = chat.memory_epoch();
        match result {
            Ok(project) => chat
                .start_context_at(now)
                .then(|| {
                    Effect::Memory(MemCmd::Context {
                        pane: id,
                        project: project.clone(),
                        epoch,
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
        if !self.ai.memory.active() {
            return;
        }
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
        self.sync_memory_line(id);
    }

    /// Whether the engine assigned to the chat task has an adapter that
    /// carries the memory block.
    fn chat_engine_carries_memory(&self) -> bool {
        let (assignment, _) = self.ai.resolve(Task::Chat);
        assignment.engine.as_deref().is_some_and(carries_memory)
    }

    /// The memory line of chat `id` shows only while the engine carries the
    /// block it announces.
    pub(super) fn sync_memory_line(&mut self, id: PaneId) {
        let show = self.chat_engine_carries_memory();
        let width = self
            .panes
            .get(&id)
            .map_or(0, |pane| usize::from(pane.size.cols));
        let rows = self.chat_viewport(id);
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            chat.show_memory_line_at(show, width, rows);
        }
        self.dirty = true;
    }

    /// The saved choice changed the engine: every chat follows it.
    pub(super) fn sync_memory_lines(&mut self) {
        let ids: Vec<PaneId> = self.panes.keys().copied().collect();
        for id in ids {
            self.sync_memory_line(id);
        }
    }
}

impl App {
    /// Memory is off: the chat `id` keeps no block and no line, and a context
    /// request still out is given up on. Idempotent and cheap when the chat
    /// holds nothing.
    pub(super) fn forget_memory(&mut self, id: PaneId) {
        let width = self
            .panes
            .get(&id)
            .map_or(0, |pane| usize::from(pane.size.cols));
        let rows = self.chat_viewport(id);
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            chat.clear_memory_at(width, rows);
        }
    }

    /// The saved choice turned memory off: every open chat forgets its block
    /// and line, and the `engram serve` question goes off screen.
    pub(super) fn memory_turned_off(&mut self) {
        let ids: Vec<PaneId> = self.panes.keys().copied().collect();
        for id in ids {
            self.forget_memory(id);
        }
        self.offer_shown = false;
        self.offer_pane = None;
        self.dirty = true;
    }

    /// Whether any chat awaits a memory reply. The runtime keeps the timer of
    /// `MemoryTick` armed exactly while this holds.
    pub fn memory_waiting(&self) -> bool {
        self.run_store_waiting()
            || self
                .panes
                .values()
                .filter_map(PaneState::chat_state)
                .any(ChatState::memory_waiting)
    }

    /// Time passed: every wait older than `MEMORY_WAIT` is given up, so no
    /// view waits for engram without a bound. A project or context request
    /// that runs out shows `memory unavailable: engram did not answer` once; a
    /// save that runs out fails like any failed save (its editor keeps the
    /// text). A reply that comes later finds nobody waiting.
    pub(super) fn on_memory_tick(&mut self) -> Vec<Effect> {
        let now = (self.clock)();
        let mut ids: Vec<PaneId> = self.panes.keys().copied().collect();
        ids.sort();
        let mut lost = Vec::new();
        for id in ids {
            let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
                continue;
            };
            let expired = chat.expire_waits(now);
            let text = Reason::NoAnswer.text();
            // With memory off the waits still end (the timer stops) but say
            // nothing about engram.
            if self.ai.memory.active() && (expired.resolve.is_some() || expired.context) {
                chat.degrade_once(Reason::NoAnswer.line());
                self.dirty = true;
            }
            if expired.resolve == Some(true) {
                self.notice = Some(note::not_ready(&text));
            }
            for save in &expired.saves {
                super::notes::save_not_done(chat, Some(save), &text);
                lost.push(save.id);
                self.dirty = true;
            }
        }
        // A run's save that got no answer switches the run to memory only.
        let mut effects = self.expire_stores(now);
        for token in lost {
            effects.extend(self.run_save_lost(token));
        }
        effects
    }
}
