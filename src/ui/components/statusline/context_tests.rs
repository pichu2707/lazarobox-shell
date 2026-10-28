//! The `context N%` mark in the right segment of the statusline
//! (chat-memory S2).

use ratatui::{Terminal, backend::TestBackend};

use super::*;
use crate::core::ai::chat::memory::MemBadge;

const PATH: &str = "/srv/app";
const ENGINE: &str = "llamacpp › qwen";

fn draw(
    context: Option<u8>,
    memory: Option<MemBadge>,
    update: Option<&'static str>,
    width: u16,
) -> String {
    let theme: &'static LazaroboxTheme = Box::leak(Box::new(LazaroboxTheme::default()));
    let line = StatusLine::input(theme, InputMode::Terminal, PATH, ENGINE)
        .chat(true)
        .context(context)
        .memory(memory)
        .update(update);
    let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
    terminal
        .draw(|frame| frame.render_widget(line, frame.area()))
        .unwrap();
    let buf = terminal.backend().buffer().clone();
    (0..width).map(|x| buf[(x, 0)].symbol()).collect()
}

#[test]
fn no_fill_shows_no_mark() {
    assert!(!draw(None, None, None, 80).contains("context"));
}

#[test]
fn the_fill_follows_the_engine() {
    let shown = draw(Some(42), None, None, 80);
    assert!(shown.contains("llamacpp › qwen · context 42%"), "{shown}");
}

#[test]
fn it_goes_before_the_memory_state_and_after_the_update_mark() {
    let shown = draw(Some(7), Some(MemBadge::Ready), Some("0.28.0"), 100);
    let at = |w: &str| shown.find(w).unwrap_or_else(|| panic!("{w}: {shown}"));
    assert!(at("update") < at("context 7%"), "{shown}");
    assert!(at("context 7%") < at("mem: ready"), "{shown}");
}

#[test]
fn a_narrow_line_drops_the_memory_state_before_the_context() {
    // Wide enough for the engine and the context, not for the memory state.
    let shown = draw(Some(7), Some(MemBadge::Ready), None, 50);
    assert!(
        shown.contains("context 7%") && !shown.contains("mem:"),
        "{shown}"
    );
}

#[test]
fn a_narrower_line_drops_the_context_whole() {
    let shown = draw(Some(7), None, None, 35);
    assert!(
        !shown.contains("context") && !shown.contains('%'),
        "{shown}"
    );
    assert!(shown.contains("CHAT"), "{shown}");
}

#[test]
fn snapshot_chat_with_the_context_fill() {
    insta::assert_snapshot!(
        "statusline_context_fill",
        draw(Some(63), None, None, 70).trim_end()
    );
}

#[test]
fn snapshot_chat_with_the_context_fill_and_memory() {
    insta::assert_snapshot!(
        "statusline_context_fill_and_memory",
        draw(Some(100), Some(MemBadge::Ready), None, 90).trim_end()
    );
}
