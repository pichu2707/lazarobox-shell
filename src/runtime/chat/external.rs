//! The user's MCP servers of a chat, started for a turn that has tools on.
//!
//! The servers belong to the chat, not to a turn: the first turn with tools
//! starts them (spawn and `tools/list`, nothing is called) and tells the chat,
//! later turns reuse them, and they stop when the chat closes, when a turn
//! comes without tools, and at quit. The set is made inside the turn's task,
//! ahead of the engine, so a cancelled turn that is still starting drops the
//! half-started servers (their processes die with them).

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use tokio::sync::mpsc;

use crate::{
    core::{
        ai::{
            chat::engine::{StreamEvent, Tools},
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

/// The configured servers and the sets that run.
#[derive(Clone, Default)]
pub(super) struct Externals {
    list: Arc<Vec<McpServer>>,
    /// The neutral working directory of every server (`$HOME`).
    home: PathBuf,
    sets: Arc<Mutex<HashMap<PaneId, Running>>>,
}

/// What a turn needs to start its chat's servers.
pub(super) struct Start {
    pub externals: Externals,
    pub slot: Slot,
}

impl Externals {
    pub fn new(list: Vec<McpServer>, home: PathBuf) -> Self {
        Self {
            list: Arc::new(list),
            home,
            sets: Arc::default(),
        }
    }

    /// A start for a turn, or `None` when nothing is configured.
    pub fn start_for(&self, slot: Slot) -> Option<Start> {
        (!self.list.is_empty()).then(|| Start {
            externals: self.clone(),
            slot,
        })
    }

    /// Takes the set of `pane` out, to stop it.
    pub fn retire(&self, pane: PaneId) -> Option<Arc<ExternalSet>> {
        self.sets.lock().ok()?.remove(&pane).map(|(_, set)| set)
    }

    /// Takes every set out, to stop them at quit.
    pub fn drain(&self) -> Vec<Arc<ExternalSet>> {
        let Ok(mut sets) = self.sets.lock() else {
            return Vec::new();
        };
        sets.drain().map(|(_, (_, set))| set).collect()
    }

    /// The set of `pane` for `active`: the running one, or a new one (the old
    /// one stops) announced to the chat through `events`.
    async fn set_for(
        &self,
        pane: PaneId,
        active: Vec<McpServer>,
        events: &mpsc::Sender<StreamEvent>,
    ) -> Arc<ExternalSet> {
        let running = self.sets.lock().ok().and_then(|sets| {
            sets.get(&pane)
                .filter(|(list, _)| *list == active)
                .map(|(_, set)| set.clone())
        });
        if let Some(set) = running {
            return set;
        }
        if let Some(old) = self.retire(pane) {
            old.shutdown().await;
        }
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
        if let Ok(mut sets) = self.sets.lock() {
            sets.insert(pane, (active, set.clone()));
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
            return;
        }
        let set = self.externals.set_for(pane, active, events).await;
        **external = set.qualified().clone();
        let _ = self.slot.set(set);
    }
}
