//! The pull worker against a fake Ollama on loopback (ollama-download).

use std::time::Duration;

use tokio::sync::mpsc;

use super::*;
use crate::runtime::loopback::tests::Fake;

const HEAD: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nTransfer-Encoding: chunked\r\n\r\n";

fn chunk(line: &str) -> Vec<u8> {
    format!("{:x}\r\n{line}\n\r\n", line.len() + 1).into_bytes()
}

fn job() -> Pull {
    Pull {
        model: "llama3.2".to_owned(),
        tag: "3b".to_owned(),
        size: Some(2_019_392_628),
        license: "LLAMA 3.2 COMMUNITY LICENSE AGREEMENT".to_owned(),
    }
}

/// The progress events up to the end, and the end.
async fn until_end(rx: &mut mpsc::Receiver<AppEvent>) -> (Vec<(u64, u64)>, End) {
    let mut seen = Vec::new();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(10), rx.recv())
            .await
            .expect("the pull ends")
            .expect("the worker is alive");
        match event {
            AppEvent::DownloadProgress { done, total } => seen.push((done, total)),
            AppEvent::DownloadEnd(end) => return (seen, end),
            _ => panic!("not a download event"),
        }
    }
}

// Spec (ollama-download, Progress then success).
#[tokio::test]
async fn progress_rises_to_100_then_the_pull_is_done() {
    let mut fake = Fake::start().await;
    fake.send(HEAD);
    for line in [
        "{\"status\":\"pulling manifest\"}",
        "{\"status\":\"pulling a\",\"digest\":\"sha256:a\",\"total\":100,\"completed\":0}",
        "{\"status\":\"pulling a\",\"digest\":\"sha256:a\",\"total\":100,\"completed\":100}",
        "{\"status\":\"pulling b\",\"digest\":\"sha256:b\",\"total\":300,\"completed\":150}",
        "{\"status\":\"pulling b\",\"digest\":\"sha256:b\",\"total\":300,\"completed\":300}",
        "{\"status\":\"verifying sha256 digest\"}",
        "{\"status\":\"success\"}",
    ] {
        fake.send(&chunk(line));
    }
    fake.send(b"0\r\n\r\n");
    fake.close();
    let (tx, mut rx) = mpsc::channel(64);
    let mut ollama = Ollama::new(fake.port, tx);
    ollama.start(job());
    let (seen, end) = until_end(&mut rx).await;
    assert_eq!(seen, [(0, 100), (100, 100), (250, 400), (400, 400)]);
    assert_eq!(end, End::Done(PathBuf::new()));
    let request = fake.request();
    assert!(
        request.starts_with("POST /api/pull HTTP/1.1\r\n"),
        "{request}"
    );
    assert!(request.ends_with("{\"model\":\"llama3.2:3b\",\"stream\":true}"));
}

// Spec (ollama-download, Error line fails the pull).
#[tokio::test]
async fn an_error_line_fails_the_pull() {
    let mut fake = Fake::start().await;
    fake.send(HEAD);
    fake.send(&chunk(
        "{\"error\":\"pull model manifest: file does not exist\"}",
    ));
    fake.send(b"0\r\n\r\n");
    fake.close();
    let (tx, mut rx) = mpsc::channel(64);
    // Kept alive: dropping the worker cancels its pull.
    let mut ollama = Ollama::new(fake.port, tx);
    ollama.start(job());
    let (seen, end) = until_end(&mut rx).await;
    assert!(seen.is_empty());
    assert_eq!(
        end,
        End::Failed("failed: pull model manifest: file does not exist".to_owned())
    );
}

#[tokio::test]
async fn a_closed_stream_fails_the_pull() {
    let mut fake = Fake::start().await;
    fake.send(HEAD);
    fake.send(&chunk(
        "{\"digest\":\"sha256:a\",\"total\":10,\"completed\":5}",
    ));
    fake.close();
    let (tx, mut rx) = mpsc::channel(64);
    // Kept alive: dropping the worker cancels its pull.
    let mut ollama = Ollama::new(fake.port, tx);
    ollama.start(job());
    let (seen, end) = until_end(&mut rx).await;
    assert_eq!(seen, [(5, 10)]);
    assert!(
        matches!(&end, End::Failed(m) if m.starts_with("failed: ")),
        "{end:?}"
    );
}

// Spec (ollama-download, Cancel closes the request).
#[tokio::test]
async fn cancel_closes_the_connection() {
    let fake = Fake::start().await;
    fake.send(HEAD);
    fake.send(&chunk(
        "{\"digest\":\"sha256:a\",\"total\":10,\"completed\":5}",
    ));
    let (tx, mut rx) = mpsc::channel(64);
    let mut ollama = Ollama::new(fake.port, tx);
    ollama.start(job());
    let first = tokio::time::timeout(Duration::from_secs(10), rx.recv()).await;
    assert!(matches!(
        first,
        Ok(Some(AppEvent::DownloadProgress { done: 5, total: 10 }))
    ));
    ollama.cancel();
    tokio::time::timeout(Duration::from_secs(10), fake.closed.notified())
        .await
        .expect("the fake sees the connection closed");
    let (seen, end) = until_end(&mut rx).await;
    assert!(seen.is_empty());
    assert_eq!(end, End::Cancelled);
}

#[tokio::test]
async fn only_one_pull_runs_at_a_time() {
    let fake = Fake::start().await;
    fake.send(HEAD);
    let (tx, _rx) = mpsc::channel(64);
    let mut ollama = Ollama::new(fake.port, tx);
    ollama.start(job());
    fake.request_read.notified().await;
    ollama.start(job());
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(fake.connections(), 1);
}
