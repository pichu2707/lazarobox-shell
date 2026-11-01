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

fn engine_row(name: &str, status: &str, current: bool) -> Row {
    pick(
        name,
        Mark::Status(status.to_owned()),
        current,
        Some(ToolMark::Capable),
    )
}

#[test]
fn a_narrow_engine_row_keeps_name_badge_and_dot_and_clips_the_status() {
    let rows = [
        (
            "claude",
            "available · 2.1.316 (Claude Code) · npm global install",
        ),
        (
            "codex",
            "available · codex-cli 0.162.1 · npm global install in ~/.local",
        ),
    ];
    for glyph in [ToolGlyph::Emoji, ToolGlyph::Text] {
        let badge = if glyph == ToolGlyph::Emoji {
            "🔧"
        } else {
            "tools"
        };
        for width in [35, 39, 40, 49, 50, 51, 62] {
            for (name, status) in rows {
                for current in [false, true] {
                    let shown = line(&engine_row(name, status, current), glyph, width);
                    let dot = if current { " ●" } else { "" };
                    assert!(shown.starts_with(&format!(" {name} available")), "{shown}");
                    assert!(shown.ends_with(&format!("{badge}{dot}")), "{width} {shown}");
                    assert!(cells(&shown) <= usize::from(width), "{width} {shown}");
                }
            }
        }
    }
}
