//! The statusline over a chat pane: the CHAT label, the engine segment and
//! the thinking indicator next to them.

use std::time::Duration;

use ratatui::{Terminal, backend::TestBackend};

use super::thinking::ThinkingView;
use super::*;
use crate::core::ai::chat::StreamPhase;

const PATH: &str = "/srv/app";
const ENGINE: &str = "ollama › llama3:8b";

fn view(others: usize) -> ThinkingView {
    ThinkingView {
        frame: 0,
        phase: StreamPhase::Thinking,
        model: "llama3:8b".into(),
        elapsed: Duration::from_secs(4),
        others,
    }
}

fn line(mode: InputMode, chat: bool, thinking: Option<ThinkingView>) -> StatusLine<'static> {
    // The theme lives for the whole test run.
    let theme: &'static LazaroboxTheme = Box::leak(Box::new(LazaroboxTheme::default()));
    StatusLine::input(theme, mode, PATH, ENGINE)
        .chat(chat)
        .thinking(thinking)
}

fn draw(line: StatusLine<'static>, width: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
    terminal
        .draw(|frame| frame.render_widget(line, frame.area()))
        .unwrap();
    terminal.backend().buffer().clone()
}

fn text(buf: &Buffer) -> String {
    (0..buf.area.width).map(|x| buf[(x, 0)].symbol()).collect()
}

#[test]
fn a_focused_chat_reads_chat_in_the_terminal_color() {
    let t = LazaroboxTheme::default();
    let buf = draw(line(InputMode::Terminal, true, None), 60);
    let shown = text(&buf);
    assert!(
        shown.contains("CHAT") && !shown.contains("TERMINAL"),
        "{shown}"
    );
    assert_eq!(buf[(1, 0)].bg, t.success_green);
}

#[test]
fn other_modes_keep_their_label_over_a_chat() {
    for (mode, label) in [(InputMode::Prefix, "PREFIX"), (InputMode::Resize, "RESIZE")] {
        let shown = text(&draw(line(mode, true, None), 60));
        assert!(shown.contains(label) && !shown.contains("CHAT"), "{shown}");
    }
}

#[test]
fn a_shell_pane_still_reads_terminal() {
    let shown = text(&draw(line(InputMode::Terminal, false, None), 60));
    assert!(shown.contains("TERMINAL"), "{shown}");
}

#[test]
fn the_path_the_label_and_the_engine_sit_in_order() {
    let shown = text(&draw(line(InputMode::Terminal, true, None), 60));
    let at = |needle: &str| {
        shown
            .find(needle)
            .unwrap_or_else(|| panic!("{needle}: {shown}"))
    };
    assert!(at("CHAT") < at(PATH) && at(PATH) < at(ENGINE), "{shown}");
}

#[test]
fn the_indicator_sits_between_the_path_and_the_engine_without_overlap() {
    let shown = text(&draw(line(InputMode::Terminal, true, Some(view(0))), 120));
    let at = |needle: &str| {
        shown
            .find(needle)
            .unwrap_or_else(|| panic!("{needle}: {shown}"))
    };
    assert!(
        at("CHAT") < at(PATH) && at(PATH) < at("▰") && at("▰") < at("4s") && at("4s") < at(ENGINE),
        "{shown}"
    );
    assert!(
        shown.contains("thinking") && shown.contains("llama3:8b"),
        "{shown}"
    );
}

#[test]
fn the_indicator_shows_in_every_mode() {
    for mode in [InputMode::Terminal, InputMode::Prefix, InputMode::Menu] {
        let shown = text(&draw(line(mode, false, Some(view(1))), 120));
        assert!(
            shown.contains("thinking") && shown.contains("+1"),
            "{shown}"
        );
    }
}

#[test]
fn without_a_stream_the_line_is_exactly_what_it_was() {
    let theme = LazaroboxTheme::default();
    let plain = StatusLine::input(&theme, InputMode::Terminal, PATH, ENGINE);
    let mut terminal = Terminal::new(TestBackend::new(80, 1)).unwrap();
    terminal
        .draw(|frame| frame.render_widget(plain, frame.area()))
        .unwrap();
    let expected = terminal.backend().buffer().clone();
    let with_none = draw(line(InputMode::Terminal, false, None), 80);
    assert_eq!(with_none, expected);
}

#[test]
fn the_indicator_leaves_the_path_a_smaller_budget() {
    let without = line(InputMode::Terminal, true, None).path_budget(120);
    let with = line(InputMode::Terminal, true, Some(view(0))).path_budget(120);
    assert!(with < without, "{with} !< {without}");
}

#[test]
fn narrow_widths_never_panic_and_keep_the_mode_block() {
    for width in 0..=80 {
        let buf = draw(line(InputMode::Terminal, true, Some(view(2))), width);
        let shown = text(&buf);
        if width >= 12 {
            assert!(shown.starts_with(" ◐ CHAT "), "{width}: {shown:?}");
        }
    }
}

#[test]
fn the_indicator_and_the_path_never_overlap_at_any_width() {
    for width in 20..=80 {
        let buf = draw(line(InputMode::Terminal, true, Some(view(1))), width);
        let shown = text(&buf);
        // No part of the indicator is cut in the middle: a partial model
        // name would be a prefix of it that is not whole.
        for cut in ["llama3:", "thinkin", "▰▰▰━━━━"] {
            if let Some(i) = shown.find(cut) {
                let rest = &shown[i..];
                let whole = ["llama3:8b", "thinking", "▰▰▰━━━━━"]
                    .iter()
                    .any(|w| rest.starts_with(w));
                assert!(whole, "{width}: {shown:?}");
            }
        }
    }
}

#[test]
fn snapshot_chat_with_the_indicator() {
    insta::assert_snapshot!(text(&draw(
        line(InputMode::Terminal, true, Some(view(0))),
        120
    )));
}
