//! The `update <ver>` mark in the right segment of the statusline.

use ratatui::{Terminal, backend::TestBackend};

use super::*;
use crate::core::ai::chat::memory::MemBadge;

const PATH: &str = "/srv/app";
const ENGINE: &str = "zsh";

fn draw(update: Option<&'static str>, memory: Option<MemBadge>, width: u16) -> (Buffer, String) {
    let theme: &'static LazaroboxTheme = Box::leak(Box::new(LazaroboxTheme::default()));
    let line = StatusLine::input(theme, InputMode::Terminal, PATH, ENGINE)
        .memory(memory)
        .update(update);
    let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
    terminal
        .draw(|frame| frame.render_widget(line, frame.area()))
        .unwrap();
    let buf = terminal.backend().buffer().clone();
    let text = (0..width).map(|x| buf[(x, 0)].symbol()).collect();
    (buf, text)
}

#[test]
fn no_update_shows_no_mark() {
    let (_, shown) = draw(None, None, 60);
    assert!(!shown.contains("update"), "{shown}");
}

#[test]
fn a_known_update_shows_the_short_mark_after_the_engine() {
    let (_, shown) = draw(Some("0.16.0"), None, 60);
    assert!(shown.contains("zsh \u{b7} update 0.16.0"), "{shown}");
    insta::assert_snapshot!("update_mark_width_60", shown.trim_end());
}

#[test]
fn the_mark_is_warning_colored() {
    let t = LazaroboxTheme::default();
    let (buf, shown) = draw(Some("0.16.0"), None, 60);
    let at = shown.find("update").unwrap();
    let col = shown[..at].chars().count();
    assert_eq!(buf[(col as u16, 0)].fg, t.warning_orange);
}

#[test]
fn the_memory_state_gives_way_before_the_update_mark() {
    // Too narrow for both: the update is the one kept.
    let (_, wide) = draw(Some("0.16.0"), Some(MemBadge::Ready), 80);
    assert!(
        wide.contains("update 0.16.0") && wide.contains("mem: ready"),
        "{wide}"
    );
    let (_, narrow) = draw(Some("0.16.0"), Some(MemBadge::Ready), 38);
    assert!(narrow.contains("update 0.16.0"), "{narrow}");
    assert!(!narrow.contains("mem:"), "{narrow}");
}

#[test]
fn the_mark_is_dropped_when_it_does_not_fit_and_never_overflows() {
    for width in 0..40 {
        let (_, shown) = draw(Some("0.16.0"), None, width);
        assert_eq!(shown.chars().count(), usize::from(width));
    }
    let (_, tiny) = draw(Some("0.16.0"), None, 20);
    assert!(!tiny.contains("update"), "{tiny}");
}

#[test]
fn the_path_budget_leaves_room_for_the_mark() {
    let theme = LazaroboxTheme::default();
    let plain = StatusLine::input(&theme, InputMode::Terminal, PATH, ENGINE);
    let marked =
        StatusLine::input(&theme, InputMode::Terminal, PATH, ENGINE).update(Some("0.16.0"));
    assert!(marked.path_budget(80) < plain.path_budget(80));
}
