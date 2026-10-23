//! The server state that turns and detection read when they run: the port of
//! each server engine and the llama-server lazarobox manages. A menu save
//! that turns the managed server on replaces it, with no restart.

use std::sync::{Arc, PoisonError, RwLock};

use super::Managed;
use crate::core::ai::ServerPorts;

#[derive(Clone, Default)]
pub struct LiveServers(Arc<RwLock<State>>);

#[derive(Default)]
struct State {
    ports: ServerPorts,
    managed: Option<Arc<Managed>>,
}

impl LiveServers {
    pub fn new(ports: ServerPorts, managed: Option<Arc<Managed>>) -> Self {
        Self(Arc::new(RwLock::new(State { ports, managed })))
    }

    /// The port of every server engine now.
    pub fn ports(&self) -> ServerPorts {
        self.0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .ports
            .clone()
    }

    /// The llama-server llamacpp turns start now, if any.
    pub fn managed(&self) -> Option<Arc<Managed>> {
        self.0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .managed
            .clone()
    }

    /// Makes `managed` the server llamacpp turns start, on its port. The one
    /// it replaces is returned, for the caller to retire.
    pub fn manage(&self, managed: Arc<Managed>) -> Option<Arc<Managed>> {
        let mut state = self.0.write().unwrap_or_else(PoisonError::into_inner);
        state.ports = state.ports.clone().with("llamacpp", managed.port());
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
