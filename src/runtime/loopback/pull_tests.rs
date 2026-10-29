//! The pull path of the loopback client (ollama-download), against the same
//! real loopback fake as the chat tests.

use std::time::Duration;

use tokio::sync::mpsc;

use super::tests::Fake;
use super::*;
use crate::core::ai::chat::stream::NdjsonSplitter;

const BODY: &[u8] = b"{\"model\":\"llama3.2:3b\",\"stream\":true}";

/// Runs one pull and returns how it ended and every progress reported.
async fn pull_to(host: &str, port: u16, path: &str) -> (Result<(), String>, Vec<(u64, u64)>) {
    let mut seen = Vec::new();
    let end = pull_with(
        &Timeouts::PULL,
        host,
        port,
        path,
        BODY,
        &mut |done, total| seen.push((done, total)),
    )
    .await;
    (end, seen)
}

/// Whether a chat turn to `path` got as far as connecting.
async fn chat_connects(path: &str) -> bool {
    let mut fake = Fake::start().await;
    fake.close();
    let (tx, mut rx) = mpsc::channel(16);
    let mut decoder = NdjsonSplitter::server();
    post_stream("127.0.0.1", fake.port, path, b"{}", &mut decoder, &tx).await;
    drop(tx);
    while rx.recv().await.is_some() {}
    tokio::time::sleep(Duration::from_millis(50)).await;
    fake.connections() == 1
}

// Spec (ai-engine-detection, Only the allowed POST paths).
#[tokio::test]
async fn only_the_allowed_post_paths() {
    let fake = Fake::start().await;
    for path in [
        "/api/delete",
        "/api/push",
        "/api/chat",
        "/api/pull/",
        "/api/pull?x=1",
    ] {
        let (end, seen) = pull_to("127.0.0.1", fake.port, path).await;
        assert_eq!(end, Err(PULL_REFUSED.to_owned()), "pull to {path:?}");
        assert!(seen.is_empty());
    }
    // A chat never posts to the pull path, nor to any other.
    let (tx, mut rx) = mpsc::channel(16);
    for path in ["/api/delete", "/api/push", PULL_PATH] {
        let mut decoder = NdjsonSplitter::server();
        post_stream("127.0.0.1", fake.port, path, b"{}", &mut decoder, &tx).await;
        let event = rx.recv().await;
        assert!(
            matches!(&event, Some(StreamEvent::Error(ChatError::Failed(m))) if m == REFUSED),
            "chat to {path:?}: {event:?}"
        );
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(fake.connections(), 0);
    // The chat paths and the pull path still go out.
    assert!(chat_connects("/api/chat").await);
    assert!(chat_connects("/v1/chat/completions").await);
    let mut pull = Fake::start().await;
    pull.close();
    let _ = pull_to("127.0.0.1", pull.port, PULL_PATH).await;
    assert_eq!(pull.connections(), 1);
}

// Spec (ai-engine-detection, The shared client stays loopback-only).
#[tokio::test]
async fn a_pull_to_a_foreign_host_is_refused_without_connecting() {
    let fake = Fake::start().await;
    for host in ["localhost", "0.0.0.0", "10.0.0.1", "::1", "127.0.0.1 "] {
        let (end, _) = pull_to(host, fake.port, PULL_PATH).await;
        assert_eq!(end, Err(PULL_REFUSED.to_owned()), "host {host:?}");
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(fake.connections(), 0);
}

// Spec (ollama-download, Pull through the local server).
#[tokio::test]
async fn a_pull_posts_its_body_with_no_credential_and_reads_progress() {
    let mut fake = Fake::start().await;
    fake.send(b"HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nTransfer-Encoding: chunked\r\n\r\n");
    for line in [
        "{\"status\":\"pulling manifest\"}\n",
        "{\"digest\":\"sha256:a\",\"total\":10,\"completed\":4}\n",
        "{\"digest\":\"sha256:a\",\"total\":10,\"completed\":10}\n",
        "{\"status\":\"success\"}\n",
    ] {
        fake.send(format!("{:x}\r\n{line}\r\n", line.len()).as_bytes());
    }
    fake.send(b"0\r\n\r\n");
    fake.close();
    let (end, seen) = pull_to("127.0.0.1", fake.port, PULL_PATH).await;
    assert_eq!(end, Ok(()));
    assert_eq!(seen, [(4, 10), (10, 10)]);
    let request = fake.request();
    assert!(
        request.starts_with(&format!(
            "POST /api/pull HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n",
            fake.port
        )),
        "{request}"
    );
    assert!(request.ends_with("\r\n\r\n{\"model\":\"llama3.2:3b\",\"stream\":true}"));
    let lower = request.to_lowercase();
    assert!(!lower.contains("authorization") && !lower.contains("cookie"));
}

#[tokio::test]
async fn a_non_200_pull_fails_with_the_status_and_the_error() {
    let mut fake = Fake::start().await;
    let body = "{\"error\":\"invalid model name\"}";
    fake.send(
        format!(
            "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    );
    fake.close();
    let (end, seen) = pull_to("127.0.0.1", fake.port, PULL_PATH).await;
    assert_eq!(end, Err("HTTP 400: invalid model name".to_owned()));
    assert!(seen.is_empty());
}

#[tokio::test]
async fn nothing_listening_cannot_connect() {
    // A port nothing listens on: bind one and let it go.
    let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let closed = free.local_addr().unwrap().port();
    drop(free);
    let (end, _) = pull_to("127.0.0.1", closed, PULL_PATH).await;
    assert_eq!(end, Err(PULL_CANNOT_CONNECT.to_owned()));
}

#[test]
fn a_pull_has_its_own_time_limits() {
    let pull = Timeouts::PULL;
    assert_eq!(pull.connect, Duration::from_secs(2));
    assert_eq!(pull.first_byte, Duration::from_secs(120));
    assert_eq!(pull.idle, Duration::from_secs(300));
    assert_eq!(pull.total, Duration::from_secs(24 * 60 * 60));
    // A chat turn keeps its own.
    assert_eq!(Timeouts::SERVER.total, Duration::from_secs(15 * 60));
}
