//! Response-shape checks and version parsing. Pure.

use serde_json::Value;

/// Models of a valid Ollama `/api/tags` body, or `None` for a foreign one.
pub fn ollama_tags(body: &[u8]) -> Option<Vec<String>> {
    Some(string_field(&array_field(body, "models")?, "name"))
}

/// The array under `key` of a JSON object body.
fn array_field(body: &[u8], key: &str) -> Option<Vec<Value>> {
    match serde_json::from_slice::<Value>(body).ok()? {
        Value::Object(mut obj) => match obj.remove(key)? {
            Value::Array(items) => Some(items),
            _ => None,
        },
        _ => None,
    }
}

/// String values of `key` in the object entries; other entries are skipped.
fn string_field(items: &[Value], key: &str) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| item.get(key)?.as_str().map(sanitize))
        .collect()
}

/// Models of a valid OpenAI-style `/v1/models` body. With an `owner`, at
/// least one entry must carry that `owned_by` marker.
pub fn openai_models(body: &[u8], owner: Option<&str>) -> Option<Vec<String>> {
    let items = array_field(body, "data")?;
    if let Some(owner) = owner
        && !string_field(&items, "owned_by").iter().any(|o| o == owner)
    {
        return None;
    }
    Some(string_field(&items, "id"))
}

/// Drops ANSI CSI sequences (`ESC [ params final`) and any remaining control
/// character, so engine output cannot inject terminal escapes into the UI.
fn sanitize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            while chars
                .next_if(|n| ('\u{20}'..='\u{3f}').contains(n))
                .is_some()
            {}
            chars.next_if(|n| ('\u{40}'..='\u{7e}').contains(n));
        } else if !c.is_control() {
            out.push(c);
        }
    }
    out
}

/// First non-empty stdout line, sanitized, trimmed and capped at 40
/// characters.
pub fn parse_version(stdout: &[u8]) -> Option<String> {
    String::from_utf8_lossy(stdout)
        .lines()
        .map(|line| sanitize(line).trim().to_string())
        .find(|line| !line.is_empty())
        .map(|line| {
            let capped: String = line.chars().take(40).collect();
            capped.trim_end().to_string()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strs(v: &[&str]) -> Option<Vec<String>> {
        Some(v.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn ollama_lists_models_in_order() {
        let body = br#"{"models":[{"name":"llama3:8b"},{"name":"qwen2.5:7b"}]}"#;
        assert_eq!(ollama_tags(body), strs(&["llama3:8b", "qwen2.5:7b"]));
        assert_eq!(ollama_tags(br#"{"models":[]}"#), Some(vec![]));
    }

    #[test]
    fn ollama_ignores_partial_entries() {
        let body = br#"{"models":[{"name":"a"},{"tag":"b"},3]}"#;
        assert_eq!(ollama_tags(body), strs(&["a"]));
    }

    #[test]
    fn ollama_rejects_foreign_bodies() {
        assert_eq!(ollama_tags(br#"{"status":"ok"}"#), None);
        assert_eq!(ollama_tags(b"[]"), None);
        assert_eq!(ollama_tags(br#"{"models":"x"}"#), None);
        assert_eq!(ollama_tags(b"<html><body>It works</body></html>"), None);
    }
    #[test]
    fn lmstudio_checks_shape_only() {
        let body = br#"{"object":"list","data":[{"id":"gemma-3-4b"}]}"#;
        assert_eq!(openai_models(body, None), strs(&["gemma-3-4b"]));
        assert_eq!(openai_models(br#"{"data":[]}"#, None), Some(vec![]));
    }

    #[test]
    fn marker_is_required_for_owned_servers() {
        let llama = br#"{"data":[{"id":"m.gguf","owned_by":"llamacpp"},{"id":"x"},{"id":3}]}"#;
        assert_eq!(
            openai_models(llama, Some("llamacpp")),
            strs(&["m.gguf", "x"])
        );
        assert_eq!(openai_models(llama, Some("vllm")), None);
        let vllm = br#"{"data":[{"id":"qwen","owned_by":"vllm"}]}"#;
        assert_eq!(openai_models(vllm, Some("vllm")), strs(&["qwen"]));
        let foreign = br#"{"data":[{"id":"gpt-x","owned_by":"localai"}]}"#;
        assert_eq!(openai_models(foreign, Some("llamacpp")), None);
        assert_eq!(openai_models(br#"{"data":[]}"#, Some("llamacpp")), None);
    }

    #[test]
    fn openai_rejects_foreign_bodies() {
        for body in [
            &br#"{"status":"ok"}"#[..],
            br#"{"data":"x"}"#,
            b"[]",
            b"<html><body>It works</body></html>",
        ] {
            assert_eq!(openai_models(body, None), None);
            assert_eq!(openai_models(body, Some("llamacpp")), None);
        }
        assert_eq!(openai_models(br#"{"data":[]}"#, Some("llamacpp")), None);
    }
    #[test]
    fn version_is_the_first_non_empty_line() {
        let some = |s: &str| Some(s.to_string());
        assert_eq!(parse_version(b"codex 0.5.0\n"), some("codex 0.5.0"));
        assert_eq!(
            parse_version(b"\n\r\n  \r\ncodex 0.5.0\r\nother\r\n"),
            some("codex 0.5.0")
        );
    }

    #[test]
    fn version_is_capped_at_forty_characters() {
        let some = |s: String| Some(s);
        assert_eq!(
            parse_version("a".repeat(40).as_bytes()),
            some("a".repeat(40))
        );
        assert_eq!(
            parse_version("a".repeat(41).as_bytes()),
            some("a".repeat(40))
        );
    }

    #[test]
    fn version_strips_ansi_and_control_characters() {
        let some = |s: &str| Some(s.to_string());
        assert_eq!(
            parse_version(b"\x1b[31mcodex 1.0\x1b[0m\n"),
            some("codex 1.0")
        );
        assert_eq!(parse_version(b"co\x07de\x00x 1\x1b"), some("codex 1"));
        assert_eq!(parse_version("a\u{9b}b\u{85}c".as_bytes()), some("abc"));
        assert_eq!(parse_version(b"\x1b[0m\ncodex 2"), some("codex 2"));
    }

    #[test]
    fn version_is_trimmed_again_after_the_cap() {
        let raw = format!("{} tail", "a".repeat(39));
        assert_eq!(parse_version(raw.as_bytes()), Some("a".repeat(39)));
    }

    #[test]
    fn model_names_are_sanitized() {
        let body = "{\"models\":[{\"name\":\"ll\\u001b[31mama\\n3\"}]}";
        assert_eq!(ollama_tags(body.as_bytes()), strs(&["llama3"]));
        let openai = "{\"data\":[{\"id\":\"a\\u001b[0mb\\u0007\"}]}";
        assert_eq!(openai_models(openai.as_bytes(), None), strs(&["ab"]));
    }

    #[test]
    fn empty_output_has_no_version() {
        assert_eq!(parse_version(b""), None);
        assert_eq!(parse_version(b" \r\n\t\n"), None);
    }
}
