//! A tool call written as text (local-model-writes D3). A small local model
//! sometimes answers with the JSON of a call instead of making it. The loop
//! asks it once to call the tool; nothing in the text is ever run.

use serde_json::Value;

/// The system line a pane gets when the model was reminded.
pub const TEXT_CALL_NOTICE: &str =
    "the model wrote a tool call as text; it was asked to call the tool";

/// How many `{` of an answer are tried as the start of a call: an answer is
/// capped, and this keeps the scan linear in practice.
const MAX_STARTS: usize = 64;

/// The name of the first offered tool that `text` writes a call to: a JSON
/// object, bare or fenced, with that `name` and an `arguments` (or
/// `parameters`) object. `None` for an answer that only names a tool.
pub fn written_call(text: &str, offered: &[String]) -> Option<String> {
    text.match_indices('{')
        .take(MAX_STARTS)
        .find_map(|(start, _)| {
            let mut values =
                serde_json::Deserializer::from_str(&text[start..]).into_iter::<Value>();
            match values.next() {
                Some(Ok(value)) => call_name(&value, offered),
                _ => None,
            }
        })
}

fn call_name(value: &Value, offered: &[String]) -> Option<String> {
    let name = value.get("name")?.as_str()?;
    let arguments = value.get("arguments").or_else(|| value.get("parameters"))?;
    (arguments.is_object() && offered.iter().any(|tool| tool == name)).then(|| name.to_owned())
}

/// The message that follows the answer in the reminder request.
pub fn reminder(name: &str) -> String {
    format!(
        "Your answer wrote a call to `{name}` as text. Text does nothing. To act, call the tool."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offered() -> Vec<String> {
        vec!["edit_file".into(), "list_dir".into()]
    }

    #[test]
    fn a_fenced_or_bare_call_to_an_offered_tool_is_found() {
        let fenced = "I will edit it:\n```json\n{\"name\": \"edit_file\", \"arguments\": {\"path\": \"a.py\", \"old\": \"\", \"new\": \"x\"}}\n```";
        assert_eq!(written_call(fenced, &offered()), Some("edit_file".into()));
        let bare = r#"{"name": "list_dir", "parameters": {"path": "."}}"#;
        assert_eq!(written_call(bare, &offered()), Some("list_dir".into()));
        let nested =
            r#"<tool_call>{"function": {"name": "list_dir", "arguments": {}}}</tool_call>"#;
        assert_eq!(written_call(nested, &offered()), Some("list_dir".into()));
    }

    #[test]
    fn a_tool_named_without_arguments_or_not_offered_is_not_a_call() {
        for text in [
            "Use edit_file to change it.",
            r#"{"name": "edit_file"}"#,
            r#"{"name": "edit_file", "arguments": "path=a.py"}"#,
            r#"{"name": "rm_rf", "arguments": {"path": "/"}}"#,
            "{ not json",
            "",
        ] {
            assert_eq!(written_call(text, &offered()), None, "{text}");
        }
    }

    #[test]
    fn the_reminder_names_the_tool() {
        assert_eq!(
            reminder("edit_file"),
            "Your answer wrote a call to `edit_file` as text. Text does nothing. To act, call the tool."
        );
    }
}
