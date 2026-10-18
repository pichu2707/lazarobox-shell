//! The user's MCP servers of a chat, started for a turn that has tools on.
//!
//! The servers belong to the chat, not to a turn: the first turn with tools
//! starts them (spawn and `tools/list`, nothing is called) and tells the chat,
//! later turns reuse them, and they stop when the chat closes, when a turn
//! comes without tools, and at quit. A server that failed to start is not
//! retried while the set is reused: the chat keeps that outcome until it is
//! closed or a turn without tools resets it.
//!
//! The set is made inside the turn's task, ahead of the engine, so a cancelled
//! turn that is still starting drops the half-started servers (their processes
//! die with them). A chat closed during a start is caught by the pane's epoch.
//! Nothing starts for an engine that will not be wired (a CLI that runs bare).

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use tokio::sync::mpsc;

use super::Wires;
use crate::{
    core::{
        ai::{
            chat::engine::{ChatRequest, StreamEvent, Tools},
            config::McpServer,
        },
        layout::PaneId,
    },
    runtime::mcp::{
        active::active_servers,
        client::Timeouts,
        external::{ExternalSet, Slot},
    },
};

/// The servers a chat is running, and what they were started for.
type Running = (Vec<McpServer>, Arc<ExternalSet>);

/// What the chats of the app have running. `epochs` counts, per pane, how
/// many times its servers were retired: a start that finds the count changed
/// belongs to a chat that was closed meanwhile.
#[derive(Default)]
struct State {
    running: HashMap<PaneId, Running>,
    epochs: HashMap<PaneId, u64>,
}

impl State {
    fn epoch(&self, pane: PaneId) -> u64 {
        self.epochs.get(&pane).copied().unwrap_or(0)
    }
}

/// The configured servers and the sets that run.
#[derive(Clone)]
pub(super) struct Externals {
    list: Arc<Vec<McpServer>>,
    /// The neutral working directory of every server (`$HOME`).
    home: PathBuf,
    /// Whether a turn's engine gets its tools wired at all.
    wires: Wires,
    sets: Arc<Mutex<State>>,
}

impl Default for Externals {
    fn default() -> Self {
        Self::new(Vec::new(), PathBuf::new(), Arc::new(|_| false))
    }
}

/// What a turn needs to start its chat's servers.
pub(super) struct Start {
    pub externals: Externals,
    pub slot: Slot,
}

impl Externals {
    pub fn new(list: Vec<McpServer>, home: PathBuf, wires: Wires) -> Self {
        Self {
            list: Arc::new(list),
            home,
            wires,
            sets: Arc::default(),
        }
    }

    /// A start for the turn of `request`, or `None` when nothing is configured
    /// or its engine will not get the tools.
    pub fn start_for(&self, slot: Slot, request: &ChatRequest) -> Option<Start> {
        let wanted = !self.list.is_empty() && (self.wires)(request);
        wanted.then(|| Start {
            externals: self.clone(),
            slot,
        })
    }

    /// Takes the set of `pane` out, to stop it, and marks the pane so that a
    /// start still in flight for it is stopped when it finishes.
    pub fn retire(&self, pane: PaneId) -> Option<Arc<ExternalSet>> {
        let mut state = self.sets.lock().ok()?;
        *state.epochs.entry(pane).or_default() += 1;
        state.running.remove(&pane).map(|(_, set)| set)
    }

    /// Takes every set out, to stop them at quit.
    pub fn drain(&self) -> Vec<Arc<ExternalSet>> {
        let Ok(mut state) = self.sets.lock() else {
            return Vec::new();
        };
        state.running.drain().map(|(_, (_, set))| set).collect()
    }

    /// The set of `pane` for `active`: the running one, or a new one (the old
    /// one stops) announced to the chat through `events`.
    async fn set_for(
        &self,
        pane: PaneId,
        active: Vec<McpServer>,
        events: &mpsc::Sender<StreamEvent>,
    ) -> Arc<ExternalSet> {
        let running = self.sets.lock().ok().and_then(|state| {
            state
                .running
                .get(&pane)
                .filter(|(list, _)| *list == active)
                .map(|(_, set)| set.clone())
        });
        if let Some(set) = running {
            return set;
        }
        if let Some(old) = self.retire(pane) {
            old.shutdown().await;
        }
        let epoch = self.sets.lock().map_or(0, |state| state.epoch(pane));
        let refs: Vec<&McpServer> = active.iter().collect();
        let set = Arc::new(
            ExternalSet::start(
                &refs,
                &self.home,
                |name| std::env::var_os(name),
                Timeouts::default(),
            )
            .await,
        );
        let kept = self.sets.lock().is_ok_and(|mut state| {
            let open = state.epoch(pane) == epoch;
            if open {
                state.running.insert(pane, (active, set.clone()));
            }
            open
        });
        if !kept {
            // The chat closed while the servers were starting: nobody owns
            // them. The turn goes on without them (its calls are refused).
            set.shutdown().await;
            return set;
        }
        for line in set.announcements() {
            let _ = events.send(StreamEvent::Notice(line)).await;
        }
        set
    }
}

impl Start {
    /// Starts or reuses the servers for the turn on `tools` and hands them to
    /// the turn: the engine's tools, and the slot its session and host read.
    pub async fn run(&self, pane: PaneId, tools: &mut Tools, events: &mpsc::Sender<StreamEvent>) {
        let Tools::On {
            origin, external, ..
        } = tools
        else {
            return;
        };
        let active = active_servers(&self.externals.list, origin.as_deref());
        if active.is_empty() {
            // Nothing applies to this chat any more: the old servers stop.
            if let Some(old) = self.externals.retire(pane) {
                old.shutdown().await;
            }
            return;
        }
        let set = self.externals.set_for(pane, active, events).await;
        **external = set.qualified().clone();
        let _ = self.slot.set(set);
    }
}
