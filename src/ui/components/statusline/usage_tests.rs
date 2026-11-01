//! The `today: N tok` segment in the right segment of the statusline
//! (ai-cost 2a.3, statusline "Usage segment").

use std::time::Duration;

use ratatui::{Terminal, backend::TestBackend};

use super::thinking::ThinkingView;
use super::*;
use crate::core::ai::chat::{StreamPhase, memory::MemBadge};

const ENGINE: &str = "ollama › llama3:8b";

fn view() -> ThinkingView {
    ThinkingView {
        frame: 0,
        phase: StreamPhase::Thinking,
        model: "llama3:8b".into(),
        elapsed: Duration::from_secs(4),
        others: 0,
    }
}

fn draw(
    usage: Option<&'static str>,
    memory: Option<MemBadge>,
    thinking: Option<ThinkingView>,
    width: u16,
) -> String {
    let theme: &'static LazaroboxTheme = Box::leak(Box::new(LazaroboxTheme::default()));
    let line = StatusLine::input(theme, InputMode::Terminal, "/srv/app", ENGINE)
        .chat(true)
        .usage(usage)
        .memory(memory)
        .thinking(thinking);
    let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
    terminal
        .draw(|frame| frame.render_widget(line, frame.area()))
        .unwrap();
    let buf = terminal.backend().buffer().clone();
    (0..width).map(|x| buf[(x, 0)].symbol()).collect()
}

#[test]
fn it_follows_the_engine_text() {
    let shown = draw(Some("today: 1234 tok"), None, None, 120);
    assert!(
        shown.contains("ollama › llama3:8b · today: 1234 tok"),
        "{shown}"
    );
    insta::assert_snapshot!(shown);
}

#[test]
fn it_goes_before_the_memory_state() {
    let shown = draw(Some("today: 1234 tok"), Some(MemBadge::Ready), None, 120);
    assert!(
        shown.contains("ollama › llama3:8b · today: 1234 tok · mem: ready"),
        "{shown}"
    );
    insta::assert_snapshot!(shown);
}

#[test]
fn no_usage_appends_nothing() {
    let with = draw(None, Some(MemBadge::Ready), None, 120);
    assert!(!with.contains("today"), "{with}");
    assert!(with.contains("llama3:8b · mem: ready"), "{with}");
}

#[test]
fn it_is_dropped_whole_before_the_memory_state_the_engine_or_the_indicator() {
    let mut dropped_somewhere = false;
    for width in 0..=100 {
        let with = draw(
            Some("today: 1234 tok"),
            Some(MemBadge::Ready),
            Some(view()),
            width,
        );
        let without = draw(None, Some(MemBadge::Ready), Some(view()), width);
        if with.contains("today") {
            assert!(
                with.contains(" · today: 1234 tok"),
                "{width}: never in part: {with:?}"
            );
            // Showing it costs the memory state, the engine text and the
            // indicator nothing.
            for part in ["▰▰▰", "thinking", "4s", ENGINE, "mem: ready"] {
                assert_eq!(
                    with.contains(part),
                    without.contains(part),
                    "{width}: {part}: {with:?} vs {without:?}"
                );
            }
        } else if without.contains("mem: ready") {
            dropped_somewhere = true;
        }
        if width >= 12 {
            assert!(with.starts_with(" ◐ CHAT "), "{width}: {with:?}");
        }
    }
    assert!(
        dropped_somewhere,
        "a width exists where only the usage goes"
    );
}

#[test]
fn it_takes_columns_from_the_path_budget_only_when_it_shows() {
    let theme: &'static LazaroboxTheme = Box::leak(Box::new(LazaroboxTheme::default()));
    let line = |usage| {
        StatusLine::input(theme, InputMode::Terminal, "/srv/app", ENGINE)
            .chat(true)
            .usage(usage)
    };
    let columns = u16::try_from(" · today: 1234 tok".chars().count()).unwrap();
    assert_eq!(
        line(None).path_budget(120) - line(Some("today: 1234 tok")).path_budget(120),
        columns
    );
    assert_eq!(
        line(None).path_budget(10),
        line(Some("today: 1234 tok")).path_budget(10)
    );
}
