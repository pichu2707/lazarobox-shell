//! The server state that turns and detection read when they run: the port of
//! each server engine, the llama-server lazarobox manages and how long it may
//! sit unused. A menu save that turns the managed server on replaces it, and
//! one that changes the idle time sets it, with no restart.

use std::{
    sync::{Arc, PoisonError, RwLock},
    time::{Duration, Instant},
};

use super::Managed;
use crate::core::ai::{
    ServerPorts, catalog::same_model, chat::engine::ChatRequest, config::OllamaOptions,
    window::ollama_num_ctx,
};

#[derive(Clone, Default)]
pub struct LiveServers(Arc<RwLock<State>>);

#[derive(Default)]
struct State {
    ports: ServerPorts,
    managed: Option<Arc<Managed>>,
    /// How long the managed server may sit unused; `None` never stops it.
    idle: Option<Duration>,
    /// The cap and keep-alive every Ollama turn carries.
    ollama: OllamaOptions,
    /// The own window of each Ollama model detection has read, by name.
    windows: Vec<(String, usize)>,
}

impl LiveServers {
    pub fn new(ports: ServerPorts, managed: Option<Arc<Managed>>) -> Self {
        Self(Arc::new(RwLock::new(State {
            ports,
            managed,
            idle: None,
            ollama: OllamaOptions::default(),
            windows: Vec::new(),
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

    /// Points the engines at other ports (tests move a fake server).
    #[cfg(test)]
    pub fn set_ports(&self, ports: ServerPorts) {
        self.0.write().unwrap_or_else(PoisonError::into_inner).ports = ports;
    }

    /// Sets the cap and keep-alive of Ollama turns.
    pub fn set_ollama(&self, options: OllamaOptions) {
        self.0
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .ollama = options;
    }

    /// The Ollama windows known now, by listed name.
    pub fn ollama_windows(&self) -> Vec<(String, usize)> {
        self.0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .windows
            .clone()
    }

    /// Replaces the known Ollama windows: what the last detection read, with
    /// the ones a failed probe could not read carried over.
    pub fn set_ollama_windows(&self, windows: Vec<(String, usize)>) {
        self.0
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .windows = windows;
    }

    /// Puts the Ollama options on `request` when it is an Ollama turn: the
    /// same values for chat, compaction, summary and worker turns, so the
    /// model is never reloaded because of them. Other engines are untouched.
    pub fn stamp(&self, request: &mut ChatRequest) {
        if request.engine != "ollama" {
            return;
        }
        let state = self.0.read().unwrap_or_else(PoisonError::into_inner);
        let max = request.model.as_deref().and_then(|model| {
            state
                .windows
                .iter()
                .find(|(name, _)| same_model("ollama", name, model))
                .map(|(_, window)| *window)
        });
        request.ollama.num_ctx = ollama_num_ctx(max, state.ollama.num_ctx_cap);
        request.ollama.keep_alive_minutes = Some(state.ollama.keep_alive_minutes);
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
    use crate::core::ai::chat::engine::{OllamaWire, Tools};

    fn request(engine: &str, model: &str) -> ChatRequest {
        ChatRequest {
            engine: engine.into(),
            model: Some(model.into()),
            system: String::new(),
            memory: None,
            messages: Vec::new(),
            tools: Tools::Off,
            families: crate::core::ai::tools::registry::FamilySet::LEGACY,
            ollama: OllamaWire::default(),
        }
    }

    fn live() -> LiveServers {
        let live = LiveServers::default();
        live.set_ollama(OllamaOptions {
            num_ctx_cap: 16_384,
            keep_alive_minutes: 30,
        });
        live.set_ollama_windows(vec![
            ("big:8b".into(), 131_072),
            ("tiny:latest".into(), 4_096),
        ]);
        live
    }

    #[test]
    fn an_ollama_turn_gets_the_capped_window_and_the_keep_alive() {
        let live = live();
        let mut req = request("ollama", "big:8b");
        live.stamp(&mut req);
        assert_eq!(
            req.ollama,
            OllamaWire {
                num_ctx: Some(16_384),
                keep_alive_minutes: Some(30)
            }
        );
        let mut small = request("ollama", "tiny");
        live.stamp(&mut small);
        assert_eq!(small.ollama.num_ctx, Some(4_096), "the :latest tag matches");
    }

    #[test]
    fn an_unknown_window_still_sends_the_keep_alive() {
        let mut req = request("ollama", "mystery:1b");
        live().stamp(&mut req);
        assert_eq!(req.ollama.num_ctx, None);
        assert_eq!(req.ollama.keep_alive_minutes, Some(30));
    }

    #[test]
    fn the_stamp_is_the_same_on_every_turn_and_zero_keep_alive_is_kept() {
        let live = live();
        let (mut a, mut b) = (request("ollama", "big:8b"), request("ollama", "big:8b"));
        live.stamp(&mut a);
        live.stamp(&mut b);
        assert_eq!(a.ollama, b.ollama);
        live.set_ollama(OllamaOptions {
            num_ctx_cap: 8_192,
            keep_alive_minutes: 0,
        });
        live.stamp(&mut a);
        assert_eq!(
            (a.ollama.num_ctx, a.ollama.keep_alive_minutes),
            (Some(8_192), Some(0))
        );
    }

    #[test]
    fn other_engines_are_never_stamped() {
        for engine in ["llamacpp", "lmstudio", "vllm", "claude", "codex", "gemini"] {
            let mut req = request(engine, "big:8b");
            live().stamp(&mut req);
            assert_eq!(req.ollama, OllamaWire::default(), "{engine}");
        }
    }

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
