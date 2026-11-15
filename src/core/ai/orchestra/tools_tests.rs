//! The coordinator's tool names and a call written as text under one of them.

use super::tools::{NAMES, parse, written_call};

#[test]
fn the_coordinator_has_seven_tools_and_parse_takes_each() {
    assert_eq!(NAMES.len(), 7);
    for name in NAMES {
        // A known name is never `UnknownTool`, whatever its arguments.
        let error = parse(name, &serde_json::json!(1)).unwrap_err();
        assert_eq!(error, super::tools::OrchError::NotAnObject, "{name}");
    }
}

#[test]
fn a_call_written_under_either_key_names_the_coordinator_tool() {
    let name = r#"{"name": "spawn_agent", "arguments": {"kind": "code"}}"#;
    let tool = "```json\n{\"tool\": \"handoff\", \"arguments\": {}}\n```";
    assert_eq!(written_call(name), Some("spawn_agent"));
    assert_eq!(written_call(tool), Some("handoff"));
}

#[test]
fn a_non_coordinator_name_or_plain_text_is_not_a_written_call() {
    for text in [
        r#"{"tool": "find_dir", "arguments": {}}"#,
        r#"{"name": "spawn_agent", "arguments": "kind=code"}"#,
        "Call spawn_agent now.",
        "",
    ] {
        assert_eq!(written_call(text), None, "{text}");
    }
}
