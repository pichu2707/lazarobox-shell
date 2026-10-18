//! A fake engram server for tests: a real loopback listener that replays the
//! recorded 3.4.0 responses by route and records every request it gets.

// What only the Linux-only tests use is dead code elsewhere.
#![cfg_attr(not(target_os = "linux"), allow(dead_code, unused_imports))]

use std::sync::{Arc, Mutex};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};

/// One request the fake received.
#[derive(Clone, Debug)]
pub struct Recorded {
    pub method: String,
    pub target: String,
    /// Header names lowercased.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Recorded {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// The JSON body, when there is one.
    pub fn json(&self) -> Option<serde_json::Value> {
        serde_json::from_str(&self.body).ok()
    }
}

/// A raw recorded response by name, as bytes.
macro_rules! raw {
    ($name:literal) => {
        &include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/core/ai/memory/fixtures/3.4.0/",
            $name,
            ".http"
        ))[..]
    };
}

pub struct FakeEngram {
    pub port: u16,
    requests: Arc<Mutex<Vec<Recorded>>>,
    task: JoinHandle<()>,
}

impl Drop for FakeEngram {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl FakeEngram {
    /// Serves `routes`: a `METHOD target-prefix` and the raw response to send.
    /// The first matching route wins; anything else gets a 404.
    pub async fn start(routes: Vec<(&'static str, &'static [u8])>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests: Arc<Mutex<Vec<Recorded>>> = Arc::default();
        let log = requests.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let Some(request) = read_request(&mut stream).await else {
                    continue;
                };
                let key = format!("{} {}", request.method, request.target);
                log.lock().unwrap().push(request);
                let answer = routes
                    .iter()
                    .find(|(prefix, _)| key.starts_with(prefix))
                    .map_or(NOT_FOUND, |(_, raw)| *raw);
                let _ = stream.write_all(answer).await;
                let _ = stream.shutdown().await;
            }
        });
        Self {
            port,
            requests,
            task,
        }
    }

    pub fn requests(&self) -> Vec<Recorded> {
        self.requests.lock().unwrap().clone()
    }
}

const NOT_FOUND: &[u8] =
    b"HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}";

async fn read_request(stream: &mut tokio::net::TcpStream) -> Option<Recorded> {
    let mut data = Vec::new();
    let mut chunk = [0u8; 4096];
    let end = loop {
        if let Some(at) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break at;
        }
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        data.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&data[..end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_string();
    let target = first.next()?.to_string();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let length: usize = headers
        .iter()
        .find(|(k, _)| k == "content-length")
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    let mut body = data[end + 4..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    Some(Recorded {
        method,
        target,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::sync::mpsc;

    use super::*;
    use crate::{
        core::{
            ai::memory::{
                kind::MemKind,
                port::{MemCmd, MemReply, Note, Reason, SaveId},
            },
            layout::PaneId,
        },
        runtime::engram::worker::{Loopback, MemoryLink, spawn},
    };

    const TOKEN: &str = "tok-ENGRAM-HTTP-9f3";
    const SECRET: &str = "SECRET-NOTE-456";

    fn happy() -> Vec<(&'static str, &'static [u8])> {
        vec![
            ("GET /health", raw!("health_3_4_0")),
            ("GET /project/current", raw!("project_200")),
            ("GET /context", raw!("context_compact")),
            ("POST /sessions/", raw!("session_end_200")),
            ("POST /sessions", raw!("session_created")),
            ("POST /observations", raw!("observation_saved_201")),
            ("DELETE /observations", raw!("delete_200")),
        ]
    }

    fn note() -> Note {
        Note {
            kind: MemKind::Decision,
            title: "Use Axum".into(),
            content: format!("Chose Axum. {SECRET}"),
        }
    }

    fn pane() -> PaneId {
        PaneId::FIRST
    }

    async fn start(
        routes: Vec<(&'static str, &'static [u8])>,
    ) -> (FakeEngram, MemoryLink, mpsc::Receiver<MemReply>) {
        let fake = FakeEngram::start(routes).await;
        let (tx, rx) = mpsc::channel(64);
        let transport = Loopback::new(fake.port, Some(TOKEN.into()));
        assert!(transport.token_present());
        let link = spawn(transport, fake.port, tx);
        (fake, link, rx)
    }

    async fn replies(rx: &mut mpsc::Receiver<MemReply>, count: usize) -> Vec<MemReply> {
        let mut out = Vec::new();
        while out.len() < count {
            let next = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await;
            out.push(next.expect("a reply in time").expect("the worker is alive"));
        }
        out
    }

    // Needs `getrandom(2)` for the session id, Linux only.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_conversation_sends_the_right_requests() {
        let (fake, link, mut rx) = start(happy()).await;
        link.send(MemCmd::Resolve {
            pane: pane(),
            cwd: "/tmp/a b&c".into(),
        });
        link.send(MemCmd::Save {
            pane: pane(),
            id: SaveId(1),
            session: None,
            project: "widget".into(),
            cwd: "/tmp/a b&c".into(),
            note: note(),
        });
        link.send(MemCmd::Context {
            pane: pane(),
            project: "widget".into(),
        });
        link.send(MemCmd::Delete {
            pane: pane(),
            notes: vec![("widget".into(), 7)],
        });
        // Health, project, saved, context, deleted.
        let got = replies(&mut rx, 5).await;
        let session = got
            .iter()
            .find_map(|r| match r {
                MemReply::Saved { session, .. } => Some(session.clone()),
                _ => None,
            })
            .expect("saved");
        link.send(MemCmd::End {
            session: session.clone(),
            project: "widget".into(),
            summary: "Decided Axum.".into(),
            cwd: None,
        });
        link.shutdown(Duration::from_secs(1)).await;

        let seen = fake.requests();
        let keys: Vec<String> = seen
            .iter()
            .map(|r| format!("{} {}", r.method, r.target))
            .collect();
        assert_eq!(keys[0], "GET /health");
        assert_eq!(keys[1], "GET /project/current?cwd=%2Ftmp%2Fa%20b%26c");
        let sessions = keys.iter().position(|k| k == "POST /sessions").unwrap();
        let observations = keys.iter().position(|k| k == "POST /observations").unwrap();
        assert!(sessions < observations, "the session comes before the save");
        for request in &seen {
            let first = request.target.split('?').next().unwrap();
            let auth = request.header("authorization");
            if request.method == "DELETE" {
                assert_eq!(auth, Some(&format!("Bearer {TOKEN}")[..]));
            } else {
                assert_eq!(auth, None, "{} {}", request.method, request.target);
            }
            // Every call that works on a project names it.
            match first {
                "/context" => assert!(request.target.contains("project=widget")),
                p if p.starts_with("/observations/") => {
                    assert!(request.target.contains("expected_project=widget"));
                }
                "/sessions" | "/observations" => {
                    assert_eq!(request.json().unwrap()["project"], "widget");
                }
                _ => {}
            }
        }
        let end = seen
            .iter()
            .find(|r| r.target == format!("/sessions/{session}/end"))
            .expect("the session was ended");
        assert_eq!(end.json().unwrap()["summary"], "Decided Axum.");
    }

    #[tokio::test]
    async fn the_token_and_the_note_never_reach_an_error_text() {
        let routes = vec![
            ("GET /health", raw!("health_3_4_0")),
            ("POST /sessions", raw!("session_created")),
            (
                "POST /observations",
                raw!("observation_400_session_project_mismatch"),
            ),
            ("DELETE /observations", raw!("delete_401_wrong_token")),
        ];
        let (_fake, link, mut rx) = start(routes).await;
        link.send(MemCmd::Save {
            pane: pane(),
            id: SaveId(1),
            session: None,
            project: "widget".into(),
            cwd: "/tmp/a b&c".into(),
            note: note(),
        });
        link.send(MemCmd::Delete {
            pane: pane(),
            notes: vec![("widget".into(), 1)],
        });
        let got = replies(&mut rx, 3).await;
        assert!(got.iter().any(|r| matches!(r, MemReply::SaveFailed { .. })));
        assert!(
            got.iter()
                .any(|r| matches!(r, MemReply::Deleted { refused: true, .. }))
        );
        let text = format!("{got:?}");
        assert!(!text.contains(TOKEN), "{text}");
        assert!(!text.contains(SECRET), "{text}");
    }

    #[tokio::test]
    async fn a_refused_port_is_not_running_and_leaks_nothing() {
        let refused = crate::runtime::refused_port::RefusedPort::new();
        let port = refused.port();
        let (tx, mut rx) = mpsc::channel(8);
        let link = spawn(Loopback::new(port, Some(TOKEN.into())), port, tx);
        link.send(MemCmd::Save {
            pane: pane(),
            id: SaveId(1),
            session: None,
            project: "widget".into(),
            cwd: "/tmp/a b&c".into(),
            note: note(),
        });
        let got = replies(&mut rx, 2).await;
        assert!(matches!(
            got[1],
            MemReply::SaveUnavailable {
                reason: Reason::NotRunning,
                ..
            }
        ));
        let text = format!("{got:?}");
        assert!(!text.contains(TOKEN) && !text.contains(SECRET), "{text}");
    }

    #[tokio::test]
    async fn an_old_server_is_never_sent_anything_else() {
        let routes = vec![("GET /health", raw!("health_1_20"))];
        let (fake, link, mut rx) = start(routes).await;
        link.send(MemCmd::Resolve {
            pane: pane(),
            cwd: "/tmp".into(),
        });
        let got = replies(&mut rx, 2).await;
        assert!(matches!(
            got[1],
            MemReply::Unavailable {
                reason: Reason::OldServer { .. },
                ..
            }
        ));
        assert_eq!(fake.requests().len(), 1);
    }
}
