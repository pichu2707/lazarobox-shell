use serde_json::{Value, json};

use super::*;
use crate::core::ai::chat::engine::{ChatMessage, ChatRequest, Role};

fn message(role: Role, content: &str) -> ChatMessage {
    ChatMessage {
        role,
        content: content.into(),
    }
}

fn request(system: &str) -> ChatRequest {
    ChatRequest {
        engine: "ollama".into(),
        model: Some("llama3:8b".into()),
        system: system.into(),
        memory: None,
        messages: vec![
            message(Role::User, "hi"),
            message(Role::Assistant, "hello"),
            message(Role::User, "and now?"),
        ],
    }
}

fn http(invocation: Invocation) -> (Wire, u16, &'static str, Value) {
    let Invocation::Http {
        wire,
        port,
        path,
        body,
    } = invocation;
    (wire, port, path, serde_json::from_slice(&body).unwrap())
}

#[test]
fn only_ollama_has_an_adapter_so_far() {
    assert_eq!(adapter_for("ollama"), Some(Adapter::Ollama));
    for id in ["opencode", "lmstudio", "llamacpp", "vllm", "claude", "nope"] {
        assert_eq!(adapter_for(id), None, "{id}");
    }
}

#[test]
fn ollama_posts_ndjson_chat_to_its_catalog_port() {
    let (wire, port, path, _) = http(Adapter::Ollama.build(&request("be brief")));
    assert_eq!((wire, port, path), (Wire::Ndjson, 11434, "/api/chat"));
}

#[test]
fn the_body_holds_the_model_the_system_prompt_the_history_and_the_stream_flag() {
    let (_, _, _, body) = http(Adapter::Ollama.build(&request("be brief")));
    assert_eq!(
        body,
        json!({
            "model": "llama3:8b",
            "stream": true,
            "messages": [
                {"role": "system", "content": "be brief"},
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": "hello"},
                {"role": "user", "content": "and now?"},
            ],
        })
    );
    let keys = body.as_object().unwrap().len();
    assert_eq!(keys, 3, "nothing else: no think, options or keep_alive");
}

#[test]
fn an_empty_system_prompt_adds_no_system_message() {
    let (_, _, _, body) = http(Adapter::Ollama.build(&request("")));
    assert_eq!(body["messages"].as_array().unwrap().len(), 3);
    assert_eq!(body["messages"][0]["role"], "user");
}

#[test]
fn text_is_data_not_structure() {
    let hostile = "\"}],\"stream\":false,{\"x\":\"\u{1b}[31m";
    let mut req = request("s\"ys\n");
    req.messages = vec![message(Role::User, hostile)];
    let (_, _, _, body) = http(Adapter::Ollama.build(&req));
    assert_eq!(body["stream"], true);
    assert_eq!(body["messages"].as_array().unwrap().len(), 2);
    assert_eq!(body["messages"][1]["content"], hostile);
}

#[test]
fn a_missing_model_is_sent_empty_so_the_server_answers_with_its_own_error() {
    let mut req = request("s");
    req.model = None;
    let (_, _, _, body) = http(Adapter::Ollama.build(&req));
    assert_eq!(body["model"], "");
}

const BLOCK: &str = "<project-memory source=\"engram\" project=\"p\">\nx\n</project-memory>";

#[test]
fn ollama_appends_the_memory_block_to_the_system_message_after_the_prompt() {
    let mut with_memory = request("be brief");
    with_memory.memory = Some(BLOCK.into());
    let (_, _, _, body) = http(Adapter::Ollama.build(&with_memory));
    let messages = body["messages"].as_array().expect("messages");
    assert_eq!(
        messages[0],
        json!({"role": "system", "content": format!("be brief\n\n{BLOCK}")})
    );
    assert_eq!(messages.len(), 4, "one system message, then the history");
    assert!(
        messages[1..]
            .iter()
            .all(|m| !m["content"].as_str().unwrap().contains("project-memory")),
        "the block is not in the history"
    );
}

#[test]
fn a_memory_block_alone_is_the_whole_system_message() {
    let mut with_memory = request("");
    with_memory.memory = Some(BLOCK.into());
    let (_, _, _, body) = http(Adapter::Ollama.build(&with_memory));
    assert_eq!(
        body["messages"][0],
        json!({"role": "system", "content": BLOCK})
    );
}

#[test]
fn without_a_block_the_system_message_is_the_prompt_alone() {
    let (_, _, _, body) = http(Adapter::Ollama.build(&request("be brief")));
    assert_eq!(
        body["messages"][0],
        json!({"role": "system", "content": "be brief"})
    );
}

#[test]
fn only_ollama_carries_the_memory_block_so_far() {
    assert!(Adapter::Ollama.carries_memory());
    assert!(carries_memory("ollama"));
    for id in ["opencode", "lmstudio", "claude", "codex", "nope"] {
        assert!(!carries_memory(id), "{id}");
    }
}
