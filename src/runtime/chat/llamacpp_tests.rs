//! The tool loop on the OpenAI wire (llama.cpp) against a scripted fake
//! server, reusing the fake of `ollama_tests`. These drive `tool_turn_with`
//! directly, so they reach the loop while llamacpp is not in `MCP_VERIFIED`.

// What only the Linux-only tests use is dead code elsewhere.
#![cfg_attr(not(target_os = "linux"), allow(dead_code, unused_imports))]

use serde_json::{Value, json};

use super::{
    ollama::{NO_TOOLS_NOTICE, Profile, tool_turn, tool_turn_with},
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

// Rejected tools --------------------------------------------------------------

fn notices(events: &[StreamEvent]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Notice(line) => Some(line.as_str()),
            _ => None,
        })
        .collect()
}

/// The recorded reply of a server started with `--no-jinja` (F3).
fn no_jinja() -> Step {
    Step::Reply(include_bytes!("../../../tests/fixtures/llamacpp/no-jinja-tools.http").to_vec())
}

#[test]
fn the_openai_profile_rejects_4xx_and_5xx_that_name_tools_or_jinja() {
    for text in [
        "HTTP 400: tools are not supported",
        "HTTP 422: unprocessable, Jinja template",
        "HTTP 500: {\"error\":{\"message\":\"tools param requires --jinja flag\"}}",
    ] {
        assert!(Profile::OpenAi.rejected(text), "{text}");
    }
    for text in [
        "HTTP 500: boom",
        "HTTP 404: model not found",
        "HTTP 200: tools",
        "the stream broke: tools",
        "stream error: jinja",
    ] {
        assert!(!Profile::OpenAi.rejected(text), "{text}");
    }
}

#[test]
fn the_ollama_profile_keeps_the_plain_400_and_no_notice() {
    assert!(Profile::Ollama.rejected("HTTP 400: nope"));
    assert!(!Profile::Ollama.rejected("HTTP 500: tools param requires --jinja flag"));
    assert!(!Profile::Ollama.rejected("HTTP 422: tools"));
    assert_eq!(Profile::Ollama.notice(), None);
    assert_eq!(Profile::OpenAi.notice(), Some(NO_TOOLS_NOTICE));
}

#[tokio::test]
async fn the_recorded_no_jinja_reply_retries_once_as_text_with_one_notice() {
    let fake = fake(vec![no_jinja(), sse_text("plain answer")]).await;
    let lab = lab(fake.port);
    let events = turn(&lab, fake.port, Profile::OpenAi, true).await;
    assert_eq!(
        events,
        [
            StreamEvent::Notice(NO_TOOLS_NOTICE.into()),
            StreamEvent::Delta("plain answer".into()),
            done()
        ]
    );
    let bodies = fake.bodies();
    assert_eq!(bodies.len(), 2);
    assert!(bodies[0]["tools"].is_array());
    assert!(bodies[1].get("tools").is_none());
    assert_eq!(bodies[1]["messages"][0]["content"], "be brief");
    assert_eq!(fake.paths(), ["/v1/chat/completions"; 2]);
}

#[tokio::test]
async fn a_400_422_or_500_naming_tools_or_jinja_retries() {
    for (code, body) in [
        (400, "tools not supported"),
        (422, "needs jinja"),
        (500, "tools param requires --jinja flag"),
    ] {
        let fake = fake(vec![
            super::ollama_tests::status(code, body),
            sse_text("ok"),
        ])
        .await;
        let lab = lab(fake.port);
        let events = turn(&lab, fake.port, Profile::OpenAi, true).await;
        assert_eq!(notices(&events), [NO_TOOLS_NOTICE], "{code}");
        assert_eq!(fake.bodies().len(), 2, "{code}");
    }
}

#[tokio::test]
async fn an_unrelated_500_is_an_error_with_no_retry_and_no_notice() {
    let fake = fake(vec![super::ollama_tests::status(500, "out of memory")]).await;
    let lab = lab(fake.port);
    let events = turn(&lab, fake.port, Profile::OpenAi, true).await;
    assert!(
        matches!(events.last(), Some(StreamEvent::Error(ChatError::Failed(t))) if t.starts_with("HTTP 500")),
        "{events:?}"
    );
    assert!(notices(&events).is_empty());
    assert_eq!(fake.bodies().len(), 1);
}

#[tokio::test]
async fn the_retry_is_made_once() {
    let fake = fake(vec![no_jinja()]).await;
    let lab = lab(fake.port);
    let events = turn(&lab, fake.port, Profile::OpenAi, true).await;
    assert_eq!(fake.bodies().len(), 2, "one retry, not a loop");
    assert!(matches!(events.last(), Some(StreamEvent::Error(_))));
    assert_eq!(notices(&events).len(), 1);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_rejection_after_a_round_is_an_error_without_retry_or_notice() {
    let fake = fake(vec![
        sse_call("find_dir", "{\"query\":\"curso\"}", Some("a")),
        no_jinja(),
    ])
    .await;
    let lab = lab(fake.port);
    let events = turn(&lab, fake.port, Profile::OpenAi, true).await;
    assert!(
        matches!(events.last(), Some(StreamEvent::Error(ChatError::Failed(t))) if t.starts_with("HTTP 500")),
        "{events:?}"
    );
    assert!(notices(&events).is_empty());
    assert_eq!(fake.bodies().len(), 2);
}

#[tokio::test]
async fn an_in_stream_error_never_matches() {
    let step = Step::Reply(
        format!(
            "{OK_HEAD}data: {}\n\n",
            json!({"error": {"message": "tools exploded"}})
        )
        .into_bytes(),
    );
    let fake = fake(vec![step]).await;
    let lab = lab(fake.port);
    let events = turn(&lab, fake.port, Profile::OpenAi, true).await;
    assert!(
        matches!(events.last(), Some(StreamEvent::Error(_))),
        "{events:?}"
    );
    assert_eq!(fake.bodies().len(), 1);
    assert!(notices(&events).is_empty());
}
