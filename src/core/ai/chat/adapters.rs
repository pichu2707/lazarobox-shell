//! Engine adapters as pure builders: a `ChatRequest` in, the description of
//! what to run out. Nothing here touches the network; the runtime executes the
//! `Invocation`.

use serde_json::{Value, json};

use super::engine::{ChatRequest, Role};
use crate::core::ai::{CATALOG, Kind};

/// The wire format of a local server's reply.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wire {
    Ndjson,
    Sse,
}

/// The engines that can chat. More arrive with their adapters.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Adapter {
    Ollama,
}

/// What the runtime runs for one turn.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Invocation {
    /// One POST to `127.0.0.1:port`, the reply decoded as `wire`.
    Http {
        wire: Wire,
        port: u16,
        path: &'static str,
        body: Vec<u8>,
    },
}

/// The adapter of an engine id, if it has one.
pub fn adapter_for(id: &str) -> Option<Adapter> {
    match id {
        "ollama" => Some(Adapter::Ollama),
        _ => None,
    }
}

/// The catalog port of a server engine.
fn catalog_port(id: &str) -> u16 {
    CATALOG
        .iter()
        .find_map(|e| match e.kind {
            Kind::Server { port, .. } if e.id == id => Some(port),
            _ => None,
        })
        .expect("a server adapter has a server catalog entry")
}

impl Adapter {
    /// The invocation for `request`. The body holds the model, the system
    /// prompt, the history and the stream flag, and nothing else: no `think`
    /// option (a reasoning model streams its `thinking` without it, see the
    /// recorded fixtures) and no earlier reasoning.
    pub fn build(self, request: &ChatRequest) -> Invocation {
        match self {
            Adapter::Ollama => Invocation::Http {
                wire: Wire::Ndjson,
                port: catalog_port("ollama"),
                path: "/api/chat",
                body: ollama_body(request),
            },
        }
    }
}

fn ollama_body(request: &ChatRequest) -> Vec<u8> {
    let system =
        (!request.system.is_empty()).then(|| json!({"role": "system", "content": request.system}));
    let messages: Vec<Value> = system
        .into_iter()
        .chain(request.messages.iter().map(|m| {
            let role = match m.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            };
            json!({"role": role, "content": m.content})
        }))
        .collect();
    let body = json!({
        "model": request.model.as_deref().unwrap_or(""),
        "messages": messages,
        "stream": true,
    });
    serde_json::to_vec(&body).expect("a JSON value always serializes")
}

/// The pinned system prompt of a turn.
///
/// Temporary: one neutral prompt for the default `programmer` profile. PR12
/// (C10b) replaces this function with `resolve_prompt`.
pub fn system_prompt() -> &'static str {
    DEFAULT_SYSTEM_PROMPT
}

/// TEMPORARY, removed by PR12 (task 12.x) when `resolve_prompt` lands.
const DEFAULT_SYSTEM_PROMPT: &str = "You are a helpful programming assistant. Answer concisely and \
     use fenced code blocks for code. If you are unsure, say so.";

#[cfg(test)]
mod tests;
