//! The tool loop on the OpenAI wire (llama.cpp) against a scripted fake
//! server, reusing the fake of `ollama_tests`. These drive `tool_turn_with`
//! directly, so they reach the loop while llamacpp is not in `MCP_VERIFIED`.

// What only the Linux-only tests use is dead code elsewhere.
#![cfg_attr(not(target_os = "linux"), allow(dead_code, unused_imports))]

use serde_json::{Value, json};

use super::{
    ollama::{Profile, tool_turn, tool_turn_with},
    ollama_tests::{Lab, OK_HEAD, Step, ask, direct, fake, lab, text},
    tests::done,
    *,
};
use crate::core::ai::chat::engine::ToolStatus;

/// An SSE reply made of `chunks`, then `[DONE]`.
fn sse(chunks: &[Value]) -> Step {
    let body: String = chunks.iter().map(|c| format!("data: {c}\n\n")).collect();
    Step::Reply(format!("{OK_HEAD}{body}data: [DONE]\n\n").into_bytes())
}

fn chunk(delta: Value, finish: Option<&str>) -> Value {
    json!({"choices": [{"index": 0, "delta": delta, "finish_reason": finish}]})
}

/// A reply with plain text.
fn sse_text(content: &str) -> Step {
    sse(&[
        chunk(json!({"content": content}), None),
        chunk(json!({}), Some("stop")),
    ])
}

/// A reply that calls one tool, with the arguments as a JSON string and `id`
/// as the call id (none when `None`).
fn sse_call(name: &str, arguments: &str, id: Option<&str>) -> Step {
    let mut call = json!({"index": 0, "type": "function",
        "function": {"name": name, "arguments": arguments}});
    if let Some(id) = id {
        call["id"] = json!(id);
    }
    sse(&[
        chunk(json!({"tool_calls": [call]}), None),
        chunk(json!({}), Some("tool_calls")),
    ])
}

/// Runs one turn on `profile` and returns every event.
async fn turn(lab: &Lab, port: u16, profile: Profile, tools: bool) -> Vec<StreamEvent> {
    let (tx, mut rx, dispatch) = direct(lab);
    tool_turn_with(
        profile,
        &ask(lab, tools),
        port,
        dispatch,
        &Timeouts::SERVER,
        &tx,
    )
    .await;
    drop(tx);
    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(event);
    }
    events
}

// Wires are not mixed ---------------------------------------------------------

#[tokio::test]
async fn the_ollama_profile_posts_to_api_chat_and_tool_turn_delegates_to_it() {
    let fake = fake(vec![text("hi")]).await;
    let lab = lab(fake.port);
    let events = turn(&lab, fake.port, Profile::Ollama, true).await;
    assert_eq!(events, [StreamEvent::Delta("hi".into()), done()]);

    let (tx, mut rx, dispatch) = direct(&lab);
    tool_turn(
        &ask(&lab, true),
        fake.port,
        dispatch,
        &Timeouts::SERVER,
        &tx,
    )
    .await;
    drop(tx);
    assert!(rx.recv().await.is_some());
    assert_eq!(fake.paths(), ["/api/chat", "/api/chat"]);
    let bodies = fake.bodies();
    assert_eq!(bodies[0], bodies[1], "tool_turn is tool_turn_with(Ollama)");
}

#[tokio::test]
async fn the_openai_profile_posts_to_chat_completions_with_the_openai_body() {
    let fake = fake(vec![sse_text("hi")]).await;
    let lab = lab(fake.port);
    let events = turn(&lab, fake.port, Profile::OpenAi, true).await;
    assert_eq!(events, [StreamEvent::Delta("hi".into()), done()]);
    assert_eq!(fake.paths(), ["/v1/chat/completions"]);
    let body = &fake.bodies()[0];
    assert_eq!(body["stream"], true);
    assert!(body["tools"].is_array(), "tools are offered");
    assert!(body.get("think").is_none());
}

// One round -------------------------------------------------------------------

#[cfg(target_os = "linux")]
#[tokio::test]
async fn one_openai_round_runs_the_call_and_echoes_the_server_id() {
    let fake = fake(vec![
        sse_call("find_dir", "{\"query\":\"curso\"}", Some("abc123")),
        sse_text("Found it"),
    ])
    .await;
    let lab = lab(fake.port);
    let events = turn(&lab, fake.port, Profile::OpenAi, true).await;
    assert_eq!(events.len(), 4, "{events:?}");
    assert!(matches!(&events[0], StreamEvent::ToolCall { name, .. } if name == "find_dir"));
    assert!(matches!(
        &events[1],
        StreamEvent::ToolResult {
            status: ToolStatus::Ok,
            ..
        }
    ));
    assert_eq!(events[2], StreamEvent::Delta("Found it".into()));
    assert_eq!(fake.paths(), ["/v1/chat/completions"; 2]);
    let messages = fake.bodies()[1]["messages"].as_array().unwrap().clone();
    let n = messages.len();
    assert_eq!(messages[n - 2]["tool_calls"][0]["id"], "abc123");
    assert_eq!(
        messages[n - 2]["tool_calls"][0]["function"]["arguments"],
        "{\"query\":\"curso\"}"
    );
    assert_eq!(messages[n - 1]["role"], "tool");
    assert_eq!(messages[n - 1]["tool_call_id"], "abc123");
    assert!(
        messages[n - 1]["content"]
            .as_str()
            .unwrap()
            .contains("curso-rust")
    );
}
