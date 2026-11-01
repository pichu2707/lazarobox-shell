use super::*;
use crate::core::ai::marks::{ToolGlyph, ToolMark};

fn pick(label: &str, mark: Mark, current: bool, tools: Option<ToolMark>) -> Row {
    Row::Pick {
        label: label.to_owned(),
        mark,
        tools,
        selected: false,
        enabled: true,
        current,
    }
}

fn line(row: &Row, glyph: ToolGlyph, width: u16) -> String {
    let theme = LazaroboxTheme::default();
    let menu = MenuState::open(&Config::default());
    let config = Config::default();
    let popup = MenuPopup::new(&menu, &config, &theme).with_glyph(glyph);
    popup.line(row, width).0
}

#[test]
fn the_tail_is_mark_badge_then_dot() {
    let row = pick("llama3:8b", Mark::Downloaded, true, Some(ToolMark::Capable));
    assert_eq!(line(&row, ToolGlyph::Emoji, 40), " llama3:8b ✓ 🔧 ●");
}

#[test]
fn text_mode_spells_the_badge() {
    let row = pick(
        "llama3:8b",
        Mark::Downloaded,
        false,
        Some(ToolMark::Capable),
    );
    assert_eq!(line(&row, ToolGlyph::Text, 40), " llama3:8b ✓ tools");
}

#[test]
fn unreliable_reads_the_same_in_both_modes() {
    let row = pick(
        "tiny:1b",
        Mark::Downloaded,
        false,
        Some(ToolMark::Unreliable),
    );
    let want = " tiny:1b ✓ tools: unreliable";
    assert_eq!(line(&row, ToolGlyph::Emoji, 60), want);
    assert_eq!(line(&row, ToolGlyph::Text, 60), want);
}

#[test]
fn a_row_without_a_badge_is_unchanged() {
    let row = pick("llama3:8b", Mark::Downloaded, true, None);
    assert_eq!(line(&row, ToolGlyph::Emoji, 40), " llama3:8b ✓ ●");
}

#[test]
fn the_badge_cells_are_counted_when_a_long_mark_is_clipped() {
    let status = Mark::Status("a-long-status-text-that-must-give-way-to-the-badge".to_owned());
    let row = pick("engine", status, true, Some(ToolMark::Capable));
    let shown = line(&row, ToolGlyph::Emoji, 50);
    assert!(shown.ends_with(" 🔧 ●"), "{shown}");
    assert!(cells(&shown) <= 50, "{shown}");
}

#[test]
fn the_badge_is_kept_when_the_popup_is_narrow() {
    let row = pick(
        "a-rather-long-model-name:8b",
        Mark::Downloaded,
        true,
        Some(ToolMark::Capable),
    );
    let shown = line(&row, ToolGlyph::Emoji, 24);
    assert!(shown.ends_with("✓ 🔧 ●"), "{shown}");
    assert!(shown.contains('…'), "{shown}");
    assert_eq!(cells(&shown), 24);
}

#[test]
fn at_the_ai_popup_width_the_label_gives_way_before_the_badge_and_dot() {
    let inner = crate::core::menu::WANT_WIDTH_AI - 2;
    let row = pick(
        "a-very-long-model-name-from-some-registry:70b-instruct-q4_K_M",
        Mark::Downloaded,
        true,
        Some(ToolMark::Unreliable),
    );
    let shown = line(&row, ToolGlyph::Emoji, inner);
    assert!(shown.ends_with(" ✓ tools: unreliable ●"), "{shown}");
    assert!(shown.contains('…'), "{shown}");
    assert_eq!(cells(&shown), usize::from(inner));
}
