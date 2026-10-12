//! Chat client tests. Wire behaviour runs against a real loopback listener on
//! an ephemeral port; every time limit runs on the paused tokio clock over an
//! in-memory duplex, so nothing here sleeps for real.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, DuplexStream, duplex},
    net::TcpListener,
    sync::Notify,
    task::JoinHandle,
    time::{Instant, advance},
};

use super::*;
use crate::core::ai::chat::{
    engine::{ChatError, Finish},
    stream::NdjsonSplitter,
};

const HEAD: &[u8] = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n";
const HELLO: &str = "{\"message\":{\"content\":\"Hel\"},\"done\":false}\n";
const DONE: &str = "{\"message\":{\"content\":\"lo\"},\"done\":true,\"done_reason\":\"stop\"}\n";
const END: &[u8] = b"0\r\n\r\n";

fn chunk(text: &str) -> Vec<u8> {
    format!("{:x}\r\n{text}\r\n", text.len()).into_bytes()
}

fn delta(text: &str) -> StreamEvent {
    StreamEvent::Delta(text.into())
}

fn failed(text: &str) -> StreamEvent {
    StreamEvent::Error(ChatError::Failed(text.into()))
}

async fn collect(mut rx: mpsc::Receiver<StreamEvent>) -> Vec<StreamEvent> {
    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(event);
    }
    events
}

// A real loopback server ------------------------------------------------------

/// Accepts one connection, records its request and relays what the test feeds.
/// Any further connection is counted and dropped.
struct Fake {
    port: u16,
    requests: Arc<Mutex<Vec<Vec<u8>>>>,
    connections: Arc<AtomicUsize>,
    feed: Option<mpsc::UnboundedSender<Vec<u8>>>,
    request_read: Arc<Notify>,
    closed: Arc<Notify>,
}

impl Fake {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (feed, fed) = mpsc::unbounded_channel::<Vec<u8>>();
        let fake = Fake {
            port,
            requests: Arc::default(),
            connections: Arc::default(),
            feed: Some(feed),
            request_read: Arc::default(),
            closed: Arc::default(),
        };
        let (requests, connections) = (fake.requests.clone(), fake.connections.clone());
        let (request_read, closed) = (fake.request_read.clone(), fake.closed.clone());
        tokio::spawn(async move {
            let mut fed = Some(fed);
            while let Ok((stream, _)) = listener.accept().await {
                connections.fetch_add(1, Ordering::SeqCst);
                let Some(mut fed) = fed.take() else { continue };
                let (requests, request_read, closed) =
                    (requests.clone(), request_read.clone(), closed.clone());
                tokio::spawn(async move {
                    let (mut read, mut write) = stream.into_split();
                    let mut raw = Vec::new();
                    let mut buf = [0u8; 1024];
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
                        match read.read(&mut buf).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => raw.extend_from_slice(&buf[..n]),
                        }
                    };
                    while raw.len() < want {
                        match read.read(&mut buf).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => raw.extend_from_slice(&buf[..n]),
                        }
                    }
                    requests.lock().unwrap().push(raw);
                    request_read.notify_one();
                    tokio::spawn(async move {
                        while matches!(read.read(&mut buf).await, Ok(n) if n > 0) {}
                        closed.notify_one();
                    });
                    while let Some(bytes) = fed.recv().await {
                        if write.write_all(&bytes).await.is_err() {
                            return;
                        }
                    }
                    let _ = write.shutdown().await;
                });
            }
        });
        fake
    }

    fn send(&self, bytes: &[u8]) {
        let _ = self.feed.as_ref().unwrap().send(bytes.to_vec());
    }

    /// Ends the reply: the server shuts its side down once all is sent.
    fn close(&mut self) {
        self.feed = None;
    }

    fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }

    fn request(&self) -> String {
        let requests = self.requests.lock().unwrap();
        String::from_utf8(requests.first().expect("no request").clone()).unwrap()
    }
}

async fn turn(port: u16, path: &str, body: &[u8]) -> Vec<StreamEvent> {
    turn_to("127.0.0.1", port, path, body).await
}

async fn turn_to(host: &str, port: u16, path: &str, body: &[u8]) -> Vec<StreamEvent> {
    let (tx, rx) = mpsc::channel(16);
    let run = async move {
        let mut decoder = NdjsonSplitter::server();
        post_stream(host, port, path, body, &mut decoder, &tx).await;
    };
    tokio::join!(run, collect(rx)).1
}

#[tokio::test]
async fn posts_http_1_1_with_the_documented_headers_and_no_credentials() {
    let mut fake = Fake::start().await;
    fake.send(HEAD);
    fake.send(&chunk(DONE));
    fake.send(END);
    fake.close();
    let body = br#"{"model":"m","stream":true}"#;
    let events = turn(fake.port, "/api/chat", body).await;
    assert_eq!(events, vec![delta("lo"), StreamEvent::Done(Finish::Stop)]);
    let expected = format!(
        "POST /api/chat HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        fake.port,
        body.len(),
        String::from_utf8_lossy(body)
    );
    assert_eq!(fake.request(), expected);
    let lower = fake.request().to_lowercase();
    assert!(!lower.contains("authorization") && !lower.contains("cookie"));
    assert_eq!(fake.connections(), 1);
}

#[tokio::test]
async fn a_foreign_host_is_refused_without_connecting() {
    let fake = Fake::start().await;
    for host in [
        "localhost",
        "127.0.0.2",
        "10.0.0.1",
        "::1",
        "",
        "127.0.0.1 ",
    ] {
        let events = turn_to(host, fake.port, "/api/chat", b"{}").await;
        assert_eq!(events, vec![failed(REFUSED)], "host {host:?}");
    }
    // Real socket, real clock: give a stray connect time to reach the listener.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(fake.connections(), 0);
}

#[tokio::test]
async fn only_the_static_chat_paths_are_posted_to() {
    let fake = Fake::start().await;
    // Detection's GET endpoints, a lookalike and a query are all refused.
    for path in ["/api/tags", "/v1/models", "/api/chat?x=1", "/api/chat/", ""] {
        let events = turn(fake.port, path, b"{}").await;
        assert_eq!(events, vec![failed(REFUSED)], "path {path:?}");
    }
    assert_eq!(fake.connections(), 0);
    assert_eq!(CHAT_PATHS, ["/api/chat", "/v1/chat/completions"]);
}

#[tokio::test]
async fn a_redirect_is_an_error_and_is_never_followed() {
    let mut fake = Fake::start().await;
    fake.send(
        b"HTTP/1.1 307 Temporary Redirect\r\nLocation: http://127.0.0.1:1/x\r\nContent-Length: 0\r\n\r\n",
    );
    fake.close();
    let events = turn(fake.port, "/api/chat", b"{}").await;
    assert_eq!(events.len(), 1);
    assert!(
        matches!(&events[0], StreamEvent::Error(ChatError::Failed(m)) if m.starts_with("HTTP 307"))
    );
    assert_eq!(fake.connections(), 1);
}

#[tokio::test]
async fn a_non_200_gives_the_status_and_a_message_of_at_most_200_chars() {
    let mut fake = Fake::start().await;
    let big = vec![b'x'; 1024 * 1024];
    let mut reply = format!(
        "HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\n\r\n",
        big.len()
    )
    .into_bytes();
    reply.extend(&big);
    fake.send(&reply);
    fake.close();
    let events = turn(fake.port, "/api/chat", b"{}").await;
    let [StreamEvent::Error(ChatError::Failed(message))] = events.as_slice() else {
        panic!("{events:?}");
    };
    assert!(message.starts_with("HTTP 404"), "{message}");
    assert!(message.chars().count() <= ERROR_MESSAGE_CAP, "{message}");
}

#[tokio::test]
async fn a_non_200_message_is_one_clean_line() {
    let mut fake = Fake::start().await;
    let body = "model\n\x1b[2Jnot\tfound\r\n";
    fake.send(
        format!(
            "HTTP/1.1 500 Oops\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    );
    fake.close();
    let events = turn(fake.port, "/api/chat", b"{}").await;
    assert_eq!(events, vec![failed("HTTP 500: model not found")]);
}

#[tokio::test]
async fn a_chunked_non_200_body_is_decoded_for_the_message() {
    let mut fake = Fake::start().await;
    fake.send(b"HTTP/1.1 400 Bad\r\nTransfer-Encoding: chunked\r\n\r\n");
    fake.send(&chunk("no such "));
    fake.send(&chunk("model"));
    fake.send(END);
    fake.close();
    let events = turn(fake.port, "/api/chat", b"{}").await;
    assert_eq!(events, vec![failed("HTTP 400: no such model")]);
}

#[tokio::test]
async fn ambiguous_or_malformed_replies_are_rejected() {
    let replies: [&[u8]; 4] = [
        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nok\r\n0\r\n\r\n",
        b"HTTP/1.1 200 OK\r\nContent-Length: +2\r\n\r\nok",
        b"HTTP/2 200\r\n\r\n",
        b"garbage\r\n\r\n",
    ];
    for reply in replies {
        let mut fake = Fake::start().await;
        fake.send(reply);
        fake.close();
        let events = turn(fake.port, "/api/chat", b"{}").await;
        assert_eq!(events, vec![failed(INVALID_REPLY)], "{reply:?}");
    }
}

#[tokio::test]
async fn a_head_over_16_kib_is_rejected() {
    let mut fake = Fake::start().await;
    let mut reply = b"HTTP/1.1 200 OK\r\nX: ".to_vec();
    reply.resize(20 * 1024, b'a');
    fake.send(&reply);
    let events = turn(fake.port, "/api/chat", b"{}").await;
    assert_eq!(events, vec![failed(INVALID_REPLY)]);
    fake.close();
}

#[tokio::test]
async fn a_chunked_reply_is_relayed_as_it_arrives() {
    let fake = Fake::start().await;
    let (tx, mut rx) = mpsc::channel(16);
    let port = fake.port;
    let task = tokio::spawn(async move {
        let mut decoder = NdjsonSplitter::server();
        post_stream("127.0.0.1", port, "/api/chat", b"{}", &mut decoder, &tx).await;
    });
    fake.send(HEAD);
    fake.send(&chunk(HELLO));
    // The first delta is here while the reply is still open.
    assert_eq!(rx.recv().await, Some(delta("Hel")));
    assert!(!task.is_finished());
    fake.send(&chunk(DONE));
    assert_eq!(rx.recv().await, Some(delta("lo")));
    fake.send(END);
    assert_eq!(rx.recv().await, Some(StreamEvent::Done(Finish::Stop)));
    task.await.unwrap();
}

#[tokio::test]
async fn a_chunk_split_across_reads_is_relayed_whole() {
    let mut fake = Fake::start().await;
    let wire = [HEAD.to_vec(), chunk(HELLO), chunk(DONE), END.to_vec()].concat();
    for part in wire.chunks(7) {
        fake.send(part);
    }
    fake.close();
    let events = turn(fake.port, "/api/chat", b"{}").await;
    assert_eq!(
        events,
        vec![delta("Hel"), delta("lo"), StreamEvent::Done(Finish::Stop)]
    );
}

#[tokio::test]
async fn content_length_and_close_delimited_replies_are_decoded() {
    let text = format!("{HELLO}{DONE}");
    let length = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{text}",
        text.len()
    );
    let close = format!("HTTP/1.0 200 OK\r\n\r\n{text}");
    for reply in [length, close] {
        let mut fake = Fake::start().await;
        fake.send(reply.as_bytes());
        fake.close();
        let events = turn(fake.port, "/api/chat", b"{}").await;
        assert_eq!(
            events,
            vec![delta("Hel"), delta("lo"), StreamEvent::Done(Finish::Stop)]
        );
    }
}

#[tokio::test]
async fn a_stream_over_the_detection_body_cap_is_not_cut() {
    let mut fake = Fake::start().await;
    let line = format!(
        "{{\"message\":{{\"content\":\"{}\"}},\"done\":false}}\n",
        "x".repeat(8 * 1024)
    );
    fake.send(HEAD);
    for _ in 0..192 {
        fake.send(&chunk(&line));
    }
    fake.send(&chunk(DONE));
    fake.send(END);
    fake.close();
    let events = turn(fake.port, "/api/chat", b"{}").await;
    let text: usize = events
        .iter()
        .map(|e| match e {
            StreamEvent::Delta(t) => t.len(),
            _ => 0,
        })
        .sum();
    assert!(text > 1024 * 1024, "{text}");
    assert_eq!(events.last(), Some(&StreamEvent::Done(Finish::Stop)));
}

#[tokio::test]
async fn a_reply_that_ends_early_is_interrupted_with_the_text_kept() {
    let mut fake = Fake::start().await;
    fake.send(HEAD);
    fake.send(&chunk(HELLO));
    fake.close();
    let events = turn(fake.port, "/api/chat", b"{}").await;
    assert_eq!(
        events,
        vec![delta("Hel"), StreamEvent::Error(ChatError::Interrupted)]
    );
}

// Real clock on purpose: a paused clock auto-advances while the socket is still
// in flight (it showed ~2 s of virtual time), which would hide the very thing
// under test. The bound sits below the 2 s connect limit, so a refusal that
// waited for that limit fails.
#[tokio::test]
async fn connect_refused_ends_at_once() {
    // A bound socket that never listens holds the port until the end of the
    // test, so a parallel test cannot take it: the connect is refused.
    let held = tokio::net::TcpSocket::new_v4().unwrap();
    held.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let port = held.local_addr().unwrap().port();
    let start = std::time::Instant::now();
    let events = turn(port, "/api/chat", b"{}").await;
    assert_eq!(events, vec![failed(CANNOT_CONNECT)]);
    assert!(start.elapsed() < Timeouts::SERVER.connect - Duration::from_millis(500));
}

#[tokio::test]
async fn cancelling_before_the_first_byte_closes_the_socket() {
    let fake = Fake::start().await;
    let (tx, _rx) = mpsc::channel(16);
    let port = fake.port;
    let task = tokio::spawn(async move {
        let mut decoder = NdjsonSplitter::server();
        post_stream("127.0.0.1", port, "/api/chat", b"{}", &mut decoder, &tx).await;
    });
    fake.request_read.notified().await;
    task.abort();
    // The guard only keeps a regression from hanging the suite.
    tokio::time::timeout(Duration::from_secs(10), fake.closed.notified())
        .await
        .expect("the server never saw the socket close");
}

#[tokio::test]
async fn a_closed_receiver_ends_the_turn_and_the_socket() {
    let fake = Fake::start().await;
    let (tx, rx) = mpsc::channel(1);
    let port = fake.port;
    let task = tokio::spawn(async move {
        let mut decoder = NdjsonSplitter::server();
        post_stream("127.0.0.1", port, "/api/chat", b"{}", &mut decoder, &tx).await;
    });
    fake.send(HEAD);
    fake.send(&chunk(HELLO));
    // The receiver goes away mid-stream, not before the request is out.
    fake.request_read.notified().await;
    drop(rx);
    fake.send(&chunk(HELLO));
    tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .expect("the turn did not end")
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), fake.closed.notified())
        .await
        .expect("the server never saw the socket close");
}

// Time limits over an in-memory duplex, on the paused clock ---------------------

struct Turn {
    server: DuplexStream,
    events: mpsc::Receiver<StreamEvent>,
    task: JoinHandle<()>,
    start: Instant,
}

fn start_turn() -> Turn {
    let (tx, events) = mpsc::channel(256);
    start_turn_on(tx, events)
}

fn start_turn_on(tx: mpsc::Sender<StreamEvent>, events: mpsc::Receiver<StreamEvent>) -> Turn {
    let (client, server) = duplex(64 * 1024);
    let task = tokio::spawn(async move {
        let mut decoder = NdjsonSplitter::server();
        drive(client, b"POST", &Timeouts::SERVER, &mut decoder, &tx).await;
    });
    Turn {
        server,
        events,
        task,
        start: Instant::now(),
    }
}

#[tokio::test(start_paused = true)]
async fn a_first_byte_slower_than_idle_is_fine() {
    let mut t = start_turn();
    let mut server = t.server;
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(100)).await;
        server.write_all(HEAD).await.unwrap();
        server.write_all(&chunk(HELLO)).await.unwrap();
        server.write_all(&chunk(DONE)).await.unwrap();
        server.write_all(END).await.unwrap();
        // Keep the socket open: the framing ends the reply.
        std::future::pending::<()>().await;
    });
    let mut events = Vec::new();
    while let Some(e) = t.events.recv().await {
        events.push(e);
    }
    assert_eq!(
        events,
        vec![
            StreamEvent::Slow,
            delta("Hel"),
            delta("lo"),
            StreamEvent::Done(Finish::Stop)
        ]
    );
    assert_eq!(t.start.elapsed(), Duration::from_secs(100));
}

#[tokio::test(start_paused = true)]
async fn no_first_byte_in_120_s_is_no_answer() {
    let mut t = start_turn();
    assert_eq!(t.events.recv().await, Some(StreamEvent::Slow));
    assert_eq!(t.events.recv().await, Some(failed(NO_ANSWER)));
    assert_eq!(t.events.recv().await, None);
    assert_eq!(t.start.elapsed(), Duration::from_secs(120));
    // The socket is closed: the server end reads the request, then EOF.
    let mut rest = Vec::new();
    t.server.read_to_end(&mut rest).await.unwrap();
    assert_eq!(rest, b"POST");
}

#[tokio::test(start_paused = true)]
async fn a_stream_silent_for_60_s_is_stalled_and_the_socket_closes() {
    let mut t = start_turn();
    t.server.write_all(HEAD).await.unwrap();
    t.server.write_all(&chunk(HELLO)).await.unwrap();
    assert_eq!(t.events.recv().await, Some(delta("Hel")));
    let delivered = Instant::now();
    assert_eq!(t.events.recv().await, Some(failed(STALLED)));
    assert_eq!(delivered.elapsed(), Duration::from_secs(60));
    t.task.await.unwrap();
    let mut rest = Vec::new();
    t.server.read_to_end(&mut rest).await.unwrap();
    assert_eq!(rest, b"POST");
}

#[tokio::test(start_paused = true)]
async fn a_stream_alive_every_59_s_is_not_stalled() {
    let mut t = start_turn();
    let mut server = t.server;
    server.write_all(HEAD).await.unwrap();
    for _ in 0..3 {
        server.write_all(&chunk(HELLO)).await.unwrap();
        assert_eq!(t.events.recv().await, Some(delta("Hel")));
        tokio::time::sleep(Duration::from_secs(59)).await;
    }
    server.write_all(&chunk(DONE)).await.unwrap();
    server.write_all(END).await.unwrap();
    assert_eq!(t.events.recv().await, Some(delta("lo")));
    assert_eq!(t.events.recv().await, Some(StreamEvent::Done(Finish::Stop)));
}

// Characterization: the transport counts bytes, so reasoning needs no special
// case. These pin that a thinking-only stretch is activity for every limit.
#[tokio::test(start_paused = true)]
async fn thinking_counts_as_the_first_byte_and_as_activity() {
    const THOUGHT: &str = "{\"message\":{\"content\":\"\",\"thinking\":\"hm\"},\"done\":false}\n";
    let mut t = start_turn();
    let mut server = t.server;
    tokio::spawn(async move {
        // Slower than the 60 s idle limit and 3 s `Slow`, faster than the
        // 120 s first-byte limit: only the first thought may arrive this late.
        tokio::time::sleep(Duration::from_secs(100)).await;
        server.write_all(HEAD).await.unwrap();
        for _ in 0..4 {
            server.write_all(&chunk(THOUGHT)).await.unwrap();
            tokio::time::sleep(Duration::from_secs(59)).await;
        }
        server.write_all(&chunk(DONE)).await.unwrap();
        server.write_all(END).await.unwrap();
        std::future::pending::<()>().await;
    });
    let mut events = Vec::new();
    while let Some(e) = t.events.recv().await {
        events.push(e);
    }
    let thought = StreamEvent::ThinkingDelta("hm".into());
    assert_eq!(
        events,
        vec![
            StreamEvent::Slow,
            thought.clone(),
            thought.clone(),
            thought.clone(),
            thought,
            delta("lo"),
            StreamEvent::Done(Finish::Stop)
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn a_turn_that_keeps_streaming_ends_at_15_minutes() {
    let mut t = start_turn();
    let mut server = t.server;
    tokio::spawn(async move {
        server.write_all(HEAD).await.unwrap();
        loop {
            if server.write_all(&chunk(HELLO)).await.is_err() {
                return;
            }
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    });
    let mut last = None;
    while let Some(e) = t.events.recv().await {
        last = Some(e);
    }
    assert_eq!(last, Some(failed(TOO_LONG)));
    assert_eq!(t.start.elapsed(), Duration::from_secs(15 * 60));
}

#[tokio::test(start_paused = true)]
async fn slow_is_sent_once_at_3_s_and_not_at_2_9_s() {
    let mut t = start_turn();
    tokio::task::yield_now().await;
    advance(Duration::from_millis(2900)).await;
    tokio::task::yield_now().await;
    assert_eq!(t.events.try_recv(), Err(mpsc::error::TryRecvError::Empty));
    advance(Duration::from_millis(100)).await;
    tokio::task::yield_now().await;
    assert_eq!(t.events.try_recv(), Ok(StreamEvent::Slow));
    advance(Duration::from_secs(20)).await;
    tokio::task::yield_now().await;
    assert_eq!(t.events.try_recv(), Err(mpsc::error::TryRecvError::Empty));
    t.task.abort();
}

#[tokio::test(start_paused = true)]
async fn no_slow_event_once_a_byte_arrived() {
    let mut t = start_turn();
    // Let the task run up to its first read before the byte is written.
    tokio::task::yield_now().await;
    t.server.write_all(b"HTTP/1.1 200 OK\r\n").await.unwrap();
    // And let it read that byte, so the idle limit applies from here on.
    tokio::task::yield_now().await;
    advance(Duration::from_millis(3500)).await;
    tokio::task::yield_now().await;
    assert_eq!(t.events.try_recv(), Err(mpsc::error::TryRecvError::Empty));
    t.task.abort();
}

#[tokio::test(start_paused = true)]
async fn a_full_channel_does_not_hold_back_the_first_byte_limit() {
    // Capacity 1 and already full: Slow is best-effort and must be dropped
    // instead of blocking the timers.
    let (tx, events) = mpsc::channel(1);
    tx.try_send(delta("queued")).unwrap();
    let mut t = start_turn_on(tx, events);
    tokio::task::yield_now().await;
    advance(Duration::from_secs(121)).await;
    tokio::task::yield_now().await;
    assert_eq!(t.events.recv().await, Some(delta("queued")));
    assert_eq!(t.events.recv().await, Some(failed(NO_ANSWER)));
    assert_eq!(t.events.recv().await, None);
    t.task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn a_receiver_dropped_before_the_first_byte_closes_the_socket_at_once() {
    let t = start_turn();
    let mut server = t.server;
    tokio::task::yield_now().await;
    drop(t.events);
    let mut rest = Vec::new();
    server.read_to_end(&mut rest).await.unwrap();
    assert_eq!(rest, b"POST");
    // No Slow timer had to fire for the turn to notice.
    assert!(
        t.start.elapsed() < Duration::from_secs(1),
        "{:?}",
        t.start.elapsed()
    );
    t.task.await.unwrap();
}

// Writing the request --------------------------------------------------------

/// A request too big for the duplex buffer, so the write is still going when
/// the server acts.
fn big_request() -> Vec<u8> {
    vec![b'a'; 1024 * 1024]
}

async fn drive_big(client: DuplexStream, timeouts: Timeouts) -> (Vec<StreamEvent>, Instant) {
    let (tx, rx) = mpsc::channel(16);
    let start = Instant::now();
    let run = async move {
        let mut decoder = NdjsonSplitter::server();
        drive(client, &big_request(), &timeouts, &mut decoder, &tx).await;
    };
    (tokio::join!(run, collect(rx)).1, start)
}

#[tokio::test(start_paused = true)]
async fn a_413_sent_while_the_body_is_still_going_out_is_reported() {
    let (client, mut server) = duplex(64 * 1024);
    // The server answers and hangs up without reading the body.
    server
        .write_all(b"HTTP/1.1 413 Payload Too Large\r\nContent-Length: 9\r\n\r\ntoo large")
        .await
        .unwrap();
    drop(server);
    let (events, _) = drive_big(client, Timeouts::SERVER).await;
    assert_eq!(events, vec![failed("HTTP 413: too large")]);
}

#[tokio::test(start_paused = true)]
async fn a_failed_write_with_no_reply_is_cannot_connect() {
    let (client, server) = duplex(64 * 1024);
    drop(server);
    let (events, _) = drive_big(client, Timeouts::SERVER).await;
    assert_eq!(events, vec![failed(CANNOT_CONNECT)]);
}

#[tokio::test(start_paused = true)]
async fn a_server_that_never_reads_the_request_is_no_answer_at_the_first_byte_limit() {
    let (client, _server) = duplex(64 * 1024);
    let (events, start) = drive_big(client, Timeouts::SERVER).await;
    assert_eq!(events, vec![failed(NO_ANSWER)]);
    assert_eq!(start.elapsed(), Duration::from_secs(120));
}

// A read error ---------------------------------------------------------------

/// Serves scripted reads, then stays silent; swallows writes.
struct Scripted {
    reads: std::collections::VecDeque<std::io::Result<Vec<u8>>>,
}

impl tokio::io::AsyncRead for Scripted {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.reads.pop_front() {
            Some(Ok(bytes)) => {
                buf.put_slice(&bytes);
                std::task::Poll::Ready(Ok(()))
            }
            Some(Err(e)) => std::task::Poll::Ready(Err(e)),
            None => std::task::Poll::Pending,
        }
    }
}

impl tokio::io::AsyncWrite for Scripted {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::task::Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}

async fn drive_scripted(reads: Vec<std::io::Result<Vec<u8>>>) -> Vec<StreamEvent> {
    let (tx, rx) = mpsc::channel(16);
    let stream = Scripted {
        reads: reads.into(),
    };
    let run = async move {
        let mut decoder = NdjsonSplitter::server();
        drive(stream, b"POST", &Timeouts::SERVER, &mut decoder, &tx).await;
    };
    tokio::join!(run, collect(rx)).1
}

#[tokio::test]
async fn a_reset_after_partial_text_is_an_interruption_with_the_text_kept() {
    let reset = std::io::Error::from(std::io::ErrorKind::ConnectionReset);
    let events = drive_scripted(vec![Ok(HEAD.to_vec()), Ok(chunk(HELLO)), Err(reset)]).await;
    assert_eq!(
        events,
        vec![delta("Hel"), StreamEvent::Error(ChatError::Interrupted)]
    );
}
