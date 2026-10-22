use std::{
    os::unix::fs::PermissionsExt, path::Path, sync::Mutex, sync::atomic::Ordering, time::Duration,
};

use futures::{FutureExt, future::BoxFuture};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{
        UnixStream,
        unix::{OwnedReadHalf, OwnedWriteHalf},
    },
    sync::mpsc,
    time::timeout,
};

use super::*;
use crate::{
    core::ai::orchestra::{RunId, Toolset, WorkerId, WorkerKey, tools::OrchCall},
    core::ai::tools::{
        Call,
        mcp::MAX_LINE_BYTES,
        registry,
        registry::{Family, FamilySet},
        registry_with,
    },
    runtime::mcp::{
        Outcome,
        external::{
            ExternalSet, Slot,
            fake::{host_env, lab, timeouts},
        },
    },
};

/// Answers every call with "done" and remembers it.
#[derive(Default)]
struct Fake(Mutex<Vec<Call>>);

impl Dispatch for Fake {
    fn call(&self, call: Call) -> BoxFuture<'static, Outcome> {
        self.0.lock().unwrap().push(call);
        async {
            Outcome {
                text: "done".into(),
                is_error: false,
            }
        }
        .boxed()
    }
}

/// Never answers; says when a call starts and when it is dropped.
struct Hang {
    called: mpsc::UnboundedSender<()>,
    dropped: mpsc::UnboundedSender<()>,
}

/// The test's end of a [`Hang`].
struct Probe {
    called: mpsc::UnboundedReceiver<()>,
    dropped: mpsc::UnboundedReceiver<()>,
}

fn hang() -> (Arc<Hang>, Probe) {
    let (called, called_rx) = mpsc::unbounded_channel();
    let (dropped, dropped_rx) = mpsc::unbounded_channel();
    let probe = Probe {
        called: called_rx,
        dropped: dropped_rx,
    };
    (Arc::new(Hang { called, dropped }), probe)
}

impl Probe {
    async fn called(&mut self) {
        timeout(Duration::from_secs(5), self.called.recv())
            .await
            .expect("no call arrived")
            .unwrap();
    }

    async fn dropped(&mut self) {
        timeout(Duration::from_secs(5), self.dropped.recv())
            .await
            .expect("the call was not dropped")
            .unwrap();
    }
}

struct SignalOnDrop(mpsc::UnboundedSender<()>);

impl Drop for SignalOnDrop {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

impl Dispatch for Hang {
    fn call(&self, _: Call) -> BoxFuture<'static, Outcome> {
        let _ = self.called.send(());
        let guard = SignalOnDrop(self.dropped.clone());
        async move {
            let _guard = guard;
            std::future::pending::<Outcome>().await
        }
        .boxed()
    }
}

/// Answers `late` once the test says so; says when a call starts and when it
/// is dropped.
struct Gated {
    hang: Hang,
    release: Arc<tokio::sync::Notify>,
}

fn gated() -> (Arc<Gated>, Probe, Arc<tokio::sync::Notify>) {
    let (hang, probe) = hang();
    let release = Arc::new(tokio::sync::Notify::new());
    let hang = Arc::into_inner(hang).unwrap();
    (
        Arc::new(Gated {
            hang,
            release: release.clone(),
        }),
        probe,
        release,
    )
}

impl Dispatch for Gated {
    fn call(&self, _: Call) -> BoxFuture<'static, Outcome> {
        let _ = self.hang.called.send(());
        let guard = SignalOnDrop(self.hang.dropped.clone());
        let release = self.release.clone();
        async move {
            let _guard = guard;
            release.notified().await;
            Outcome {
                text: "late".into(),
                is_error: false,
            }
        }
        .boxed()
    }
}

/// Panics before it returns a future, or inside it.
struct Panics {
    in_future: bool,
}

impl Dispatch for Panics {
    fn call(&self, _: Call) -> BoxFuture<'static, Outcome> {
        if !self.in_future {
            panic!("boom");
        }
        async { panic!("boom") }.boxed()
    }
}

fn hub(dir: &Path) -> Hub {
    Hub::new(&[dir.join("run")]).unwrap()
}

fn hub_in(dir: &Path) -> Hub {
    hub(dir)
}

fn pane(n: u32) -> PaneId {
    PaneId::for_test(n)
}

fn tools() -> Vec<ToolSpec> {
    registry(&[])
}

type Client = (BufReader<OwnedReadHalf>, OwnedWriteHalf);

async fn connect(socket: &Path) -> Client {
    let (r, w) = UnixStream::connect(socket).await.unwrap().into_split();
    (BufReader::new(r), w)
}

async fn send(client: &mut Client, line: &str) {
    // A closed peer is not an error for the sender: the test reads the close.
    let _ = client.1.write_all(format!("{line}\n").as_bytes()).await;
}

async fn send_raw(client: &mut Client, bytes: &[u8]) {
    let _ = client.1.write_all(bytes).await;
}

/// The next line as JSON, or `None` when the server closed the connection.
async fn recv(client: &mut Client) -> Option<Value> {
    let mut line = String::new();
    let read = tokio::time::timeout(Duration::from_secs(5), client.0.read_line(&mut line))
        .await
        .expect("the server neither replied nor closed");
    match read {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(serde_json::from_str(&line).unwrap()),
    }
}

fn hello(token: &str) -> String {
    json!({"lazarobox": 1, "token": token}).to_string()
}

async fn ready(started: &Started) -> Client {
    let mut c = connect(&started.socket).await;
    send(&mut c, &hello(started.token.reveal())).await;
    assert_eq!(recv(&mut c).await, Some(json!({"ok": true})));
    send(
        &mut c,
        r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
    )
    .await;
    assert!(recv(&mut c).await.unwrap()["result"].is_object());
    c
}

fn open(hub: &mut Hub, n: u32, dispatch: Arc<dyn Dispatch>) -> Started {
    hub.open(pane(n), dispatch, Slot::default(), FamilySet::LEGACY)
        .unwrap()
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[tokio::test]
async fn the_dir_is_0700_and_the_socket_0600_named_by_pid_and_pane() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    let s = open(&mut hub, 7, Arc::new(Fake::default()));
    assert_eq!(mode(&tmp.path().join("run")), 0o700);
    assert_eq!(mode(&s.socket), 0o600);
    assert_eq!(
        s.socket,
        tmp.path()
            .join("run")
            .join(format!("s-{}-7.sock", std::process::id()))
    );
}

#[tokio::test]
async fn the_first_candidate_that_cannot_be_private_is_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    let blocker = tmp.path().join("file");
    std::fs::write(&blocker, "x").unwrap();
    let hub = Hub::<PaneId>::new(&[blocker.join("lazarobox"), tmp.path().join("run")]).unwrap();
    assert_eq!(hub.dir, tmp.path().join("run"));
    assert!(Hub::<PaneId>::new(&[blocker.join("x")]).is_err());
    assert!(Hub::<PaneId>::new(&[]).is_err());
}

#[tokio::test]
async fn a_wrong_token_a_missing_one_and_a_bad_hello_are_closed_unserved() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    let fake = Arc::new(Fake::default());
    let s = open(&mut hub, 1, fake.clone());
    let line = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
    let wrong = "0".repeat(32);
    let bad = [
        hello(&wrong),
        hello(""),
        json!({"lazarobox": 1}).to_string(),
        json!({"lazarobox": 2, "token": s.token.reveal()}).to_string(),
        json!({"token": s.token.reveal()}).to_string(),
        "not json".to_owned(),
        line.to_owned(),
    ];
    for first in bad {
        let mut c = connect(&s.socket).await;
        send(&mut c, &first).await;
        send(&mut c, line).await;
        assert_eq!(recv(&mut c).await, None, "{first}");
    }
    assert!(fake.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_token_does_not_open_another_chats_socket() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    let a = open(&mut hub, 1, Arc::new(Fake::default()));
    let b = open(&mut hub, 2, Arc::new(Fake::default()));
    let mut c = connect(&b.socket).await;
    send(&mut c, &hello(a.token.reveal())).await;
    assert_eq!(recv(&mut c).await, None);
    let mut c = connect(&a.socket).await;
    send(&mut c, &hello(a.token.reveal())).await;
    assert_eq!(recv(&mut c).await, Some(json!({"ok": true})));
}

#[tokio::test]
async fn a_call_reaches_the_dispatch_of_its_chat_and_the_result_comes_back() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    let fake = Arc::new(Fake::default());
    let other = Arc::new(Fake::default());
    let s = open(&mut hub, 1, fake.clone());
    open(&mut hub, 2, other.clone());
    let mut c = ready(&s).await;
    send(
        &mut c,
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"find_dir","arguments":{"query":"rust"}}}"#,
    )
    .await;
    let got = recv(&mut c).await.unwrap();
    assert_eq!(got["id"], 5);
    assert_eq!(got["result"]["content"][0]["text"], "done");
    assert_eq!(got["result"]["isError"], false);
    assert_eq!(
        *fake.0.lock().unwrap(),
        [Call::FindDir {
            query: "rust".into()
        }]
    );
    assert!(other.0.lock().unwrap().is_empty());
}

// P0b.3: the set a session opens with is the one that gates its parse, so a
// client that names a tool it was not offered gets the unknown-tool text and
// the dispatch never sees a call.
#[tokio::test]
async fn a_tool_outside_the_sessions_families_is_refused_before_the_dispatch() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    let fake = Arc::new(Fake::default());
    let read_only = FamilySet::EMPTY.with(Family::Read);
    let s = hub
        .open(pane(1), fake.clone(), Slot::default(), read_only)
        .unwrap();
    let mut c = ready(&s).await;
    assert_eq!(listed(&mut c).await, ["find_dir", "list_dir", "read_file"]);
    send(
        &mut c,
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"run_in_pane","arguments":{"command":"ls"}}}"#,
    )
    .await;
    let got = recv(&mut c).await.unwrap();
    assert_eq!(got["result"]["content"][0]["text"], "unknown tool");
    assert_eq!(got["result"]["isError"], true);
    // An offered tool still reaches the dispatch.
    send(
        &mut c,
        r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"find_dir","arguments":{"query":"x"}}}"#,
    )
    .await;
    assert_eq!(recv(&mut c).await.unwrap()["result"]["isError"], false);
    assert_eq!(
        *fake.0.lock().unwrap(),
        [Call::FindDir { query: "x".into() }],
        "run_in_pane never reached the dispatch"
    );
}

// The session derives its offered list from its set, so the two cannot
// disagree: an empty set offers nothing and accepts nothing.
#[tokio::test]
async fn the_session_lists_exactly_the_built_ins_of_its_set() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    let fake = Arc::new(Fake::default());
    let s = hub
        .open(pane(1), fake.clone(), Slot::default(), FamilySet::EMPTY)
        .unwrap();
    let mut c = ready(&s).await;
    assert!(listed(&mut c).await.is_empty());
    send(
        &mut c,
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"find_dir","arguments":{"query":"x"}}}"#,
    )
    .await;
    let got = recv(&mut c).await.unwrap();
    assert_eq!(got["result"]["content"][0]["text"], "unknown tool");
    assert!(fake.0.lock().unwrap().is_empty());
    // The LEGACY session lists what `registry_with` says, in its order.
    let legacy = open(&mut hub, 2, fake);
    let mut c = ready(&legacy).await;
    let want: Vec<String> = registry_with(&[], FamilySet::LEGACY)
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(listed(&mut c).await, want);
}

#[tokio::test]
async fn an_oversize_line_gets_the_error_and_the_connection_keeps_serving() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    let s = open(&mut hub, 1, Arc::new(Fake::default()));
    let mut c = ready(&s).await;
    // Exactly at the cap: read whole, then refused as bad JSON.
    send(&mut c, &"a".repeat(MAX_LINE_BYTES)).await;
    assert_eq!(recv(&mut c).await.unwrap()["error"]["code"], -32700);
    // One over, and then three times over: refused unread, then resynced.
    for size in [MAX_LINE_BYTES + 1, 3 * MAX_LINE_BYTES] {
        send(&mut c, &"a".repeat(size)).await;
        let got = recv(&mut c).await.unwrap();
        assert_eq!(got["error"]["code"], -32600, "{size}");
        assert_eq!(got["id"], Value::Null);
        send(&mut c, r#"{"jsonrpc":"2.0","id":9,"method":"ping"}"#).await;
        assert_eq!(recv(&mut c).await.unwrap()["id"], 9);
    }
}

#[tokio::test]
async fn an_oversize_line_without_a_newline_is_not_buffered() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    let s = open(&mut hub, 1, Arc::new(Fake::default()));
    let mut c = ready(&s).await;
    // Sent in pieces, with the newline only at the end: the error comes once.
    for _ in 0..3 {
        send_raw(&mut c, &vec![b'a'; MAX_LINE_BYTES]).await;
    }
    send_raw(&mut c, b"\n").await;
    assert_eq!(recv(&mut c).await.unwrap()["error"]["code"], -32600);
    send(&mut c, r#"{"jsonrpc":"2.0","id":9,"method":"ping"}"#).await;
    assert_eq!(recv(&mut c).await.unwrap()["id"], 9);
}

#[tokio::test]
async fn bytes_that_are_not_utf8_get_an_error_and_the_connection_keeps_serving() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    let s = open(&mut hub, 1, Arc::new(Fake::default()));
    let mut c = ready(&s).await;
    send_raw(&mut c, b"\xff\xfe\n").await;
    assert_eq!(recv(&mut c).await.unwrap()["error"]["code"], -32700);
    send(&mut c, r#"{"jsonrpc":"2.0","id":9,"method":"ping"}"#).await;
    assert_eq!(recv(&mut c).await.unwrap()["id"], 9);
}

#[tokio::test]
async fn an_oversize_hello_is_closed() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    let s = open(&mut hub, 1, Arc::new(Fake::default()));
    let mut c = connect(&s.socket).await;
    let padded = json!({"lazarobox": 1, "token": s.token.reveal(), "pad": "x".repeat(HELLO_MAX)});
    send(&mut c, &padded.to_string()).await;
    assert_eq!(recv(&mut c).await, None);
}

#[tokio::test]
async fn a_silent_client_is_dropped_after_the_handshake_timeout() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    hub.handshake = Duration::from_millis(50);
    let s = open(&mut hub, 1, Arc::new(Fake::default()));
    let mut c = connect(&s.socket).await;
    assert_eq!(recv(&mut c).await, None);
}

#[tokio::test]
async fn a_peer_of_another_uid_is_closed_even_with_the_right_token() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    hub.uid = hub.uid.wrapping_add(1);
    let s = open(&mut hub, 1, Arc::new(Fake::default()));
    let mut c = connect(&s.socket).await;
    send(&mut c, &hello(s.token.reveal())).await;
    assert_eq!(recv(&mut c).await, None);
}

#[tokio::test]
async fn closing_the_chat_removes_the_socket_and_ends_everything_live() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    let (fake, mut probe) = hang();
    let s = open(&mut hub, 1, fake);
    let mut c = ready(&s).await;
    send(
        &mut c,
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"find_dir","arguments":{"query":"x"}}}"#,
    )
    .await;
    probe.called().await;
    assert!(probe.dropped.try_recv().is_err());
    hub.close(pane(1));
    assert!(!s.socket.exists());
    assert_eq!(recv(&mut c).await, None);
    assert!(UnixStream::connect(&s.socket).await.is_err());
    probe.dropped().await;
    hub.close(pane(1));
}

#[tokio::test]
async fn ending_a_session_from_its_handle_does_what_close_does() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    let (fake, mut probe) = hang();
    let s = open(&mut hub, 1, fake);
    let mut c = ready(&s).await;
    send(
        &mut c,
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"find_dir","arguments":{"query":"x"}}}"#,
    )
    .await;
    probe.called().await;
    s.end.call();
    assert!(!s.socket.exists());
    assert_eq!(recv(&mut c).await, None);
    assert!(UnixStream::connect(&s.socket).await.is_err());
    probe.dropped().await;
    // Twice is harmless, and so is the hub's own close afterwards.
    s.end.call();
    hub.close(pane(1));
}

#[tokio::test]
async fn the_handle_of_a_replaced_session_leaves_the_new_one_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    let old = open(&mut hub, 1, Arc::new(Fake::default()));
    let new = open(&mut hub, 1, Arc::new(Fake::default()));
    assert_eq!(old.socket, new.socket, "one socket path per pane");
    old.end.call();
    assert!(new.socket.exists());
    let mut c = ready(&new).await;
    send(&mut c, r#"{"jsonrpc":"2.0","id":9,"method":"ping"}"#).await;
    assert_eq!(recv(&mut c).await.unwrap()["id"], 9);
}

#[tokio::test]
async fn a_client_cancel_drops_the_call_and_the_session_keeps_serving() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    let (fake, mut probe) = hang();
    let s = open(&mut hub, 1, fake);
    let mut c = ready(&s).await;
    send(
        &mut c,
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"find_dir","arguments":{"query":"x"}}}"#,
    )
    .await;
    probe.called().await;
    send(
        &mut c,
        r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":5}}"#,
    )
    .await;
    send(&mut c, r#"{"jsonrpc":"2.0","id":9,"method":"ping"}"#).await;
    assert_eq!(recv(&mut c).await.unwrap()["id"], 9);
    probe.dropped().await;
}

#[tokio::test]
async fn a_client_that_hangs_up_drops_its_pending_call() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    hub.drain = Duration::from_secs(60);
    let (fake, mut probe) = hang();
    let s = open(&mut hub, 1, fake);
    let mut c = ready(&s).await;
    send(
        &mut c,
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"find_dir","arguments":{"query":"x"}}}"#,
    )
    .await;
    probe.called().await;
    drop(c);
    probe.dropped().await;
}

#[tokio::test]
async fn reopening_a_pane_retires_the_old_token() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    let old = open(&mut hub, 1, Arc::new(Fake::default()));
    let new = open(&mut hub, 1, Arc::new(Fake::default()));
    assert_ne!(old.token.reveal(), new.token.reveal());
    let mut c = connect(&new.socket).await;
    send(&mut c, &hello(old.token.reveal())).await;
    assert_eq!(recv(&mut c).await, None);
}

#[tokio::test]
async fn exiting_removes_every_socket() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    let a = open(&mut hub, 1, Arc::new(Fake::default()));
    let b = open(&mut hub, 2, Arc::new(Fake::default()));
    drop(hub);
    assert!(!a.socket.exists() && !b.socket.exists());
}

#[test]
fn the_sweep_removes_only_the_sockets_of_dead_runs() {
    let tmp = tempfile::tempdir().unwrap();
    for name in [
        "s-100-1.sock",
        "s-200-2.sock",
        "s-200-3.sock",
        "s-x-1.sock",
        "s-300-1",
        "notes.txt",
    ] {
        std::fs::write(tmp.path().join(name), "").unwrap();
    }
    sweep(tmp.path(), &|pid| pid == 200);
    let mut left: Vec<_> = std::fs::read_dir(tmp.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    left.sort();
    assert_eq!(
        left,
        [
            "notes.txt",
            "s-200-2.sock",
            "s-200-3.sock",
            "s-300-1",
            "s-x-1.sock"
        ]
    );
}

#[tokio::test]
async fn starting_sweeps_a_socket_left_by_a_crashed_run() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("run");
    std::fs::create_dir_all(&dir).unwrap();
    // pid_max is below 2^22, so this pid is never a live process.
    let stale = dir.join(format!("s-{}-1.sock", u32::MAX - 1));
    std::fs::write(&stale, "").unwrap();
    let _hub = Hub::<PaneId>::new(&[dir]).unwrap();
    assert!(!stale.exists());
}

#[tokio::test]
async fn read_line_caps_what_it_keeps_and_resyncs_at_the_newline() {
    let mut wire = b"ok\n".to_vec();
    wire.extend(vec![b'a'; 100_000]);
    wire.extend(b"\nlast\nunterminated");
    // A tiny buffer makes the skip loop run many times.
    let mut r = BufReader::with_capacity(16, wire.as_slice());
    assert_eq!(read_line(&mut r, 8).await.unwrap(), Line::Text("ok".into()));
    assert_eq!(read_line(&mut r, 8).await.unwrap(), Line::Oversize);
    assert_eq!(
        read_line(&mut r, 8).await.unwrap(),
        Line::Text("last".into())
    );
    assert_eq!(read_line(&mut r, 8).await.unwrap(), Line::Eof);
}

const CALL: &str = r#"{"jsonrpc":"2.0","id":ID,"method":"tools/call","params":{"name":"find_dir","arguments":{"query":"x"}}}"#;

fn call_line(id: u32) -> String {
    CALL.replace("ID", &id.to_string())
}

#[tokio::test]
async fn a_call_whose_id_is_still_in_flight_is_refused_with_its_id() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    let (fake, mut probe) = hang();
    let s = open(&mut hub, 1, fake);
    let mut c = ready(&s).await;
    send(&mut c, &call_line(5)).await;
    probe.called().await;
    send(&mut c, &call_line(5)).await;
    let got = recv(&mut c).await.unwrap();
    assert_eq!(
        (got["id"].clone(), got["error"]["code"].clone()),
        (json!(5), json!(-32600))
    );
    // The first call was not replaced: cancelling id 5 drops exactly it.
    assert!(
        probe.called.try_recv().is_err(),
        "the duplicate was not dispatched"
    );
    send(
        &mut c,
        r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":5}}"#,
    )
    .await;
    probe.dropped().await;
}

#[tokio::test]
async fn calls_over_the_in_flight_cap_are_refused_with_their_id() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    let (fake, mut probe) = hang();
    let s = open(&mut hub, 1, fake);
    let mut c = ready(&s).await;
    for id in 1..=MAX_IN_FLIGHT as u32 {
        send(&mut c, &call_line(id)).await;
        probe.called().await;
    }
    let over = MAX_IN_FLIGHT as u32 + 1;
    send(&mut c, &call_line(over)).await;
    let got = recv(&mut c).await.unwrap();
    assert_eq!(
        (got["id"].clone(), got["error"]["code"].clone()),
        (json!(over), json!(TOO_BUSY))
    );
    assert!(probe.called.try_recv().is_err());
    // A slot frees up when a call is cancelled.
    send(
        &mut c,
        r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}"#,
    )
    .await;
    probe.dropped().await;
    send(&mut c, &call_line(over)).await;
    probe.called().await;
}

#[tokio::test]
async fn connections_over_the_per_session_cap_are_closed_after_the_handshake() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    let s = open(&mut hub, 1, Arc::new(Fake::default()));
    let mut held = Vec::new();
    for _ in 0..MAX_CONNS {
        held.push(ready(&s).await);
    }
    let mut c = connect(&s.socket).await;
    send(&mut c, &hello(s.token.reveal())).await;
    assert_eq!(recv(&mut c).await, None);
    // Hanging up frees a slot.
    drop(held.pop());
    let mut c = connect(&s.socket).await;
    send(&mut c, &hello(s.token.reveal())).await;
    for _ in 0..200 {
        if let Some(reply) = recv(&mut c).await {
            assert_eq!(reply, json!({"ok": true}));
            return;
        }
        c = connect(&s.socket).await;
        send(&mut c, &hello(s.token.reveal())).await;
        tokio::task::yield_now().await;
    }
    panic!("the freed slot was never reusable");
}

#[tokio::test]
async fn connections_waiting_for_their_handshake_are_capped() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    let s = open(&mut hub, 1, Arc::new(Fake::default()));
    let mut silent = Vec::new();
    for _ in 0..MAX_PENDING {
        silent.push(connect(&s.socket).await);
    }
    // The next one is dropped at once, not after the handshake timeout.
    let mut extra = connect(&s.socket).await;
    assert_eq!(recv(&mut extra).await, None);
}

#[tokio::test]
async fn nothing_is_dispatched_once_the_session_is_marked_closed() {
    let fake = Arc::new(Fake::default());
    let token = Token::generate().unwrap();
    let secret = token.reveal().to_owned();
    let conn = Conn::new(
        token,
        tools(),
        fake.clone(),
        Slot::default(),
        unsafe { libc::geteuid() },
        Duration::from_secs(5),
        DRAIN_GRACE,
    );
    let closed = conn.closed.clone();
    let permit = conn.pending.clone().try_acquire_owned().unwrap();
    let (near, far) = UnixStream::pair().unwrap();
    let server = tokio::spawn(serve(near, conn, permit));
    let (r, w) = far.into_split();
    let mut c: Client = (BufReader::new(r), w);
    send(&mut c, &hello(&secret)).await;
    assert_eq!(recv(&mut c).await, Some(json!({"ok": true})));
    send(
        &mut c,
        r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
    )
    .await;
    assert!(recv(&mut c).await.unwrap()["result"].is_object());
    // `close()` sets this before it aborts anything; abort lands later.
    closed.store(true, Ordering::SeqCst);
    send(&mut c, &call_line(1)).await;
    server.await.unwrap();
    assert!(fake.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_panicking_dispatch_answers_an_error_result_and_the_session_survives() {
    for in_future in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let mut hub = hub(tmp.path());
        let s = open(&mut hub, 1, Arc::new(Panics { in_future }));
        let mut c = ready(&s).await;
        send(&mut c, &call_line(5)).await;
        let got = recv(&mut c).await.unwrap();
        assert_eq!(got["id"], 5, "{in_future}");
        assert_eq!(got["result"]["isError"], true);
        assert!(
            !got["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("boom")
        );
        send(&mut c, r#"{"jsonrpc":"2.0","id":9,"method":"ping"}"#).await;
        assert_eq!(recv(&mut c).await.unwrap()["id"], 9);
    }
}

const CALL_5: &str = r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"find_dir","arguments":{"query":"x"}}}"#;

/// Half-closes the client's write side, as the shim does when its stdin ends.
async fn half_close(client: &mut Client) {
    client.1.shutdown().await.unwrap();
}

impl Probe {
    /// Asserts the call is not dropped for a while. A negative check has to
    /// wait; the window only matters for the case where the session is wrong.
    async fn stays_alive(&mut self) {
        let waited = timeout(Duration::from_millis(100), self.dropped.recv()).await;
        assert!(waited.is_err(), "the call was dropped too early");
    }
}

#[tokio::test]
async fn a_half_closed_client_still_gets_the_reply_of_a_call_in_flight() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    let (fake, mut probe, release) = gated();
    let s = open(&mut hub, 1, fake);
    let mut c = ready(&s).await;
    send(&mut c, CALL_5).await;
    probe.called().await;
    half_close(&mut c).await;
    probe.stays_alive().await;
    release.notify_one();
    let got = recv(&mut c).await.unwrap();
    assert_eq!(got["id"], 5);
    assert_eq!(got["result"]["content"][0]["text"], "late");
    // With every reply flushed the session ends the connection.
    assert_eq!(recv(&mut c).await, None);
    assert!(probe.dropped.try_recv().is_ok());
}

#[tokio::test]
async fn a_call_still_pending_when_the_grace_ends_is_aborted() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    hub.drain = Duration::from_millis(400);
    let (fake, mut probe, _release) = gated();
    let s = open(&mut hub, 1, fake);
    let mut c = ready(&s).await;
    send(&mut c, CALL_5).await;
    probe.called().await;
    half_close(&mut c).await;
    probe.stays_alive().await;
    probe.dropped().await;
    assert_eq!(recv(&mut c).await, None);
}

#[tokio::test]
async fn closing_the_chat_aborts_a_draining_call_at_once() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    hub.drain = Duration::from_secs(60);
    let (fake, mut probe, _release) = gated();
    let s = open(&mut hub, 1, fake);
    let mut c = ready(&s).await;
    send(&mut c, CALL_5).await;
    probe.called().await;
    half_close(&mut c).await;
    probe.stays_alive().await;
    hub.close(pane(1));
    probe.dropped().await;
    assert_eq!(recv(&mut c).await, None);
}

#[tokio::test]
async fn a_full_hangup_during_the_drain_drops_the_call_without_waiting_for_the_grace() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    hub.drain = Duration::from_secs(60);
    let (fake, mut probe, _release) = gated();
    let s = open(&mut hub, 1, fake);
    let mut c = ready(&s).await;
    send(&mut c, CALL_5).await;
    probe.called().await;
    half_close(&mut c).await;
    probe.stays_alive().await;
    drop(c);
    probe.dropped().await;
}

/// The names a client gets for `tools/list`.
async fn listed(client: &mut Client) -> Vec<String> {
    send(client, r#"{"jsonrpc":"2.0","id":9,"method":"tools/list"}"#).await;
    let reply = recv(client).await.unwrap();
    reply["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_owned())
        .collect()
}

// The set is started after the session is open (the turn starts it), so the
// slot is read when a connection is served, not when the session opens.
#[tokio::test]
async fn a_session_offers_the_tools_of_the_set_its_slot_holds() {
    let dir = tempfile::tempdir().unwrap();
    let lab = lab();
    let servers = [lab.server("fs", "search", "found")];
    let refs: Vec<&crate::core::ai::config::McpServer> = servers.iter().collect();
    let set = Arc::new(
        ExternalSet::start(
            &refs,
            &lab.home,
            host_env,
            timeouts(Duration::from_secs(10)),
        )
        .await,
    );
    let mut hub = hub(dir.path());
    let slot = Slot::default();
    let started = hub
        .open(
            pane(1),
            Arc::new(Fake::default()),
            slot.clone(),
            FamilySet::LEGACY,
        )
        .unwrap();
    assert!(slot.set(set.clone()).is_ok());
    let mut c = ready(&started).await;
    let names = listed(&mut c).await;
    assert!(names.contains(&"fs__search".to_owned()), "{names:?}");
    assert!(names.contains(&"read_file".to_owned()), "{names:?}");
    set.shutdown().await;
}

#[tokio::test]
async fn an_empty_slot_leaves_only_the_built_in_tools() {
    let dir = tempfile::tempdir().unwrap();
    let mut hub = hub(dir.path());
    let started = open(&mut hub, 1, Arc::new(Fake::default()));
    let mut c = ready(&started).await;
    let names = listed(&mut c).await;
    assert!(names.contains(&"read_file".to_owned()), "{names:?}");
    assert!(names.iter().all(|n| !n.contains("__")), "{names:?}");
}

fn worker_hub(dir: &Path) -> Hub<WorkerKey> {
    Hub::new(&[dir.join("run")]).unwrap()
}

fn worker(run: u64, n: u32) -> WorkerKey {
    WorkerKey {
        run: RunId(run),
        worker: WorkerId(n),
    }
}

fn open_worker(hub: &mut Hub<WorkerKey>, key: WorkerKey, toolset: Toolset) -> Started {
    hub.open(
        key,
        Arc::new(Fake::default()),
        Slot::default(),
        toolset.families(),
    )
    .unwrap()
}

// Spec (lazarobox-mcp-server, Own socket per worker): the socket of a worker
// is named by run and worker, and no two workers share a path or a token.
#[tokio::test]
async fn each_worker_gets_its_own_socket_and_token_named_by_run_and_worker() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = worker_hub(tmp.path());
    let one = open_worker(&mut hub, worker(3, 1), Toolset::ReadOnly);
    let two = open_worker(&mut hub, worker(3, 2), Toolset::ReadOnly);
    let dir = tmp.path().join("run");
    let pid = std::process::id();
    assert_eq!(one.socket, dir.join(format!("s-{pid}-w3-1.sock")));
    assert_eq!(two.socket, dir.join(format!("s-{pid}-w3-2.sock")));
    assert_eq!(mode(&one.socket), 0o600);
    assert_ne!(one.token.reveal(), two.token.reveal());
    // Run and worker are not interchangeable: w1-12 is not w12-1.
    let a = open_worker(&mut hub, worker(1, 12), Toolset::ReadOnly);
    let b = open_worker(&mut hub, worker(12, 1), Toolset::ReadOnly);
    assert_ne!(a.socket, b.socket);
    assert!(a.socket.exists() && b.socket.exists());
    // One token opens only its own socket.
    let mut c = connect(&two.socket).await;
    send(&mut c, &hello(one.token.reveal())).await;
    assert_eq!(recv(&mut c).await, None);
    // Both go when the workers end.
    hub.close(worker(3, 1));
    assert!(!one.socket.exists() && two.socket.exists());
    drop(hub);
    assert!(!two.socket.exists() && !a.socket.exists() && !b.socket.exists());
}

// A pane and a worker may share a folder: their names never meet.
#[tokio::test]
async fn a_pane_socket_and_a_worker_socket_do_not_collide() {
    let tmp = tempfile::tempdir().unwrap();
    let mut panes = hub(tmp.path());
    let mut workers = worker_hub(tmp.path());
    let p = open(&mut panes, 1, Arc::new(Fake::default()));
    let w = open_worker(&mut workers, worker(1, 1), Toolset::ReadOnly);
    assert_ne!(p.socket, w.socket);
    assert!(p.socket.exists() && w.socket.exists());
}

// Design D7 / the P0 contract: for a worker key too, the offered list and
// the parse gate come from the same families.
#[tokio::test]
async fn a_worker_session_offers_its_toolset_and_refuses_orchestration() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = worker_hub(tmp.path());
    let fake = Arc::new(Fake::default());
    let s = hub
        .open(
            worker(1, 1),
            fake.clone(),
            Slot::default(),
            Toolset::ReadOnly.families(),
        )
        .unwrap();
    let mut c = ready(&s).await;
    assert_eq!(listed(&mut c).await, ["find_dir", "list_dir", "read_file"]);
    for (id, name, args) in [
        (5, "spawn_agent", r#"{"kind":"code","prompt":"p"}"#),
        (6, "run_in_pane", r#"{"command":"ls"}"#),
    ] {
        send(
            &mut c,
            &format!(
                r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"{name}","arguments":{args}}}}}"#
            ),
        )
        .await;
        let got = recv(&mut c).await.unwrap();
        assert_eq!(
            got["result"]["content"][0]["text"], "unknown tool",
            "{name}"
        );
        assert_eq!(got["result"]["isError"], true, "{name}");
    }
    assert!(
        fake.0.lock().unwrap().is_empty(),
        "nothing reached the dispatch"
    );
}

#[tokio::test]
async fn an_orchestrator_session_lists_and_dispatches_the_orchestration_tools() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    let fake = Arc::new(Fake::default());
    let s = hub
        .open(
            pane(1),
            fake.clone(),
            Slot::default(),
            Toolset::Orchestrator.families(),
        )
        .unwrap();
    let mut c = ready(&s).await;
    let names = listed(&mut c).await;
    assert_eq!(
        names,
        [
            "find_dir",
            "list_dir",
            "read_file",
            "spawn_agent",
            "wait",
            "collect",
            "cancel"
        ]
    );
    send(
        &mut c,
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"cancel","arguments":{"id":2}}}"#,
    )
    .await;
    assert_eq!(recv(&mut c).await.unwrap()["result"]["isError"], false);
    assert_eq!(
        *fake.0.lock().unwrap(),
        [Call::Orchestra(OrchCall::Cancel { id: WorkerId(2) })]
    );
}

#[test]
fn the_sweep_knows_the_worker_socket_form() {
    let tmp = tempfile::tempdir().unwrap();
    // pid 100 is dead, pid 200 is alive.
    let dead_workers = ["s-100-w1-1.sock", "s-100-w12-345.sock"];
    let kept = [
        "s-200-w1-1.sock",
        // Dead pid, but not a socket of ours: left alone.
        "s-100-w1.sock",
        "s-100-w-1.sock",
        "s-100-w1-.sock",
        "s-100-wx-1.sock",
        "s-100-w1-1-2.sock",
        "s-100-1w.sock",
        "s-100-w1-1",
        "s-x-w1-1.sock",
    ];
    for name in dead_workers.iter().chain(&kept) {
        std::fs::write(tmp.path().join(name), "").unwrap();
    }
    sweep(tmp.path(), &|pid| pid == 200);
    for name in dead_workers {
        assert!(!tmp.path().join(name).exists(), "{name} should go");
    }
    for name in kept {
        assert!(tmp.path().join(name).exists(), "{name} should stay");
    }
}

// A worker never lists or routes a user server's tool, even when the caller
// hands the hub a populated slot: the key type decides, not the caller.
#[tokio::test]
async fn a_worker_session_ignores_a_populated_external_slot() {
    let dir = tempfile::tempdir().unwrap();
    let lab = lab();
    let servers = [lab.server("fs", "search", "found")];
    let refs: Vec<&crate::core::ai::config::McpServer> = servers.iter().collect();
    let set = Arc::new(
        ExternalSet::start(
            &refs,
            &lab.home,
            host_env,
            timeouts(Duration::from_secs(10)),
        )
        .await,
    );
    let slot = Slot::default();
    assert!(slot.set(set.clone()).is_ok());
    let fake = Arc::new(Fake::default());
    let mut hub = worker_hub(dir.path());
    let s = hub
        .open(
            worker(1, 1),
            fake.clone(),
            slot.clone(),
            Toolset::ReadOnly.families(),
        )
        .unwrap();
    let mut c = ready(&s).await;
    assert_eq!(listed(&mut c).await, ["find_dir", "list_dir", "read_file"]);
    send(
        &mut c,
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"fs__search","arguments":{}}}"#,
    )
    .await;
    let got = recv(&mut c).await.unwrap();
    assert_eq!(got["result"]["content"][0]["text"], "unknown tool");
    assert_eq!(got["result"]["isError"], true);
    assert!(fake.0.lock().unwrap().is_empty());
    // The same slot on a pane hub still serves the external tool.
    let mut panes = hub_in(dir.path());
    let p = panes
        .open(pane(1), Arc::new(Fake::default()), slot, FamilySet::LEGACY)
        .unwrap();
    let mut c = ready(&p).await;
    assert!(listed(&mut c).await.contains(&"fs__search".to_owned()));
    set.shutdown().await;
}
