//! The wait line carries the seconds at draw time, and only what fits.

use std::time::{Duration, Instant};

use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

use super::*;
use crate::core::ai::chat::{
    SendOutcome,
    engine::{Availability, StreamEvent},
};

fn t0() -> Instant {
    use std::sync::OnceLock;
    static BASE: OnceLock<Instant> = OnceLock::new();
    *BASE.get_or_init(Instant::now)
}

fn at(secs: u64) -> Instant {
    t0() + Duration::from_secs(secs)
}

fn slow_chat(engine: &str) -> ChatState {
    let mut chat = ChatState::new(None);
    let ready = Availability::Ready {
        engine: engine.into(),
        model: Some("m".into()),
    };
    let SendOutcome::Sent { turn, .. } = chat.send("hi", &ready, "system") else {
        panic!("refused");
    };
    chat.apply_at(turn, StreamEvent::Slow, at(10));
    chat
}

/// The rows of the pane drawn at `now`, trimmed.
fn rows(chat: &ChatState, now: Instant, width: u16) -> Vec<String> {
    let area = Rect::new(0, 0, width, 8);
    let mut buf = Buffer::empty(area);
    ChatView::new(chat, &LazaroboxTheme::default())
        .now(now)
        .render(area, &mut buf);
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

fn wait_rows(rows: Vec<String>) -> Vec<String> {
    rows.into_iter()
        .filter(|r| {
            r.contains("waiting")
                || r.contains("model")
                || r.contains("gives up")
                || r.ends_with('s')
        })
        .collect()
}

#[test]
fn the_seconds_and_the_limit_follow_the_clock_without_a_new_generation() {
    let chat = slow_chat("ollama");
    let generation = chat.generation();
    let first = rows(&chat, at(52), 80);
    assert!(
        first
            .iter()
            .any(|r| r == "still waiting for the model… 45s (gives up at 120s)"),
        "{first:?}"
    );
    let later = rows(&chat, at(53), 80);
    assert!(
        later.iter().any(|r| r.ends_with("46s (gives up at 120s)")),
        "{later:?}"
    );
    assert_eq!(chat.generation(), generation);
}

#[test]
fn a_narrow_pane_drops_the_limit_then_the_seconds_and_never_a_row() {
    let chat = slow_chat("ollama");
    // 20 columns: "the model…" + " 45s" fits, the limit does not.
    let narrow = wait_rows(rows(&chat, at(52), 20));
    assert_eq!(narrow, ["still waiting for", "the model… 45s"]);
    // 11 columns: the label wraps in three rows and nothing is added.
    let tiny = wait_rows(rows(&chat, at(52), 11));
    assert!(tiny.iter().all(|r| r.chars().count() <= 11), "{tiny:?}");
    assert!(!tiny.iter().any(|r| r.contains("gives up")), "{tiny:?}");
}

#[test]
fn a_cli_wait_line_has_no_seconds_and_no_limit() {
    let chat = slow_chat("claude");
    let shown = rows(&chat, at(52), 80);
    assert!(
        shown.iter().any(|r| r == "still waiting for the model…"),
        "{shown:?}"
    );
    assert!(!shown.iter().any(|r| r.contains("gives up")), "{shown:?}");
}
