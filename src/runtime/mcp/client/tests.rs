use std::{future::Future, time::Duration};

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

    async fn send_raw(&mut self, line: &[u8]) {
        self.tx.write_all(line).await.unwrap();
        self.tx.write_all(b"\n").await.unwrap();
    }

    /// Whether the client closed its end without another line.
    async fn ended(&mut self) -> bool {
        matches!(
            tokio::time::timeout(WAIT, read_line(&mut self.rx, MAX_LINE_BYTES)).await,
            Ok(Ok(Line::Eof))
        )
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
        self.handshake_with(PROTOCOL_VERSION).await
    }

    async fn handshake_with(&mut self, version: &str) -> Value {
        let (id, params) = self.request("initialize").await;
        self.reply(
            id,
            json!({"protocolVersion": version, "capabilities": {}, "serverInfo": {"name": "fake"}}),
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
    pair_with(timeouts, 1 << 21)
}

/// `to_server` is the size of the pipe the client writes to: a small one that
/// the fake never reads from is a server that stopped reading its stdin.
fn pair_with(timeouts: Timeouts, to_server: usize) -> (McpClient, Fake) {
    let (client_tx, server_rx) = duplex(to_server);
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
    // S1: MCP forbids cancelling `initialize`, so nothing follows it.
    drop(client);
    assert!(fake.ended().await, "no cancel notice for initialize");
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

/// Runs `work` for at most 3 s: a stuck write would otherwise hang the test.
async fn bounded<T>(work: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(3), work)
        .await
        .expect("returned within the bound")
}

fn stalled(call: Duration) -> (McpClient, Fake) {
    pair_with(
        Timeouts {
            handshake: Duration::from_millis(100),
            call,
        },
        8,
    )
}

// C1 and W1: one deadline covers the write and the wait.

#[tokio::test]
async fn a_server_that_stopped_reading_times_out_a_call() {
    let (mut client, _fake) = stalled(Duration::from_millis(100));
    let big = json!({"a": "x".repeat(2000)});
    let outcome = bounded(client.call("t", big, never())).await;
    assert!(outcome.is_error);
    assert!(outcome.text.contains("timed out"), "{}", outcome.text);
}

#[tokio::test]
async fn a_server_that_stopped_reading_does_not_hold_a_cancel() {
    let (mut client, _fake) = stalled(WAIT);
    let big = json!({"a": "x".repeat(2000)});
    let cancel = tokio::time::sleep(Duration::from_millis(100));
    let outcome = bounded(client.call("t", big, cancel)).await;
    assert!(outcome.text.contains("cancelled"), "{}", outcome.text);
}

#[tokio::test]
async fn a_stalled_handshake_times_out() {
    let (mut client, _fake) = stalled(WAIT);
    assert_eq!(bounded(client.handshake()).await, Err(ClientError::Timeout));
}

#[tokio::test]
async fn a_stalled_initialized_notice_times_out() {
    // The initialize request fits the pipe, the notice after it does not.
    let (mut client, mut fake) = pair_with(
        Timeouts {
            handshake: Duration::from_millis(200),
            call: WAIT,
        },
        160,
    );
    let answer = format!(
        r#"{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":"{PROTOCOL_VERSION}"}}}}"#
    );
    let (done, ()) = tokio::join!(
        bounded(client.handshake()),
        fake.send_raw(answer.as_bytes())
    );
    assert_eq!(done, Err(ClientError::Timeout));
}

#[tokio::test]
async fn a_write_cut_short_kills_the_client() {
    let (mut client, _fake) = stalled(Duration::from_millis(100));
    let big = json!({"a": "x".repeat(2000)});
    bounded(client.call("t", big, never())).await;
    let again = bounded(client.call("t", json!({}), never())).await;
    assert!(
        again.is_error && again.text.contains("closed"),
        "{}",
        again.text
    );
    assert_eq!(bounded(client.list_tools()).await, Err(ClientError::Closed));
    assert_eq!(bounded(client.handshake()).await, Err(ClientError::Closed));
}

#[tokio::test]
async fn a_plain_cancel_keeps_the_client_and_a_late_reply_is_skipped() {
    let (mut client, mut fake) = pair(timeouts());
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let serve = async {
        let (first, _) = fake.request("tools/call").await;
        stop.send(()).unwrap();
        fake.recv().await; // the cancel notice
        // The late answer of the cancelled call, then the real one.
        fake.reply(
            first,
            json!({"content": [{"type": "text", "text": "stale"}]}),
        )
        .await;
        let (second, _) = fake.request("tools/call").await;
        fake.reply(
            second,
            json!({"content": [{"type": "text", "text": "fresh"}]}),
        )
        .await;
    };
    let ask = async {
        let cancelled = client
            .call("t", json!({}), async {
                let _ = stopped.await;
            })
            .await;
        let next = client.call("t", json!({}), never()).await;
        (cancelled, next)
    };
    let ((cancelled, next), ()) = tokio::join!(ask, serve);
    assert!(cancelled.is_error);
    assert_eq!(next.text, "fresh");
}

// W2: a bad answer to our own request fails at once.

#[tokio::test]
async fn an_oversize_reply_fails_fast() {
    let (mut client, mut fake) = pair(timeouts());
    let serve = async {
        fake.request("tools/call").await;
        fake.send_raw(&vec![b'x'; MAX_LINE_BYTES + 10]).await;
    };
    let (outcome, ()) = tokio::join!(bounded(client.call("t", json!({}), never())), serve);
    assert!(outcome.text.contains("reply too large"), "{}", outcome.text);
}

#[tokio::test]
async fn a_reply_that_is_not_text_fails_fast() {
    let (mut client, mut fake) = pair(timeouts());
    let serve = async {
        fake.request("tools/call").await;
        fake.send_raw(&[0xff, 0xfe, b'{']).await;
    };
    let (outcome, ()) = tokio::join!(bounded(client.call("t", json!({}), never())), serve);
    assert!(outcome.text.contains("not text"), "{}", outcome.text);
}

#[tokio::test]
async fn a_malformed_answer_to_our_id_fails_fast() {
    let (mut client, mut fake) = pair(timeouts());
    let serve = async {
        let (id, _) = fake.request("tools/call").await;
        // An error object with no code.
        let line = json!({"jsonrpc": "2.0", "id": id, "error": {"message": "x"}});
        fake.send_raw(line.to_string().as_bytes()).await;
        let (id, _) = fake.request("tools/call").await;
        let line = json!({"jsonrpc": "2.0", "id": id});
        fake.send_raw(line.to_string().as_bytes()).await;
    };
    let ask = async {
        let first = bounded(client.call("t", json!({}), never())).await;
        let second = bounded(client.call("t", json!({}), never())).await;
        (first, second)
    };
    let ((first, second), ()) = tokio::join!(ask, serve);
    assert!(first.text.contains("bad reply"), "{}", first.text);
    assert!(second.text.contains("bad reply"), "{}", second.text);
}

#[tokio::test]
async fn garbage_for_nobody_is_still_skipped() {
    let (mut client, mut fake) = pair(timeouts());
    let serve = async {
        let (id, _) = fake.request("tools/call").await;
        fake.send_raw(b"not json at all").await;
        fake.send_raw(br#"{"jsonrpc":"2.0","method":"notifications/message"}"#)
            .await;
        fake.reply(id, json!({"content": [{"type": "text", "text": "ok"}]}))
            .await;
    };
    let (outcome, ()) = tokio::join!(client.call("t", json!({}), never()), serve);
    assert_eq!(outcome.text, "ok");
}

// W3: the schema is validated, not just measured.

async fn schema_of(schema: Value) -> Value {
    let (mut client, mut fake) = pair(timeouts());
    let tools = json!([{"name": "t", "inputSchema": schema}]);
    let (listed, ()) = tokio::join!(client.list_tools(), fake.answer_list(tools));
    listed.unwrap().remove(0).schema
}

#[tokio::test]
async fn a_schema_that_is_not_an_object_type_is_replaced() {
    let bare = json!({"type": "object"});
    for bad in [
        json!({"type": "string"}),
        json!({"properties": {}}),
        json!({"type": ["object"]}),
        json!([1]),
    ] {
        assert_eq!(schema_of(bad.clone()).await, bare, "{bad}");
    }
    let good = json!({"type": "object", "required": ["q"]});
    assert_eq!(schema_of(good.clone()).await, good);
}

#[tokio::test]
async fn every_string_of_a_schema_is_cleaned() {
    let schema = schema_of(json!({
        "type": "object",
        "properties": {"mode": {
            "type": "string",
            "description": "pick \u{1b}[31mone\u{202e}",
            "enum": ["a\u{1b}[2J", "b\u{202e}"],
        }},
    }))
    .await;
    let mode = &schema["properties"]["mode"];
    assert_eq!(mode["description"], "pick one");
    assert_eq!(mode["enum"], json!(["a", "b"]));
    assert_eq!(mode["type"], "string", "plain strings are kept");
}

#[tokio::test]
async fn a_schema_nested_too_deep_is_replaced() {
    let nest = |depth: usize| {
        let mut inner = json!({"type": "string"});
        for _ in 0..depth {
            inner = json!({"items": inner});
        }
        json!({"type": "object", "properties": inner})
    };
    let bare = json!({"type": "object"});
    assert_ne!(schema_of(nest(20)).await, bare, "a shallow one is kept");
    assert_eq!(schema_of(nest(40)).await, bare);
}

// W4: a result is cleaned and capped here too.

#[tokio::test]
async fn a_result_text_is_cleaned_and_capped() {
    let (mut client, mut fake) = pair(timeouts());
    let huge = "é".repeat(100 * 1024);
    let serve = async {
        let (id, _) = fake.request("tools/call").await;
        let text = format!("go \u{1b}[31mred\u{202e} {huge}");
        fake.reply(
            id,
            json!({"isError": true, "content": [{"type": "text", "text": text}]}),
        )
        .await;
    };
    let (outcome, ()) = tokio::join!(client.call("t", json!({}), never()), serve);
    assert!(outcome.is_error);
    assert!(outcome.text.starts_with("go red é"), "{:.20}", outcome.text);
    assert!(!outcome.text.contains('\u{1b}') && !outcome.text.contains('\u{202e}'));
    let cap = crate::core::ai::tools::label::OUTPUT_CAP;
    assert!(outcome.text.len() <= cap && outcome.text.len() > cap - 4);
}

// S2: only the revisions D8 names.

#[tokio::test]
async fn an_unsupported_revision_closes_the_client() {
    let (mut client, mut fake) = pair(timeouts());
    let serve = async {
        let (id, _) = fake.request("initialize").await;
        fake.reply(id, json!({"protocolVersion": "1999-01-01"}))
            .await;
    };
    let (done, ()) = tokio::join!(client.handshake(), serve);
    assert!(matches!(done, Err(ClientError::Protocol(_))), "{done:?}");
    assert_eq!(client.list_tools().await, Err(ClientError::Closed));
}

#[tokio::test]
async fn an_older_supported_revision_is_accepted() {
    let (mut client, mut fake) = pair(timeouts());
    let (done, _) = tokio::join!(client.handshake(), fake.handshake_with("2024-11-05"));
    assert_eq!(done, Ok(()));
}
