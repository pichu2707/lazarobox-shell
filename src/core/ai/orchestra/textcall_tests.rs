//! A tool call written as text (orchestrator worker flow, tool calls as text).

use super::textcall::mentions_call;

fn offered() -> Vec<String> {
    ["write_file", "edit_file", "create_dir", "propose_batch"]
        .map(str::to_owned)
        .to_vec()
}

fn seen(text: &str) -> bool {
    mentions_call(text, &offered())
}

#[test]
fn a_bare_name_and_arguments_object_is_a_call() {
    assert!(seen(
        r#"{"name": "write_file", "arguments": {"path": "a.rs", "content": "x"}}"#
    ));
}

#[test]
fn parameters_instead_of_arguments_is_a_call() {
    assert!(seen(
        r#"{"name": "edit_file", "parameters": {"path": "a", "old": "b", "new": "c"}}"#
    ));
}

#[test]
fn a_tool_key_is_a_call() {
    assert!(seen(
        r#"{"tool": "create_dir", "arguments": {"path": "src"}}"#
    ));
}

#[test]
fn the_openai_function_shape_is_a_call() {
    assert!(seen(
        r#"{"type": "function", "function": {"name": "write_file", "arguments": "{\"path\":\"a\"}"}}"#
    ));
}

#[test]
fn a_call_inside_a_code_fence_is_a_call() {
    assert!(seen(
        "Here it is:\n```json\n{\"name\": \"write_file\", \"arguments\": {\"path\": \"a\"}}\n```\nDone."
    ));
}

#[test]
fn a_tool_call_tag_is_a_call() {
    assert!(seen(
        "<tool_call>\n{\"name\": \"propose_batch\", \"arguments\": {}}\n</tool_call>"
    ));
}

#[test]
fn a_call_in_a_list_of_calls_is_a_call() {
    assert!(seen(
        r#"[{"name": "create_dir", "arguments": {"path": "s"}}, {"name": "write_file", "arguments": {}}]"#
    ));
}

// Guard: prose that names the tool is not a call.
#[test]
fn guard_prose_naming_the_tool_is_not_a_call() {
    assert!(!seen(
        "I will use write_file to create the file, then propose_batch."
    ));
    assert!(!seen(r#"Call "write_file" with {"path": "a"} please"#));
}

// Guard: a tool the worker was not offered is not a call to report.
#[test]
fn guard_a_tool_that_was_not_offered_is_not_a_call() {
    assert!(!seen(
        r#"{"name": "delete_file", "arguments": {"path": "a"}}"#
    ));
    assert!(!seen(
        r#"{"name": "read_file", "arguments": {"path": "a"}}"#
    ));
}

// Guard: an object that only has a `name` is data, not a call.
#[test]
fn guard_a_name_without_arguments_is_not_a_call() {
    assert!(!seen(r#"{"name": "write_file"}"#));
    assert!(!seen(
        r#"{"name": "write_file", "description": "stages a file"}"#
    ));
}

// Guard: a name that is not text is not a call.
#[test]
fn guard_a_name_that_is_not_text_is_not_a_call() {
    assert!(!seen(r#"{"name": ["write_file"], "arguments": {}}"#));
}

// Guard: broken JSON and empty text are not calls.
#[test]
fn guard_broken_json_and_empty_text_are_not_calls() {
    assert!(!seen(r#"{"name": "write_file", "arguments": "#));
    assert!(!seen(""));
}

// Guard: nothing is offered, nothing is a call.
#[test]
fn guard_no_offered_tool_means_no_call() {
    assert!(!mentions_call(
        r#"{"name": "write_file", "arguments": {}}"#,
        &[]
    ));
}
