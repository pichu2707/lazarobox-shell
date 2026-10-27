//! The Ollama tool loop against a scripted fake server: real sockets, the
//! real clock, every wait bounded. The n-th connection gets the n-th step (the
//! last one repeats), and the bodies the loop sent are recorded.

// What only the Linux-only tests use is dead code elsewhere.
#![cfg_attr(not(target_os = "linux"), allow(dead_code, unused_imports))]

use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Notify,
    time::timeout,
};

use super::{tests::done, *};
use crate::{
    core::ai::{
        chat::{
            ANSWER_CAP, ChatState, Entry, REASONING_CAP, SendOutcome,
            approval::ApprovalAsk,
            engine::{Availability, ChatMessage, Decision, Role, ToolStatus, Tools},
        },
        tools::Locality,
    },
    runtime::{
        loopback::{NO_ANSWER, TOO_LONG, Timeouts},
        tools::{Limits, ToolHost, TurnScope},
    },
};

const OK_HEAD: &str = "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n";

fn pane(n: u32) -> PaneId {
    PaneId::for_test(n)
}

#[derive(Clone)]
enum Step {
    Reply(Vec<u8>),
    /// Reads the request, then answers nothing and keeps the connection.
    Hang,
    /// Answers after a pause.
    Delay(u64, Vec<u8>),
}

fn lines(values: &[Value]) -> Step {
    let body: String = values.iter().map(|v| format!("{v}\n")).collect();
    Step::Reply(format!("{OK_HEAD}{body}").into_bytes())
}

fn end_line() -> Value {
    json!({"message": {"content": ""}, "done": true, "done_reason": "stop"})
}

/// A reply that calls one tool.
fn calls(name: &str, arguments: Value) -> Step {
    lines(&[
        json!({"message": {"content": "", "tool_calls": [
            {"function": {"name": name, "arguments": arguments}}
        ]}, "done": false}),
        end_line(),
    ])
}

/// A reply that calls several tools.
fn calls_many(list: &[(&str, Value)]) -> Step {
    let entries: Vec<Value> = list
        .iter()
        .map(|(name, arguments)| json!({"function": {"name": name, "arguments": arguments}}))
        .collect();
    lines(&[
        json!({"message": {"content": "", "tool_calls": entries}, "done": false}),
        end_line(),
    ])
}

/// A reply with plain text.
fn text(content: &str) -> Step {
    lines(&[
        json!({"message": {"content": content}, "done": false}),
        end_line(),
    ])
}

fn status(code: u16, body: &str) -> Step {
    Step::Reply(
        format!(
            "HTTP/1.1 {code} X\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .into_bytes(),
    )
}

struct Fake {
    port: u16,
    bodies: Arc<Mutex<Vec<Value>>>,
    connections: Arc<AtomicUsize>,
    /// A `Hang` step read its request.
    hung: Arc<Notify>,
    /// The client closed a hung connection.
    closed: Arc<Notify>,
}

impl Fake {
    fn bodies(&self) -> Vec<Value> {
        self.bodies.lock().unwrap().clone()
    }
}

async fn fake(steps: Vec<Step>) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fake = Fake {
        port: listener.local_addr().unwrap().port(),
        bodies: Arc::default(),
        connections: Arc::default(),
        hung: Arc::default(),
        closed: Arc::default(),
    };
    let (bodies, connections) = (fake.bodies.clone(), fake.connections.clone());
    let (hung, closed) = (fake.hung.clone(), fake.closed.clone());
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let n = connections.fetch_add(1, Ordering::SeqCst);
            let step = steps[n.min(steps.len() - 1)].clone();
            let (bodies, hung, closed) = (bodies.clone(), hung.clone(), closed.clone());
            tokio::spawn(async move {
                let mut raw = Vec::new();
                let mut buf = [0u8; 8192];
                let end = loop {
                    if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                        break end + 4;
                    }
                    match stream.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => raw.extend_from_slice(&buf[..n]),
                    }
                };
                let head = String::from_utf8_lossy(&raw[..end]).to_lowercase();
                let length: usize = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(0);
                while raw.len() < end + length {
                    match stream.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => raw.extend_from_slice(&buf[..n]),
                    }
                }
                bodies
                    .lock()
                    .unwrap()
                    .push(serde_json::from_slice(&raw[end..end + length]).unwrap());
                match step {
                    Step::Reply(bytes) => {
                        let _ = stream.write_all(&bytes).await;
                        let _ = stream.shutdown().await;
                    }
                    Step::Delay(ms, bytes) => {
                        tokio::time::sleep(Duration::from_millis(ms)).await;
                        let _ = stream.write_all(&bytes).await;
                        let _ = stream.shutdown().await;
                    }
                    Step::Hang => {
                        hung.notify_one();
                        while matches!(stream.read(&mut buf).await, Ok(n) if n > 0) {}
                        closed.notify_one();
                    }
                }
            });
        }
    });
    fake
}

struct Lab {
    _dir: tempfile::TempDir,
    home: PathBuf,
    host: Arc<ToolHost>,
    chats: Chats,
    rx: mpsc::Receiver<Tagged>,
}

fn lab(port: u16) -> Lab {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap().join("home");
    std::fs::create_dir_all(home.join("dev/curso-rust")).unwrap();
    std::fs::write(home.join("note.txt"), "hello from the note").unwrap();
    let host = ToolHost::new(Some(home.clone().into()), None, None, Limits::default());
    let (tx, rx) = mpsc::channel(APP_CHANNEL);
    let chats = Chats::new(adapter_runner_at(Some(port)), tx)
        .with_tools(host.clone(), vec![dir.path().join("run")]);
    Lab {
        _dir: dir,
        home,
        host,
        chats,
        rx,
    }
}

fn ask(lab: &Lab, tools: bool) -> ChatRequest {
    let mut req = tests::request("be brief");
    req.messages = vec![ChatMessage {
        role: Role::User,
        content: "find the rust course".into(),
    }];
    if tools {
        req.tools = Tools::On {
            origin: Some(lab.home.clone()),
            external: Default::default(),
            mcp: None,
        };
    }
    req
}

async fn next(rx: &mut mpsc::Receiver<Tagged>) -> Tagged {
    timeout(Duration::from_secs(10), rx.recv())
        .await
        .expect("an event within 10 s")
        .expect("the channel is open")
}

/// Events up to and including the final one.
async fn finish(rx: &mut mpsc::Receiver<Tagged>) -> Vec<StreamEvent> {
    let mut events = Vec::new();
    loop {
        let (_, _, event) = next(rx).await;
        let last = matches!(event, StreamEvent::Done(_) | StreamEvent::Error(_));
        events.push(event);
        if last {
            return events;
        }
    }
}

async fn next_call(rx: &mut mpsc::Receiver<Tagged>) -> (u64, CallId, Option<ApprovalAsk>) {
    loop {
        if let (_, turn, StreamEvent::ToolCall { id, approval, .. }) = next(rx).await {
            return (turn, id, approval.map(|ask| *ask));
        }
    }
}

fn tool_calls(events: &[StreamEvent]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e, StreamEvent::ToolCall { .. }))
        .count()
}

// One round ------------------------------------------------------------------

// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn one_round_gives_one_call_and_result_and_the_second_request_carries_them() {
    let fake = fake(vec![
        calls("find_dir", json!({"query": "curso"})),
        text("Found it"),
    ])
    .await;
    let mut lab = lab(fake.port);
    lab.chats.send(pane(1), 1, ask(&lab, true));
    let events = finish(&mut lab.rx).await;
    assert_eq!(events.len(), 4, "{events:?}");
    assert!(
        matches!(&events[0], StreamEvent::ToolCall { name, approval: None, .. } if name == "find_dir"),
        "{events:?}"
    );
    assert!(matches!(
        &events[1],
        StreamEvent::ToolResult {
            status: ToolStatus::Ok,
            ..
        }
    ));
    assert_eq!(events[2], StreamEvent::Delta("Found it".into()));
    assert_eq!(events[3], done());

    let bodies = fake.bodies();
    assert_eq!(bodies.len(), 2);
    assert!(
        bodies[0]["tools"].is_array(),
        "the first request offers tools"
    );
    let system = bodies[0]["messages"][0]["content"].as_str().unwrap();
    assert!(system.starts_with("be brief"), "{system}");
    assert!(system.len() > "be brief".len(), "the tools prompt is added");
    let messages = bodies[1]["messages"].as_array().unwrap();
    let n = messages.len();
    assert_eq!(messages[n - 2]["role"], "assistant");
    assert_eq!(
        messages[n - 2]["tool_calls"][0]["function"]["name"],
        "find_dir"
    );
    assert_eq!(messages[n - 1]["role"], "tool");
    assert_eq!(messages[n - 1]["tool_name"], "find_dir");
    assert!(
        messages[n - 1]["content"]
            .as_str()
            .unwrap()
            .contains("curso-rust"),
        "{}",
        messages[n - 1]
    );
    // Ollama goes through no MCP socket.
    assert!(lab.chats.session(pane(1)).is_none());
}

#[tokio::test]
async fn a_turn_without_tools_is_one_plain_request() {
    let fake = fake(vec![text("hi")]).await;
    let mut lab = lab(fake.port);
    lab.chats.send(pane(1), 1, ask(&lab, false));
    let events = finish(&mut lab.rx).await;
    assert_eq!(events, [StreamEvent::Delta("hi".into()), done()]);
    let bodies = fake.bodies();
    assert_eq!(bodies.len(), 1);
    assert!(bodies[0].get("tools").is_none());
    let Invocation::Http { body, .. } = Adapter::Ollama.build(&ask(&lab, false)) else {
        panic!("ollama is an http engine");
    };
    assert_eq!(bodies[0], serde_json::from_slice::<Value>(&body).unwrap());
}

// Tools on, but the app has no tool host: a plain text turn.
#[tokio::test]
async fn tools_on_without_a_host_is_a_plain_turn() {
    let fake = fake(vec![text("hi")]).await;
    let (tx, mut rx) = mpsc::channel(APP_CHANNEL);
    let mut chats = Chats::new(adapter_runner_at(Some(fake.port)), tx);
    let lab = lab(fake.port);
    chats.send(pane(1), 1, ask(&lab, true));
    let events = finish(&mut rx).await;
    assert_eq!(events, [StreamEvent::Delta("hi".into()), done()]);
    assert!(fake.bodies()[0].get("tools").is_none());
}

#[tokio::test]
async fn a_call_the_registry_refuses_still_gets_a_tool_message() {
    let fake = fake(vec![calls("rm_rf", json!({})), text("ok")]).await;
    let mut lab = lab(fake.port);
    lab.chats.send(pane(1), 1, ask(&lab, true));
    let events = finish(&mut lab.rx).await;
    assert_eq!(tool_calls(&events), 0, "{events:?}");
    assert_eq!(events.last(), Some(&done()));
    let bodies = fake.bodies();
    let tool = bodies[1]["messages"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(tool["role"], "tool");
    assert_eq!(tool["content"], "unknown tool");
}

// P0b.2: the set the request offered also gates the parse. A model that names
// a tool it was not offered gets the unknown-tool message and nothing runs.
#[tokio::test]
async fn a_tool_outside_the_offered_families_is_refused_and_not_run() {
    use crate::core::ai::tools::registry::{Family, FamilySet};
    let fake = fake(vec![
        calls("run_in_pane", json!({"command": "ls"})),
        text("ok"),
    ])
    .await;
    let mut lab = lab(fake.port);
    let mut req = ask(&lab, true);
    req.families = FamilySet::EMPTY.with(Family::Read);
    lab.chats.send(pane(1), 1, req);
    let events = finish(&mut lab.rx).await;
    assert_eq!(tool_calls(&events), 0, "{events:?}");
    let bodies = fake.bodies();
    let offered: Vec<&str> = bodies[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["function"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(offered, ["find_dir", "list_dir", "read_file"]);
    let tool = bodies[1]["messages"].as_array().unwrap().last().unwrap();
    assert_eq!(tool["role"], "tool");
    assert_eq!(tool["content"], "unknown tool");
}

// The cap ----------------------------------------------------------------------

#[tokio::test]
async fn the_round_cap_ends_the_turn_with_a_system_line_and_idle() {
    let fake = fake(vec![calls("find_dir", json!({"query": "curso"}))]).await;
    let mut lab = lab(fake.port);
    lab.chats.send(pane(1), 1, ask(&lab, true));
    let events = finish(&mut lab.rx).await;
    assert_eq!(fake.bodies().len(), ollama::MAX_ROUNDS);
    // The calls of the last reply are not run: nobody could use the result.
    assert_eq!(tool_calls(&events), ollama::MAX_ROUNDS - 1);
    let n = events.len();
    assert_eq!(
        events[n - 2],
        StreamEvent::Notice(ollama::CAP_NOTICE.into())
    );
    assert_eq!(events[n - 1], done());
}

// Spec (chat-engines): the cap is 8 rounds, and 24 once a coding family is on.
#[test]
fn the_round_limit_follows_the_coding_families() {
    use crate::core::ai::tools::registry::{Family, FamilySet};
    assert_eq!(ollama::max_rounds(FamilySet::LEGACY), 8);
    assert_eq!(ollama::max_rounds(FamilySet::EMPTY), 8);
    for family in [
        Family::Search,
        Family::Write,
        Family::Exec,
        Family::Git,
        Family::GitWrite,
    ] {
        assert_eq!(
            ollama::max_rounds(FamilySet::LEGACY.with(family)),
            24,
            "{family:?}"
        );
    }
}

#[tokio::test]
async fn a_coding_family_raises_the_round_cap_of_a_turn() {
    use crate::core::ai::tools::registry::{Family, FamilySet};
    let fake = fake(vec![calls("find_dir", json!({"query": "curso"}))]).await;
    let mut lab = lab(fake.port);
    let mut request = ask(&lab, true);
    request.families = FamilySet::LEGACY.with(Family::Search);
    lab.chats.send(pane(1), 1, request);
    let events = finish(&mut lab.rx).await;
    assert_eq!(fake.bodies().len(), 24);
    assert_eq!(tool_calls(&events), 23);
}

// Cancel -----------------------------------------------------------------------

#[tokio::test]
async fn cancel_inside_the_loop_closes_the_socket_and_sends_nothing_more() {
    let fake = fake(vec![
        calls("find_dir", json!({"query": "curso"})),
        Step::Hang,
    ])
    .await;
    let mut lab = lab(fake.port);
    lab.chats.send(pane(1), 1, ask(&lab, true));
    timeout(Duration::from_secs(10), fake.hung.notified())
        .await
        .expect("the follow-up request arrived");
    lab.chats.cancel(pane(1));
    timeout(Duration::from_secs(10), fake.closed.notified())
        .await
        .expect("the socket closed");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(fake.connections.load(Ordering::SeqCst), 2);
}

// Rejection --------------------------------------------------------------------

#[tokio::test]
async fn a_400_on_a_request_with_tools_retries_once_without_tools() {
    let fake = fake(vec![
        status(400, "{\"error\":\"this model does not support tools\"}"),
        text("plain answer"),
    ])
    .await;
    let mut lab = lab(fake.port);
    lab.chats.send(pane(1), 1, ask(&lab, true));
    let events = finish(&mut lab.rx).await;
    assert_eq!(events, [StreamEvent::Delta("plain answer".into()), done()]);
    let bodies = fake.bodies();
    assert_eq!(bodies.len(), 2);
    assert!(bodies[0]["tools"].is_array());
    assert!(bodies[1].get("tools").is_none());
    assert_eq!(
        bodies[0]["model"], bodies[1]["model"],
        "the model is unchanged"
    );
    // The fallback must not advertise tools the request no longer offers.
    assert_eq!(bodies[1]["messages"][0]["content"], "be brief");
    assert_ne!(bodies[0]["messages"][0]["content"], "be brief");
}

// A retried reply is plain text: whatever calls it carries are not run.
#[tokio::test]
async fn calls_in_the_reply_to_the_retry_are_not_run() {
    let fake = fake(vec![
        status(400, "no tools"),
        calls_many(&[
            ("run_in_pane", json!({"command": "ls"})),
            ("read_file", json!({"path": "~/note.txt"})),
        ]),
    ])
    .await;
    let mut lab = lab(fake.port);
    lab.chats.send(pane(1), 1, ask(&lab, true));
    let events = finish(&mut lab.rx).await;
    assert_eq!(events, [done()]);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(fake.bodies().len(), 2, "no request follows");
}

// After a round a 400 is an ordinary error: the model did take tools.
#[tokio::test]
async fn a_400_after_a_round_is_an_error_not_a_retry() {
    let fake = fake(vec![
        calls("find_dir", json!({"query": "curso"})),
        status(400, "context too long"),
    ])
    .await;
    let mut lab = lab(fake.port);
    lab.chats.send(pane(1), 1, ask(&lab, true));
    let events = finish(&mut lab.rx).await;
    assert!(
        matches!(events.last(), Some(StreamEvent::Error(ChatError::Failed(t))) if t.starts_with("HTTP 400")),
        "{events:?}"
    );
    assert_eq!(fake.bodies().len(), 2);
}

#[tokio::test]
async fn the_retry_is_made_once_and_only_for_a_400() {
    let both = fake(vec![status(400, "nope")]).await;
    let mut lab_a = lab(both.port);
    lab_a.chats.send(pane(1), 1, ask(&lab_a, true));
    let events = finish(&mut lab_a.rx).await;
    assert_eq!(both.bodies().len(), 2, "one retry, not a loop");
    assert!(
        matches!(events.last(), Some(StreamEvent::Error(ChatError::Failed(t))) if t.starts_with("HTTP 400"))
    );

    let server_error = fake(vec![status(500, "boom")]).await;
    let mut lab_b = lab(server_error.port);
    lab_b.chats.send(pane(1), 1, ask(&lab_b, true));
    let events = finish(&mut lab_b.rx).await;
    assert_eq!(server_error.bodies().len(), 1, "a 500 is not a rejection");
    assert!(
        matches!(events.last(), Some(StreamEvent::Error(ChatError::Failed(t))) if t.starts_with("HTTP 500"))
    );
}

// The fallback of Ollama stays a plain 400, with no notice: a 5xx that names
// tools or jinja is an ordinary error there.
#[tokio::test]
async fn ollama_does_not_retry_a_5xx_that_mentions_tools_and_never_adds_a_notice() {
    let server_error = fake(vec![status(500, "tools param requires --jinja flag")]).await;
    let mut lab_a = lab(server_error.port);
    lab_a.chats.send(pane(1), 1, ask(&lab_a, true));
    let events = finish(&mut lab_a.rx).await;
    assert_eq!(server_error.bodies().len(), 1, "no retry");
    assert!(
        matches!(events.last(), Some(StreamEvent::Error(ChatError::Failed(t))) if t.starts_with("HTTP 500"))
    );
    assert!(!events.iter().any(|e| matches!(e, StreamEvent::Notice(_))));

    let rejected = fake(vec![status(400, "no tools"), text("plain")]).await;
    let mut lab_b = lab(rejected.port);
    lab_b.chats.send(pane(1), 1, ask(&lab_b, true));
    let events = finish(&mut lab_b.rx).await;
    assert_eq!(events, [StreamEvent::Delta("plain".into()), done()]);
}

// Approvals --------------------------------------------------------------------

// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_local_read_runs_without_asking() {
    let fake = fake(vec![
        calls("read_file", json!({"path": "~/note.txt"})),
        text("read it"),
    ])
    .await;
    let mut lab = lab(fake.port);
    lab.chats.send(pane(1), 1, ask(&lab, true));
    let events = finish(&mut lab.rx).await;
    assert!(
        matches!(&events[0], StreamEvent::ToolCall { approval: None, .. }),
        "{events:?}"
    );
    let bodies = fake.bodies();
    let tool = bodies[1]["messages"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert!(
        tool["content"]
            .as_str()
            .unwrap()
            .contains("hello from the note")
    );
}

#[tokio::test]
async fn run_in_pane_waits_for_the_user_then_continues_with_the_answer() {
    for (decision, expected) in [
        (Decision::Sent, "typed into the user's pane"),
        (
            Decision::Denied("the user declined"),
            "denied: the user declined",
        ),
    ] {
        let fake = fake(vec![
            calls("run_in_pane", json!({"command": "cd ~/dev/curso-rust"})),
            text("done"),
        ])
        .await;
        let mut lab = lab(fake.port);
        lab.chats.send(pane(1), 4, ask(&lab, true));
        let (turn, id, approval) = next_call(&mut lab.rx).await;
        assert_eq!(
            approval,
            Some(ApprovalAsk::Run {
                command: "cd ~/dev/curso-rust".into()
            })
        );
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(
            fake.bodies().len(),
            1,
            "no follow-up while the user decides"
        );
        assert!(lab.chats.decide(pane(1), turn, id, decision));
        let events = finish(&mut lab.rx).await;
        assert_eq!(events.last(), Some(&done()));
        let bodies = fake.bodies();
        assert_eq!(bodies.len(), 2);
        let tool = bodies[1]["messages"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()
            .clone();
        let content = tool["content"].as_str().unwrap();
        assert!(content.contains(expected), "{content}");
    }
}

// History ----------------------------------------------------------------------

#[tokio::test]
async fn tool_messages_are_not_part_of_the_next_turns_history() {
    let fake = fake(vec![
        calls("find_dir", json!({"query": "curso"})),
        text("Found it"),
        text("again"),
    ])
    .await;
    let mut lab = lab(fake.port);
    let ready = Availability::Ready {
        engine: "ollama".into(),
        model: Some("llama3:8b".into()),
    };
    let mut state = ChatState::new(None);
    let SendOutcome::Sent { turn, mut request } = state.send("find the rust course", &ready, "")
    else {
        panic!("send refused");
    };
    request.tools = Tools::On {
        origin: Some(lab.home.clone()),
        external: Default::default(),
        mcp: None,
    };
    lab.chats.send(pane(1), turn, request);
    for event in finish(&mut lab.rx).await {
        state.apply(turn, event);
    }
    let SendOutcome::Sent { request, .. } = state.send("and now?", &ready, "") else {
        panic!("send refused");
    };
    let texts: Vec<_> = request
        .messages
        .iter()
        .map(|m| (m.role, m.content.as_str()))
        .collect();
    assert_eq!(
        texts,
        [
            (Role::User, "find the rust course"),
            (Role::Assistant, "Found it"),
            (Role::User, "and now?"),
        ]
    );
    // The transcript still shows the tool row.
    assert!(state.entries().iter().any(|e| matches!(e, Entry::Tool(_))));
}

// Limits -----------------------------------------------------------------------

#[tokio::test]
async fn the_sanitizer_cleans_the_text_of_every_round() {
    let fake = fake(vec![
        lines(&[
            json!({"message": {"content": "\u{1b}[31mone ", "tool_calls": [
                {"function": {"name": "find_dir", "arguments": {"query": "curso"}}}
            ]}, "done": false}),
            end_line(),
        ]),
        text("two\u{1b}]0;title\u{7}"),
    ])
    .await;
    let mut lab = lab(fake.port);
    lab.chats.send(pane(1), 1, ask(&lab, true));
    let events = finish(&mut lab.rx).await;
    let answer: String = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Delta(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(answer, "one \ntwo", "rounds are told apart");
}

#[tokio::test]
async fn the_answer_cap_covers_the_whole_turn_and_stops_the_loop() {
    let block = "a".repeat(64 * 1024);
    let mut values: Vec<Value> = (0..(ANSWER_CAP / block.len() + 1))
        .map(|_| json!({"message": {"content": block}, "done": false}))
        .collect();
    values.push(json!({"message": {"content": "", "tool_calls": [
        {"function": {"name": "find_dir", "arguments": {"query": "curso"}}}
    ]}, "done": false}));
    values.push(end_line());
    let fake = fake(vec![lines(&values), text("never")]).await;
    let mut lab = lab(fake.port);
    lab.chats.send(pane(1), 1, ask(&lab, true));
    let events = finish(&mut lab.rx).await;
    assert_eq!(
        events.last(),
        Some(&StreamEvent::Error(ChatError::Truncated))
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(fake.bodies().len(), 1, "no follow-up after the cap");
    assert_eq!(tool_calls(&events), 0);
}

fn direct(
    lab: &Lab,
) -> (
    mpsc::Sender<StreamEvent>,
    mpsc::Receiver<StreamEvent>,
    Arc<dyn crate::runtime::mcp::Dispatch>,
) {
    let (tx, rx) = mpsc::channel(TURN_CHANNEL);
    let dispatch = lab.host.dispatch(TurnScope::new(
        tx.clone(),
        Locality::Local,
        lab.home.clone(),
    ));
    (tx, rx, dispatch)
}

#[tokio::test]
async fn a_follow_up_that_never_answers_ends_under_the_first_byte_limit() {
    let fake = fake(vec![
        calls("find_dir", json!({"query": "curso"})),
        Step::Hang,
    ])
    .await;
    let lab = lab(fake.port);
    let (tx, mut rx, dispatch) = direct(&lab);
    let timeouts = Timeouts {
        first_byte: Duration::from_millis(300),
        ..Timeouts::SERVER
    };
    ollama::tool_turn(&ask(&lab, true), fake.port, dispatch, &timeouts, &tx).await;
    drop(tx);
    let mut last = None;
    while let Some(event) = rx.recv().await {
        last = Some(event);
    }
    assert_eq!(
        last,
        Some(StreamEvent::Error(ChatError::Failed(NO_ANSWER.into())))
    );
}

#[tokio::test]
async fn the_total_limit_covers_all_rounds() {
    // Each round answers in 150 ms, under the total of 500 ms: only the sum
    // runs it out.
    let Step::Reply(call) = calls("find_dir", json!({"query": "curso"})) else {
        unreachable!()
    };
    let fake = fake(vec![Step::Delay(150, call)]).await;
    let lab = lab(fake.port);
    let (tx, mut rx, dispatch) = direct(&lab);
    let timeouts = Timeouts {
        total: Duration::from_millis(500),
        ..Timeouts::SERVER
    };
    ollama::tool_turn(&ask(&lab, true), fake.port, dispatch, &timeouts, &tx).await;
    drop(tx);
    let mut last = None;
    while let Some(event) = rx.recv().await {
        last = Some(event);
    }
    assert_eq!(
        last,
        Some(StreamEvent::Error(ChatError::Failed(TOO_LONG.into())))
    );
    assert!(fake.bodies().len() >= 2, "{} rounds", fake.bodies().len());
}

// Cancel while the user decides: the call is denied and nothing else is sent.
#[tokio::test]
async fn cancel_while_an_approval_waits_denies_it_and_sends_nothing() {
    let fake = fake(vec![
        calls("run_in_pane", json!({"command": "ls"})),
        text("no"),
    ])
    .await;
    let mut lab = lab(fake.port);
    lab.chats.send(pane(1), 3, ask(&lab, true));
    let (turn, id, approval) = next_call(&mut lab.rx).await;
    assert!(approval.is_some());
    lab.chats.cancel(pane(1));
    assert!(
        !lab.chats.decide(pane(1), turn, id, Decision::Sent),
        "the wait is gone"
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(fake.bodies().len(), 1);
}

// Buffers ---------------------------------------------------------------------

#[tokio::test]
async fn the_echoed_reasoning_and_text_are_capped() {
    let block = "a".repeat(64 * 1024);
    let mut values: Vec<Value> = Vec::new();
    for _ in 0..(REASONING_CAP / block.len() + 2) {
        values.push(json!({"message": {"content": "", "thinking": block}, "done": false}));
    }
    for _ in 0..(ANSWER_CAP / block.len() + 2) {
        values.push(json!({"message": {"content": block}, "done": false}));
    }
    values.push(json!({"message": {"content": "", "tool_calls": [
        {"function": {"name": "find_dir", "arguments": {"query": "curso"}}}
    ]}, "done": false}));
    values.push(end_line());
    let fake = fake(vec![lines(&values), text("end")]).await;
    let lab = lab(fake.port);
    let (tx, mut rx, dispatch) = direct(&lab);
    // Drain like the turn task does, so the relay never blocks.
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    ollama::tool_turn(
        &ask(&lab, true),
        fake.port,
        dispatch,
        &Timeouts::SERVER,
        &tx,
    )
    .await;
    let bodies = fake.bodies();
    assert_eq!(bodies.len(), 2);
    let assistant = &bodies[1]["messages"][2];
    assert_eq!(assistant["role"], "assistant");
    assert_eq!(assistant["thinking"].as_str().unwrap().len(), REASONING_CAP);
    assert_eq!(assistant["content"].as_str().unwrap().len(), ANSWER_CAP);
}

// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn tool_results_stop_being_echoed_past_the_cumulative_cap() {
    // A read result is cut at 32 KiB: two reads per reply reach the cap sooner.
    let read = ("read_file", json!({"path": "~/big.txt"}));
    let fake = fake(vec![calls_many(&[read.clone(), read])]).await;
    let mut lab = lab(fake.port);
    std::fs::write(lab.home.join("big.txt"), "x".repeat(60_000)).unwrap();
    lab.chats.send(pane(1), 1, ask(&lab, true));
    let events = finish(&mut lab.rx).await;
    assert_eq!(events.last(), Some(&done()));
    let bodies = fake.bodies();
    let last = bodies.last().unwrap()["messages"]
        .as_array()
        .unwrap()
        .clone();
    let results: Vec<&str> = last
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| m["content"].as_str().unwrap())
        .collect();
    assert!(results.len() >= 8, "{} results", results.len());
    let kept: usize = results
        .iter()
        .filter(|r| **r != ollama::DROPPED)
        .map(|r| r.len())
        .sum();
    assert!(kept <= ollama::RESULTS_CAP, "{kept} bytes kept");
    assert_eq!(results.last(), Some(&ollama::DROPPED));
    assert!(
        results.first().unwrap().contains("xxxx"),
        "early results are kept"
    );
}

// A dispatch that panics answers that one call with an error, once.
struct Panics;

impl crate::runtime::mcp::Dispatch for Panics {
    fn call(
        &self,
        _call: crate::core::ai::tools::Call,
    ) -> futures::future::BoxFuture<'static, crate::runtime::mcp::Outcome> {
        panic!("boom")
    }
}

#[tokio::test]
async fn a_panicking_call_gets_an_error_result_and_the_loop_goes_on() {
    let fake = fake(vec![
        calls("find_dir", json!({"query": "curso"})),
        text("after"),
    ])
    .await;
    let lab = lab(fake.port);
    let (tx, mut rx) = mpsc::channel(TURN_CHANNEL);
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    ollama::tool_turn(
        &ask(&lab, true),
        fake.port,
        Arc::new(Panics),
        &Timeouts::SERVER,
        &tx,
    )
    .await;
    std::panic::set_hook(hook);
    drop(tx);
    let mut last = None;
    while let Some(event) = rx.recv().await {
        last = Some(event);
    }
    assert_eq!(last, Some(done()));
    let bodies = fake.bodies();
    let tool = bodies[1]["messages"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(tool["content"], ollama::FAILED);
}
