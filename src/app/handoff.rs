//! The handoffs of an orchestrator run in engram (agent-orchestrator D13,
//! D17): each is saved under a stable key as it appears, the closed ledger at
//! the end. A run with memory off turns it on for itself only; when engram
//! cannot be reached or a save fails the run goes on with the handoffs in its
//! ledger and the pane says so, once. Never silent, never blocking.

use std::collections::HashSet;

use super::*;
use crate::core::ai::{
    chat::memory::{MEMORY_WAIT, Readiness},
    memory::port::{MemCmd, MemReply, SaveId},
    orchestra::{
        RunId,
        persist::{Action, Item, MEMORY_ON, NOT_PERSISTED, RunSave},
    },
};

/// The saves of one run.
pub(super) struct RunStore {
    pane: PaneId,
    save: RunSave,
    /// The saves sent whose answer has not come.
    tokens: HashSet<SaveId>,
    /// The run is over: the store goes when its last answer does.
    ended: bool,
    /// When the project was asked for, while it is awaited.
    asked: Instant,
}

impl App {
    /// An item of run `run` of pane `id` is ready to be stored.
    pub(super) fn store_item(&mut self, id: PaneId, run: RunId, item: Item) -> Vec<Effect> {
        let mut effects = Vec::new();
        if !self.run_stores.contains_key(&run) {
            effects = self.open_store(id, run);
        }
        let actions = self
            .run_stores
            .get_mut(&run)
            .map(|store| store.save.add(item))
            .unwrap_or_default();
        effects.extend(self.store_actions(run, actions));
        effects
    }

    /// The first item of a run: memory is turned on for it when the config
    /// has it off (the config is untouched), and the project is looked for
    /// unless the chat knows it. A run that cannot say where to save goes to
    /// the degraded mode at once.
    fn open_store(&mut self, id: PaneId, run: RunId) -> Vec<Effect> {
        let active = self.ai.memory.active();
        let now = (self.clock)();
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        let mut effects = Vec::new();
        if !active {
            effects.push(Effect::Memory(MemCmd::EnableForRun));
            chat.push_system_line(MEMORY_ON.to_owned());
        }
        let known = chat
            .session()
            .map(|s| s.project.clone())
            .or_else(|| chat.project().map(str::to_owned));
        let waiting_on_chat = active && matches!(chat.readiness(), Readiness::Resolving { .. });
        let cwd = chat.origin_cwd().map(|p| p.to_string_lossy().into_owned());
        let epoch = chat.memory_epoch();
        let mut save = RunSave::new(known.clone());
        let mut actions = Vec::new();
        if known.is_none() && !waiting_on_chat {
            match cwd {
                Some(cwd) => effects.push(Effect::Memory(MemCmd::Resolve {
                    pane: id,
                    cwd,
                    epoch,
                })),
                None => actions = save.resolved(None),
            }
        }
        self.run_stores.insert(
            run,
            RunStore {
                pane: id,
                save,
                tokens: HashSet::new(),
                ended: false,
                asked: now,
            },
        );
        self.dirty = true;
        effects.extend(self.store_actions(run, actions));
        effects
    }

    /// Carries out what the run's store decided.
    fn store_actions(&mut self, run: RunId, actions: Vec<Action>) -> Vec<Effect> {
        let now = (self.clock)();
        let mut effects = Vec::new();
        for action in actions {
            let Some(store) = self.run_stores.get_mut(&run) else {
                break;
            };
            let Some(chat) = self
                .panes
                .get_mut(&store.pane)
                .and_then(PaneState::chat_mut)
            else {
                continue;
            };
            self.dirty = true;
            match action {
                Action::Degrade => chat.push_system_line(NOT_PERSISTED.to_owned()),
                Action::Save { item, project } => {
                    // A save writes under the project its session was
                    // registered with.
                    let project = chat.session().map_or(project, |s| s.project.clone());
                    let token = chat.quiet_save_sent_at(&project, now);
                    store.tokens.insert(token);
                    effects.push(Effect::Memory(MemCmd::Save {
                        pane: store.pane,
                        id: token,
                        session: chat.session().map(|s| s.id.clone()),
                        project,
                        cwd: chat
                            .origin_cwd()
                            .map(|p| p.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        note: item.note,
                        topic_key: Some(item.key),
                    }));
                }
            }
        }
        effects
    }

    /// The run is over: its store goes once no save awaits an answer.
    pub(super) fn end_store(&mut self, run: RunId) {
        if let Some(store) = self.run_stores.get_mut(&run) {
            store.ended = true;
        }
        self.drop_if_over(run);
    }

    /// Whether a run's store waits for the project of its pane: the runtime
    /// keeps the memory timer armed meanwhile.
    pub(super) fn run_store_waiting(&self) -> bool {
        self.run_stores.values().any(|store| store.save.waiting())
    }

    /// Gives up on a project that did not come in `MEMORY_WAIT`.
    pub(super) fn expire_stores(&mut self, now: Instant) -> Vec<Effect> {
        let runs: Vec<RunId> = self
            .run_stores
            .iter()
            .filter(|(_, store)| {
                store.save.waiting() && now.saturating_duration_since(store.asked) >= MEMORY_WAIT
            })
            .map(|(run, _)| *run)
            .collect();
        let mut effects = Vec::new();
        for run in runs {
            let actions = self
                .run_stores
                .get_mut(&run)
                .map(|store| store.save.resolved(None))
                .unwrap_or_default();
            effects.extend(self.store_actions(run, actions));
            self.drop_if_over(run);
        }
        effects
    }

    /// A memory reply of pane `id` as the runs' stores hear it: whether it
    /// was a run save's answer (nothing else handles it) and the effects
    /// that follow. The project and unavailable replies are only heard, the
    /// chat still takes them.
    pub(super) fn on_run_reply(&mut self, id: PaneId, reply: &MemReply) -> (bool, Vec<Effect>) {
        match reply {
            MemReply::Project { result, .. } => (
                false,
                self.resolve_stores(id, result.as_ref().ok().cloned()),
            ),
            MemReply::Unavailable { .. } => (false, self.resolve_stores(id, None)),
            MemReply::Saved {
                id: token,
                project,
                session,
                ..
            } => {
                let Some(run) = self.answered(*token) else {
                    return (false, Vec::new());
                };
                if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
                    chat.save_answered(*token);
                    chat.adopt_session(session, project);
                }
                self.drop_if_over(run);
                (true, self.link_effects(id))
            }
            MemReply::SaveFailed { id: token, .. }
            | MemReply::SaveUnavailable { id: token, .. } => {
                let Some(run) = self.answered(*token) else {
                    return (false, Vec::new());
                };
                if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
                    chat.save_answered(*token);
                }
                (true, self.fail_store(run))
            }
            _ => (false, Vec::new()),
        }
    }

    /// The run whose save `token` was, now answered. Its store goes when the
    /// run is over and nothing else awaits.
    fn answered(&mut self, token: SaveId) -> Option<RunId> {
        let run = self
            .run_stores
            .iter()
            .find_map(|(run, store)| store.tokens.contains(&token).then_some(*run))?;
        self.run_stores.get_mut(&run)?.tokens.remove(&token);
        Some(run)
    }

    /// A save of run `run` failed or got no answer: the run goes on in
    /// memory and the pane says so, once.
    fn fail_store(&mut self, run: RunId) -> Vec<Effect> {
        let actions = self
            .run_stores
            .get_mut(&run)
            .map(|store| store.save.failed())
            .unwrap_or_default();
        let effects = self.store_actions(run, actions);
        self.drop_if_over(run);
        effects
    }

    fn drop_if_over(&mut self, run: RunId) {
        if self
            .run_stores
            .get(&run)
            .is_some_and(|store| store.ended && store.tokens.is_empty() && !store.save.waiting())
        {
            self.run_stores.remove(&run);
        }
    }

    /// The project of pane `id` came, or memory cannot be used: the stores
    /// that waited for it go on or degrade.
    fn resolve_stores(&mut self, id: PaneId, project: Option<String>) -> Vec<Effect> {
        let runs: Vec<RunId> = self
            .run_stores
            .iter()
            .filter(|(_, store)| store.pane == id && store.save.waiting())
            .map(|(run, _)| *run)
            .collect();
        let mut effects = Vec::new();
        for run in runs {
            let actions = self
                .run_stores
                .get_mut(&run)
                .map(|store| store.save.resolved(project.clone()))
                .unwrap_or_default();
            effects.extend(self.store_actions(run, actions));
        }
        effects
    }

    /// A save that ran out of time (the memory tick) when it was a run's.
    pub(super) fn run_save_lost(&mut self, token: SaveId) -> Vec<Effect> {
        match self.answered(token) {
            Some(run) => self.fail_store(run),
            None => Vec::new(),
        }
    }
}
