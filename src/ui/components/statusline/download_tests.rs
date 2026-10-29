//! The `↓ <percent>%` mark of a Hugging Face download, after the other
//! right-hand marks (config-menu, Download in the status bar).

use ratatui::{Terminal, backend::TestBackend};

use super::*;
use crate::core::ai::chat::memory::MemBadge;

fn draw(
    download: Option<&'static str>,
    update: Option<&'static str>,
    memory: Option<MemBadge>,
    width: u16,
) -> String {
    let theme: &'static LazaroboxTheme = Box::leak(Box::new(LazaroboxTheme::default()));
    let line = StatusLine::input(theme, InputMode::Terminal, "/srv", "zsh")
        .memory(memory)
        .update(update)
        .download(download);
    let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
    terminal
        .draw(|frame| frame.render_widget(line, frame.area()))
        .unwrap();
    let buf = terminal.backend().buffer().clone();
    (0..width).map(|x| buf[(x, 0)].symbol()).collect()
}

#[test]
fn no_download_shows_no_mark() {
    assert!(!draw(None, None, None, 60).contains('↓'));
}

#[test]
fn a_download_shows_its_percent_after_the_other_marks() {
    let shown = draw(Some("↓ 42%"), Some("0.16.0"), Some(MemBadge::Ready), 90);
    assert!(
        shown.contains("zsh · update 0.16.0 · mem: ready · ↓ 42% "),
        "{shown}"
    );
    let alone = draw(Some("↓ 42%"), None, None, 60);
    assert!(alone.ends_with("zsh · ↓ 42% "), "{alone}");
}

#[test]
fn the_download_mark_outranks_the_others_when_narrow_and_never_overflows() {
    let narrow = draw(Some("↓ 42%"), Some("0.16.0"), Some(MemBadge::Ready), 34);
    assert!(narrow.contains("↓ 42%"), "{narrow}");
    assert!(!narrow.contains("mem:"), "{narrow}");
    for width in 0..50 {
        let shown = draw(Some("↓ 42%"), Some("0.16.0"), None, width);
        assert_eq!(shown.chars().count(), usize::from(width));
    }
}
