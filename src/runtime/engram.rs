//! The engram memory in the runtime: a worker task between the app and the
//! local engram server. The app never waits on it.

use std::time::Duration;

use tokio::sync::mpsc;

use crate::core::ai::{
    config::MemoryConfig,
    memory::port::{MemCmd, MemReply, Reason},
};

pub mod serve;
pub mod worker;

pub use worker::{Loopback, MemoryLink, Transport, spawn, spawn_with};

#[cfg(test)]
mod fake_engram;

/// The runtime's handle on the memory worker. It exists in every run, but the
/// worker behind it starts only on the first command, and never when memory is
/// not active in the config: then no task runs and nothing touches the network.
pub struct Memory {
    start: Option<Box<dyn FnOnce() -> MemoryLink>>,
    link: Option<MemoryLink>,
    /// Where an inactive memory tells a pane its command was not run.
    declined: Option<mpsc::Sender<MemReply>>,
}

impl Memory {
    /// Memory that does nothing.
    pub fn off() -> Self {
        Self {
            start: None,
            link: None,
            declined: None,
        }
    }

    /// An inactive memory answers the commands of a pane with
    /// `Unavailable(RestartNeeded)` on `replies`, instead of dropping them:
    /// memory turned on in the menu during a run has no worker until the next
    /// start, and the pane that asked must not wait for it. Nothing else
    /// changes: no worker, no network. An active memory ignores `replies`.
    pub fn answering_when_off(mut self, replies: mpsc::Sender<MemReply>) -> Self {
        if self.start.is_none() {
            self.declined = Some(replies);
        }
        self
    }

    /// Memory for `config`: off unless it is `active()`. `start` builds the
    /// worker for the configured port, on the first command.
    pub fn for_config(
        config: &MemoryConfig,
        start: impl FnOnce(u16) -> MemoryLink + 'static,
    ) -> Self {
        if !config.active() {
            return Self::off();
        }
        let port = config.port;
        Self {
            start: Some(Box::new(move || start(port))),
            link: None,
            declined: None,
        }
    }

    /// Queues a command; returns at once. Needs a tokio runtime on the first
    /// command of an active memory.
    pub fn send(&mut self, cmd: MemCmd) {
        // Quitting never starts a worker; the runtime calls `shutdown`.
        if cmd == MemCmd::Quit {
            return;
        }
        if self.link.is_none()
            && let Some(start) = self.start.take()
        {
            self.link = Some(start());
        }
        if let Some(link) = &self.link {
            link.send(cmd);
        } else if let (Some(replies), Some(pane)) = (&self.declined, cmd.pane()) {
            let reason = Reason::RestartNeeded;
            let _ = replies.try_send(MemReply::Unavailable {
                pane: Some(pane),
                reason,
            });
        }
    }

    /// Flushes what is queued and stops the worker, waiting at most `bound`.
    pub async fn shutdown(&mut self, bound: Duration) {
        self.start = None;
        self.declined = None;
        if let Some(link) = self.link.take() {
            link.shutdown(bound).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        rc::Rc,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use tokio::sync::mpsc;

    use super::*;
    use crate::{
        core::{
            ai::memory::{port::MemReply, route::Call},
            layout::PaneId,
        },
        runtime::loopback::{CallError, Limits, Reply},
    };

    /// Counts the calls it gets and answers none of them.
    #[derive(Clone, Default)]
    struct Recording(Arc<AtomicUsize>);

    impl Transport for Recording {
        async fn call(&self, _call: &Call, _limits: Limits) -> Result<Reply, CallError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(CallError::Connect)
        }
    }

    fn resolve() -> MemCmd {
        MemCmd::Resolve {
            pane: PaneId::FIRST,
            cwd: "/x".into(),
        }
    }

    /// A memory whose start is recorded: how often it ran and with which port.
    struct Rig {
        memory: Memory,
        started: Rc<Cell<usize>>,
        port: Rc<Cell<u16>>,
        transport: Recording,
        rx: mpsc::Receiver<MemReply>,
    }

    fn memory_of(config: MemoryConfig) -> Rig {
        let started = Rc::new(Cell::new(0));
        let port_seen = Rc::new(Cell::new(0));
        let transport = Recording::default();
        let (tx, rx) = mpsc::channel(8);
        let (s, p, t) = (started.clone(), port_seen.clone(), transport.clone());
        let memory = Memory::for_config(&config, move |port| {
            s.set(s.get() + 1);
            p.set(port);
            spawn(t, port, tx)
        });
        Rig {
            memory,
            started,
            port: port_seen,
            transport,
            rx,
        }
    }

    fn config(enabled: bool, acknowledged: bool, port: u16) -> MemoryConfig {
        MemoryConfig {
            enabled,
            acknowledged,
            port,
        }
    }

    #[tokio::test]
    async fn an_inactive_memory_never_starts_a_worker_or_touches_the_network() {
        for cfg in [
            MemoryConfig::default(),
            config(true, false, 9123),
            config(false, true, 9123),
        ] {
            let mut rig = memory_of(cfg);
            rig.memory.send(resolve());
            rig.memory.send(MemCmd::StartServer);
            rig.memory.shutdown(Duration::from_secs(1)).await;
            tokio::task::yield_now().await;
            assert_eq!(rig.started.get(), 0, "{cfg:?}: no worker");
            assert_eq!(
                rig.transport.0.load(Ordering::SeqCst),
                0,
                "{cfg:?}: no call"
            );
            assert!(rig.rx.try_recv().is_err(), "{cfg:?}: no reply");
        }
    }

    #[tokio::test]
    async fn an_active_memory_starts_its_worker_lazily_and_once_on_the_configured_port() {
        let mut rig = memory_of(config(true, true, 9123));
        assert_eq!(
            rig.started.get(),
            0,
            "nothing runs before the first command"
        );
        rig.memory.send(resolve());
        rig.memory.send(resolve());
        assert_eq!(rig.started.get(), 1);
        assert_eq!(rig.port.get(), 9123, "the port comes from the config");
        rig.rx.recv().await.expect("the worker answers");
        assert_eq!(rig.transport.0.load(Ordering::SeqCst), 1, "one probe");
        rig.memory.shutdown(Duration::from_secs(1)).await;
    }

    #[tokio::test]
    async fn quitting_alone_does_not_start_a_worker() {
        let mut rig = memory_of(config(true, true, 9123));
        rig.memory.send(MemCmd::Quit);
        rig.memory.shutdown(Duration::from_secs(1)).await;
        assert_eq!(rig.started.get(), 0);
    }

    #[tokio::test]
    async fn an_inactive_memory_that_answers_tells_the_pane_to_restart() {
        let (tx, mut rx) = mpsc::channel(8);
        let mut rig = memory_of(MemoryConfig::default());
        rig.memory = std::mem::replace(&mut rig.memory, Memory::off()).answering_when_off(tx);
        rig.memory.send(resolve());
        assert_eq!(
            rx.try_recv().expect("an answer at once"),
            MemReply::Unavailable {
                pane: Some(PaneId::FIRST),
                reason: Reason::RestartNeeded,
            }
        );
        // Commands without a pane have nobody to tell.
        rig.memory.send(MemCmd::StartServer);
        rig.memory.send(MemCmd::End {
            session: "s".into(),
            project: "p".into(),
            summary: "x".into(),
        });
        assert!(rx.try_recv().is_err());
        assert_eq!(rig.started.get(), 0, "still no worker");
        assert_eq!(rig.transport.0.load(Ordering::SeqCst), 0, "still no call");
        rig.memory.shutdown(Duration::from_secs(1)).await;
        rig.memory.send(resolve());
        assert!(rx.try_recv().is_err(), "nothing after the shutdown");
    }

    #[tokio::test]
    async fn an_active_memory_does_not_use_the_off_answer() {
        let (tx, mut rx) = mpsc::channel(8);
        let mut rig = memory_of(config(true, true, 9123));
        rig.memory = std::mem::replace(&mut rig.memory, Memory::off()).answering_when_off(tx);
        rig.memory.send(resolve());
        assert_eq!(rig.started.get(), 1);
        assert!(rx.try_recv().is_err());
        rig.memory.shutdown(Duration::from_secs(1)).await;
    }

    #[test]
    fn the_default_memory_is_off() {
        let mut memory = Memory::off();
        memory.send(resolve());
        assert!(memory.link.is_none());
    }
}
