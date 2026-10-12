use std::time::{Duration, Instant};

use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::Color,
    widgets::Widget,
};

use super::*;
use crate::core::ai::chat::{
    SendOutcome,
    engine::{Availability, ChatError, Finish, StreamEvent},
    transcript::Scroll,
};

fn theme() -> LazaroboxTheme {
    LazaroboxTheme::default()
}

fn base() -> Instant {
    use std::sync::OnceLock;
    static BASE: OnceLock<Instant> = OnceLock::new();
    *BASE.get_or_init(Instant::now)
}

fn at(ms: u64) -> Instant {
    base() + Duration::from_millis(ms)
}

fn ready() -> Availability {
    Availability::Ready {
        engine: "ollama".into(),
        model: Some("llama3:8b".into()),
    }
}

/// A chat with one finished exchange.
fn exchange(user: &str, answer: &str) -> ChatState {
    let mut chat = ChatState::new(None);
    send(&mut chat, user, 0);
    let turn = chat.turn();
    chat.apply_at(turn, StreamEvent::Delta(answer.into()), at(10));
    chat.apply_at(turn, StreamEvent::Done(Finish::Stop), at(20));
    chat
}

fn send(chat: &mut ChatState, text: &str, ms: u64) {
    let SendOutcome::Sent { .. } = chat.send_at(text, &ready(), "", at(ms)) else {
        panic!("send refused");
    };
}

fn draw(chat: &ChatState, width: u16, height: u16) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    ChatView::new(chat, &theme()).render(area, &mut buf);
    buf
}

fn text(buf: &Buffer) -> String {
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn fg(buf: &Buffer, x: u16, y: u16) -> Option<Color> {
    buf[(x, y)].style().fg
}

fn bg(buf: &Buffer, x: u16, y: u16) -> Option<Color> {
    buf[(x, y)].style().bg
}

fn typed(chat: &mut ChatState, body: &str) {
    for c in body.chars() {
        if c == '\n' {
            chat.composer_mut().newline();
        } else {
            chat.composer_mut().insert(c);
        }
    }
}

// Layout ----------------------------------------------------------------------

#[test]
fn the_transcript_is_above_and_the_composer_at_the_bottom() {
    let mut chat = exchange("hi there", "hello, friend");
    typed(&mut chat, "draft");
    let buf = draw(&chat, 40, 12);
    insta::assert_snapshot!(text(&buf));
    let layout = ChatView::new(&chat, &theme()).layout(Rect::new(0, 0, 40, 12));
    assert_eq!(layout.composer, Rect::new(0, 11, 40, 1));
    assert_eq!(layout.transcript, Rect::new(0, 0, 40, 11));
}

#[test]
fn user_and_assistant_messages_look_different() {
    let chat = exchange("hi there", "hello");
    let buf = draw(&chat, 40, 12);
    // Row 0 is the user message (`› hi there`), row 2 the answer.
    assert_eq!(fg(&buf, 0, 0), Some(theme().primary_cyan));
    assert_ne!(fg(&buf, 0, 2), Some(theme().primary_cyan));
    assert_eq!(buf[(0, 2)].symbol(), "h");
}

#[test]
fn the_composer_has_its_own_background() {
    let chat = exchange("hi", "yo");
    let buf = draw(&chat, 20, 6);
    assert_eq!(bg(&buf, 5, 5), Some(theme().bg_panel));
    assert_eq!(bg(&buf, 5, 0), Some(theme().bg_base));
}

#[test]
fn the_composer_grows_with_its_text_up_to_five_rows() {
    let mut chat = exchange("hi", "yo");
    typed(&mut chat, "a\nb");
    let layout = ChatView::new(&chat, &theme()).layout(Rect::new(0, 0, 20, 20));
    assert_eq!(layout.composer.height, 2);
    assert_eq!(layout.transcript.height, 18);
    typed(&mut chat, "\nc\nd\ne\nf\ng\nh\ni");
    let layout = ChatView::new(&chat, &theme()).layout(Rect::new(0, 0, 20, 20));
    assert_eq!(layout.composer.height, 5, "capped at five rows");
    assert_eq!(layout.transcript.height, 15);
    let buf = draw(&chat, 20, 20);
    insta::assert_snapshot!(text(&buf));
}

#[test]
fn the_transcript_keeps_at_least_one_row() {
    let mut chat = exchange("hi", "yo");
    typed(&mut chat, "1\n2\n3\n4\n5");
    let layout = ChatView::new(&chat, &theme()).layout(Rect::new(3, 2, 20, 3));
    assert_eq!(layout.transcript, Rect::new(3, 2, 20, 1));
    assert_eq!(layout.composer, Rect::new(3, 3, 20, 2));
    let tiny = ChatView::new(&chat, &theme()).layout(Rect::new(0, 0, 20, 1));
    assert_eq!(tiny.composer.height, 1);
    assert_eq!(tiny.transcript.height, 0);
}

// Transcript content ---------------------------------------------------------------

#[test]
fn a_fenced_block_uses_the_code_style() {
    let chat = exchange("q", "intro\n```rust\nlet x = 1;\nlet y = 2;\n```\noutro");
    let buf = draw(&chat, 30, 12);
    insta::assert_snapshot!(text(&buf));
    // Rows: 0 user, 1 blank, 2 intro, 3 fence, 4-5 code, 6 fence, 7 outro.
    assert_eq!(fg(&buf, 0, 4), Some(theme().success_green));
    assert_eq!(bg(&buf, 0, 4), Some(theme().bg_panel));
    assert_eq!(bg(&buf, 29, 4), Some(theme().bg_panel), "the row is filled");
    assert_ne!(fg(&buf, 0, 2), Some(theme().success_green));
    assert_ne!(fg(&buf, 0, 7), Some(theme().success_green));
}

#[test]
fn tab_indented_code_keeps_its_indentation_on_screen() {
    let chat = exchange(
        "q",
        "```go\nfunc main() {\n\tif ok {\n\t\trun()\n\t}\n}\n```",
    );
    let buf = draw(&chat, 30, 12);
    insta::assert_snapshot!(text(&buf));
}

#[test]
fn markdown_is_drawn_literally_and_unstyled() {
    let chat = exchange("q", "**bold** # title | a | b |");
    let buf = draw(&chat, 40, 8);
    insta::assert_snapshot!(text(&buf));
    assert_eq!(buf[(0, 2)].symbol(), "*");
    assert_eq!(
        buf[(0, 2)].style().add_modifier,
        ratatui::style::Modifier::empty()
    );
}

#[test]
fn a_long_line_wraps_inside_the_pane() {
    let chat = exchange("q", "alpha beta gamma delta epsilon zeta eta theta iota");
    let buf = draw(&chat, 16, 10);
    insta::assert_snapshot!(text(&buf));
}

#[test]
fn an_unclosed_fence_while_streaming_is_code() {
    let mut chat = ChatState::new(None);
    send(&mut chat, "q", 0);
    let turn = chat.turn();
    chat.apply_at(turn, StreamEvent::Delta("ok\n```\nlet x".into()), at(5));
    let buf = draw(&chat, 20, 8);
    assert_eq!(
        fg(&buf, 0, 4),
        Some(theme().success_green),
        "{}",
        text(&buf)
    );
}

#[test]
fn waiting_text_is_muted() {
    let mut chat = ChatState::new(None);
    send(&mut chat, "q", 0);
    let buf = draw(&chat, 30, 6);
    assert!(
        text(&buf).contains("waiting for the model…"),
        "{}",
        text(&buf)
    );
    assert_eq!(fg(&buf, 0, 2), Some(theme().text_muted));
}

#[test]
fn system_lines_and_partial_marks_are_styled() {
    let mut chat = ChatState::new(None);
    send(&mut chat, "q", 0);
    let turn = chat.turn();
    chat.apply_at(turn, StreamEvent::Delta("Hel".into()), at(5));
    chat.apply_at(
        turn,
        StreamEvent::Error(ChatError::Failed("boom".into())),
        at(6),
    );
    let buf = draw(&chat, 30, 8);
    let t = text(&buf);
    assert!(t.contains("(error)") && t.contains("• boom"), "{t}");
    // Rows: 0 user, 1 blank, 2 Hel, 3 (error), 4 blank, 5 • boom.
    assert_eq!(fg(&buf, 0, 3), Some(theme().text_muted));
    assert_eq!(fg(&buf, 0, 5), Some(theme().warning_orange));
}

// Scrolling ---------------------------------------------------------------------------

fn long_chat() -> ChatState {
    let answer = (1..=30)
        .map(|n| format!("line {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    exchange("q", &answer)
}

#[test]
fn the_view_follows_the_bottom() {
    let chat = long_chat();
    let buf = draw(&chat, 20, 8); // 7 transcript rows
    let t = text(&buf);
    assert!(t.contains("line 30") && !t.contains("line 1\n"), "{t}");
}

#[test]
fn a_pinned_view_stays_on_the_same_rows_while_the_answer_grows() {
    let mut chat = long_chat();
    chat.set_scroll(Scroll::Pinned(5));
    let before = text(&draw(&chat, 20, 8));
    let turn = chat.turn();
    // The exchange is over; start another that adds lines at the end.
    send(&mut chat, "more", 100);
    let turn2 = chat.turn();
    assert_ne!(turn, turn2);
    chat.set_scroll(Scroll::Pinned(5));
    chat.apply_at(turn2, StreamEvent::Delta("x\ny\nz".into()), at(110));
    let after = text(&draw(&chat, 20, 8));
    assert_eq!(before, after, "same content stays visible");
    chat.set_scroll(Scroll::Follow);
    assert!(text(&draw(&chat, 20, 8)).contains('z'));
}

// Reasoning -------------------------------------------------------------------------------

fn thinking_chat(text: &str) -> ChatState {
    let mut chat = ChatState::new(None);
    send(&mut chat, "hi", 0);
    let turn = chat.turn();
    chat.apply_at(turn, StreamEvent::ThinkingDelta(text.into()), at(100));
    chat
}

#[test]
fn streaming_reasoning_is_dimmed_behind_the_gutter() {
    let mut chat = thinking_chat("Let me");
    let turn = chat.turn();
    chat.apply_at(turn, StreamEvent::ThinkingDelta(" check".into()), at(200));
    let buf = draw(&chat, 30, 8);
    insta::assert_snapshot!(text(&buf));
    assert_eq!(buf[(0, 2)].symbol(), "┊");
    assert_eq!(fg(&buf, 0, 2), Some(theme().text_muted));
    assert_eq!(fg(&buf, 5, 2), Some(theme().text_muted));
    assert!(!text(&buf).contains("waiting for the model"));
}

#[test]
fn reasoning_wraps_at_the_width_less_the_gutter() {
    let chat = thinking_chat("abcdefghijkl");
    let buf = draw(&chat, 8, 8);
    insta::assert_snapshot!(text(&buf));
}

#[test]
fn a_collapsed_block_is_one_line_above_the_answer() {
    let mut chat = thinking_chat("Let me check");
    let turn = chat.turn();
    chat.apply_at(turn, StreamEvent::Delta("Hi".into()), at(2400));
    let buf = draw(&chat, 30, 8);
    insta::assert_snapshot!(text(&buf));
    assert_eq!(buf[(0, 2)].symbol(), "▸");
    assert_eq!(fg(&buf, 0, 2), Some(theme().ai_purple));
}

#[test]
fn an_expanded_block_shows_the_dimmed_text() {
    let mut chat = thinking_chat("Let me check");
    let turn = chat.turn();
    chat.apply_at(turn, StreamEvent::Delta("Hi".into()), at(2400));
    assert!(chat.toggle_reasoning());
    let buf = draw(&chat, 30, 8);
    insta::assert_snapshot!(text(&buf));
    assert_eq!(buf[(0, 2)].symbol(), "▾");
    assert_eq!(fg(&buf, 4, 3), Some(theme().text_muted));
}

#[test]
fn reasoning_is_literal_with_no_code_styling() {
    let chat = thinking_chat("```rust\n# heading\n*bold*");
    let buf = draw(&chat, 30, 8);
    insta::assert_snapshot!(text(&buf));
    for y in 2..5 {
        assert_eq!(fg(&buf, 3, y), Some(theme().text_muted));
        assert_ne!(bg(&buf, 3, y), Some(theme().bg_panel));
    }
}

#[test]
fn reasoning_follows_the_bottom_and_a_pinned_view_stays() {
    let mut chat = ChatState::new(None);
    send(&mut chat, "hi", 0);
    let turn = chat.turn();
    let lines = |n: usize| {
        (1..=n)
            .map(|i| format!("r{i}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    chat.apply_at(turn, StreamEvent::ThinkingDelta(lines(20)), at(10));
    let t = text(&draw(&chat, 20, 8));
    assert!(t.contains("r20"), "following the bottom: {t}");
    chat.set_scroll(Scroll::Pinned(3));
    let pinned = text(&draw(&chat, 20, 8));
    chat.apply_at(
        turn,
        StreamEvent::ThinkingDelta("\nr21\nr22".into()),
        at(20),
    );
    assert_eq!(text(&draw(&chat, 20, 8)), pinned);
}

#[test]
fn a_pane_without_reasoning_draws_no_reasoning_row() {
    let chat = exchange("q", "plain answer");
    let t = text(&draw(&chat, 30, 8));
    assert!(!t.contains('┊') && !t.contains("reasoning"), "{t}");
}

// Tiny panes ---------------------------------------------------------------------------------

#[test]
fn nothing_panics_from_1x1_to_10x2() {
    let mut states = vec![ChatState::new(None), exchange("q", "a\n```\nb")];
    let mut waiting = ChatState::new(None);
    send(&mut waiting, "q", 0);
    states.push(waiting);
    let streaming = thinking_chat("Let me\ncheck");
    states.push(streaming);
    let mut collapsed = thinking_chat("Let me");
    let turn = collapsed.turn();
    collapsed.apply_at(turn, StreamEvent::Delta("Hi".into()), at(900));
    states.push(collapsed.clone());
    collapsed.toggle_reasoning();
    states.push(collapsed);
    let mut typing = exchange("q", "a");
    typed(&mut typing, "日本語\nsecond\nthird");
    states.push(typing);
    for state in &states {
        for w in 1..=10 {
            for h in 1..=2 {
                let area = Rect::new(2, 1, w, h);
                let mut buf = Buffer::empty(Rect::new(0, 0, w + 4, h + 3));
                ChatView::new(state, &theme()).render(area, &mut buf);
                let _ = ChatView::new(state, &theme()).cursor(area);
                // Nothing is drawn outside the area.
                for y in 0..buf.area.height {
                    for x in 0..buf.area.width {
                        if !area.contains(Position::new(x, y)) {
                            assert_eq!(buf[(x, y)].symbol(), " ", "({x},{y}) in {w}x{h}");
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn an_empty_area_draws_nothing() {
    let chat = exchange("q", "a");
    let mut buf = Buffer::empty(Rect::new(0, 0, 5, 5));
    ChatView::new(&chat, &theme()).render(Rect::new(0, 0, 0, 3), &mut buf);
    ChatView::new(&chat, &theme()).render(Rect::new(0, 0, 3, 0), &mut buf);
    assert_eq!(text(&buf).trim(), "");
}

// The cursor (carry-over 8.0) ------------------------------------------------------------------

#[test]
fn the_cursor_sits_in_the_empty_composer() {
    let chat = ChatState::new(None);
    let th = theme();
    let view = ChatView::new(&chat, &th);
    assert_eq!(
        view.cursor(Rect::new(0, 0, 20, 6)),
        Some(Position::new(0, 5))
    );
    assert_eq!(
        view.cursor(Rect::new(30, 4, 20, 6)),
        Some(Position::new(30, 9))
    );
}

#[test]
fn the_cursor_follows_the_typed_text() {
    let mut chat = ChatState::new(None);
    typed(&mut chat, "abc");
    let th = theme();
    let view = ChatView::new(&chat, &th);
    assert_eq!(
        view.cursor(Rect::new(0, 0, 20, 6)),
        Some(Position::new(3, 5))
    );
}

#[test]
fn the_cursor_follows_wrapping_newlines_and_the_row_cap() {
    let mut chat = ChatState::new(None);
    typed(&mut chat, "abcdefgh");
    let th = theme();
    let view = ChatView::new(&chat, &th);
    // Width 5: rows `abcde` and `fgh`; the composer takes two rows.
    assert_eq!(
        view.cursor(Rect::new(0, 0, 5, 6)),
        Some(Position::new(3, 5))
    );
    let mut chat = ChatState::new(None);
    typed(&mut chat, "1\n2\n3\n4\n5\n6\n7");
    let th = theme();
    let view = ChatView::new(&chat, &th);
    assert_eq!(
        view.cursor(Rect::new(0, 0, 10, 10)),
        Some(Position::new(1, 9))
    );
}

#[test]
fn the_cursor_moves_with_the_composer_cursor() {
    let mut chat = ChatState::new(None);
    typed(&mut chat, "abc");
    chat.composer_mut().left();
    chat.composer_mut().left();
    let th = theme();
    let view = ChatView::new(&chat, &th);
    assert_eq!(
        view.cursor(Rect::new(0, 0, 20, 6)),
        Some(Position::new(1, 5))
    );
}

#[test]
fn no_cursor_when_the_pane_cannot_hold_it() {
    let chat = ChatState::new(None);
    let th = theme();
    let view = ChatView::new(&chat, &th);
    assert_eq!(view.cursor(Rect::new(0, 0, 0, 5)), None);
    assert_eq!(view.cursor(Rect::new(0, 0, 5, 0)), None);
    let mut full = ChatState::new(None);
    typed(&mut full, "abcde");
    // The cursor is past a full row of a 5-wide pane: a fresh row below.
    let th = theme();
    let view = ChatView::new(&full, &th);
    assert_eq!(
        view.cursor(Rect::new(0, 0, 5, 6)),
        Some(Position::new(0, 5))
    );
    // One row only: the window scrolls to the cursor row, so it stays visible.
    assert_eq!(
        view.cursor(Rect::new(0, 0, 5, 1)),
        Some(Position::new(0, 0))
    );
}
