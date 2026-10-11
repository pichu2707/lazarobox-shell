//! Runtime probes for AI engine detection: loopback HTTP, PATH lookup and
//! `--version`. The facts are classified by the pure `core::ai::reduce`.

use std::{
    ffi::OsStr, fs, net::SocketAddr, os::unix::fs::PermissionsExt, path::PathBuf, time::Duration,
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};

use crate::core::ai::http::{MAX_BODY, parse_response};

/// Room for the status line and headers above the 1 MiB body cap.
const READ_SLACK: usize = 16 * 1024;

/// Covers connect, write and read of one loopback request.
pub const HTTP_TIMEOUT: Duration = Duration::from_millis(800);

/// GETs `path` from `127.0.0.1:port`. `None` on any failure or timeout.
pub async fn http_get(port: u16, path: &'static str) -> Option<(u16, Vec<u8>)> {
    timeout(HTTP_TIMEOUT, exchange(port, path)).await.ok()?
}

async fn exchange(port: u16, path: &str) -> Option<(u16, Vec<u8>)> {
    // HTTP/1.0 with `Connection: close`: the reply ends at EOF. The host is
    // fixed, so no request can leave the machine.
    let mut stream = TcpStream::connect(SocketAddr::from(([127, 0, 0, 1], port)))
        .await
        .ok()?;
    let request = format!(
        "GET {path} HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await.ok()?;
    // Headers fit well inside the slack; a longer reply is cut and rejected.
    let mut raw = Vec::new();
    (&mut stream)
        .take((MAX_BODY + READ_SLACK) as u64)
        .read_to_end(&mut raw)
        .await
        .ok()?;
    parse_response(&raw)
}

/// The first executable regular file called `name` in an absolute `path`
/// entry. Empty and relative entries are skipped.
pub fn find_on_path(name: &str, path: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(name))
        .find(|file| {
            fs::metadata(file)
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::Path,
        sync::{Arc, Mutex},
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    type Requests = Arc<Mutex<Vec<Vec<u8>>>>;

    /// A loopback server on an ephemeral port that records each request and
    /// answers it with `reply` (then closes). `None` reply: accept and stay
    /// silent.
    async fn fake_server(reply: Option<Vec<u8>>) -> (u16, Requests) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests: Requests = Arc::default();
        let seen = requests.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let (seen, reply) = (seen.clone(), reply.clone());
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 512];
                    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        match stream.read(&mut chunk).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                    }
                    seen.lock().unwrap().push(buf);
                    match reply {
                        Some(bytes) => {
                            let _ = stream.write_all(&bytes).await;
                            let _ = stream.shutdown().await;
                        }
                        None => std::future::pending::<()>().await,
                    }
                });
            }
        });
        (port, requests)
    }

    #[tokio::test]
    async fn http_get_returns_status_and_body() {
        let reply = b"HTTP/1.0 200 OK\r\nContent-Length: 12\r\n\r\n{\"ok\":true}\n".to_vec();
        let (port, _) = fake_server(Some(reply)).await;
        assert_eq!(
            http_get(port, "/api/tags").await,
            Some((200, b"{\"ok\":true}\n".to_vec()))
        );
    }

    #[tokio::test]
    async fn http_get_decodes_a_chunked_reply() {
        let reply =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n"
                .to_vec();
        let (port, _) = fake_server(Some(reply)).await;
        assert_eq!(
            http_get(port, "/v1/models").await,
            Some((200, b"abcde".to_vec()))
        );
    }

    #[tokio::test]
    async fn http_get_sends_exactly_the_documented_request() {
        let reply = b"HTTP/1.0 200 OK\r\nContent-Length: 0\r\n\r\n".to_vec();
        let (port, requests) = fake_server(Some(reply)).await;
        http_get(port, "/api/tags").await;
        let expected = format!(
            "GET /api/tags HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
        );
        assert_eq!(*requests.lock().unwrap(), vec![expected.into_bytes()]);
    }

    #[tokio::test]
    async fn http_get_gives_up_on_a_silent_server() {
        let (port, _) = fake_server(None).await;
        let answer =
            tokio::time::timeout(Duration::from_millis(1200), http_get(port, "/api/tags")).await;
        assert_eq!(answer, Ok(None));
    }

    #[tokio::test]
    async fn http_get_rejects_a_reply_over_the_cap() {
        let mut reply = b"HTTP/1.0 200 OK\r\n\r\n".to_vec();
        reply.resize(reply.len() + 1024 * 1024 + 1, b'x');
        let (port, _) = fake_server(Some(reply)).await;
        let answer =
            tokio::time::timeout(Duration::from_millis(1200), http_get(port, "/api/tags")).await;
        assert_eq!(answer, Ok(None));
    }

    #[tokio::test]
    async fn http_get_fails_fast_on_a_closed_port() {
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            listener.local_addr().unwrap().port()
        };
        let answer =
            tokio::time::timeout(Duration::from_millis(400), http_get(port, "/api/tags")).await;
        assert_eq!(answer, Ok(None));
    }

    #[tokio::test]
    async fn http_get_returns_a_redirect_without_following_it() {
        let reply =
            b"HTTP/1.0 301 Moved\r\nLocation: http://example.com/\r\nContent-Length: 0\r\n\r\n"
                .to_vec();
        let (port, requests) = fake_server(Some(reply)).await;
        assert_eq!(http_get(port, "/api/tags").await, Some((301, Vec::new())));
        assert_eq!(requests.lock().unwrap().len(), 1);
    }

    fn write_file(dir: &Path, name: &str, mode: u32) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    fn join(dirs: &[&Path]) -> std::ffi::OsString {
        std::env::join_paths(dirs).unwrap()
    }

    /// Serializes the tests that change the process working directory.
    static CWD: Mutex<()> = Mutex::new(());

    #[test]
    fn find_on_path_finds_an_executable_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = write_file(dir.path(), "codex", 0o755);
        assert_eq!(
            find_on_path("codex", &join(&[dir.path()])),
            Some(file.clone())
        );
        assert_eq!(find_on_path("gemini", &join(&[dir.path()])), None);
    }

    #[test]
    fn find_on_path_first_match_wins() {
        let (first, second) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        write_file(second.path(), "codex", 0o755);
        let winner = write_file(first.path(), "codex", 0o755);
        assert_eq!(
            find_on_path("codex", &join(&[first.path(), second.path()])),
            Some(winner)
        );
    }

    #[test]
    fn find_on_path_ignores_files_without_the_exec_bit() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), "codex", 0o644);
        assert_eq!(find_on_path("codex", &join(&[dir.path()])), None);
    }

    #[test]
    fn find_on_path_ignores_directories() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("codex")).unwrap();
        assert_eq!(find_on_path("codex", &join(&[dir.path()])), None);
    }

    #[test]
    fn find_on_path_skips_empty_and_relative_entries() {
        let _guard = CWD.lock().unwrap_or_else(|e| e.into_inner());
        let cwd = tempfile::tempdir().unwrap();
        write_file(cwd.path(), "codex", 0o755);
        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(cwd.path()).unwrap();
        let found = find_on_path("codex", OsStr::new(":.:bin:"));
        std::env::set_current_dir(previous).unwrap();
        assert_eq!(found, None);
    }
}
