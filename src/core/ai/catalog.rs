//! Static model catalogs for the AI picker.
//!
//! The Ollama list is informational and may be out of date: it is never
//! fetched or refreshed, and lazarobox never downloads, pulls or installs a
//! model. LM Studio, llama.cpp and vLLM ids depend on local files, so they
//! have no curated entries.

/// Well-known Ollama tags, shown as `download required` when not detected.
/// Informational and possibly stale. Explicit size tags avoid the
/// `:latest` ambiguity.
pub const OLLAMA_CURATED: &[&str] = &[
    "llama3.2:3b",
    "llama3.1:8b",
    "qwen3:8b",
    "qwen2.5-coder:7b",
    "qwen3-coder:30b",
    "gemma3:4b",
    "mistral:7b",
    "deepseek-r1:8b",
    "phi4:14b",
    "gpt-oss:20b",
];

/// The static model list of a CLI engine as `(label, value)` pairs; empty for
/// any other id. The label is what the picker shows, the value is what is
/// saved and passed to the CLI. claude uses the CLI aliases, which always
/// resolve to the newest model of each family. gemini uses explicit ids: its
/// own `flash` and `pro` aliases may still point at retired 2.5 models.
/// opencode has only `default` here: the rest of its models are listed by the
/// CLI itself at detection (`opencode models`) and added by the picker.
pub fn cli_models(engine: &str) -> &'static [(&'static str, &'static str)] {
    match engine {
        "claude" => &[
            ("default (CLI picks)", "default"),
            ("Opus 5.5", "opus"),
            ("Sonnet 5.5", "sonnet"),
            ("Haiku 5.5", "haiku"),
            ("Fable 5.1", "fable"),
        ],
        "gemini" => &[
            ("default (auto)", "default"),
            ("Flash 3.8", "gemini-3.8-flash"),
            ("Flash 3.5", "gemini-3.5-flash"),
            ("Flash Lite 3.5", "gemini-3.5-flash-lite"),
        ],
        "codex" | "opencode" => &[("default", "default")],
        _ => &[],
    }
}

/// The curated models of an engine; only Ollama has any.
pub fn curated(engine: &str) -> &'static [&'static str] {
    match engine {
        "ollama" => OLLAMA_CURATED,
        _ => &[],
    }
}

/// Whether two model names are the same model for `engine`. Matching is
/// exact, except that an Ollama name without a tag equals `:latest`.
pub fn same_model(engine: &str, a: &str, b: &str) -> bool {
    if engine == "ollama" {
        with_tag(a) == with_tag(b)
    } else {
        a == b
    }
}

/// The name with `:latest` appended when its last path segment has no tag.
fn with_tag(name: &str) -> std::borrow::Cow<'_, str> {
    let segment = name.rsplit('/').next().unwrap_or(name);
    if segment.contains(':') {
        name.into()
    } else {
        format!("{name}:latest").into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ai::CATALOG;

    #[test]
    fn curated_ollama_is_the_ten_documented_tags() {
        assert_eq!(
            OLLAMA_CURATED,
            [
                "llama3.2:3b",
                "llama3.1:8b",
                "qwen3:8b",
                "qwen2.5-coder:7b",
                "qwen3-coder:30b",
                "gemma3:4b",
                "mistral:7b",
                "deepseek-r1:8b",
                "phi4:14b",
                "gpt-oss:20b",
            ]
        );
    }

    #[test]
    fn curated_catalog_is_valid_and_ollama_only() {
        let names = curated("ollama");
        assert!((8..=12).contains(&names.len()));
        let mut seen = std::collections::HashSet::new();
        for name in names {
            assert!(!name.is_empty());
            assert!(seen.insert(*name), "duplicate {name}");
        }
        for engine in CATALOG.iter().filter(|e| e.id != "ollama") {
            assert_eq!(curated(engine.id), &[] as &[&str], "{}", engine.id);
        }
        assert_eq!(curated("nope"), &[] as &[&str]);
    }

    #[test]
    fn cli_catalogs() {
        assert_eq!(
            cli_models("claude"),
            [
                ("default (CLI picks)", "default"),
                ("Opus 5.5", "opus"),
                ("Sonnet 5.5", "sonnet"),
                ("Haiku 5.5", "haiku"),
                ("Fable 5.1", "fable"),
            ]
        );
        assert_eq!(
            cli_models("gemini"),
            [
                ("default (auto)", "default"),
                ("Flash 3.8", "gemini-3.8-flash"),
                ("Flash 3.5", "gemini-3.5-flash"),
                ("Flash Lite 3.5", "gemini-3.5-flash-lite"),
            ]
        );
        for id in ["codex", "opencode"] {
            assert_eq!(cli_models(id), [("default", "default")], "{id}");
        }
        for id in ["ollama", "lmstudio", "llamacpp", "vllm", "nope"] {
            assert_eq!(cli_models(id), &[] as &[(&str, &str)], "{id}");
        }
    }

    #[test]
    fn cli_values_are_valid_unique_cli_model_names() {
        for id in ["claude", "gemini", "codex", "opencode"] {
            let mut seen = std::collections::HashSet::new();
            for (label, value) in cli_models(id) {
                assert!(!label.is_empty() && !value.is_empty(), "{id}");
                assert!(!value.starts_with('-') && !value.contains(char::is_whitespace));
                assert!(seen.insert(*value), "duplicate {value}");
            }
            assert_eq!(cli_models(id)[0].1, "default", "{id}");
        }
    }

    #[test]
    fn same_model_is_exact_except_ollama_latest() {
        assert!(same_model("ollama", "llama3", "llama3:latest"));
        assert!(same_model("ollama", "llama3:latest", "llama3"));
        assert!(same_model("ollama", "llama3:8b", "llama3:8b"));
        assert!(!same_model("ollama", "llama3", "llama3:8b"));
        assert!(!same_model("ollama", "llama3", "llama32"));
        assert!(same_model("ollama", "host:5000/m", "host:5000/m:latest"));
        assert!(!same_model("lmstudio", "llama3", "llama3:latest"));
        assert!(same_model("lmstudio", "m.gguf", "m.gguf"));
        assert!(!same_model("lmstudio", "m.gguf", "M.gguf"));
    }
}
