//! The server state that turns and detection read when they run: the port of
//! each server engine, the llama-server lazarobox manages and how long it may
//! sit unused. A menu save that turns the managed server on replaces it, and
//! one that changes the idle time sets it, with no restart.

use std::{
    sync::{Arc, PoisonError, RwLock},
    time::{Duration, Instant},
};

use super::Managed;
use crate::core::ai::ServerPorts;

#[derive(Clone, Default)]
pub struct LiveServers(Arc<RwLock<State>>);

#[derive(Default)]
struct State {
    ports: ServerPorts,
    managed: Option<Arc<Managed>>,
    /// How long the managed server may sit unused; `None` never stops it.
    idle: Option<Duration>,
}

impl LiveServers {
    pub fn new(ports: ServerPorts, managed: Option<Arc<Managed>>) -> Self {
        Self(Arc::new(RwLock::new(State {
            ports,
            managed,
            idle: None,
        })))
    }

    /// The port of every server engine now: llamacpp's is the managed
    /// server's, which moves to the port it runs on once it starts.
    pub fn ports(&self) -> ServerPorts {
        let state = self.0.read().unwrap_or_else(PoisonError::into_inner);
        match &state.managed {
            Some(managed) => state.ports.clone().with("llamacpp", managed.port()),
            None => state.ports.clone(),
        }
    }

    /// The llama-server llamacpp turns start now, if any.
    pub fn managed(&self) -> Option<Arc<Managed>> {
        self.0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .managed
            .clone()
    }

    /// Sets how long the managed server may sit unused; `None` never stops it.
    pub fn set_idle(&self, idle: Option<Duration>) {
        self.0.write().unwrap_or_else(PoisonError::into_inner).idle = idle;
    }

    /// Stops the managed server when it was unused for the idle time up to
    /// `now` (see [`Managed::stop_if_idle`]). `true` when it stopped it.
    pub async fn stop_if_idle(&self, now: Instant) -> bool {
        let (managed, idle) = {
            let state = self.0.read().unwrap_or_else(PoisonError::into_inner);
            (state.managed.clone(), state.idle)
        };
        match (managed, idle) {
            (Some(managed), Some(idle)) => managed.stop_if_idle(idle, now).await,
            _ => false,
        }
    }

    /// Makes `managed` the server llamacpp turns start, on its port. The one
    /// it replaces is returned, for the caller to retire.
    pub fn manage(&self, managed: Arc<Managed>) -> Option<Arc<Managed>> {
        let mut state = self.0.write().unwrap_or_else(PoisonError::into_inner);
        state.managed.replace(managed)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn server(model: &str, port: u16) -> Arc<Managed> {
        Arc::new(Managed::new("".into(), PathBuf::from(model), port, None))
    }

    #[test]
    fn managing_moves_llamacpp_to_the_server_s_port_and_hands_back_the_old_one() {
        let ports = ServerPorts::default()
            .with("llamacpp", 8080)
            .with("vllm", 8001);
        let first = server("/m/a.gguf", 39281);
        let live = LiveServers::new(ports, Some(Arc::clone(&first)));
        let copy = live.clone();
        let second = server("/m/b.gguf", 39090);
        let old = live.manage(Arc::clone(&second)).expect("the first one");
        assert!(Arc::ptr_eq(&old, &first));
        // Every copy sees the change.
        assert!(Arc::ptr_eq(&copy.managed().unwrap(), &second));
        assert_eq!(copy.ports().port("llamacpp"), Some(39090));
        assert_eq!(copy.ports().port("vllm"), Some(8001));
    }

    #[test]
    fn the_first_managed_server_replaces_nothing() {
        let live = LiveServers::default();
        assert!(live.managed().is_none());
        assert!(live.manage(server("/m/a.gguf", 39281)).is_none());
        assert_eq!(live.ports().port("llamacpp"), Some(39281));
    }
}
