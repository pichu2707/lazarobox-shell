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
        .filter_map(|item| item.get(key)?.as_str().map(str::to_owned))
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

/// First non-empty stdout line, trimmed and capped at 40 characters.
pub fn parse_version(stdout: &[u8]) -> Option<String> {
    String::from_utf8_lossy(stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.chars().take(40).collect())
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
    fn empty_output_has_no_version() {
        assert_eq!(parse_version(b""), None);
        assert_eq!(parse_version(b" \r\n\t\n"), None);
    }
}
