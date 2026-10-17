//! A turn that offers tools gets a session; the calls that wait for the user
//! are answered by (pane, turn, call). Real unix sockets and the real clock,
//! every wait bounded; nothing sleeps.

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
