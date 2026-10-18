use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, BufReader, DuplexStream, duplex};

use super::*;
use crate::core::ai::tools::{
    mcp::{Frame, MAX_LINE_BYTES, PROTOCOL_VERSION},
    registry,
    servers::qualify,
};
use crate::runtime::mcp::session::{Line, read_line};

const WAIT: Duration = Duration::from_secs(5);

fn timeouts() -> Timeouts {
    Timeouts {
        handshake: WAIT,
        call: WAIT,
    }
}

/// The server's end of the pipe pair: it speaks the same `Frame` codec.
struct Fake {
    rx: BufReader<DuplexStream>,
    tx: DuplexStream,
}

impl Fake {
    async fn recv(&mut self) -> Frame {
        match tokio::time::timeout(WAIT, read_line(&mut self.rx, MAX_LINE_BYTES))
            .await
            .expect("the client sends within the bound")
        {
            Ok(Line::Text(text)) => Frame::parse(&text).expect("a frame"),
            other => panic!("expected a line, got {other:?}"),
        }
    }

    async fn send(&mut self, frame: Frame) {
        let line = format!("{}\n", frame.to_line());
        self.tx.write_all(line.as_bytes()).await.unwrap();
    }

    async fn request(&mut self, method: &str) -> (Value, Value) {
        match self.recv().await {
            Frame::Request {
                id,
                method: got,
                params,
            } if got == method => (id, params),
            other => panic!("expected request {method}, got {other:?}"),
        }
    }

    async fn reply(&mut self, id: Value, result: Value) {
        self.send(Frame::Response { id, result }).await;
    }

    /// Serves the handshake and returns what `initialize` carried.
    async fn handshake(&mut self) -> Value {
        let (id, params) = self.request("initialize").await;
        self.reply(
            id,
            json!({"protocolVersion": PROTOCOL_VERSION, "capabilities": {}, "serverInfo": {"name": "fake"}}),
        )
        .await;
        match self.recv().await {
            Frame::Notification { method, .. } => assert_eq!(method, "notifications/initialized"),
            other => panic!("expected the initialized notification, got {other:?}"),
        }
        params
    }

    async fn answer_list(&mut self, tools: Value) {
        let (id, _) = self.request("tools/list").await;
        self.reply(id, json!({ "tools": tools })).await;
    }
}

fn pair(timeouts: Timeouts) -> (McpClient, Fake) {
    let (client_tx, server_rx) = duplex(1 << 21);
    let (server_tx, client_rx) = duplex(1 << 21);
    let client = McpClient::over(client_rx, client_tx, timeouts);
    let fake = Fake {
        rx: BufReader::new(server_rx),
        tx: server_tx,
    };
    (client, fake)
}

fn never() -> std::future::Pending<()> {
    std::future::pending()
}

#[tokio::test]
async fn the_handshake_asks_for_the_pinned_revision_then_announces_itself() {
    let (mut client, mut fake) = pair(timeouts());
    let (done, params) = tokio::join!(client.handshake(), fake.handshake());
    done.expect("the handshake succeeds");
    assert_eq!(params["protocolVersion"], PROTOCOL_VERSION);
    assert_eq!(params["clientInfo"]["name"], "lazarobox");
}

#[tokio::test]
async fn a_server_that_never_answers_the_handshake_times_out() {
    let (mut client, mut fake) = pair(Timeouts {
        handshake: Duration::from_millis(100),
        call: WAIT,
    });
    let (done, _) = tokio::join!(client.handshake(), fake.request("initialize"));
    assert_eq!(done, Err(ClientError::Timeout));
}

#[tokio::test]
async fn the_tool_list_is_read_with_schema_and_defaults() {
    let (mut client, mut fake) = pair(timeouts());
    let tools = json!([
        {"name": "search", "description": "find things", "inputSchema": {"type": "object", "properties": {"q": {"type": "string"}}}},
        {"name": "bare"},
        {"description": "no name"},
        {"name": 7},
    ]);
    let (listed, ()) = tokio::join!(client.list_tools(), fake.answer_list(tools));
    let listed = listed.expect("the list is read");
    assert_eq!(listed.len(), 2, "entries without a string name are skipped");
    assert_eq!(listed[0].name, "search");
    assert_eq!(listed[0].description, "find things");
    assert_eq!(listed[0].schema["properties"]["q"]["type"], "string");
    assert_eq!(listed[1].name, "bare");
    assert_eq!(listed[1].schema, json!({"type": "object"}));
}

#[tokio::test]
async fn at_most_64_tools_are_kept() {
    let (mut client, mut fake) = pair(timeouts());
    let many: Vec<Value> = (0..70).map(|i| json!({"name": format!("t{i}")})).collect();
    let (listed, ()) = tokio::join!(client.list_tools(), fake.answer_list(json!(many)));
    let listed = listed.unwrap();
    assert_eq!(listed.len(), 64);
    assert_eq!(listed[63].name, "t63");
}

#[tokio::test]
async fn hostile_metadata_is_sanitized_and_capped() {
    let (mut client, mut fake) = pair(timeouts());
    let huge = "x".repeat(100 * 1024);
    let tools = json!([
        {"name": "a", "description": format!("hi \u{1b}[31mred\u{202e} {huge}")},
        {"name": "b", "inputSchema": {"type": "object", "pad": huge}},
    ]);
    let (listed, ()) = tokio::join!(client.list_tools(), fake.answer_list(tools));
    let listed = listed.unwrap();
    let text = &listed[0].description;
    assert!(text.starts_with("hi "), "the text is kept: {text:?}");
    assert!(!text.contains('\u{1b}') && !text.contains('\u{202e}'));
    assert!(text.chars().count() <= 1024);
    assert_eq!(listed[1].schema, json!({"type": "object"}), "a huge schema");
}

#[tokio::test]
async fn a_call_sends_the_raw_name_and_returns_the_text() {
    let (mut client, mut fake) = pair(timeouts());
    let serve = async {
        let (id, params) = fake.request("tools/call").await;
        fake.reply(
            id,
            json!({"content": [{"type": "text", "text": "one"}, {"type": "image", "data": "AA=="}, {"type": "text", "text": "two"}]}),
        )
        .await;
        params
    };
    let (outcome, params) = tokio::join!(client.call("fs__odd", json!({"p": 1}), never()), serve);
    assert_eq!(params, json!({"name": "fs__odd", "arguments": {"p": 1}}));
    assert!(!outcome.is_error);
    assert_eq!(outcome.text, "one\n[image content omitted]\ntwo");
}

#[tokio::test]
async fn a_tool_error_and_a_rpc_error_are_error_results() {
    let (mut client, mut fake) = pair(timeouts());
    let serve = async {
        let (id, _) = fake.request("tools/call").await;
        let blocks = json!([{"type": "text", "text": "boom"}]);
        fake.reply(id, json!({"isError": true, "content": blocks}))
            .await;
        let (id, _) = fake.request("tools/call").await;
        let message = "bad \u{1b}[2Jthing".to_owned();
        fake.send(Frame::Error {
            id,
            code: -32000,
            message,
        })
        .await;
    };
    let ask = async {
        let first = client.call("t", json!({}), never()).await;
        let second = client.call("t", json!({}), never()).await;
        (first, second)
    };
    let ((first, second), ()) = tokio::join!(ask, serve);
    assert_eq!((first.is_error, first.text.as_str()), (true, "boom"));
    assert!(second.is_error);
    assert!(second.text.contains("bad") && !second.text.contains('\u{1b}'));
}

#[tokio::test]
async fn a_timeout_gives_an_error_result_and_cancels_the_request() {
    let (mut client, mut fake) = pair(Timeouts {
        handshake: WAIT,
        call: Duration::from_millis(100),
    });
    let serve = async {
        let (id, _) = fake.request("tools/call").await;
        match fake.recv().await {
            Frame::Notification { method, params } => {
                assert_eq!(method, "notifications/cancelled");
                assert_eq!(params["requestId"], id);
            }
            other => panic!("expected a cancel, got {other:?}"),
        }
    };
    let (outcome, ()) = tokio::join!(client.call("t", json!({}), never()), serve);
    assert!(outcome.is_error);
    assert!(outcome.text.contains("timed out"), "{}", outcome.text);
}

#[tokio::test]
async fn a_cancelled_turn_cancels_the_call_in_flight() {
    let (mut client, mut fake) = pair(timeouts());
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let serve = async {
        let (id, _) = fake.request("tools/call").await;
        stop.send(()).unwrap();
        match fake.recv().await {
            Frame::Notification { method, params } => {
                assert_eq!(method, "notifications/cancelled");
                assert_eq!(params["requestId"], id);
            }
            other => panic!("expected a cancel, got {other:?}"),
        }
    };
    let cancel = async {
        let _ = stopped.await;
    };
    let (outcome, ()) = tokio::join!(client.call("t", json!({}), cancel), serve);
    assert!(outcome.is_error);
    assert!(outcome.text.contains("cancelled"), "{}", outcome.text);
}

#[tokio::test]
async fn a_server_that_hangs_up_gives_an_error_result_not_a_hang() {
    let (mut client, fake) = pair(timeouts());
    drop(fake);
    let outcome = client.call("t", json!({}), never()).await;
    assert!(outcome.is_error);
    assert!(outcome.text.contains("closed"), "{}", outcome.text);
    assert_eq!(client.list_tools().await, Err(ClientError::Closed));
}

#[tokio::test]
async fn a_server_that_dies_mid_call_gives_an_error_result() {
    let (mut client, mut fake) = pair(timeouts());
    let serve = async {
        fake.request("tools/call").await;
        drop(fake);
    };
    let (outcome, ()) = tokio::join!(client.call("t", json!({}), never()), serve);
    assert!(outcome.is_error);
    assert!(outcome.text.contains("closed"), "{}", outcome.text);
}

#[tokio::test]
async fn pings_are_answered_and_other_requests_refused_while_waiting() {
    let (mut client, mut fake) = pair(timeouts());
    let serve = async {
        let (id, _) = fake.request("tools/call").await;
        fake.send(Frame::Request {
            id: json!("p"),
            method: "ping".into(),
            params: Value::Null,
        })
        .await;
        fake.send(Frame::Request {
            id: json!("s"),
            method: "sampling/createMessage".into(),
            params: Value::Null,
        })
        .await;
        let ping = fake.recv().await;
        let sampling = fake.recv().await;
        fake.reply(id, json!({"content": []})).await;
        (ping, sampling)
    };
    let (outcome, (ping, sampling)) = tokio::join!(client.call("t", json!({}), never()), serve);
    assert!(!outcome.is_error);
    assert_eq!(
        ping,
        Frame::Response {
            id: json!("p"),
            result: json!({})
        }
    );
    assert!(
        matches!(sampling, Frame::Error { code: -32601, .. }),
        "{sampling:?}"
    );
}

/// Note 2 of the 4a review: a call is routed by the `qualify` mapping, never
/// by splitting on `__`, and a collision is reported, not silently lost.
#[tokio::test]
async fn calls_route_by_raw_name_and_a_collision_is_reported() {
    let (mut a, mut fake_a) = pair(timeouts());
    let (mut b, mut fake_b) = pair(timeouts());
    let (la, lb, ..) = tokio::join!(
        a.list_tools(),
        b.list_tools(),
        fake_a.answer_list(json!([{"name": "odd__name"}, {"name": "run_in_pane"}, {"name": "dup"}, {"name": "dup"}])),
        fake_b.answer_list(json!([{"name": "odd__name"}, {"name": "other"}])),
    );
    let (la, lb) = (la.unwrap(), lb.unwrap());
    let qualified = qualify(&[("a", la.as_slice()), ("b", lb.as_slice())]);
    let names: Vec<String> = qualified.external().into_iter().map(|t| t.name).collect();
    assert_eq!(
        names,
        [
            "a__odd__name",
            "a__run_in_pane",
            "a__dup",
            "b__odd__name",
            "b__other"
        ]
    );
    assert_eq!(
        qualified.dropped[0].notice(),
        "mcp: a: dup dropped, same name as a/dup"
    );
    let registered: Vec<String> = registry(&qualified.external())
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(
        registered[..4],
        ["find_dir", "list_dir", "read_file", "run_in_pane"]
    );
    assert!(registered.contains(&"a__run_in_pane".to_owned()));

    let (server, raw) = qualified.raw_of("a__odd__name").unwrap();
    assert_eq!((server, raw), ("a", "odd__name"));
    let serve = async {
        let (id, params) = fake_a.request("tools/call").await;
        fake_a
            .reply(id, json!({"content": [{"type": "text", "text": "from a"}]}))
            .await;
        params
    };
    let (outcome, params) = tokio::join!(a.call(raw, json!({}), never()), serve);
    assert_eq!(params["name"], "odd__name");
    assert_eq!(outcome.text, "from a");
    drop((b, fake_b));
}
