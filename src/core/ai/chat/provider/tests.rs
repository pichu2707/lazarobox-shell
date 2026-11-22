use unicode_width::UnicodeWidthChar;

use super::*;

#[test]
fn classify_table() {
    let cases = [
        ("claude", Some("claude-opus-4"), Provider::Anthropic),
        ("claude", None, Provider::Anthropic),
        ("gemini", Some("gemini-2.5-pro"), Provider::Google),
        ("codex", Some("gpt-5"), Provider::OpenAi),
        ("ollama", Some("llama3"), Provider::Local),
        ("llamacpp", None, Provider::Local),
        ("lmstudio", Some("claude-x"), Provider::Local),
        ("vllm", Some("gpt-oss"), Provider::Local),
        ("foo", Some("claude-3"), Provider::Anthropic),
        ("foo", Some("gemini-1"), Provider::Google),
        ("foo", Some("gpt-4o"), Provider::OpenAi),
        ("foo", Some("o3-mini"), Provider::OpenAi),
        ("foo", Some("codex-mini"), Provider::OpenAi),
        ("foo", Some("bar"), Provider::Other),
        ("foo/bar", None, Provider::Other),
    ];
    for (engine, model, want) in cases {
        assert_eq!(classify(engine, model), want, "{engine} {model:?}");
    }
}

#[test]
fn labels_are_distinct_and_named() {
    assert_eq!(Provider::Anthropic.label(), "Anthropic");
    assert_eq!(Provider::OpenAi.label(), "OpenAI");
    let labels: Vec<_> = Provider::ALL.iter().map(|p| p.label()).collect();
    assert_eq!(labels.len(), 5);
    assert!(labels.iter().all(|l| !l.is_empty()));
}

#[test]
fn every_provider_glyph_is_one_column_everywhere() {
    for p in Provider::ALL {
        let g = p.glyph();
        assert_eq!(g.width(), Some(1), "{p:?}");
        assert_eq!(g.width_cjk(), Some(1), "{p:?} is East Asian Ambiguous");
        assert_ne!(g, '\u{FE0F}');
        assert_ne!(g, '\u{25CE}');
        assert_eq!(format!("{g}").chars().count(), 1);
    }
}
