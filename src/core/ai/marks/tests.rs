use super::*;

#[test]
fn params_of_reads_the_ollama_size_strings() {
    assert_eq!(params_of("8.0B"), Some(8_000_000_000));
    assert_eq!(params_of("137M"), Some(137_000_000));
    assert_eq!(params_of("70.6B"), Some(70_600_000_000));
    assert_eq!(params_of("8x7B"), Some(56_000_000_000));
}

#[test]
fn params_of_accepts_lowercase_and_rejects_a_fractional_expert_count() {
    assert_eq!(params_of("8b"), Some(8_000_000_000));
    assert_eq!(params_of("1.5x7B"), None);
}

#[test]
fn params_of_rejects_what_is_not_a_size() {
    assert_eq!(params_of(""), None);
    assert_eq!(params_of("junk"), None);
    assert_eq!(params_of("B"), None);
}

#[test]
fn tools_and_three_billion_or_more_is_capable() {
    assert_eq!(
        tool_mark(Tools::Yes, Some(8_000_000_000)),
        Some(ToolMark::Capable)
    );
    assert_eq!(
        tool_mark(Tools::Yes, Some(3_000_000_000)),
        Some(ToolMark::Capable)
    );
}

#[test]
fn tools_under_three_billion_is_unreliable() {
    assert_eq!(
        tool_mark(Tools::Yes, Some(2_900_000_000)),
        Some(ToolMark::Unreliable)
    );
    assert_eq!(
        tool_mark(Tools::Yes, Some(137_000_000)),
        Some(ToolMark::Unreliable)
    );
}

#[test]
fn no_tools_unknown_or_unknown_size_gives_no_mark() {
    assert_eq!(tool_mark(Tools::No, Some(8_000_000_000)), None);
    assert_eq!(tool_mark(Tools::Unknown, Some(8_000_000_000)), None);
    assert_eq!(tool_mark(Tools::Yes, None), None);
}

#[test]
fn the_linux_console_gets_text() {
    assert_eq!(tool_glyph(Some("linux"), true), ToolGlyph::Text);
}

#[test]
fn a_non_utf8_locale_gets_text() {
    assert_eq!(tool_glyph(Some("xterm-256color"), false), ToolGlyph::Text);
    assert_eq!(tool_glyph(None, false), ToolGlyph::Text);
}

#[test]
fn a_utf8_terminal_gets_the_emoji() {
    assert_eq!(tool_glyph(Some("xterm-256color"), true), ToolGlyph::Emoji);
    assert_eq!(tool_glyph(None, true), ToolGlyph::Emoji);
}

#[test]
fn the_default_glyph_is_text() {
    assert_eq!(ToolGlyph::default(), ToolGlyph::Text);
}

#[test]
fn the_wrench_is_two_cells_wide() {
    use unicode_width::UnicodeWidthStr;
    assert_eq!(badge_text(ToolMark::Capable, ToolGlyph::Emoji).width(), 2);
}

#[test]
fn badge_text_maps_both_marks_in_both_modes() {
    assert_eq!(badge_text(ToolMark::Capable, ToolGlyph::Emoji), "🔧");
    assert_eq!(badge_text(ToolMark::Capable, ToolGlyph::Text), "tools");
    assert_eq!(
        badge_text(ToolMark::Unreliable, ToolGlyph::Emoji),
        "tools: unreliable"
    );
    assert_eq!(
        badge_text(ToolMark::Unreliable, ToolGlyph::Text),
        "tools: unreliable"
    );
}

#[test]
fn linux_console_variants_and_dumb_terminals_get_text() {
    assert_eq!(tool_glyph(Some("linux-16color"), true), ToolGlyph::Text);
    assert_eq!(tool_glyph(Some("linux-c"), true), ToolGlyph::Text);
    assert_eq!(tool_glyph(Some("dumb"), true), ToolGlyph::Text);
}

#[test]
fn a_term_that_only_contains_linux_still_gets_the_emoji() {
    assert_eq!(tool_glyph(Some("xterm-linux"), true), ToolGlyph::Emoji);
}
