//! A turn that offers tools gets a session; the calls that wait for the user
//! are answered by (pane, turn, call). Real unix sockets and the real clock,
//! every wait bounded; nothing sleeps.

// What only the Linux-only tests use is dead code elsewhere.
#![cfg_attr(not(target_os = "linux"), allow(dead_code, unused_imports))]

use std::{path::PathBuf, sync::Mutex, time::Duration};

use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    sync::{Notify, oneshot},
    time::timeout,
};

use super::{
    tests::{done, request, scripted},
    *,
};
use crate::{
    core::ai::chat::{
        approval::ApprovalAsk,
        engine::{CallId, Decision, Finish, ToolStatus, Tools},
    },
    runtime::tools::{Limits, ToolHost, pending},
};

fn pane(n: u32) -> PaneId {
    PaneId::for_test(n)
}

struct Lab {
    dir: tempfile::TempDir,
    home: PathBuf,
    host: Arc<ToolHost>,
}

fn lab() -> Lab {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap().join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("note.txt"), "hello from the note").unwrap();
    let host = ToolHost::new(Some(home.clone().into()), None, None, Limits::default());
    Lab { dir, home, host }
}

/// A runner that never ends the turn by itself.
fn idle() -> Runner {
    Arc::new(|_request, _tx| Box::pin(std::future::pending()))
}

fn tooled(lab: &Lab, runner: Runner) -> (Chats, mpsc::Receiver<Tagged>) {
    let (tx, rx) = mpsc::channel(APP_CHANNEL);
    let dirs = vec![lab.dir.path().join("run")];
    (
        Chats::new(runner, tx).with_tools(lab.host.clone(), dirs),
        rx,
    )
}

/// A cloud engine with tools on, so a read asks.
fn with_tools(lab: &Lab) -> ChatRequest {
    let mut req = request("");
    req.engine = "claude".into();
    req.tools = Tools::On {
        origin: Some(lab.home.clone()),
        mcp: None,
    };
    req
}

struct Client {
    reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: tokio::net::unix::OwnedWriteHalf,
}

impl Client {
    /// Connects to the session of `pane` and initializes MCP.
    async fn open(chats: &Chats, pane: PaneId) -> Client {
        let started = chats.session(pane).expect("a session");
        let (reader, writer) = UnixStream::connect(&started.socket)
            .await
            .unwrap()
            .into_split();
        let mut client = Client {
            reader: BufReader::new(reader),
            writer,
        };
        let hello = json!({"lazarobox": 1, "token": started.token.reveal()});
        client.send(&hello).await;
        assert_eq!(client.reply().await, Some(json!({"ok": true})));
        let init = json!({"jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": {"protocolVersion": "2025-06-18"}});
        client.send(&init).await;
        assert!(client.reply().await.unwrap()["result"].is_object());
        client
    }

    async fn send(&mut self, value: &Value) {
        self.writer
            .write_all(format!("{value}\n").as_bytes())
            .await
            .unwrap();
    }

    async fn call(&mut self, id: u64, name: &str, arguments: Value) {
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": {"name": name, "arguments": arguments}}))
            .await;
    }

    /// The next line, or `None` at the end of the connection.
    async fn reply(&mut self) -> Option<Value> {
        let mut text = String::new();
        let read = timeout(Duration::from_secs(5), self.reader.read_line(&mut text))
            .await
            .expect("no reply");
        match read {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(serde_json::from_str(&text).unwrap()),
        }
    }
}

async fn next(rx: &mut mpsc::Receiver<Tagged>) -> Tagged {
    timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("an event within 5 s")
        .expect("the channel is open")
}

/// The next announced call: (pane, turn, id, what it asks).
async fn next_call(rx: &mut mpsc::Receiver<Tagged>) -> (PaneId, u64, CallId, Option<ApprovalAsk>) {
    loop {
        if let (pane, turn, StreamEvent::ToolCall { id, approval, .. }) = next(rx).await {
            return (pane, turn, id, approval.map(|ask| *ask));
        }
    }
}

async fn next_result(rx: &mut mpsc::Receiver<Tagged>) -> (PaneId, u64, CallId, ToolStatus) {
    loop {
        if let (pane, turn, StreamEvent::ToolResult { id, status, .. }) = next(rx).await {
            return (pane, turn, id, status);
        }
    }
}

fn note(lab: &Lab) -> Option<ApprovalAsk> {
    Some(ApprovalAsk::Read {
        path: lab.home.join("note.txt").to_str().unwrap().into(),
    })
}

// A cloud read: the app hears the ask under its pane and turn, the answer
// reaches the call, and only then the file is read.
// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_call_waits_for_the_answer_of_its_pane_and_turn() {
    let lab = lab();
    let (mut chats, mut rx) = tooled(&lab, idle());
    chats.send(pane(1), 7, with_tools(&lab));
    let mut client = Client::open(&chats, pane(1)).await;
    client
        .call(5, "read_file", json!({"path": "~/note.txt"}))
        .await;
    let (p, t, id, ask) = next_call(&mut rx).await;
    assert_eq!((p, t, ask), (pane(1), 7, note(&lab)));
    // Answers for another pane, turn or call find nothing and change nothing.
    assert!(!chats.decide(pane(2), 7, id, Decision::Sent));
    assert!(!chats.decide(pane(1), 6, id, Decision::Sent));
    assert!(!chats.decide(pane(1), 7, CallId(id.0 + 1), Decision::Sent));
    assert!(chats.decide(pane(1), 7, id, Decision::Sent));
    // A second answer for the same call is stale.
    assert!(!chats.decide(pane(1), 7, id, Decision::Denied("late")));
    let reply = client.reply().await.unwrap();
    assert_eq!(reply["id"], 5);
    assert_eq!(reply["result"]["isError"], false);
    assert!(
        reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("hello from the note")
    );
    assert_eq!(next_result(&mut rx).await, (pane(1), 7, id, ToolStatus::Ok));
}

#[tokio::test]
async fn a_denied_call_never_opens_the_file() {
    let lab = lab();
    let (mut chats, mut rx) = tooled(&lab, idle());
    chats.send(pane(1), 1, with_tools(&lab));
    let mut client = Client::open(&chats, pane(1)).await;
    client
        .call(5, "read_file", json!({"path": "~/note.txt"}))
        .await;
    let (_, _, id, _) = next_call(&mut rx).await;
    assert!(chats.decide(pane(1), 1, id, Decision::Denied("the user declined")));
    let reply = client.reply().await.unwrap();
    assert_eq!(reply["result"]["isError"], true);
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("declined") && !text.contains("hello"),
        "{text}"
    );
    assert_eq!(
        next_result(&mut rx).await,
        (pane(1), 1, id, ToolStatus::Denied)
    );
}

// Two chats: closing one ends only its calls.
// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn closing_one_chat_ends_only_its_own_calls() {
    let lab = lab();
    let (mut chats, mut rx) = tooled(&lab, idle());
    chats.send(pane(1), 1, with_tools(&lab));
    chats.send(pane(2), 1, with_tools(&lab));
    let mut one = Client::open(&chats, pane(1)).await;
    let mut two = Client::open(&chats, pane(2)).await;
    one.call(1, "read_file", json!({"path": "~/note.txt"}))
        .await;
    let (_, _, first, _) = next_call(&mut rx).await;
    two.call(1, "read_file", json!({"path": "~/note.txt"}))
        .await;
    let (_, _, second, _) = next_call(&mut rx).await;

    chats.end_session(pane(1));
    assert_eq!(
        next_result(&mut rx).await,
        (pane(1), 1, first, ToolStatus::Denied)
    );
    assert!(chats.session(pane(1)).is_none());
    assert!(!chats.decide(pane(1), 1, first, Decision::Sent));
    // Its client is told or sees the connection end; it is never served.
    assert!(
        one.reply()
            .await
            .is_none_or(|r| r["result"]["isError"] == true)
    );

    // The other chat still waits, and its answer still works.
    assert!(chats.session(pane(2)).is_some());
    assert!(chats.decide(pane(2), 1, second, Decision::Sent));
    assert_eq!(
        two.reply().await.unwrap()["result"]["isError"],
        false,
        "the other chat's call is served"
    );
    assert_eq!(
        next_result(&mut rx).await,
        (pane(2), 1, second, ToolStatus::Ok)
    );
}

// The turn ends (here by finishing; a timeout or a cancel end it the same
// way) while the user has not answered: the call is denied, never approved.
#[tokio::test]
async fn a_turn_that_ends_denies_the_calls_still_waiting() {
    let lab = lab();
    let gate = Arc::new(Notify::new());
    let runner: Runner = {
        let gate = gate.clone();
        Arc::new(move |_request, tx| {
            let gate = gate.clone();
            Box::pin(async move {
                gate.notified().await;
                let _ = tx.send(StreamEvent::Done(Finish::Stop)).await;
            })
        })
    };
    let (mut chats, mut rx) = tooled(&lab, runner);
    chats.send(pane(1), 1, with_tools(&lab));
    let mut client = Client::open(&chats, pane(1)).await;
    client
        .call(5, "read_file", json!({"path": "~/note.txt"}))
        .await;
    let (_, _, id, _) = next_call(&mut rx).await;
    gate.notify_one();
    let reply = client.reply().await.unwrap();
    assert_eq!(reply["result"]["isError"], true);
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    assert!(!text.contains("hello"), "{text}");
    assert!(!chats.decide(pane(1), 1, id, Decision::Sent));
}

#[tokio::test]
async fn cancelling_the_turn_denies_its_waiting_calls() {
    let lab = lab();
    let (mut chats, mut rx) = tooled(&lab, idle());
    chats.send(pane(1), 3, with_tools(&lab));
    let mut client = Client::open(&chats, pane(1)).await;
    client
        .call(5, "read_file", json!({"path": "~/note.txt"}))
        .await;
    let (_, _, id, _) = next_call(&mut rx).await;
    chats.cancel(pane(1));
    assert!(!chats.decide(pane(1), 3, id, Decision::Sent));
    let reply = client.reply().await.unwrap();
    assert_eq!(reply["result"]["isError"], true);
}

#[tokio::test]
async fn a_newer_turn_replaces_the_session_and_ends_the_old_calls() {
    let lab = lab();
    let (mut chats, mut rx) = tooled(&lab, idle());
    chats.send(pane(1), 1, with_tools(&lab));
    let old_token = chats.session(pane(1)).unwrap().token.reveal().to_owned();
    let mut client = Client::open(&chats, pane(1)).await;
    client
        .call(5, "read_file", json!({"path": "~/note.txt"}))
        .await;
    let (_, _, id, _) = next_call(&mut rx).await;

    chats.send(pane(1), 2, with_tools(&lab));
    assert_ne!(chats.session(pane(1)).unwrap().token.reveal(), old_token);
    assert!(!chats.decide(pane(1), 1, id, Decision::Sent));
    let ended = client.reply().await;
    assert!(ended.is_none_or(|r| r["result"]["isError"] == true));
}

// An adapter that finds its engine cannot be wired (no jail, a layout the jail
// refuses) says so through the wire it was given, and the session goes then,
// not at the next send.
#[tokio::test]
async fn a_turn_that_falls_back_to_text_closes_its_session() {
    let lab = lab();
    let runner: Runner = Arc::new(|request, tx| {
        Box::pin(async move {
            let Tools::On {
                mcp: Some(wire), ..
            } = &request.tools
            else {
                panic!("the turn must carry the session: {:?}", request.tools);
            };
            wire.unwire.call();
            let _ = tx.send(StreamEvent::Done(Finish::Stop)).await;
        })
    });
    let (mut chats, mut rx) = tooled(&lab, runner);
    chats.send(pane(1), 1, with_tools(&lab));
    let socket = chats.session(pane(1)).unwrap().socket.clone();
    loop {
        if matches!(next(&mut rx).await.2, StreamEvent::Done(_)) {
            break;
        }
    }
    assert!(!socket.exists(), "the socket goes with the fallback");
    assert!(UnixStream::connect(&socket).await.is_err());
}

#[tokio::test]
async fn a_turn_without_tools_has_no_session_and_closes_the_old_one() {
    let lab = lab();
    let (mut chats, _rx) = tooled(&lab, idle());
    chats.send(pane(1), 1, request(""));
    assert!(chats.session(pane(1)).is_none());
    chats.send(pane(1), 2, with_tools(&lab));
    let socket = chats.session(pane(1)).unwrap().socket.clone();
    assert!(socket.exists());
    chats.send(pane(1), 3, request(""));
    assert!(chats.session(pane(1)).is_none());
    assert!(!socket.exists(), "the socket of the old session is gone");
}

#[tokio::test]
async fn tools_without_tooling_or_for_an_unknown_engine_run_as_text() {
    let lab = lab();
    let (tx, mut rx) = mpsc::channel(APP_CHANNEL);
    let mut bare = Chats::new(scripted(vec![done()]), tx);
    bare.send(pane(1), 1, with_tools(&lab));
    assert!(bare.session(pane(1)).is_none());
    assert_eq!(next(&mut rx).await.2, done());

    let (mut chats, _rx) = tooled(&lab, idle());
    let mut unknown = with_tools(&lab);
    unknown.engine = "no-such-engine".into();
    chats.send(pane(1), 1, unknown);
    assert!(chats.session(pane(1)).is_none());
}

// The adapter's timers read the turn's pending count: it is set only for a
// turn with tools, and a call that waits for the user is in it.
#[tokio::test]
async fn a_waiting_call_is_counted_for_the_timers_of_its_turn() {
    let lab = lab();
    let (seen_tx, seen_rx) = oneshot::channel();
    let seen = Mutex::new(Some(seen_tx));
    let runner: Runner = Arc::new(move |_request, _tx| {
        let current = pending::current();
        if let Some(tx) = seen.lock().unwrap().take() {
            let _ = tx.send(current);
        }
        Box::pin(std::future::pending())
    });
    let (mut chats, mut rx) = tooled(&lab, runner);
    chats.send(pane(1), 1, with_tools(&lab));
    let count = timeout(Duration::from_secs(5), seen_rx)
        .await
        .unwrap()
        .unwrap()
        .expect("a turn with tools has a pending count");
    assert_eq!(count.count(), 0);
    let mut client = Client::open(&chats, pane(1)).await;
    client
        .call(5, "read_file", json!({"path": "~/note.txt"}))
        .await;
    let (_, _, id, _) = next_call(&mut rx).await;
    assert_eq!(count.count(), 1);
    assert!(chats.decide(pane(1), 1, id, Decision::Sent));
    client.reply().await.unwrap();
    assert_eq!(count.count(), 0);

    // A turn without tools has none.
    let (none_tx, none_rx) = oneshot::channel();
    let none = Mutex::new(Some(none_tx));
    let (mut plain, _rx) = tooled(
        &lab,
        Arc::new(move |_request, _tx| {
            if let Some(tx) = none.lock().unwrap().take() {
                let _ = tx.send(pending::current().is_some());
            }
            Box::pin(std::future::pending())
        }),
    );
    plain.send(pane(1), 1, request(""));
    assert!(
        !timeout(Duration::from_secs(5), none_rx)
            .await
            .unwrap()
            .unwrap()
    );
}

/// A runner that hands the request it got to the test, then ends the turn.
fn capturing(seen: Arc<Mutex<Vec<ChatRequest>>>) -> Runner {
    Arc::new(move |request, tx| {
        let seen = seen.clone();
        Box::pin(async move {
            seen.lock().unwrap().push(request);
            let _ = tx.send(done()).await;
        })
    })
}

#[tokio::test]
async fn the_runner_gets_the_session_of_the_turn_in_the_request() {
    let lab = lab();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (mut chats, mut rx) = tooled(&lab, capturing(seen.clone()));
    chats.send(pane(1), 1, with_tools(&lab));
    next(&mut rx).await;
    let started = chats.session(pane(1)).expect("a session");
    let seen = seen.lock().unwrap();
    let Tools::On {
        mcp: Some(wire), ..
    } = &seen[0].tools
    else {
        panic!("a wire expected: {:?}", seen[0].tools);
    };
    assert_eq!(wire.socket, started.socket);
    assert_eq!(wire.token.reveal(), started.token.reveal());
}

#[tokio::test]
async fn a_turn_without_tools_gets_no_wire_and_no_session() {
    let lab = lab();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (mut chats, mut rx) = tooled(&lab, capturing(seen.clone()));
    chats.send(pane(1), 1, request(""));
    next(&mut rx).await;
    assert!(chats.session(pane(1)).is_none());
    assert_eq!(seen.lock().unwrap()[0].tools, Tools::Off);
}

fn chat_prompt() -> String {
    use crate::core::ai::{
        chat::prompts::builtin,
        config::{Profile, Task},
    };
    builtin(Profile::Programmer, Task::Chat)
        .trim_end()
        .to_owned()
}

/// What the runner got for a turn sent to `chats` with `req`.
async fn sent(
    chats: &mut Chats,
    rx: &mut mpsc::Receiver<Tagged>,
    seen: &Arc<Mutex<Vec<ChatRequest>>>,
    req: ChatRequest,
) -> ChatRequest {
    let before = seen.lock().unwrap().len();
    chats.send(pane(1), 1, req);
    next(rx).await;
    seen.lock().unwrap()[before].clone()
}

// The tools prompt is not decided here. A session only means the engine MAY be
// wired: `plan` can still fall back to text (no jail, no shim, a refused
// layout), so the adapter hands the runtime both texts and `plan` picks the
// wired one (see the `real_turn` tests in `cli/tests.rs`). Whatever happens
// here, the system prompt of the request is the one the app built.
#[tokio::test]
async fn the_chats_never_rewrite_the_system_prompt() {
    let lab = lab();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (mut chats, mut rx) = tooled(&lab, capturing(seen.clone()));
    let mut req = with_tools(&lab);
    req.system = chat_prompt();
    let got = sent(&mut chats, &mut rx, &seen, req.clone()).await;
    assert!(
        matches!(got.tools, Tools::On { mcp: Some(_), .. }),
        "a session"
    );
    assert_eq!(got.system, chat_prompt());

    // Tools off.
    let mut off = request("");
    off.system = chat_prompt();
    let got = sent(&mut chats, &mut rx, &seen, off).await;
    assert_eq!(got.tools, Tools::Off);
    assert_eq!(got.system, chat_prompt());

    // Tools on for a server engine: its adapter builds an HTTP request and never
    // reads the wire.
    let mut server = req.clone();
    server.engine = "ollama".into();
    let got = sent(&mut chats, &mut rx, &seen, server).await;
    assert_eq!(got.system, chat_prompt());

    // No tooling at all.
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (tx, mut rx) = mpsc::channel(APP_CHANNEL);
    let mut bare = Chats::new(capturing(seen.clone()), tx);
    let got = sent(&mut bare, &mut rx, &seen, req.clone()).await;
    assert!(matches!(got.tools, Tools::On { mcp: None, .. }));
    assert_eq!(got.system, chat_prompt());
}

// The hub cannot open (its folder cannot be made): no session, no wire, and the
// prompt untouched.
#[tokio::test]
async fn a_hub_that_cannot_open_leaves_the_turn_text_with_its_prompt() {
    let lab = lab();
    let blocker = lab.dir.path().join("file");
    std::fs::write(&blocker, "").unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (tx, mut rx) = mpsc::channel(APP_CHANNEL);
    let mut chats = Chats::new(capturing(seen.clone()), tx)
        .with_tools(lab.host.clone(), vec![blocker.join("run")]);
    let mut req = with_tools(&lab);
    req.system = chat_prompt();
    let got = sent(&mut chats, &mut rx, &seen, req).await;
    assert!(chats.session(pane(1)).is_none());
    assert!(
        matches!(got.tools, Tools::On { mcp: None, .. }),
        "{:?}",
        got.tools
    );
    assert_eq!(got.system, chat_prompt());
}

// A pane that moves from a CLI engine to Ollama must not keep the socket of
// the CLI session: Ollama opens none of its own.
#[tokio::test]
async fn a_turn_on_ollama_closes_the_cli_session_the_pane_had() {
    let lab = lab();
    let (mut chats, _rx) = tooled(&lab, idle());
    chats.send(pane(1), 1, with_tools(&lab));
    let socket = chats.session(pane(1)).expect("a session").socket.clone();
    assert!(socket.exists());
    let mut req = with_tools(&lab);
    req.engine = "ollama".into();
    chats.send(pane(1), 2, req);
    assert!(chats.session(pane(1)).is_none());
    assert!(!socket.exists(), "the old socket is still there");
}
