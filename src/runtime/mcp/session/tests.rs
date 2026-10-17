use std::{
    os::unix::fs::PermissionsExt,
    path::Path,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use futures::{FutureExt, future::BoxFuture};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{
        UnixStream,
        unix::{OwnedReadHalf, OwnedWriteHalf},
    },
};

use super::*;
use crate::{
    core::ai::tools::{Call, mcp::MAX_LINE_BYTES, registry},
    runtime::mcp::Outcome,
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

/// Never answers; says when the call was dropped.
struct Hang(Arc<AtomicBool>);

struct SetOnDrop(Arc<AtomicBool>);

impl Drop for SetOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

impl Dispatch for Hang {
    fn call(&self, _: Call) -> BoxFuture<'static, Outcome> {
        let guard = SetOnDrop(self.0.clone());
        async move {
            let _guard = guard;
            std::future::pending::<Outcome>().await
        }
        .boxed()
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
    hub.open(pane(n), tools(), dispatch).unwrap()
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
    let dropped = Arc::new(AtomicBool::new(false));
    let s = open(&mut hub, 1, Arc::new(Hang(dropped.clone())));
    let mut c = ready(&s).await;
    send(
        &mut c,
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"find_dir","arguments":{"query":"x"}}}"#,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!dropped.load(Ordering::SeqCst));
    hub.close(pane(1));
    assert!(!s.socket.exists());
    assert_eq!(recv(&mut c).await, None);
    assert!(UnixStream::connect(&s.socket).await.is_err());
    for _ in 0..100 {
        if dropped.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        dropped.load(Ordering::SeqCst),
        "the pending call is dropped"
    );
    hub.close(pane(1));
}

#[tokio::test]
async fn a_client_cancel_drops_the_call_and_the_session_keeps_serving() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    let dropped = Arc::new(AtomicBool::new(false));
    let s = open(&mut hub, 1, Arc::new(Hang(dropped.clone())));
    let mut c = ready(&s).await;
    send(
        &mut c,
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"find_dir","arguments":{"query":"x"}}}"#,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    send(
        &mut c,
        r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":5}}"#,
    )
    .await;
    send(&mut c, r#"{"jsonrpc":"2.0","id":9,"method":"ping"}"#).await;
    assert_eq!(recv(&mut c).await.unwrap()["id"], 9);
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn a_client_that_hangs_up_drops_its_pending_call() {
    let tmp = tempfile::tempdir().unwrap();
    let mut hub = hub(tmp.path());
    let dropped = Arc::new(AtomicBool::new(false));
    let s = open(&mut hub, 1, Arc::new(Hang(dropped.clone())));
    let mut c = ready(&s).await;
    send(
        &mut c,
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"find_dir","arguments":{"query":"x"}}}"#,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    drop(c);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(dropped.load(Ordering::SeqCst));
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
