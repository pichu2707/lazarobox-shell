//! Response-shape checks and version parsing. Pure.

use serde_json::Value;

use super::sanitize::clean;

/// `clean` for one-line text: newlines and tabs go too.
fn sanitize(text: &str) -> String {
    clean(text).chars().filter(|c| !c.is_control()).collect()
}

/// Models of a valid Ollama `/api/tags` body, or `None` for a foreign one.
pub fn ollama_tags(body: &[u8]) -> Option<Vec<String>> {
    Some(model_names(&array_field(body, "models")?, "name"))
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

/// Model names under `key`: sanitized, non-empty, each capped at
/// `MAX_NAME` characters, at most `MAX_MODELS` of them.
fn model_names(items: &[Value], key: &str) -> Vec<String> {
    string_field(items, key)
        .into_iter()
        .filter(|name| !name.is_empty())
        .map(|name| name.chars().take(MAX_NAME).collect())
        .take(MAX_MODELS)
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
    Some(model_names(&items, "id"))
}

/// Most codex models kept.
const MAX_CODEX_MODELS: usize = 64;

/// The listed models of a `codex debug models` output as `(label, slug)`,
/// ordered by the account's priority. Only `visibility == "list"` entries
/// count. A slug must be usable as a CLI model value (same rules as the chat
/// adapters: no leading `-`, no whitespace or control characters, at most
/// `MAX_NAME` characters) and not `default`; later duplicates are dropped.
/// The label is the display name, or the slug when that is empty. Anything
/// malformed yields an empty list.
pub fn codex_models(raw: &str) -> Vec<(String, String)> {
    let Some(items) = array_field(raw.as_bytes(), "models") else {
        return Vec::new();
    };
    let mut found: Vec<(i64, String, String)> = items
        .iter()
        .filter(|item| item.get("visibility").and_then(Value::as_str) == Some("list"))
        .filter_map(|item| {
            let slug = item.get("slug")?.as_str()?;
            if !valid_slug(slug) {
                return None;
            }
            let label = item
                .get("display_name")
                .and_then(Value::as_str)
                .map(sanitize)
                .map(|l| l.trim().chars().take(MAX_NAME).collect::<String>())
                .filter(|l| !l.is_empty())
                .unwrap_or_else(|| slug.to_owned());
            let priority = item.get("priority").and_then(Value::as_i64);
            Some((priority.unwrap_or(i64::MAX), label, slug.to_owned()))
        })
        .collect();
    // Stable: equal priorities keep the order codex printed.
    found.sort_by_key(|(priority, ..)| *priority);
    let mut seen = std::collections::HashSet::new();
    found
        .into_iter()
        .filter(|(_, _, slug)| seen.insert(slug.clone()))
        .map(|(_, label, slug)| (label, slug))
        .take(MAX_CODEX_MODELS)
        .collect()
}

fn valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug != "default"
        && slug.chars().count() <= MAX_NAME
        && !slug.starts_with('-')
        && !slug.chars().any(|c| c.is_control() || c.is_whitespace())
}

/// Longest model name kept, in characters.
const MAX_NAME: usize = 128;

/// Most models kept from one engine.
const MAX_MODELS: usize = 256;

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

    const CODEX_FIXTURE: &str = include_str!("../../../tests/fixtures/codex/debug-models.json");

    fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter()
            .map(|(l, s)| (l.to_string(), s.to_string()))
            .collect()
    }

    #[test]
    fn codex_fixture_keeps_listed_models_by_priority() {
        assert_eq!(
            codex_models(CODEX_FIXTURE),
            pairs(&[
                ("GPT-6.1-Sol", "gpt-6.1-sol"),
                ("GPT-6-Astra", "gpt-6-astra"),
                ("GPT-6-Sol", "gpt-6-sol"),
                ("GPT-6-Luna", "gpt-6-luna"),
                ("GPT-5.6-Sol", "gpt-5.6-sol"),
                ("GPT-5.6-Terra", "gpt-5.6-terra"),
                ("GPT-5.6-Luna", "gpt-5.6-luna"),
            ])
        );
    }

    #[test]
    fn codex_sorts_by_priority_and_keeps_ties_stable() {
        let raw = r#"{"models":[
            {"slug":"c","display_name":"C","visibility":"list","priority":3},
            {"slug":"a","display_name":"A","visibility":"list","priority":1},
            {"slug":"b2","display_name":"B2","visibility":"list","priority":2},
            {"slug":"b1","display_name":"B1","visibility":"list","priority":2},
            {"slug":"z","display_name":"Z","visibility":"list"}]}"#;
        let slugs: Vec<_> = codex_models(raw).into_iter().map(|(_, s)| s).collect();
        assert_eq!(slugs, ["a", "b2", "b1", "c", "z"]);
    }

    #[test]
    fn codex_label_falls_back_to_the_slug() {
        let raw = r#"{"models":[
            {"slug":"a","visibility":"list","priority":1},
            {"slug":"b","display_name":"\u001b[31m","visibility":"list","priority":2}]}"#;
        assert_eq!(codex_models(raw), pairs(&[("a", "a"), ("b", "b")]));
    }

    #[test]
    fn codex_drops_invalid_hidden_and_duplicate_slugs() {
        let long = "x".repeat(129);
        let raw = format!(
            r#"{{"models":[
            {{"slug":"-flag","display_name":"F","visibility":"list","priority":1}},
            {{"slug":"has space","display_name":"S","visibility":"list","priority":2}},
            {{"slug":"tab\there","display_name":"T","visibility":"list","priority":3}},
            {{"slug":"nl\n","display_name":"N","visibility":"list","priority":4}},
            {{"slug":"","display_name":"E","visibility":"list","priority":5}},
            {{"slug":"default","display_name":"D","visibility":"list","priority":6}},
            {{"slug":"{long}","display_name":"L","visibility":"list","priority":7}},
            {{"slug":"hid","display_name":"H","visibility":"hide","priority":8}},
            {{"slug":"nov","display_name":"V","priority":9}},
            {{"slug":7,"display_name":"Num","visibility":"list","priority":10}},
            {{"slug":"ok","display_name":"OK","visibility":"list","priority":11}},
            {{"slug":"ok","display_name":"Again","visibility":"list","priority":12}},
            "junk", null]}}"#
        );
        assert_eq!(codex_models(&raw), pairs(&[("OK", "ok")]));
    }

    #[test]
    fn codex_caps_the_list_at_sixty_four() {
        let items: Vec<_> = (0..100)
            .map(|i| {
                format!(
                    r#"{{"slug":"m{i}","display_name":"M{i}","visibility":"list","priority":{i}}}"#
                )
            })
            .collect();
        let raw = format!(r#"{{"models":[{}]}}"#, items.join(","));
        let got = codex_models(&raw);
        assert_eq!(got.len(), 64);
        assert_eq!(got[63].1, "m63");
    }

    #[test]
    fn codex_malformed_inputs_are_empty() {
        for raw in [
            "",
            "not json",
            "[]",
            "{}",
            r#"{"models":{}}"#,
            r#"{"models":"x"}"#,
            r#"{"models":[]}"#,
            r#"{"models":[{"slug":"a""#,
        ] {
            assert_eq!(codex_models(raw), Vec::<(String, String)>::new(), "{raw}");
        }
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

    #[test]
    fn sanitize_strips_osc_sequences_with_either_terminator() {
        assert_eq!(sanitize("\x1b]0;title\x07x"), "x");
        assert_eq!(sanitize("\x1b]0;title\x1b\\x"), "x");
        assert_eq!(sanitize("a\x1b]8;;http://e\x07link\x1b]8;;\x07b"), "alinkb");
        assert_eq!(sanitize("a\x1b]0;never ends"), "a");
    }

    #[test]
    fn sanitize_covers_csi_intermediate_and_final_byte_edges() {
        assert_eq!(sanitize("\x1b[1 qx"), "x");
        assert_eq!(sanitize("\x1b[@x"), "x");
        assert_eq!(sanitize("\x1b[?25~x"), "x");
        assert_eq!(sanitize("\x1b[/~x"), "x");
        assert_eq!(sanitize("\x1b[\u{7f}x"), "x");
    }

    #[test]
    fn sanitize_drops_bidi_and_format_characters() {
        assert_eq!(sanitize("exe\u{202e}gnp.txt"), "exegnp.txt");
        for c in [
            '\u{200b}',
            '\u{200f}',
            '\u{202a}',
            '\u{202e}',
            '\u{2066}',
            '\u{2069}',
            '\u{feff}',
            '\u{061c}',
            '\u{2028}',
            '\u{2029}',
            '\u{00ad}',
            '\u{2060}',
            '\u{2064}',
            '\u{206a}',
            '\u{206f}',
            '\u{e0000}',
            '\u{e0041}',
            '\u{e007f}',
        ] {
            assert_eq!(sanitize(&format!("a{c}b")), "ab", "U+{:04X}", c as u32);
        }
        assert_eq!(
            sanitize("a\u{200a}\u{2010}\u{2027}\u{202f}\u{2065}\u{2070}\u{00ac}\u{00ae}\u{e0080}b"),
            "a\u{200a}\u{2010}\u{2027}\u{202f}\u{2065}\u{2070}\u{00ac}\u{00ae}\u{e0080}b"
        );
    }

    fn names(n: usize, len: usize) -> Vec<u8> {
        let items: Vec<String> = (0..n)
            .map(|i| {
                format!(
                    "{{\"name\":\"{}\"}}",
                    format!("{i:x}")
                        .repeat(len)
                        .chars()
                        .take(len)
                        .collect::<String>()
                )
            })
            .collect();
        format!("{{\"models\":[{}]}}", items.join(",")).into_bytes()
    }

    #[test]
    fn model_names_are_capped_at_128_characters() {
        let long = |n: usize| format!("{{\"models\":[{{\"name\":\"{}\"}}]}}", "a".repeat(n));
        assert_eq!(ollama_tags(long(128).as_bytes()), strs(&[&"a".repeat(128)]));
        assert_eq!(ollama_tags(long(129).as_bytes()), strs(&[&"a".repeat(128)]));
        let wide = format!("{{\"data\":[{{\"id\":\"{}\"}}]}}", "é".repeat(200));
        assert_eq!(
            openai_models(wide.as_bytes(), None),
            strs(&[&"é".repeat(128)])
        );
    }

    #[test]
    fn the_model_list_is_capped_at_256_entries() {
        assert_eq!(ollama_tags(&names(256, 3)).map(|m| m.len()), Some(256));
        assert_eq!(ollama_tags(&names(257, 3)).map(|m| m.len()), Some(256));
    }

    #[test]
    fn names_that_sanitize_to_empty_are_dropped() {
        let body = "{\"models\":[{\"name\":\"\\u001b[0m\"},{\"name\":\"\"},{\"name\":\"\\u202e\"},{\"name\":\"ok\"}]}";
        assert_eq!(ollama_tags(body.as_bytes()), strs(&["ok"]));
        let openai = "{\"data\":[{\"id\":\"\\u001b]0;t\\u0007\"},{\"id\":\"m\"}]}";
        assert_eq!(openai_models(openai.as_bytes(), None), strs(&["m"]));
    }
}
