//! A write-tool call a model wrote as text instead of calling it.
//!
//! Some local models answer with `{"name": "write_file", "arguments": ...}`
//! in their final text, bare or in a code fence, and no tool runs. The check
//! is conservative: it parses real JSON and wants a tool the worker was
//! offered together with its arguments, so prose that names a tool never
//! counts.

use serde_json::{Deserializer, Value};

use crate::core::ai::tools::registry::{Family, FamilySet};
use crate::core::ai::tools::registry_with;

/// The most text looked at, and the most places tried in it: a worker's
/// answer is untrusted and may be long.
const SCAN_BYTES: usize = 64 * 1024;
const MAX_STARTS: usize = 256;

/// The write tools a worker with the Write family is offered, by name.
pub fn write_tool_names() -> Vec<String> {
    registry_with(&[], FamilySet::EMPTY.with(Family::Write))
        .into_iter()
        .map(|tool| tool.name)
        .collect()
}

/// Whether `text` holds a call to one of the `offered` tools written as a
/// JSON object: `{"name"|"tool": .., "arguments"|"parameters": ..}`, or the
/// same under `function`. The object may be bare, in a code fence, in a
/// `<tool_call>` tag or in a list.
pub fn mentions_call(text: &str, offered: &[String]) -> bool {
    if offered.is_empty() {
        return false;
    }
    let mut end = text.len().min(SCAN_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let text = &text[..end];
    text.match_indices('{')
        .take(MAX_STARTS)
        .filter_map(|(at, _)| {
            Deserializer::from_str(&text[at..])
                .into_iter::<Value>()
                .next()?
                .ok()
        })
        .any(|value| is_call(&value, offered))
}

fn is_call(value: &Value, offered: &[String]) -> bool {
    let named = |object: &Value| {
        let name = object.get("name").or_else(|| object.get("tool"));
        let has_args = object.get("arguments").is_some() || object.get("parameters").is_some();
        has_args
            && name
                .and_then(Value::as_str)
                .is_some_and(|name| offered.iter().any(|tool| tool == name))
    };
    named(value) || value.get("function").is_some_and(named)
}
