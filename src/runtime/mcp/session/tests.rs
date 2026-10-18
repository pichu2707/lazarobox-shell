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
    core::ai::tools::{Call, mcp::MAX_LINE_BYTES, registry},
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
    hub.open(pane(n), tools(), dispatch, Slot::default())
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
    let hub = Hub::new(&[blocker.join("lazarobox"), tmp.path().join("run")]).unwrap();
    assert_eq!(hub.dir, tmp.path().join("run"));
    assert!(Hub::new(&[blocker.join("x")]).is_err());
    assert!(Hub::new(&[]).is_err());
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
    let _hub = Hub::new(&[dir]).unwrap();
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
        .open(pane(1), tools(), Arc::new(Fake::default()), slot.clone())
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
