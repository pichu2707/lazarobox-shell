use unicode_width::UnicodeWidthChar;

use super::*;

/// Characters that default to emoji presentation (Unicode `Emoji_Presentation`
/// = Yes): everything from U+1F000 up, plus these BMP ranges. They draw two
/// columns wide in many terminals, so a provider glyph must not be one.
fn emoji_presentation(c: char) -> bool {
    const BMP: [(u32, u32); 28] = [
        (0x231A, 0x231B),
        (0x23E9, 0x23EC),
        (0x23F0, 0x23F0),
        (0x23F3, 0x23F3),
        (0x25FD, 0x25FE),
        (0x2614, 0x2615),
        (0x2648, 0x2653),
        (0x267F, 0x267F),
        (0x2693, 0x2693),
        (0x26A1, 0x26A1),
        (0x26AA, 0x26AB),
        (0x26BD, 0x26BE),
        (0x26C4, 0x26C5),
        (0x26CE, 0x26CE),
        (0x26D4, 0x26D4),
        (0x26EA, 0x26EA),
        (0x26F2, 0x26F3),
        (0x26F5, 0x26F5),
        (0x26FA, 0x26FA),
        (0x26FD, 0x26FD),
        (0x2705, 0x2705),
        (0x270A, 0x270B),
        (0x2728, 0x2728),
        (0x274C, 0x274C),
        (0x274E, 0x274E),
        (0x2753, 0x2757),
        (0x2795, 0x2797),
        (0x2B1B, 0x2B1C),
    ];
    let u = u32::from(c);
    u >= 0x1F000
        || [0x27B0, 0x27BF, 0x2B50, 0x2B55].contains(&u)
        || BMP.iter().any(|&(a, b)| (a..=b).contains(&u))
}

#[test]
fn the_emoji_check_catches_emoji() {
    assert!(emoji_presentation('\u{2705}'));
    assert!(emoji_presentation('\u{1F600}'));
    assert!(!emoji_presentation('\u{273B}'));
}

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
        ("foo", Some("o1"), Provider::OpenAi),
        ("foo", Some("o4-mini"), Provider::OpenAi),
        ("foo", Some("o3:latest"), Provider::OpenAi),
        ("foo", Some("o1x"), Provider::Other),
        ("foo", Some("o3d-mini"), Provider::Other),
        ("foo", Some("o4a"), Provider::Other),
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
        assert!(!emoji_presentation(g), "{p:?} would draw as a wide emoji");
    }
}
