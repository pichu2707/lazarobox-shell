//! The live runner against a recording fake server on an ephemeral port. Real
//! sockets and the real clock; every wait is bounded. These tests never touch a
//! real Ollama: the recorded fixtures stand in for it.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use serde_json::Value;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Notify,
    time::timeout,
};

use super::*;
use crate::core::ai::chat::{
    ANSWER_CAP, ChatState, Entry, REASONING_CAP, SendOutcome,
    engine::{Availability, ChatMessage, Finish, Role, Tools},
};

const QWEN3: &str = include_str!("../../../tests/fixtures/ollama/qwen3-thinking.ndjson");
const LLAMACPP: &str = include_str!("../../../tests/fixtures/llamacpp/qwen2.5-plain.sse");
const OK_HEAD: &str = "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n";

fn pane(n: u32) -> PaneId {
    PaneId::for_test(n)
}

fn request(model: &str) -> ChatRequest {
    ChatRequest {
        families: crate::core::ai::tools::registry::FamilySet::LEGACY,
        engine: "ollama".into(),
        model: Some(model.into()),
        system: "be brief".into(),
        memory: None,
        tools: Tools::Off,
        messages: vec![
            ChatMessage {
                role: Role::User,
                content: "hi".into(),
            },
            ChatMessage {
                role: Role::Assistant,
                content: "hello".into(),
            },
            ChatMessage {
                role: Role::User,
                content: "and now?".into(),
            },
        ],
    }
}

/// A server that records every request (head and body) and answers each
/// connection with `reply`, then closes.
struct Recorder {
    port: u16,
    requests: Arc<Mutex<Vec<Vec<u8>>>>,
    connections: Arc<AtomicUsize>,
}

impl Recorder {
    async fn start(reply: Vec<u8>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let recorder = Recorder {
            port: listener.local_addr().unwrap().port(),
            requests: Arc::default(),
            connections: Arc::default(),
        };
        let (requests, connections) = (recorder.requests.clone(), recorder.connections.clone());
        let reply = Arc::new(reply);
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                connections.fetch_add(1, Ordering::SeqCst);
                let (requests, reply) = (requests.clone(), reply.clone());
                tokio::spawn(async move {
                    let mut raw = Vec::new();
                    let mut buf = [0u8; 4096];
                    let want = loop {
                        if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = String::from_utf8_lossy(&raw[..end]).to_lowercase();
                            let length = head
                                .lines()
                                .find_map(|l| l.strip_prefix("content-length:"))
                                .and_then(|v| v.trim().parse::<usize>().ok())
                                .unwrap_or(0);
                            break end + 4 + length;
                        }
                        match stream.read(&mut buf).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => raw.extend_from_slice(&buf[..n]),
                        }
                    };
                    while raw.len() < want {
                        match stream.read(&mut buf).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => raw.extend_from_slice(&buf[..n]),
                        }
                    }
                    requests.lock().unwrap().push(raw);
                    let _ = stream.write_all(&reply).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        recorder
    }

    fn request(&self, n: usize) -> (String, Value) {
        let raw = self.requests.lock().unwrap()[n].clone();
        let end = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let head = String::from_utf8(raw[..end].to_vec()).unwrap();
        (head, serde_json::from_slice(&raw[end + 4..]).unwrap())
    }
}

fn reply(body: &str) -> Vec<u8> {
    format!("{OK_HEAD}{body}").into_bytes()
}

/// One turn through the live runner, up to and including the final event.
async fn run(port: u16, req: ChatRequest) -> Vec<StreamEvent> {
    let (tx, mut rx) = mpsc::channel(APP_CHANNEL);
    let mut chats = Chats::new(adapter_runner_at(Some(port)), tx);
    chats.send(pane(1), 1, req);
    let mut events = Vec::new();
    loop {
        let (_, _, event) = timeout(Duration::from_secs(10), rx.recv())
            .await
            .expect("an event within 10 s")
            .expect("the channel is open");
        let last = matches!(event, StreamEvent::Done(_) | StreamEvent::Error(_));
        events.push(event);
        if last {
            return events;
        }
    }
}

fn failed(text: &str) -> StreamEvent {
    StreamEvent::Error(ChatError::Failed(text.into()))
}

fn text(events: &[StreamEvent], pick: fn(&StreamEvent) -> Option<&str>) -> String {
    events.iter().filter_map(pick).collect()
}

fn answer(e: &StreamEvent) -> Option<&str> {
    match e {
        StreamEvent::Delta(t) => Some(t),
        _ => None,
    }
}

fn reasoning(e: &StreamEvent) -> Option<&str> {
    match e {
        StreamEvent::ThinkingDelta(t) => Some(t),
        _ => None,
    }
}

#[tokio::test]
async fn a_turn_posts_the_pinned_request_and_streams_the_answer() {
    let body = concat!(
        "{\"message\":{\"content\":\"Hel\"},\"done\":false}\n",
        "{\"message\":{\"content\":\"lo\"},\"done\":true,\"done_reason\":\"stop\"}\n",
    );
    let server = Recorder::start(reply(body)).await;
    let events = run(server.port, request("llama3:8b")).await;
    assert_eq!(
        events,
        [
            StreamEvent::Delta("Hel".into()),
            StreamEvent::Delta("lo".into()),
            StreamEvent::Done(Finish::Stop)
        ]
    );
    let (head, json) = server.request(0);
    let head = head.to_lowercase();
    assert!(head.starts_with("post /api/chat http/1.1\r\n"), "{head}");
    assert!(head.contains("host: 127.0.0.1:"), "{head}");
    for forbidden in ["authorization", "cookie", "proxy-"] {
        assert!(!head.contains(forbidden), "{forbidden} in {head}");
    }
    assert_eq!(json["model"], "llama3:8b");
    assert_eq!(json["stream"], true);
    assert_eq!(json["messages"].as_array().unwrap().len(), 4);
    assert_eq!(json.as_object().unwrap().len(), 3, "no think option");
    // Only the chat POST: nothing pulled a model or fetched anything else.
    assert_eq!(server.connections.load(Ordering::SeqCst), 1);
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_reasoning_model_streams_thinking_then_the_answer() {
    let server = Recorder::start(reply(QWEN3)).await;
    let events = run(server.port, request("qwen3:14b")).await;
    assert_eq!(text(&events, answer), "4.");
    assert!(text(&events, reasoning).starts_with("Okay"));
    assert_eq!(events.last(), Some(&StreamEvent::Done(Finish::Stop)));
    // Reasoning ends before the answer starts, as recorded.
    let first_answer = events.iter().position(|e| answer(e).is_some()).unwrap();
    assert!(
        events[..first_answer]
            .iter()
            .all(|e| reasoning(e).is_some())
    );
}

#[tokio::test]
async fn reasoning_past_the_cap_is_cut_without_an_error() {
    let line = format!(
        "{{\"message\":{{\"thinking\":\"{}\"}},\"done\":false}}\n",
        "t".repeat(1024)
    );
    let mut body = line.repeat(REASONING_CAP / 1024 + 64);
    body.push_str("{\"message\":{\"content\":\"ok\"},\"done\":false}\n");
    body.push_str("{\"done\":true,\"done_reason\":\"stop\"}\n");
    let server = Recorder::start(reply(&body)).await;
    let events = run(server.port, request("qwen3:14b")).await;
    assert_eq!(text(&events, reasoning).len(), REASONING_CAP);
    assert_eq!(text(&events, answer), "ok");
    assert_eq!(events.last(), Some(&StreamEvent::Done(Finish::Stop)));
}

#[tokio::test]
async fn the_request_does_not_carry_earlier_reasoning() {
    let availability = Availability::Ready {
        engine: "ollama".into(),
        model: Some("qwen3:14b".into()),
    };
    // Turn one runs through the live runner and is folded into a real chat.
    let first = Recorder::start(reply(QWEN3)).await;
    let mut chat = ChatState::new(None);
    let SendOutcome::Sent { turn, request: one } = chat.send("what is 2+2?", &availability, "sys")
    else {
        panic!("first send refused");
    };
    let events = run(first.port, one).await;
    let thought = text(&events, reasoning);
    assert!(thought.len() > 20, "the fixture reasons: {thought:?}");
    let events_answer = events.clone();
    for event in events {
        chat.apply(turn, event);
    }
    assert!(
        chat.entries()
            .iter()
            .any(|e| matches!(e, Entry::Reasoning(r) if r.text() == thought)),
        "the chat holds the reasoning entry"
    );
    let answer_text = text(&events_answer, answer);
    assert_eq!(answer_text, "4.");
    assert!(chat.messages().any(|m| m.text == answer_text));
    // Turn two is built from that chat: the POST body has the answer, never
    // the reasoning.
    let SendOutcome::Sent { request: two, .. } = chat.send("and now?", &availability, "sys") else {
        panic!("second send refused");
    };
    assert_eq!(two.messages.len(), 3);
    let second = Recorder::start(reply(QWEN3)).await;
    let _ = run(second.port, two).await;
    let (_, json) = second.request(0);
    let body = json.to_string();
    assert!(body.contains(&answer_text), "{body}");
    assert!(!body.contains(&thought), "reasoning leaked into {body}");
    assert!(!body.contains("Okay"), "reasoning leaked into {body}");
}

#[tokio::test]
async fn a_saved_model_change_applies_to_the_next_message() {
    let body = "{\"done\":true,\"done_reason\":\"stop\"}\n";
    let server = Recorder::start(reply(body)).await;
    let _ = run(server.port, request("llama3:8b")).await;
    let _ = run(server.port, request("qwen3:14b")).await;
    assert_eq!(server.request(0).1["model"], "llama3:8b");
    assert_eq!(server.request(1).1["model"], "qwen3:14b");
}

#[tokio::test]
async fn an_error_object_in_the_stream_ends_the_turn() {
    let server = Recorder::start(reply("{\"error\":\"model requires more memory\"}\n")).await;
    assert_eq!(
        run(server.port, request("big")).await,
        [failed("model requires more memory")]
    );
}

#[tokio::test]
async fn a_malformed_line_ends_the_turn() {
    let body = "{\"message\":{\"content\":\"a\"},\"done\":false}\nnot json\n";
    let server = Recorder::start(reply(body)).await;
    assert_eq!(
        run(server.port, request("m")).await,
        [
            StreamEvent::Delta("a".into()),
            failed("malformed stream data")
        ]
    );
}

#[tokio::test]
async fn an_oversized_line_ends_the_turn() {
    let body = format!("{}\n", "a".repeat((1 << 20) + 16));
    let server = Recorder::start(reply(&body)).await;
    assert_eq!(
        run(server.port, request("m")).await,
        [failed("stream line over the size limit")]
    );
}

#[tokio::test]
async fn a_404_for_an_unknown_model_is_a_short_error() {
    let body = format!(
        "{{\"error\":\"model 'nope' not found\"}}{}",
        " ".repeat(1 << 20)
    );
    let head = format!(
        "HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let server = Recorder::start(format!("{head}{body}").into_bytes()).await;
    let events = run(server.port, request("nope")).await;
    let [StreamEvent::Error(ChatError::Failed(message))] = events.as_slice() else {
        panic!("one error expected, got {events:?}");
    };
    assert!(message.contains("404"), "{message}");
    assert!(message.chars().count() <= 200, "{message}");
    // The turn only ever asked to chat: no pull to fix the missing model.
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_redirect_is_an_error_and_the_location_is_never_requested() {
    let other = Recorder::start(reply("{\"done\":true}\n")).await;
    let redirect = format!(
        "HTTP/1.1 307 Temporary Redirect\r\nLocation: http://127.0.0.1:{}/api/chat\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        other.port
    );
    let server = Recorder::start(redirect.into_bytes()).await;
    let events = run(server.port, request("m")).await;
    assert!(
        matches!(events.as_slice(), [StreamEvent::Error(_)]),
        "{events:?}"
    );
    assert_eq!(other.connections.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn ambiguous_framing_is_rejected_before_any_delta() {
    let raw = "HTTP/1.1 200 OK\r\nContent-Length: 10\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\nabc\r\n0\r\n\r\n";
    let server = Recorder::start(raw.as_bytes().to_vec()).await;
    let events = run(server.port, request("m")).await;
    assert!(
        matches!(events.as_slice(), [StreamEvent::Error(_)]),
        "{events:?}"
    );
}

#[tokio::test]
async fn the_answer_cap_ends_the_turn_with_an_error() {
    let line = format!(
        "{{\"message\":{{\"content\":\"{}\"}},\"done\":false}}\n",
        "a".repeat(1024)
    );
    let server = Recorder::start(reply(&line.repeat(ANSWER_CAP / 1024 + 64))).await;
    let events = run(server.port, request("m")).await;
    assert_eq!(
        events.last(),
        Some(&StreamEvent::Error(ChatError::Truncated))
    );
    assert_eq!(text(&events, answer).len(), ANSWER_CAP);
}

#[tokio::test]
async fn an_openai_compatible_turn_posts_the_pinned_request_and_streams_the_answer() {
    for engine in ["lmstudio", "llamacpp", "vllm"] {
        let server = Recorder::start(reply(LLAMACPP)).await;
        let mut req = request("qwen2.5");
        req.engine = engine.into();
        let events = run(server.port, req).await;
        assert_eq!(
            events,
            [
                StreamEvent::Delta("Hi".into()),
                StreamEvent::Delta(" there".into()),
                StreamEvent::Delta(".".into()),
                StreamEvent::Done(Finish::Stop)
            ],
            "{engine}"
        );
        let (head, json) = server.request(0);
        let head = head.to_lowercase();
        assert!(
            head.starts_with("post /v1/chat/completions http/1.1\r\n"),
            "{engine}: {head}"
        );
        assert!(head.contains("host: 127.0.0.1:"), "{engine}: {head}");
        for forbidden in ["authorization", "cookie", "proxy-"] {
            assert!(!head.contains(forbidden), "{engine}: {forbidden} in {head}");
        }
        assert_eq!(json["model"], "qwen2.5", "{engine}");
        assert_eq!(json["stream"], true, "{engine}");
        assert_eq!(json["messages"].as_array().unwrap().len(), 4, "{engine}");
        assert_eq!(json.as_object().unwrap().len(), 3, "{engine}");
        assert_eq!(server.connections.load(Ordering::SeqCst), 1, "{engine}");
        assert_eq!(server.requests.lock().unwrap().len(), 1, "{engine}");
    }
}

#[tokio::test]
async fn a_turn_goes_to_the_configured_port_of_its_engine() {
    let llamacpp = Recorder::start(reply(LLAMACPP)).await;
    let ollama = Recorder::start(reply("{\"done\":true}\n")).await;
    let ports = ServerPorts::default()
        .with("llamacpp", llamacpp.port)
        .with("ollama", ollama.port);
    let (tx, mut rx) = mpsc::channel(APP_CHANNEL);
    let mut chats = Chats::new(adapter_runner_with(ports), tx);
    let mut req = request("qwen2.5");
    req.engine = "llamacpp".into();
    chats.send(pane(1), 1, req);
    loop {
        let (_, _, event) = timeout(Duration::from_secs(10), rx.recv())
            .await
            .expect("an event within 10 s")
            .expect("the channel is open");
        if matches!(event, StreamEvent::Done(_) | StreamEvent::Error(_)) {
            assert_eq!(event, StreamEvent::Done(Finish::Stop));
            break;
        }
    }
    assert_eq!(llamacpp.connections.load(Ordering::SeqCst), 1);
    assert_eq!(ollama.connections.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn an_engine_without_an_adapter_fails_the_turn_and_connects_nowhere() {
    let server = Recorder::start(reply("{\"done\":true}\n")).await;
    let mut req = request("m");
    req.engine = "opencode".into();
    assert_eq!(run(server.port, req).await, [failed(NO_ADAPTER)]);
    assert_eq!(server.connections.load(Ordering::SeqCst), 0);
}

/// Sends the head and one thinking line, then stalls until the client closes
/// the connection. `closed` fires when it does.
async fn start_stalling_server() -> (u16, Arc<Notify>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let closed = Arc::new(Notify::new());
    let signal = closed.clone();
    tokio::spawn(async move {
        let Ok((mut stream, _)) = listener.accept().await else {
            return;
        };
        let mut buf = [0u8; 4096];
        let mut raw = Vec::new();
        // The request head is enough: the body is never needed here.
        while !raw.windows(4).any(|w| w == b"\r\n\r\n") {
            match stream.read(&mut buf).await {
                Ok(0) | Err(_) => return,
                Ok(n) => raw.extend_from_slice(&buf[..n]),
            }
        }
        let line = "{\"message\":{\"thinking\":\"Let me\"},\"done\":false}\n";
        let _ = stream.write_all(OK_HEAD.as_bytes()).await;
        let _ = stream.write_all(line.as_bytes()).await;
        // Stall: nothing more is sent. A read that ends is the client leaving.
        loop {
            match stream.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
        signal.notify_one();
    });
    (port, closed)
}

#[tokio::test]
async fn cancelling_while_the_model_thinks_closes_the_connection_and_ends_the_events() {
    let (port, closed) = start_stalling_server().await;
    let (tx, mut rx) = mpsc::channel(APP_CHANNEL);
    let mut chats = Chats::new(adapter_runner_at(Some(port)), tx);
    chats.send(pane(1), 1, request("qwen3:14b"));
    let (_, turn, first) = timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("the thinking delta arrives")
        .expect("the channel is open");
    assert_eq!(
        (turn, first),
        (1, StreamEvent::ThinkingDelta("Let me".into()))
    );

    chats.cancel(pane(1));

    // The adapter was dropped: the socket is closed on the server side.
    timeout(Duration::from_secs(5), closed.notified())
        .await
        .expect("the connection closes after the cancel");
    // No further event, not even a final one: the app ends the turn itself.
    assert!(
        timeout(Duration::from_millis(200), rx.recv())
            .await
            .is_err(),
        "no event after the cancel"
    );
}

/// Turns of `req` through a runner that manages a llama-server, up to and
/// including the final event, with the `Chats` kept for a shutdown.
async fn run_managed(
    ports: ServerPorts,
    managed: Arc<Managed>,
    req: ChatRequest,
) -> (Vec<StreamEvent>, Chats) {
    let (tx, mut rx) = mpsc::channel(APP_CHANNEL);
    let live = LiveServers::new(ports, Some(managed));
    let mut chats = Chats::new(adapter_runner_live(live.clone()), tx).with_live(live);
    chats.send(pane(1), 1, req);
    let mut events = Vec::new();
    loop {
        let (_, _, event) = timeout(Duration::from_secs(10), rx.recv())
            .await
            .expect("an event within 10 s")
            .expect("the channel is open");
        let last = matches!(event, StreamEvent::Done(_) | StreamEvent::Error(_));
        events.push(event);
        if last {
            return (events, chats);
        }
    }
}

/// A managed llama-server whose program is `program` (its folder is the
/// `PATH`), with a probe that answers 200 once `ready` exists.
fn managed_at(program: &std::path::Path, port: u16, ready: std::path::PathBuf) -> Arc<Managed> {
    let probe: managed::Probe = Arc::new(move |_| {
        let up = ready.exists();
        Box::pin(async move { up.then_some(200) })
    });
    Arc::new(Managed::with(
        program.file_name().unwrap().to_owned(),
        format!("{}:/usr/bin:/bin", program.parent().unwrap().display()).into(),
        "/m/q.gguf".into(),
        port,
        None,
        probe,
        Duration::from_secs(5),
    ))
}

// Spec: A managed llama-server starts on the first turn.
#[tokio::test]
async fn a_llamacpp_turn_starts_the_managed_server_and_quitting_stops_it() {
    let llamacpp = Recorder::start(reply(LLAMACPP)).await;
    let dir = tempfile::tempdir().unwrap();
    let (program, started) = (dir.path().join("llama-server"), dir.path().join("started"));
    std::fs::write(
        &program,
        format!("#!/bin/sh\ntouch '{}'\nexec sleep 30\n", started.display()),
    )
    .unwrap();
    std::fs::set_permissions(
        &program,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    let managed = managed_at(&program, llamacpp.port, started.clone());
    let mut req = request("qwen2.5");
    req.engine = "llamacpp".into();
    let ports = ServerPorts::default().with("llamacpp", llamacpp.port);
    let (events, mut chats) = run_managed(ports, managed.clone(), req).await;
    assert_eq!(events.last(), Some(&StreamEvent::Done(Finish::Stop)));
    assert_eq!(llamacpp.connections.load(Ordering::SeqCst), 1);
    let pid = managed.pid().await.expect("the server runs");
    chats.shutdown(Duration::from_secs(5)).await;
    assert_eq!(managed.pid().await, None);
    assert!(crate::runtime::proc::process_state(pid as libc::pid_t).is_none());
}

// Spec S4 (llamacpp-pick-model): a server the menu turned on after startup
// is the one the next llamacpp turn starts and talks to, and quitting stops it.
#[tokio::test]
async fn a_turn_uses_the_managed_server_turned_on_while_running() {
    let llamacpp = Recorder::start(reply(LLAMACPP)).await;
    let dir = tempfile::tempdir().unwrap();
    let (program, started) = (dir.path().join("llama-server"), dir.path().join("started"));
    std::fs::write(
        &program,
        format!("#!/bin/sh\ntouch '{}'\nexec sleep 30\n", started.display()),
    )
    .unwrap();
    std::fs::set_permissions(
        &program,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    // Unmanaged at startup, on a port where nothing listens.
    let live = LiveServers::new(ServerPorts::default().with("llamacpp", 9), None);
    let (tx, mut rx) = mpsc::channel(APP_CHANNEL);
    let mut chats = Chats::new(adapter_runner_live(live.clone()), tx).with_live(live.clone());
    let managed = managed_at(&program, llamacpp.port, started);
    assert!(live.manage(Arc::clone(&managed)).is_none());
    let mut req = request("qwen2.5");
    req.engine = "llamacpp".into();
    chats.send(pane(1), 1, req);
    let last = loop {
        let (_, _, event) = timeout(Duration::from_secs(10), rx.recv())
            .await
            .expect("an event within 10 s")
            .expect("the channel is open");
        if matches!(event, StreamEvent::Done(_) | StreamEvent::Error(_)) {
            break event;
        }
    };
    assert_eq!(last, StreamEvent::Done(Finish::Stop));
    assert_eq!(llamacpp.connections.load(Ordering::SeqCst), 1);
    let pid = managed.pid().await.expect("the server runs");
    chats.shutdown(Duration::from_secs(5)).await;
    assert!(crate::runtime::proc::process_state(pid as libc::pid_t).is_none());
}

// Spec: A failed start says why.
#[tokio::test]
async fn a_managed_start_that_fails_ends_the_turn_and_connects_nowhere() {
    let llamacpp = Recorder::start(reply(LLAMACPP)).await;
    let dir = tempfile::tempdir().unwrap();
    let managed = managed_at(
        &dir.path().join("nope"),
        llamacpp.port,
        dir.path().join("x"),
    );
    let mut req = request("qwen2.5");
    req.engine = "llamacpp".into();
    let ports = ServerPorts::default().with("llamacpp", llamacpp.port);
    let (events, _chats) = run_managed(ports, managed, req).await;
    assert_eq!(
        events,
        [failed("could not start llama-server: is it on the PATH?")]
    );
    assert_eq!(llamacpp.connections.load(Ordering::SeqCst), 0);
}

// Spec: A managed llama-server starts on the first turn (other engines).
#[tokio::test]
async fn a_turn_of_another_engine_never_starts_the_managed_server() {
    let ollama = Recorder::start(reply("{\"done\":true}\n")).await;
    let dir = tempfile::tempdir().unwrap();
    let managed = managed_at(&dir.path().join("nope"), 39281, dir.path().join("x"));
    let ports = ServerPorts::default().with("ollama", ollama.port);
    let (events, _chats) = run_managed(ports, managed.clone(), request("m")).await;
    assert_eq!(events, [StreamEvent::Done(Finish::Stop)]);
    assert_eq!(managed.pid().await, None);
}
