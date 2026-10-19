//! `request` tests. Wire behaviour runs against a real loopback listener on an
//! ephemeral port; every time limit runs on the paused tokio clock.

use std::sync::{Arc, Mutex};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    time::Instant,
};

use super::*;

type Requests = Arc<Mutex<Vec<Vec<u8>>>>;

/// Every call the pure builders can produce is one `request` accepts, so the
/// method table and the builders cannot drift apart.
#[test]
fn every_builder_call_is_allowed_by_the_route_table() {
    use crate::core::ai::memory::{
        kind::MemKind,
        port::Note,
        route::{self, Method as Verb},
    };
    const SID: &str = "lazarobox-0123456789abcdef0123456789abcdef";
    let note = Note {
        kind: MemKind::Decision,
        title: "t".into(),
        content: "c".into(),
    };
    let calls = [
        route::health(),
        route::project_current("/tmp/a b&c\r\nX: y"),
        route::context("my proj"),
        route::register_session(SID, "p", "/x").unwrap(),
        route::end_session(SID, "s").unwrap(),
        route::save(SID, "p", &note),
        route::delete(7, "p"),
    ];
    for verb in [Verb::Get, Verb::Post, Verb::Delete] {
        let expected = match verb {
            Verb::Get => Method::Get,
            Verb::Post => Method::Post,
            Verb::Delete => Method::Delete,
        };
        assert_eq!(Method::from(verb), expected);
    }
    for call in calls {
        let method = Method::from(call.method);
        assert!(allowed_target(&call.target), "{}", call.target);
        assert!(method_fits_route(method, &call.target), "{}", call.target);
    }
}

/// Accepts connections, records each request and answers with `reply`, then
/// closes. `None` reply: accept and stay silent.
async fn serve(reply: Option<Vec<u8>>) -> (u16, Requests) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let requests: Requests = Arc::default();
    let seen = requests.clone();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let (seen, reply) = (seen.clone(), reply.clone());
            tokio::spawn(async move {
                let mut raw = Vec::new();
                let mut buf = [0u8; 4096];
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
                    match stream.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => raw.extend_from_slice(&buf[..n]),
                    }
                };
                while raw.len() < want {
                    match stream.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => raw.extend_from_slice(&buf[..n]),
                    }
                }
                seen.lock().unwrap().push(raw);
                match reply {
                    Some(reply) => {
                        let _ = stream.write_all(&reply).await;
                        let _ = stream.shutdown().await;
                    }
                    None => std::future::pending::<()>().await,
                }
            });
        }
    });
    (port, requests)
}

fn last(requests: &Requests) -> String {
    let requests = requests.lock().unwrap();
    String::from_utf8(requests.last().expect("no request").clone()).unwrap()
}

fn none_seen(requests: &Requests) -> bool {
    requests.lock().unwrap().is_empty()
}

fn ok_json(body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

async fn get(port: u16, target: &str) -> Result<Reply, CallError> {
    request(port, Method::Get, target, None, None, Limits::READ).await
}

// Request bytes ---------------------------------------------------------------

#[tokio::test]
async fn get_sends_exactly_the_documented_bytes() {
    let (port, seen) = serve(Some(ok_json("{}"))).await;
    get(port, "/health").await.unwrap();
    assert_eq!(
        last(&seen),
        format!(
            "GET /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
        )
    );
}

#[tokio::test]
async fn post_sends_content_type_and_length_with_the_body() {
    let (port, seen) = serve(Some(ok_json("{}"))).await;
    let body = br#"{"a":1}"#;
    let target = "/observations";
    request(port, Method::Post, target, Some(body), None, Limits::WRITE)
        .await
        .unwrap();
    assert_eq!(
        last(&seen),
        format!(
            "POST /observations HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: 7\r\nConnection: close\r\n\r\n{{\"a\":1}}"
        )
    );
}

#[tokio::test]
async fn a_post_without_a_body_has_no_content_headers() {
    let (port, seen) = serve(Some(ok_json("{}"))).await;
    let target = "/sessions/x/end";
    request(port, Method::Post, target, None, None, Limits::WRITE)
        .await
        .unwrap();
    let sent = last(&seen);
    assert!(!sent.contains("Content-Type") && !sent.contains("Content-Length"));
}

#[tokio::test]
async fn delete_sends_the_bearer_token_and_never_a_cookie() {
    let (port, seen) = serve(Some(ok_json("{}"))).await;
    let target = "/observations/7?expected_project=p";
    let got = request(
        port,
        Method::Delete,
        target,
        None,
        Some("tok-123"),
        Limits::WRITE,
    )
    .await;
    got.unwrap();
    assert_eq!(
        last(&seen),
        format!(
            "DELETE /observations/7?expected_project=p HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\nAuthorization: Bearer tok-123\r\nConnection: close\r\n\r\n"
        )
    );
    assert!(!last(&seen).contains("Cookie"));
}

#[tokio::test]
async fn no_authorization_header_without_a_token() {
    let (port, seen) = serve(Some(ok_json("{}"))).await;
    get(port, "/context?project=p").await.unwrap();
    assert!(!last(&seen).contains("Authorization"));
}

// Statuses --------------------------------------------------------------------

#[tokio::test]
async fn statuses_200_to_299_and_400_to_599_come_back_with_their_body() {
    for status in [200u16, 201, 204, 400, 404, 409, 500, 599] {
        let body = format!("{{\"s\":{status}}}");
        let reply = format!(
            "HTTP/1.1 {status} X\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let (port, _) = serve(Some(reply.into_bytes())).await;
        let got = get(port, "/health").await;
        let want = Reply {
            status,
            body: body.into_bytes(),
        };
        assert_eq!(got, Ok(want));
    }
}

#[tokio::test]
async fn a_redirect_is_an_error_and_its_location_is_never_requested() {
    let (target_port, target_seen) = serve(Some(ok_json("{}"))).await;
    let reply = format!(
        "HTTP/1.1 307 Temporary Redirect\r\nLocation: http://127.0.0.1:{target_port}/health\r\nContent-Length: 0\r\n\r\n"
    );
    let (port, _) = serve(Some(reply.into_bytes())).await;
    assert_eq!(get(port, "/health").await, Err(CallError::Invalid));
    tokio::task::yield_now().await;
    assert!(none_seen(&target_seen));
}

#[tokio::test]
async fn an_informational_status_is_invalid() {
    let reply = b"HTTP/1.1 100 Continue\r\nContent-Length: 0\r\n\r\n";
    let (port, _) = serve(Some(reply.to_vec())).await;
    assert_eq!(get(port, "/health").await, Err(CallError::Invalid));
}

// Framing and cap -------------------------------------------------------------

#[tokio::test]
async fn a_chunked_reply_is_decoded() {
    let reply =
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n";
    let (port, _) = serve(Some(reply.to_vec())).await;
    let got = get(port, "/health").await.unwrap();
    assert_eq!(got.body, b"abcde");
}

#[tokio::test]
async fn a_close_delimited_reply_is_read_to_the_end() {
    let (port, _) = serve(Some(b"HTTP/1.1 200 OK\r\n\r\nrest".to_vec())).await;
    assert_eq!(get(port, "/health").await.unwrap().body, b"rest");
}

#[tokio::test]
async fn content_length_with_transfer_encoding_is_rejected() {
    let reply = b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n0\r\n\r\n";
    let (port, _) = serve(Some(reply.to_vec())).await;
    assert_eq!(get(port, "/health").await, Err(CallError::Invalid));
}

#[tokio::test]
async fn conflicting_or_signed_content_length_is_rejected() {
    for head in [
        "Content-Length: 1\r\nContent-Length: 2\r\n",
        "Content-Length: +1\r\n",
    ] {
        let reply = format!("HTTP/1.1 200 OK\r\n{head}\r\nab");
        let (port, _) = serve(Some(reply.into_bytes())).await;
        let got = get(port, "/health").await;
        assert_eq!(got, Err(CallError::Invalid), "{head}");
    }
}

#[tokio::test]
async fn malformed_chunking_is_rejected() {
    let reply = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\nabc\r\n0\r\n\r\n";
    let (port, _) = serve(Some(reply.to_vec())).await;
    assert_eq!(get(port, "/health").await, Err(CallError::Invalid));
}

#[tokio::test]
async fn a_malformed_head_is_invalid() {
    for raw in [
        &b"not http at all\r\n\r\n"[..],
        b"HTTP/1.1 abc OK\r\n\r\n",
        b"",
        b"HTTP/1.1 200 OK\r\nContent-Le",
    ] {
        let (port, _) = serve(Some(raw.to_vec())).await;
        let got = get(port, "/health").await;
        assert_eq!(got, Err(CallError::Invalid), "{raw:?}");
    }
}

#[tokio::test]
async fn a_truncated_body_is_invalid() {
    let short = b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nabc";
    let (port, _) = serve(Some(short.to_vec())).await;
    assert_eq!(get(port, "/health").await, Err(CallError::Invalid));
    let cut = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nabc";
    let (port, _) = serve(Some(cut.to_vec())).await;
    assert_eq!(get(port, "/health").await, Err(CallError::Invalid));
}

fn length_reply(size: usize) -> Vec<u8> {
    let mut reply = format!("HTTP/1.1 200 OK\r\nContent-Length: {size}\r\n\r\n").into_bytes();
    reply.extend(std::iter::repeat_n(b'a', size));
    reply
}

fn chunked_reply(size: usize) -> Vec<u8> {
    let mut reply = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
    let half = size / 2;
    for n in [half, size - half] {
        reply.extend_from_slice(format!("{n:x}\r\n").as_bytes());
        reply.extend(std::iter::repeat_n(b'a', n));
        reply.extend_from_slice(b"\r\n");
    }
    reply.extend_from_slice(b"0\r\n\r\n");
    reply
}

#[tokio::test]
async fn a_body_of_exactly_256_kib_is_accepted_in_both_framings() {
    for reply in [
        length_reply(REQUEST_BODY_CAP),
        chunked_reply(REQUEST_BODY_CAP),
    ] {
        let (port, _) = serve(Some(reply)).await;
        let got = request(port, Method::Get, "/context", None, None, Limits::WRITE).await;
        assert_eq!(got.unwrap().body.len(), REQUEST_BODY_CAP);
    }
}

#[tokio::test]
async fn one_byte_over_256_kib_is_too_large_in_both_framings() {
    for reply in [
        length_reply(REQUEST_BODY_CAP + 1),
        chunked_reply(REQUEST_BODY_CAP + 1),
    ] {
        let (port, _) = serve(Some(reply)).await;
        let got = request(port, Method::Get, "/context", None, None, Limits::WRITE).await;
        assert_eq!(got, Err(CallError::TooLarge));
    }
}

#[tokio::test]
async fn a_close_delimited_body_over_the_cap_is_too_large() {
    let mut reply = b"HTTP/1.1 200 OK\r\n\r\n".to_vec();
    reply.extend(std::iter::repeat_n(b'a', REQUEST_BODY_CAP + 1));
    let (port, _) = serve(Some(reply)).await;
    let got = request(port, Method::Get, "/context", None, None, Limits::WRITE).await;
    assert_eq!(got, Err(CallError::TooLarge));
}

// Refusals --------------------------------------------------------------------

#[tokio::test]
async fn a_foreign_host_makes_no_connection() {
    let (port, seen) = serve(Some(ok_json("{}"))).await;
    for host in [
        "localhost",
        "0.0.0.0",
        "::1",
        "example.com",
        "127.0.0.2",
        "",
    ] {
        let got = request_to(host, port, Method::Get, "/health", None, None, Limits::READ).await;
        assert_eq!(got, Err(CallError::Refused), "{host}");
    }
    tokio::task::yield_now().await;
    assert!(none_seen(&seen));
}

#[tokio::test]
async fn targets_outside_the_allowlist_or_with_bad_bytes_are_refused() {
    let (port, seen) = serve(Some(ok_json("{}"))).await;
    for target in [
        "/",
        "/admin",
        "/healthz",
        "/sessionsx",
        "/health/../admin",
        "health",
        "http://evil/health",
        "//evil/health",
        "/health x",
        "/health\r\nX: y",
        "/health\n",
        "/health\r",
        "/health\t",
        "/health\0",
        "/h\u{e9}alth",
        "/context?q=\u{e9}",
        "",
    ] {
        let got = get(port, target).await;
        assert_eq!(got, Err(CallError::Refused), "{target:?}");
    }
    tokio::task::yield_now().await;
    assert!(none_seen(&seen));
}

#[tokio::test]
async fn dot_segments_encoded_separators_and_fragments_in_the_path_are_refused() {
    let (port, seen) = serve(Some(ok_json("{}"))).await;
    for target in [
        "/observations/%2e%2e/export",
        "/observations/%2E%2E/export",
        "/observations/.%2e/export",
        "/observations/%2e./export",
        "/observations/./7",
        "/observations/%2e/7",
        "/observations/..",
        "/observations/%2e%2e",
        "/observations%2f7",
        "/observations/..%2fexport",
        "/observations/%2F7",
        "/observations%5c7",
        "/observations/%5C7",
        "/observations\\7",
        "/health#x",
        "/context?project=p#x",
    ] {
        assert!(!allowed_target(target), "{target:?}");
        let got = request(port, Method::Get, target, None, None, Limits::READ).await;
        assert_eq!(got, Err(CallError::Refused), "{target:?}");
    }
    tokio::task::yield_now().await;
    assert!(none_seen(&seen));
}

#[tokio::test]
async fn an_encoded_dot_dot_in_the_query_still_passes() {
    let (port, _) = serve(Some(ok_json("{}"))).await;
    assert!(
        get(port, "/context?cwd=%2Ftmp%2F..%2Fx&a=%2e%2e")
            .await
            .is_ok()
    );
}

async fn send(port: u16, method: Method, target: &str) -> Result<Reply, CallError> {
    let body = (method == Method::Post).then_some(&b"{}"[..]);
    let auth = (method == Method::Delete).then_some("tok");
    request(port, method, target, body, auth, Limits::WRITE).await
}

#[tokio::test]
async fn every_engram_route_is_allowed_with_its_own_method() {
    let (port, _) = serve(Some(ok_json("{}"))).await;
    for (method, target) in [
        (Method::Get, "/health"),
        (Method::Get, "/project/current?cwd=%2Ftmp"),
        (Method::Get, "/context?project=p&scope=project"),
        (Method::Get, "/context?cwd=%2Ftmp%2F..%2Fx"),
        (Method::Get, "/search?q=x"),
        (Method::Post, "/sessions"),
        (Method::Post, "/sessions/lazarobox-ab/end"),
        (Method::Post, "/observations"),
        (Method::Delete, "/observations/7?expected_project=p"),
    ] {
        assert!(
            send(port, method, target).await.is_ok(),
            "{method:?} {target}"
        );
    }
}

#[tokio::test]
async fn a_method_on_a_route_it_does_not_belong_to_is_refused_before_connecting() {
    let (port, seen) = serve(Some(ok_json("{}"))).await;
    for (method, target) in [
        (Method::Delete, "/sessions"),
        (Method::Delete, "/sessions/x/end"),
        (Method::Delete, "/observations"),
        (Method::Delete, "/observations/"),
        (Method::Delete, "/observations/7/extra"),
        (Method::Delete, "/context"),
        (Method::Delete, "/health"),
        (Method::Delete, "/search"),
        (Method::Post, "/health"),
        (Method::Post, "/context"),
        (Method::Post, "/search"),
        (Method::Post, "/project/current"),
        (Method::Post, "/observations/7"),
        (Method::Post, "/sessions/x"),
        (Method::Post, "/sessions//end"),
        (Method::Post, "/sessions/x/end/more"),
        (Method::Get, "/sessions"),
        (Method::Get, "/sessions/x/end"),
        (Method::Get, "/observations"),
        (Method::Get, "/observations/7"),
    ] {
        let got = send(port, method, target).await;
        assert_eq!(got, Err(CallError::Refused), "{method:?} {target}");
    }
    tokio::task::yield_now().await;
    assert!(none_seen(&seen));
}

#[tokio::test]
async fn a_token_is_refused_off_delete_and_when_it_is_not_a_header_value() {
    let (port, seen) = serve(Some(ok_json("{}"))).await;
    for method in [Method::Get, Method::Post] {
        let got = request(port, method, "/health", None, Some("tok"), Limits::READ).await;
        assert_eq!(got, Err(CallError::Refused), "{method:?}");
    }
    let target = "/observations/1?expected_project=p";
    for token in ["a\r\nCookie: x", "a b", "", "t\u{e9}"] {
        let got = request(
            port,
            Method::Delete,
            target,
            None,
            Some(token),
            Limits::WRITE,
        )
        .await;
        assert_eq!(got, Err(CallError::Refused), "{token:?}");
    }
    tokio::task::yield_now().await;
    assert!(none_seen(&seen));
}

#[tokio::test]
async fn a_body_is_only_allowed_on_post() {
    let (port, seen) = serve(Some(ok_json("{}"))).await;
    for method in [Method::Get, Method::Delete] {
        let body = Some(&b"{}"[..]);
        let got = request(port, method, "/observations/1", body, None, Limits::WRITE).await;
        assert_eq!(got, Err(CallError::Refused), "{method:?}");
    }
    tokio::task::yield_now().await;
    assert!(none_seen(&seen));
}

#[tokio::test]
async fn a_request_body_over_the_cap_is_too_large_and_sends_nothing() {
    let (port, seen) = serve(Some(ok_json("{}"))).await;
    let big = vec![b'a'; REQUEST_BODY_CAP + 1];
    let got = request(
        port,
        Method::Post,
        "/observations",
        Some(&big),
        None,
        Limits::WRITE,
    )
    .await;
    assert_eq!(got, Err(CallError::TooLarge));
    tokio::task::yield_now().await;
    assert!(none_seen(&seen));
}

#[tokio::test]
async fn an_early_reply_survives_a_server_that_closes_before_the_body_is_sent() {
    for (status, text) in [(413u16, "Payload Too Large"), (401, "Unauthorized")] {
        let (client, mut server) = tokio::io::duplex(64);
        tokio::spawn(async move {
            let reply = format!("HTTP/1.1 {status} {text}\r\nContent-Length: 2\r\n\r\nno");
            let _ = server.write_all(reply.as_bytes()).await;
            // Dropped here: the request body is never read.
        });
        let got = exchange(client, &vec![b'a'; 4096]).await;
        let reply = got.unwrap_or_else(|e| panic!("{status}: {e:?}"));
        assert_eq!((reply.status, reply.body), (status, b"no".to_vec()));
    }
}

#[tokio::test]
async fn a_failed_write_with_no_reply_is_invalid() {
    let (client, server) = tokio::io::duplex(64);
    drop(server);
    assert_eq!(
        exchange(client, &vec![b'a'; 4096]).await,
        Err(CallError::Invalid)
    );
}

// Connection and time ---------------------------------------------------------

#[tokio::test]
async fn a_refused_connection_is_connect() {
    let refused = crate::runtime::refused_port::RefusedPort::new();
    assert_eq!(
        get(refused.port(), "/health").await,
        Err(CallError::Connect)
    );
}

// WSL's mirrored networking drops a connect to a closed loopback port instead
// of refusing it, so a timed-out connect is told apart by whether anything
// holds the port. Elsewhere the connect is refused at once: same answer.
#[tokio::test]
async fn a_closed_port_is_connect_whether_refused_or_dropped() {
    let port = untouched_free_port();
    assert_eq!(get(port, "/health").await, Err(CallError::Connect));
}

/// A port that no socket holds, found without binding it on Linux: on WSL a
/// bind makes the port Linux's for a while and later connects to it are
/// refused instead of dropped, which would hide the case under test. The start
/// is random so an earlier run's port is not picked again, and below WSL's
/// ephemeral range, which it also refuses.
fn untouched_free_port() -> u16 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    let start = 20_000 + (nanos % 20_000) as u16;
    let free = |port: &u16| {
        if cfg!(target_os = "linux") {
            !["/proc/net/tcp", "/proc/net/tcp6"]
                .iter()
                .filter_map(|table| std::fs::read_to_string(table).ok())
                .any(|table| table.contains(&format!(":{port:04X} ")))
        } else {
            std::net::TcpListener::bind(("127.0.0.1", *port)).is_ok()
        }
    };
    (start..start + 100).find(free).expect("a free port")
}

#[test]
fn a_port_nothing_holds_is_free() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    assert!(port_is_free(addr));
}

#[test]
fn a_port_with_a_listener_or_a_bound_socket_is_not_free() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    assert!(!port_is_free(listener.local_addr().unwrap()));
    // A socket that is bound and never listens holds the port on every
    // system (`RefusedPort` closes its socket outside Linux).
    let bound = tokio::net::TcpSocket::new_v4().unwrap();
    bound.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    assert!(!port_is_free(SocketAddr::from((
        [127, 0, 0, 1],
        bound.local_addr().unwrap().port()
    ))));
}

// A server whose accept queue is full drops new connects, so the connect
// times out while the port is held: that is a server that did not answer,
// never one that is not running. Linux only: it drops the SYN of a full
// queue, which is what makes the connect hang.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_connect_that_times_out_on_a_held_port_stays_a_timeout() {
    let socket = tokio::net::TcpSocket::new_v4().unwrap();
    socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let listener = socket.listen(0).unwrap();
    let addr = listener.local_addr().unwrap();
    // Fill the queue; the listener never accepts.
    let mut queued = Vec::new();
    while let Ok(Ok(stream)) =
        tokio::time::timeout(Duration::from_millis(100), TcpStream::connect(addr)).await
    {
        queued.push(stream);
        assert!(queued.len() < 64, "the accept queue never filled");
    }
    assert_eq!(get(addr.port(), "/health").await, Err(CallError::Timeout));
}

// Real clock for the same reason as the trickle test below: on a paused clock
// a connect that macOS completes a moment late lets the clock jump to the
// 200 ms connect limit. The window is wide enough for a slow runner and still
// tells each total limit apart from the next.
#[tokio::test]
async fn a_silent_server_times_out_at_each_total_limit() {
    let (port, _) = serve(None).await;
    for (limits, ms) in [
        (Limits::HEALTH, 300u64),
        (Limits::READ, 800),
        (Limits::WRITE, 2000),
    ] {
        let total = Duration::from_millis(ms);
        let start = Instant::now();
        let got = tokio::time::timeout(
            total + Duration::from_secs(5),
            request(port, Method::Get, "/health", None, None, limits),
        )
        .await
        .expect("the total limit ends a silent reply");
        let elapsed = start.elapsed();
        assert_eq!(got, Err(CallError::Timeout));
        assert!(elapsed >= total, "{ms} ms: {elapsed:?}");
        assert!(
            elapsed < total + Duration::from_secs(2),
            "{ms} ms: {elapsed:?}"
        );
    }
}

// Real clock on purpose: on a paused clock the runtime goes idle while the
// connect is still in flight (macOS completes it a moment later than Linux),
// the clock jumps past the 200 ms connect limit and the request times out
// there, not at the total limit under test. A bound keeps a regression from
// hanging: a total limit that the trickle kept resetting would never end.
#[tokio::test]
async fn a_server_that_trickles_past_the_total_limit_times_out() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf).await;
        let head = b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n";
        let _ = stream.write_all(head).await;
        loop {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if stream.write_all(b"a").await.is_err() {
                return;
            }
        }
    });
    let start = Instant::now();
    let got = tokio::time::timeout(
        Duration::from_secs(5),
        request(port, Method::Get, "/health", None, None, Limits::READ),
    )
    .await
    .expect("the total limit ends a trickling reply");
    assert_eq!(got, Err(CallError::Timeout));
    assert!(
        start.elapsed() >= Limits::READ.total,
        "{:?}",
        start.elapsed()
    );
}
