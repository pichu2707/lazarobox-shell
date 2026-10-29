//! Characterization of the 0.18.0 tool surface: the exact `tools/list` bytes
//! and the text a model gets back for each refused call. Captured BEFORE the
//! registry refactor; every later slice must keep them green with all
//! toggles off.

use serde_json::{Value, json};

use super::mcp::{Action, Server, refusal_text};
use super::{parse_call, registry};

const TOOLS_LIST: &str = include_str!("golden/tools_list.json");
const PARSE_ERRORS: &str = include_str!("golden/parse_errors.txt");

fn tools_list_bytes() -> String {
    let mut server = Server::new(registry(&[]));
    let line = |id: u64, method: &str, params: Value| {
        json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string()
    };
    server.handle(&line(
        1,
        "initialize",
        json!({"protocolVersion": "2025-06-18"}),
    ));
    match server.handle(&line(2, "tools/list", json!({}))) {
        Action::Reply(value) => value.to_string(),
        other => panic!("expected a reply, got {other:?}"),
    }
}

fn parse_error_table() -> String {
    let long = "a".repeat(super::MAX_ARG_BYTES + 1);
    let wide = "é".repeat(super::MAX_ARG_BYTES / 2 + 1); // 4098 bytes, 2049 chars
    let cases: Vec<(&str, Value)> = vec![
        ("bash", json!({"path": "a"})),
        ("read_file", json!(null)),
        ("read_file", json!([1])),
        ("read_file", json!({})),
        ("read_file", json!({"file": "a"})),
        ("read_file", json!({"path": 1})),
        ("read_file", json!({"path": "a", "mode": "w"})),
        ("read_file", json!({"path": long})),
        ("find_dir", json!({"path": "a"})),
        ("find_dir", json!({"query": null})),
        ("list_dir", json!({"path": ["a"]})),
        ("run_in_pane", json!({"command": "ls", "enter": true})),
        ("run_in_pane", json!({"command": long})),
        // Error order: wrong type before extra key, extra key before length.
        ("read_file", json!({"path": 1, "x": 1})),
        ("read_file", json!({"path": long, "x": 1})),
        // The cap is in bytes, not chars; the limit itself is accepted.
        ("read_file", json!({"path": wide})),
        ("read_file", json!({"path": ""})),
        (
            "read_file",
            json!({"path": "a".repeat(super::MAX_ARG_BYTES)}),
        ),
    ];
    let mut out = String::new();
    for (name, args) in cases {
        let text = match parse_call(name, &args) {
            Ok(call) => format!("ok {}", call.name()),
            Err(error) => refusal_text(error).to_owned(),
        };
        let shown = args.to_string();
        let shown = shown.chars().take(40).collect::<String>();
        out.push_str(&format!("{name} {shown} => {text}\n"));
    }
    out
}

#[test]
fn tools_list_bytes_match_the_golden() {
    assert_eq!(tools_list_bytes(), TOOLS_LIST);
}

#[test]
fn parse_error_texts_match_the_golden() {
    assert_eq!(parse_error_table(), PARSE_ERRORS);
}

#[test]
fn the_golden_is_not_vacuous() {
    assert!(TOOLS_LIST.contains("\"run_in_pane\""));
    assert!(PARSE_ERRORS.contains("=> unexpected argument"));
    assert!(PARSE_ERRORS.contains("=> argument too long"));
}

// Spec (code-write, Family and toggle; agent-tools, Off equals 0.18.0): with
// every toggle off a write tool is not listed and a call to it is answered
// byte for byte like a name that does not exist.
#[test]
fn a_write_tool_with_the_toggle_off_is_answered_like_an_unknown_name() {
    let call = |name: &str, args: Value| {
        let mut server = Server::new(registry(&[]));
        let line = |id: u64, method: &str, params: Value| {
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string()
        };
        server.handle(&line(
            1,
            "initialize",
            json!({"protocolVersion": "2025-06-18"}),
        ));
        let params = json!({"name": name, "arguments": args});
        match server.handle(&line(2, "tools/call", params)) {
            Action::Reply(value) => value.to_string(),
            other => panic!("expected a refusal, got {other:?}"),
        }
    };
    let unknown = call("bash", json!({"path": "a"}));
    assert!(unknown.contains("unknown tool"), "{unknown}");
    for (name, args) in [
        ("create_dir", json!({"path": "d"})),
        ("write_file", json!({"path": "a", "content": "x"})),
        ("edit_file", json!({"path": "a", "old": "x", "new": "y"})),
        ("propose_batch", json!({})),
    ] {
        assert_eq!(call(name, args), unknown, "{name}");
        assert!(!TOOLS_LIST.contains(name), "{name}");
    }
}
