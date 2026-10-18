//! The MCP wire protocol the lazarobox tools speak, pure: newline-delimited
//! JSON-RPC 2.0 on `serde_json::Value`. No IO lives here, and there is no
//! serde derive and no `rmcp` (see the design, D8). The session of slice 2d
//! reads lines and writes the replies; this module only decides.
//!
//! The subset, pinned to revision `2025-06-18`:
//!
//! | Message                      | Kind         | Answer                                                     |
//! |------------------------------|--------------|------------------------------------------------------------|
//! | `initialize`                 | request      | the asked revision if it is `2025-06-18`, `2025-03-26` or `2024-11-05`, else `2025-06-18`; capabilities `tools` only |
//! | `notifications/initialized`  | notification | nothing                                                    |
//! | `ping`                       | request      | `{}`, also before `initialize`                             |
//! | `tools/list`                 | request      | the registry, with `inputSchema`; needs `initialize`       |
//! | `tools/call`                 | request      | [`Action::Call`] for the host, or an error result; needs `initialize` |
//! | `notifications/cancelled`    | notification | [`Action::Cancel`] with the `requestId`                    |
//! | any other request            | request      | -32601                                                     |
//! | any other notification       | notification | ignored                                                    |
//!
//! Errors echo the request `id` when the line is JSON and its `id` is a string
//! or a number, even if the line is refused for another reason; they carry
//! `id: null` only when no id can be known (bad JSON, a batch, an unusable
//! id, a line over [`MAX_LINE_BYTES`]): -32700
//! (not JSON), -32600 (not one request object, a batch, a duplicate JSON key,
//! a line over [`MAX_LINE_BYTES`], an unusable id, a tool method before
//! `initialize`), -32602 (bad parameters). A tool that is unknown or whose
//! arguments are refused is a tool result with `isError: true`, so the model
//! can read it. Nothing here ends the session: after any error the next line
//! is served.
//!
//! [`Frame`] is the codec both sides share. The client of slice 4b reads and
//! writes the same frames.

#[cfg(test)]
use super::registry;
use super::{Call, ToolError, ToolSpec, parse_call};
use serde_json::{Value, json};
use std::collections::HashSet;

/// The longest line served, in bytes, newline excluded.
pub const MAX_LINE_BYTES: usize = 1 << 20;

/// The revision this server speaks.
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// Revisions echoed back when the client asks for them.
pub const OLDER_VERSIONS: [&str; 2] = ["2025-03-26", "2024-11-05"];

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

/// A JSON-RPC error: what [`Frame::parse`] gives back for a bad line.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    /// The id of the refused line when it had a usable one, else `Null`, so
    /// the client waiting on it gets an answer.
    pub id: Value,
}

impl RpcError {
    fn new(code: i64, message: &str, id: &Value) -> Self {
        RpcError {
            code,
            message: message.to_owned(),
            id: id.clone(),
        }
    }

    fn invalid(message: &str, id: &Value) -> Self {
        Self::new(INVALID_REQUEST, message, id)
    }
}

/// One JSON-RPC message. A missing `params` is `Null`.
#[derive(Clone, PartialEq, Debug)]
pub enum Frame {
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    Notification {
        method: String,
        params: Value,
    },
    Response {
        id: Value,
        result: Value,
    },
    Error {
        id: Value,
        code: i64,
        message: String,
    },
}

/// A request id is a string or a number. `null` is not one.
fn is_id(id: &Value) -> bool {
    id.is_string() || id.is_number()
}

impl Frame {
    /// Reads one line. Rejects what is not one JSON-RPC 2.0 object: bad JSON
    /// is -32700; a batch, a wrong shape, a duplicate key at any depth and an
    /// unusable id are -32600. The line cap is the caller's, see
    /// [`Server::handle`].
    pub fn parse(line: &str) -> Result<Frame, RpcError> {
        let value: Value = serde_json::from_str(line)
            .map_err(|_| RpcError::new(PARSE_ERROR, "parse error", &Value::Null))?;
        // Echo an id the client can be waiting on, whatever else is wrong.
        let hint = value
            .get("id")
            .filter(|id| is_id(id))
            .cloned()
            .unwrap_or(Value::Null);
        if has_duplicate_keys(line) {
            return Err(RpcError::invalid("duplicate key", &hint));
        }
        let Value::Object(mut object) = value else {
            return Err(RpcError::invalid("expected one request object", &hint));
        };
        if object.get("jsonrpc") != Some(&json!("2.0")) {
            return Err(RpcError::invalid("expected jsonrpc 2.0", &hint));
        }
        let id = object.remove("id");
        if let Some(method) = object.remove("method") {
            let Value::String(method) = method else {
                return Err(RpcError::invalid("method must be a string", &hint));
            };
            let params = object.remove("params").unwrap_or(Value::Null);
            return match id {
                None => Ok(Frame::Notification { method, params }),
                Some(id) if is_id(&id) => Ok(Frame::Request { id, method, params }),
                Some(_) => Err(RpcError::invalid(
                    "id must be a string or a number",
                    &Value::Null,
                )),
            };
        }
        let id = id.filter(|id| is_id(id) || id.is_null());
        match (id, object.remove("error"), object.remove("result")) {
            (Some(id), Some(error), None) => {
                let code = error.get("code").and_then(Value::as_i64);
                let message = error.get("message").and_then(Value::as_str);
                match (code, message) {
                    (Some(code), Some(message)) => Ok(Frame::Error {
                        id,
                        code,
                        message: message.to_owned(),
                    }),
                    _ => Err(RpcError::invalid("malformed error", &hint)),
                }
            }
            (Some(id), None, Some(result)) if is_id(&id) => Ok(Frame::Response { id, result }),
            _ => Err(RpcError::invalid("not a request or a response", &hint)),
        }
    }

    pub fn to_value(&self) -> Value {
        let mut out = json!({ "jsonrpc": "2.0" });
        match self {
            Frame::Request { id, method, params } => {
                out["id"] = id.clone();
                out["method"] = json!(method);
                if !params.is_null() {
                    out["params"] = params.clone();
                }
            }
            Frame::Notification { method, params } => {
                out["method"] = json!(method);
                if !params.is_null() {
                    out["params"] = params.clone();
                }
            }
            Frame::Response { id, result } => {
                out["id"] = id.clone();
                out["result"] = result.clone();
            }
            Frame::Error { id, code, message } => {
                out["id"] = id.clone();
                out["error"] = json!({ "code": code, "message": message });
            }
        }
        out
    }

    /// The frame as one line, without the newline. `serde_json` escapes
    /// every newline inside a string, so it never spans two lines.
    pub fn to_line(&self) -> String {
        self.to_value().to_string()
    }
}

/// What the session does with one line.
#[derive(Clone, PartialEq, Debug)]
pub enum Action {
    /// Nothing to send.
    None,
    /// Send this message.
    Reply(Value),
    /// Run this call in the host, then answer `id` with [`tool_result`].
    Call { id: Value, call: Call },
    /// The client withdrew the request `id`.
    Cancel { id: Value },
}

/// The server side of one session.
pub struct Server {
    tools: Vec<ToolSpec>,
    initialized: bool,
}

fn error_reply(id: Value, code: i64, message: &str) -> Action {
    Action::Reply(
        Frame::Error {
            id,
            code,
            message: message.to_owned(),
        }
        .to_value(),
    )
}

fn result_reply(id: Value, result: Value) -> Action {
    Action::Reply(Frame::Response { id, result }.to_value())
}

/// The answer to a `tools/call`: the text and whether it is an error.
pub fn tool_result(id: &Value, text: &str, is_error: bool) -> Value {
    Frame::Response {
        id: id.clone(),
        result: json!({
            "content": [{ "type": "text", "text": text }],
            "isError": is_error,
        }),
    }
    .to_value()
}

/// Short and content-free: it goes back to a model.
pub fn refusal_text(error: ToolError) -> &'static str {
    match error {
        ToolError::UnknownTool => "unknown tool",
        ToolError::NotAnObject => "arguments must be an object",
        ToolError::MissingArgument => "missing argument",
        ToolError::WrongType => "argument must be a string",
        ToolError::ExtraArgument => "unexpected argument",
        ToolError::TooLong => "argument too long",
    }
}

impl Server {
    /// A server that offers `tools`, normally [`registry`] of the session.
    /// Tools of user servers are not callable here until slice 4a.
    pub fn new(tools: Vec<ToolSpec>) -> Self {
        Server {
            tools,
            initialized: false,
        }
    }

    /// Serves one line (no newline). Never panics and never ends the session.
    pub fn handle(&mut self, line: &str) -> Action {
        if line.len() > MAX_LINE_BYTES {
            return error_reply(Value::Null, INVALID_REQUEST, "line too long");
        }
        match Frame::parse(line) {
            Err(error) => error_reply(error.id, error.code, &error.message),
            Ok(Frame::Request { id, method, params }) => self.request(id, &method, &params),
            Ok(Frame::Notification { method, params }) => notification(&method, &params),
            // This server sends no requests, so it has nothing to match.
            Ok(Frame::Response { .. } | Frame::Error { .. }) => Action::None,
        }
    }

    fn request(&mut self, id: Value, method: &str, params: &Value) -> Action {
        match method {
            "ping" => result_reply(id, json!({})),
            "initialize" => self.initialize(id, params),
            "tools/list" | "tools/call" if !self.initialized => {
                error_reply(id, INVALID_REQUEST, "not initialized")
            }
            "tools/list" => {
                let tools: Vec<Value> = self
                    .tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "name": tool.name,
                            "description": tool.description,
                            "inputSchema": tool.schema,
                        })
                    })
                    .collect();
                result_reply(id, json!({ "tools": tools }))
            }
            "tools/call" => call(id, params),
            _ => error_reply(id, METHOD_NOT_FOUND, "method not found"),
        }
    }

    fn initialize(&mut self, id: Value, params: &Value) -> Action {
        let Some(asked) = params.get("protocolVersion").and_then(Value::as_str) else {
            return error_reply(id, INVALID_PARAMS, "protocolVersion must be a string");
        };
        let version = if OLDER_VERSIONS.contains(&asked) {
            asked
        } else {
            PROTOCOL_VERSION
        };
        self.initialized = true;
        result_reply(
            id,
            json!({
                "protocolVersion": version,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "lazarobox", "version": env!("CARGO_PKG_VERSION") },
            }),
        )
    }
}

fn notification(method: &str, params: &Value) -> Action {
    match method {
        "notifications/cancelled" => match params.get("requestId") {
            Some(id) if is_id(id) => Action::Cancel { id: id.clone() },
            _ => Action::None,
        },
        // `notifications/initialized` needs no state: tools open at
        // `initialize`. Unknown notifications have no id to answer.
        _ => Action::None,
    }
}

fn call(id: Value, params: &Value) -> Action {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return error_reply(id, INVALID_PARAMS, "name must be a string");
    };
    let empty = json!({});
    let arguments = params.get("arguments").unwrap_or(&empty);
    match parse_call(name, arguments) {
        Ok(call) => Action::Call { id, call },
        Err(error) => Action::Reply(tool_result(&id, refusal_text(error), true)),
    }
}

/// Whether any JSON object in `line` repeats a key, at any depth, after
/// unescaping (`"path"` is `"path"`). `serde_json::Value` keeps only the
/// last duplicate, so this reads the raw text. It expects text that already
/// parsed as JSON and checks nothing else.
fn has_duplicate_keys(line: &str) -> bool {
    // One entry per open container: `Some` for an object (the keys seen so
    // far), `None` for an array.
    let mut open: Vec<Option<HashSet<String>>> = Vec::new();
    // The next string in an object is a key: right after `{` and after `,`.
    let mut key_next = false;
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                open.push(Some(HashSet::new()));
                key_next = true;
            }
            b'[' => {
                open.push(None);
                key_next = false;
            }
            b'}' | b']' => {
                open.pop();
                key_next = false;
            }
            b',' => key_next = matches!(open.last(), Some(Some(_))),
            b'"' => {
                let start = i;
                i += 1;
                while bytes[i] != b'"' {
                    // A backslash escapes the next byte, a quote included.
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                if key_next {
                    let key: String = serde_json::from_str(&line[start..=i]).unwrap_or_default();
                    if let Some(Some(keys)) = open.last_mut()
                        && !keys.insert(key)
                    {
                        return true;
                    }
                    key_next = false;
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn server() -> Server {
        Server::new(registry(&[]))
    }

    fn line(value: Value) -> String {
        value.to_string()
    }

    fn request(id: Value, method: &str, params: Value) -> String {
        line(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
    }

    fn reply(action: Action) -> Value {
        match action {
            Action::Reply(value) => value,
            other => panic!("expected a reply, got {other:?}"),
        }
    }

    fn initialized(version: &str) -> Server {
        let mut server = server();
        let init = request(
            json!(0),
            "initialize",
            json!({"protocolVersion": version, "capabilities": {}}),
        );
        reply(server.handle(&init));
        server
    }

    fn error_code(value: &Value) -> i64 {
        value["error"]["code"].as_i64().expect("an error code")
    }

    // Spec (lazarobox-mcp-server, handshake): initialize pins the revision.
    #[test]
    fn initialize_answers_the_pinned_revision() {
        let mut server = server();
        let out = reply(server.handle(&request(
            json!(1),
            "initialize",
            json!({"protocolVersion": "2025-06-18"}),
        )));
        assert_eq!(out["jsonrpc"], "2.0");
        assert_eq!(out["id"], 1);
        assert_eq!(out["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(out["result"]["capabilities"]["tools"]["listChanged"], false);
        assert_eq!(out["result"]["serverInfo"]["name"], "lazarobox");
    }

    #[test]
    fn initialize_echoes_the_older_revisions_and_pins_for_unknown() {
        for (asked, answered) in [
            ("2025-03-26", "2025-03-26"),
            ("2024-11-05", "2024-11-05"),
            ("1999-01-01", "2025-06-18"),
        ] {
            let out = reply(server().handle(&request(
                json!("a"),
                "initialize",
                json!({"protocolVersion": asked}),
            )));
            assert_eq!(out["result"]["protocolVersion"], answered, "{asked}");
            assert_eq!(out["id"], "a");
        }
    }

    #[test]
    fn initialize_needs_a_version_string() {
        for params in [json!({}), json!({"protocolVersion": 3}), json!(null)] {
            let out = reply(server().handle(&request(json!(1), "initialize", params)));
            assert_eq!(error_code(&out), -32602);
        }
    }

    #[test]
    fn the_initialized_notification_and_ping() {
        let mut server = initialized("2025-06-18");
        let note = line(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        assert_eq!(server.handle(&note), Action::None);
        let out = reply(server.handle(&request(json!(7), "ping", json!(null))));
        assert_eq!(out["result"], json!({}));
        assert_eq!(out["id"], 7);
    }

    // Spec: the response lists exactly the registry tools with their schemas.
    #[test]
    fn tools_list_returns_the_registry() {
        let mut server = initialized("2025-06-18");
        let out = reply(server.handle(&request(json!(2), "tools/list", json!({}))));
        let tools = out["result"]["tools"].as_array().unwrap();
        let want = registry(&[]);
        assert_eq!(tools.len(), want.len());
        for (got, spec) in tools.iter().zip(&want) {
            assert_eq!(got["name"], spec.name.as_str());
            assert_eq!(got["description"], spec.description.as_str());
            assert_eq!(got["inputSchema"], spec.schema);
        }
    }

    #[test]
    fn tools_call_hands_a_validated_call_to_the_host() {
        let mut server = initialized("2025-06-18");
        let action = server.handle(&request(
            json!(3),
            "tools/call",
            json!({"name": "read_file", "arguments": {"path": "a.rs"}}),
        ));
        assert_eq!(
            action,
            Action::Call {
                id: json!(3),
                call: Call::ReadFile {
                    path: "a.rs".into()
                }
            }
        );
    }

    // Spec: an unknown tool is an error result, not a protocol error.
    #[test]
    fn an_unknown_tool_is_an_error_result() {
        let mut server = initialized("2025-06-18");
        let out = reply(server.handle(&request(
            json!(4),
            "tools/call",
            json!({"name": "bash", "arguments": {"command": "ls"}}),
        )));
        assert!(out.get("error").is_none(), "{out}");
        assert_eq!(out["id"], 4);
        assert_eq!(out["result"]["isError"], true);
        assert_eq!(out["result"]["content"][0]["type"], "text");
    }

    #[test]
    fn a_refused_argument_is_an_error_result_too() {
        let mut server = initialized("2025-06-18");
        for args in [
            json!({}),
            json!({"path": 1}),
            json!({"path": "a", "mode": "w"}),
            json!([1]),
        ] {
            let params = json!({"name": "read_file", "arguments": args});
            let out = reply(server.handle(&request(json!(5), "tools/call", params)));
            assert_eq!(out["result"]["isError"], true, "{args}");
        }
        // Absent arguments count as an empty object.
        let out = reply(server.handle(&request(
            json!(6),
            "tools/call",
            json!({"name": "read_file"}),
        )));
        assert_eq!(out["result"]["isError"], true);
    }

    #[test]
    fn tools_call_with_bad_params_is_minus_32602() {
        let mut server = initialized("2025-06-18");
        for params in [json!(null), json!([]), json!({}), json!({"name": 4})] {
            let out = reply(server.handle(&request(json!(1), "tools/call", params)));
            assert_eq!(error_code(&out), -32602);
        }
    }

    #[test]
    fn tools_need_an_initialize_first() {
        let mut server = server();
        for method in ["tools/list", "tools/call"] {
            let out = reply(server.handle(&request(json!(1), method, json!({}))));
            assert_eq!(error_code(&out), -32600, "{method}");
        }
        // ping is allowed at any time.
        reply(server.handle(&request(json!(2), "ping", json!(null))));
    }

    #[test]
    fn cancelled_names_the_request_to_stop() {
        let mut server = initialized("2025-06-18");
        let note = line(json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": {"requestId": 9, "reason": "user"}
        }));
        assert_eq!(server.handle(&note), Action::Cancel { id: json!(9) });
        let bad = line(json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": {}
        }));
        assert_eq!(server.handle(&bad), Action::None);
    }

    // Spec: an unknown method gets a JSON-RPC error and the server keeps going.
    #[test]
    fn unknown_methods_get_minus_32601() {
        let mut server = initialized("2025-06-18");
        let out = reply(server.handle(&request(json!(8), "resources/read", json!({}))));
        assert_eq!(error_code(&out), -32601);
        assert_eq!(out["id"], 8);
        // An unknown notification is ignored: it has no id to answer.
        let note = line(json!({"jsonrpc": "2.0", "method": "resources/updated"}));
        assert_eq!(server.handle(&note), Action::None);
        reply(server.handle(&request(json!(9), "ping", json!(null))));
    }

    #[test]
    fn bad_json_is_minus_32700_with_a_null_id() {
        let mut server = server();
        for text in ["", "{", "not json", "{\"a\":1} trailing"] {
            let out = reply(server.handle(text));
            assert_eq!(error_code(&out), -32700, "{text:?}");
            assert_eq!(out["id"], Value::Null);
        }
    }

    #[test]
    fn invalid_requests_are_minus_32600() {
        let mut server = server();
        // The id is echoed when the line has a usable one, else null.
        let cases = [
            ("[]".to_owned(), Value::Null),
            (
                line(json!([{"jsonrpc": "2.0", "id": 1, "method": "ping"}])),
                Value::Null,
            ),
            ("1".to_owned(), Value::Null),
            (line(json!({"id": 1, "method": "ping"})), json!(1)),
            (
                line(json!({"jsonrpc": "1.0", "id": 1, "method": "ping"})),
                json!(1),
            ),
            (line(json!({"jsonrpc": "2.0", "id": "a"})), json!("a")),
            (
                line(json!({"jsonrpc": "2.0", "id": 1, "method": 5})),
                json!(1),
            ),
            (
                line(json!({"jsonrpc": "2.0", "id": {"a": 1}, "method": "ping"})),
                Value::Null,
            ),
            (
                line(json!({"jsonrpc": "2.0", "id": null, "method": "ping"})),
                Value::Null,
            ),
        ];
        for (text, id) in cases {
            let out = reply(server.handle(&text));
            assert_eq!(error_code(&out), -32600, "{text}");
            assert_eq!(out["id"], id, "{text}");
        }
    }

    // Review note from 2b: duplicate keys are invisible once parsed to a Value.
    #[test]
    fn duplicate_keys_are_refused_at_any_depth() {
        let mut server = initialized("2025-06-18");
        let cases = [
            r#"{"jsonrpc":"2.0","id":1,"method":"ping","method":"tools/call"}"#,
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"find_dir","name":"bash","arguments":{}}}"#,
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"read_file","arguments":{"path":"a","path":"/etc/passwd"}}}"#,
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"read_file","arguments":{"path":"a","path":"b"}}}"#,
            r#"{"jsonrpc":"2.0","id":1,"method":"ping","params":[{"k":1,"k":2}]}"#,
        ];
        for text in cases {
            let out = reply(server.handle(text));
            assert_eq!(error_code(&out), -32600, "{text}");
            assert_eq!(out["id"], 1, "{text}");
        }
    }

    // A client waiting on the id of a refused line must get its answer.
    #[test]
    fn a_refused_line_with_a_valid_id_echoes_it() {
        let mut server = server();
        for (text, id) in [
            (
                r#"{"jsonrpc":"2.0","id":7,"method":"ping","method":"ping"}"#,
                json!(7),
            ),
            (r#"{"jsonrpc":"2.0","id":"r-1","method":3}"#, json!("r-1")),
            (r#"{"jsonrpc":"1.0","id":7.5,"method":"ping"}"#, json!(7.5)),
        ] {
            let out = reply(server.handle(text));
            assert_eq!(error_code(&out), -32600, "{text}");
            assert_eq!(out["id"], id, "{text}");
        }
    }

    // `p\u0061th` and `\u0070ath` both spell `path`; so does a plain one.
    #[test]
    fn differently_escaped_spellings_of_a_key_collide() {
        for text in [
            r#"{"p\u0061th":1,"path":2}"#,
            r#"{"\u0070ath":1,"p\u0061th":2}"#,
            r#"{"a":{"\u0061":1,"a":2}}"#,
        ] {
            assert!(has_duplicate_keys(text), "{text}");
        }
        assert!(!has_duplicate_keys(r#"{"p\u0061th":1,"pat\u0068s":2}"#));
    }

    // serde_json stops at its recursion limit: the scanner never sees these.
    #[test]
    fn deeply_nested_lines_are_a_parse_error_not_a_panic() {
        let mut server = server();
        let arrays = format!("{}{}", "[".repeat(300), "]".repeat(300));
        let objects = format!("{}1{}", r#"{"a":"#.repeat(300), "}".repeat(300));
        let params = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"ping","params":{}0{}}}"#,
            "[".repeat(300),
            "]".repeat(300)
        );
        for text in [arrays, objects, params] {
            let out = reply(server.handle(&text));
            assert_eq!(error_code(&out), -32700);
            assert_eq!(out["id"], Value::Null);
        }
        reply(server.handle(&request(json!(2), "ping", json!(null))));
    }

    #[test]
    fn the_same_key_in_sibling_objects_is_fine() {
        assert!(!has_duplicate_keys(
            r#"{"a":{"k":1},"b":{"k":2},"c":[{"k":1},{"k":2}]}"#
        ));
        assert!(!has_duplicate_keys(
            r#"{"a":"k\"k","b":"{\"k\":1,\"k\":2}"}"#
        ));
        assert!(has_duplicate_keys(r#"{"a":{"k":1,"k":2}}"#));
    }

    // Spec: a message over the cap gets an error with id null and the state
    // machine keeps going.
    #[test]
    fn an_oversized_line_gets_an_error_and_the_server_keeps_going() {
        let mut server = initialized("2025-06-18");
        let pad = "a".repeat(MAX_LINE_BYTES);
        let big = request(json!(1), "ping", json!({"pad": pad}));
        assert!(big.len() > MAX_LINE_BYTES);
        let out = reply(server.handle(&big));
        assert_eq!(error_code(&out), -32600);
        assert_eq!(out["id"], Value::Null);
        let out = reply(server.handle(&request(json!(2), "ping", json!(null))));
        assert_eq!(out["id"], 2);
    }

    #[test]
    fn a_line_at_the_cap_is_still_read() {
        let mut server = initialized("2025-06-18");
        let empty = request(json!(1), "ping", json!({"pad": ""}));
        let pad = "a".repeat(MAX_LINE_BYTES - empty.len());
        let at_cap = request(json!(1), "ping", json!({"pad": pad}));
        assert_eq!(at_cap.len(), MAX_LINE_BYTES);
        assert_eq!(reply(server.handle(&at_cap))["result"], json!({}));
    }

    #[test]
    fn tool_result_carries_the_text_and_the_flag() {
        let out = tool_result(&json!(3), "done", false);
        assert_eq!(out["id"], 3);
        assert_eq!(out["result"]["isError"], false);
        assert_eq!(
            out["result"]["content"],
            json!([{"type": "text", "text": "done"}])
        );
        assert_eq!(
            tool_result(&json!("x"), "no", true)["result"]["isError"],
            true
        );
    }

    // 2c.4: the client of slice 4b reads and writes the same frames.
    #[test]
    fn frames_round_trip_through_a_line() {
        let frames = [
            Frame::Request {
                id: json!(1),
                method: "ping".into(),
                params: json!({"a": 1}),
            },
            Frame::Notification {
                method: "notifications/initialized".into(),
                params: Value::Null,
            },
            Frame::Response {
                id: json!("x"),
                result: json!({"ok": true}),
            },
            Frame::Error {
                id: Value::Null,
                code: -32700,
                message: "bad".into(),
            },
        ];
        for frame in frames {
            let text = frame.to_line();
            assert!(!text.contains('\n'));
            assert_eq!(Frame::parse(&text), Ok(frame));
        }
    }
}
