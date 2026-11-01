use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use super::*;
use crate::core::ai::chat::{
    ChatState, Msg, MsgStatus, Phase,
    engine::{ChatError, Finish, Role, StreamEvent},
    state_tests::{chat, ready},
};

#[test]
fn a_wide_prefix_leaves_room_by_display_width_not_by_char_count() {
    use crate::core::ai::chat::wrap::text_width;
    let mut out = Vec::new();
    push_wrapped(
        &mut out,
        LineKind::Assistant,
        "日本 ",
        "   ",
        "abcdefghi jklmnopq",
        12,
    );
    assert!(!out.is_empty());
    for line in &out {
        assert!(text_width(&line.text) <= 12, "{:?}", line.text);
    }
}

fn at(base: Instant, ms: u64) -> Instant {
    base + Duration::from_millis(ms)
}

/// One string per row: a kind letter, then the text.
fn rows(lines: &[Line]) -> Vec<String> {
    lines
        .iter()
        .map(|l| {
            let k = match l.kind {
                LineKind::User => 'U',
                LineKind::Assistant => 'A',
                LineKind::Code => 'C',
                LineKind::System => 'S',
                LineKind::Waiting => 'W',
                LineKind::Mark => 'M',
                LineKind::Reasoning => 'R',
                LineKind::ReasoningHead => 'H',
                LineKind::Hint => 'T',
                LineKind::Tool => 'L',
                LineKind::ToolAlert => 'X',
                LineKind::Usage => 'G',
                LineKind::Blank => '_',
            };
            format!("{k}|{}", l.text)
        })
        .collect()
}

fn answered(state: &mut ChatState, user: &str, answer: &str) {
    state.set_usage_config(crate::core::ai::config::UsageConfig::without_line());
    let base = Instant::now();
    let SendOutcome::Sent { turn, .. } = state.send_at(user, &ready(), "", base) else {
        panic!("refused")
    };
    state.apply_at(turn, StreamEvent::Delta(answer.into()), at(base, 1));
    state.apply_at(turn, StreamEvent::Done(Finish::Stop), at(base, 2));
}

use crate::core::ai::chat::SendOutcome;

#[test]
fn messages_are_classified_and_separated_by_a_blank_row() {
    let mut state = chat();
    answered(&mut state, "hi", "hello");
    assert_eq!(rows(&build(&state, 20)), ["U|› hi", "_|", "A|hello"]);
}

#[test]
fn long_lines_wrap_inside_the_width_and_user_rows_keep_the_indent() {
    let mut state = chat();
    answered(&mut state, "abcdefghij", "0123456789ab");
    let lines = build(&state, 6);
    assert_eq!(
        rows(&lines),
        [
            "U|› abcd",
            "U|  efgh",
            "U|  ij",
            "_|",
            "A|012345",
            "A|6789ab"
        ]
    );
}

#[test]
fn prose_breaks_at_spaces_and_code_breaks_hard() {
    let mut state = chat();
    answered(
        &mut state,
        "q",
        "alpha beta gamma delta\n```\nlet longname = 1;\n```",
    );
    assert_eq!(
        rows(&build(&state, 12))[2..],
        [
            "A|alpha beta",
            "A|gamma delta",
            "C|```",
            "C|let longname",
            "C| = 1;",
            "C|```"
        ]
    );
}

#[test]
fn a_fenced_block_is_code_and_the_text_around_it_is_plain() {
    let mut state = chat();
    answered(
        &mut state,
        "q",
        "before\n```rust\nlet x = 1;\nlet y = 2;\n```\nafter",
    );
    let r = rows(&build(&state, 30));
    assert_eq!(
        &r[2..],
        [
            "A|before",
            "C|```rust",
            "C|let x = 1;",
            "C|let y = 2;",
            "C|```",
            "A|after"
        ]
    );
}

#[test]
fn tabs_become_spaces_so_indentation_survives() {
    let mut state = chat();
    answered(
        &mut state,
        "q",
        "```\nfn f() {\n\tlet x = 1;\n\t\tx\n}\n```\nplain\ttab",
    );
    assert_eq!(
        rows(&build(&state, 30))[2..],
        [
            "C|```",
            "C|fn f() {",
            "C|    let x = 1;",
            "C|        x",
            "C|}",
            "C|```",
            "A|plain   tab",
        ]
    );
}

#[test]
fn a_fence_line_with_a_backtick_after_the_opening_ticks_is_not_a_fence() {
    let mut state = chat();
    answered(&mut state, "q", "use ```a``` inline\n```a```\nstill prose");
    let got = rows(&build(&state, 40));
    assert_eq!(
        got[2..],
        ["A|use ```a``` inline", "A|```a```", "A|still prose"]
    );
    // An info string without backticks still opens a block.
    let mut state = chat();
    answered(&mut state, "q", "```rust title=\"x\"\ncode");
    assert_eq!(
        rows(&build(&state, 40))[2..],
        ["C|```rust title=\"x\"", "C|code"]
    );
}

#[test]
fn an_unclosed_fence_makes_the_rest_of_the_message_code() {
    let mut state = chat();
    answered(&mut state, "q", "ok\n```\nlet x = 1;\nmore");
    let r = rows(&build(&state, 30));
    assert_eq!(&r[2..], ["A|ok", "C|```", "C|let x = 1;", "C|more"]);
}

#[test]
fn the_fence_state_does_not_leak_into_the_next_message() {
    let mut state = chat();
    answered(&mut state, "q", "```\nopen");
    answered(&mut state, "r", "plain");
    let r = rows(&build(&state, 30));
    assert_eq!(r.last().unwrap(), "A|plain");
}

#[test]
fn other_markdown_is_literal() {
    let mut state = chat();
    answered(&mut state, "q", "**bold** # title | a | b |\n- item");
    let r = rows(&build(&state, 40));
    assert_eq!(&r[2..], ["A|**bold** # title | a | b |", "A|- item"]);
}

#[test]
fn system_lines_and_partial_marks_are_distinct_rows() {
    let base = Instant::now();
    let mut state = chat();
    let SendOutcome::Sent { turn, .. } = state.send_at("hi", &ready(), "", base) else {
        panic!()
    };
    state.apply_at(turn, StreamEvent::Delta("Hel".into()), at(base, 1));
    state.apply_at(
        turn,
        StreamEvent::Error(ChatError::Failed("boom".into())),
        at(base, 2),
    );
    let r = rows(&build(&state, 30));
    assert_eq!(&r[2..], ["A|Hel", "M|(error)", "_|", "S|• boom"]);
    state.cancel_at(at(base, 3));
    answered(&mut state, "next", "x");
    let mut partial = chat();
    let SendOutcome::Sent { turn, .. } = partial.send_at("hi", &ready(), "", base) else {
        panic!()
    };
    partial.apply_at(turn, StreamEvent::Delta("Hel".into()), at(base, 1));
    partial.cancel_at(at(base, 2));
    assert_eq!(rows(&build(&partial, 30))[2..], ["A|Hel", "M|(cancelled)"]);
}

#[test]
fn the_waiting_text_ends_the_transcript() {
    let base = Instant::now();
    let mut state = chat();
    state.send_at("hi", &ready(), "", base);
    assert_eq!(
        rows(&build(&state, 30)),
        ["U|› hi", "_|", "W|waiting for the model…"]
    );
    let turn = state.turn();
    state.apply_at(turn, StreamEvent::Slow, at(base, 3000));
    assert_eq!(
        rows(&build(&state, 30)).last().unwrap(),
        "W|still waiting for the model…"
    );
    state.apply_at(turn, StreamEvent::Delta("a".into()), at(base, 3001));
    assert!(!rows(&build(&state, 30)).iter().any(|r| r.starts_with("W|")));
}

// Reasoning ---------------------------------------------------------------------

fn thinking(state: &mut ChatState, text: &str, ms: u64) {
    let turn = state.turn();
    state.apply_at(
        turn,
        StreamEvent::ThinkingDelta(text.into()),
        at(base0(), ms),
    );
}

fn base0() -> Instant {
    use std::sync::OnceLock;
    static BASE: OnceLock<Instant> = OnceLock::new();
    *BASE.get_or_init(Instant::now)
}

fn streaming_reasoning(text: &str) -> ChatState {
    let mut state = chat();
    state.send_at("hi", &ready(), "", base0());
    thinking(&mut state, text, 100);
    state
}

#[test]
fn streaming_reasoning_is_gutter_rows_replacing_the_waiting_text() {
    let state = streaming_reasoning("Let me check");
    assert_eq!(
        rows(&build(&state, 30)),
        ["U|› hi", "_|", "R|┊ Let me check"]
    );
}

#[test]
fn reasoning_wraps_at_the_width_less_the_gutter() {
    let state = streaming_reasoning("abcdefgh\nij");
    assert_eq!(
        rows(&build(&state, 6))[2..],
        ["R|┊ abcd", "R|┊ efgh", "R|┊ ij"]
    );
}

#[test]
fn reasoning_is_literal_text() {
    let state = streaming_reasoning("```rust\n# heading\n*bold*");
    assert_eq!(
        rows(&build(&state, 30))[2..],
        ["R|┊ ```rust", "R|┊ # heading", "R|┊ *bold*"]
    );
}

fn collapsed_after(ms_to_answer: u64) -> ChatState {
    let mut state = streaming_reasoning("Let me check");
    let turn = state.turn();
    state.apply_at(
        turn,
        StreamEvent::Delta("Hi".into()),
        at(base0(), 100 + ms_to_answer),
    );
    state
}

#[test]
fn a_collapsed_block_is_one_line_before_the_answer() {
    let state = collapsed_after(2300);
    assert_eq!(
        rows(&build(&state, 30))[2..],
        ["H|▸ reasoning (2.3s)", "A|Hi"]
    );
}

#[test]
fn an_expanded_block_shows_its_text_behind_the_gutter() {
    let mut state = collapsed_after(2300);
    assert!(state.toggle_reasoning());
    assert_eq!(
        rows(&build(&state, 30))[2..],
        ["H|▾ reasoning (2.3s)", "R|┊ Let me check", "A|Hi"]
    );
    assert!(state.toggle_reasoning());
    assert_eq!(rows(&build(&state, 30))[2], "H|▸ reasoning (2.3s)");
}

#[test]
fn a_turn_without_reasoning_has_no_reasoning_row() {
    let mut state = chat();
    answered(&mut state, "q", "a");
    assert!(
        build(&state, 30)
            .iter()
            .all(|l| !matches!(l.kind, LineKind::Reasoning | LineKind::ReasoningHead))
    );
}

#[test]
fn a_cancelled_reasoning_only_turn_is_one_collapsed_line() {
    let mut state = streaming_reasoning("Let me");
    state.cancel_at(at(base0(), 1100));
    assert_eq!(
        rows(&build(&state, 30)),
        ["U|› hi", "_|", "H|▸ reasoning (1.0s)"]
    );
}

#[test]
fn the_reasoning_block_follows_a_resumed_conversation_with_no_block() {
    let state = ChatState::resumed(
        None,
        vec![
            Msg {
                role: Role::User,
                text: "q".into(),
                status: MsgStatus::Complete,
            },
            Msg {
                role: Role::Assistant,
                text: "a".into(),
                status: MsgStatus::Interrupted,
            },
        ],
    );
    assert_eq!(
        rows(&build(&state, 30)),
        ["U|› q", "_|", "A|a", "M|(interrupted)"]
    );
}

// Any width, any state -------------------------------------------------------------

#[test]
fn building_at_any_width_from_0_to_13_never_panics_and_always_yields_rows() {
    let mut states = vec![];
    let mut a = chat();
    answered(
        &mut a,
        "日本語 text",
        "```\nlong code line here\n```\nplain words wrap",
    );
    states.push(a);
    let mut b = streaming_reasoning("some reasoning that is long\nsecond");
    states.push(b.clone());
    b.toggle_reasoning();
    states.push(collapsed_after(1500));
    let mut e = collapsed_after(1500);
    e.toggle_reasoning();
    states.push(e);
    for state in &states {
        for width in 0..14 {
            assert!(!build(state, width).is_empty(), "width {width}");
        }
    }
    assert!(build(&chat(), 0).is_empty(), "an empty chat has no rows");
}

#[test]
fn rows_fit_the_width_at_reasonable_sizes() {
    let mut state = chat();
    answered(
        &mut state,
        "a long user message with several words",
        "an equally long answer with words",
    );
    for width in 8..30 {
        for line in build(&state, width) {
            let cols = crate::core::ai::chat::wrap::text_width(&line.text);
            assert!(cols <= width, "{width}: {line:?}");
        }
    }
}

// Scrolling ----------------------------------------------------------------------

#[test]
fn following_shows_the_last_rows() {
    assert_eq!(Scroll::Follow.top(30, 10), 20);
    assert_eq!(
        Scroll::Follow.top(5, 10),
        0,
        "short content starts at the top"
    );
    assert_eq!(Scroll::Follow.top(10, 10), 0);
}

#[test]
fn page_up_pins_one_page_above_the_bottom() {
    let s = Scroll::Follow.page_up(30, 10);
    assert_eq!(s, Scroll::Pinned(11));
    assert_eq!(s.top(30, 10), 11);
    assert_eq!(s.page_up(30, 10), Scroll::Pinned(2));
    assert_eq!(
        Scroll::Pinned(2).page_up(30, 10),
        Scroll::Pinned(0),
        "clamps at the top"
    );
    assert_eq!(Scroll::Pinned(0).page_up(30, 10), Scroll::Pinned(0));
}

#[test]
fn page_up_with_everything_visible_stays_glued() {
    assert_eq!(Scroll::Follow.page_up(5, 10), Scroll::Follow);
    assert_eq!(Scroll::Follow.page_up(10, 10), Scroll::Follow);
}

#[test]
fn page_down_returns_to_following_at_the_end() {
    assert_eq!(Scroll::Pinned(0).page_down(30, 10), Scroll::Pinned(9));
    assert_eq!(Scroll::Pinned(5).page_down(30, 10), Scroll::Pinned(14));
    assert_eq!(Scroll::Pinned(11).page_down(30, 10), Scroll::Follow);
    assert_eq!(Scroll::Pinned(15).page_down(30, 10), Scroll::Follow);
    assert_eq!(Scroll::Follow.page_down(30, 10), Scroll::Follow);
}

#[test]
fn lines_up_moves_the_view_by_n_lines_from_the_bottom() {
    assert_eq!(Scroll::Follow.lines_up(1, 30, 10), Scroll::Pinned(19));
    assert_eq!(Scroll::Follow.lines_up(3, 30, 10), Scroll::Pinned(17));
    assert_eq!(Scroll::Pinned(17).lines_up(3, 30, 10), Scroll::Pinned(14));
}

#[test]
fn lines_up_clamps_at_the_top() {
    assert_eq!(Scroll::Pinned(2).lines_up(3, 30, 10), Scroll::Pinned(0));
    assert_eq!(Scroll::Pinned(0).lines_up(1, 30, 10), Scroll::Pinned(0));
    assert_eq!(Scroll::Follow.lines_up(100, 30, 10), Scroll::Pinned(0));
}

#[test]
fn lines_up_counts_wrapped_lines_of_the_total() {
    // 12 wrapped rows in a 5-row viewport: the bottom is top 7.
    assert_eq!(Scroll::Follow.lines_up(2, 12, 5), Scroll::Pinned(5));
}

#[test]
fn lines_with_everything_visible_stay_glued() {
    assert_eq!(Scroll::Follow.lines_up(3, 5, 10), Scroll::Follow);
    assert_eq!(Scroll::Follow.lines_up(1, 10, 10), Scroll::Follow);
    assert_eq!(Scroll::Follow.lines_down(3, 5, 10), Scroll::Follow);
}

#[test]
fn lines_down_returns_to_following_at_the_end() {
    assert_eq!(Scroll::Pinned(0).lines_down(1, 30, 10), Scroll::Pinned(1));
    assert_eq!(Scroll::Pinned(5).lines_down(3, 30, 10), Scroll::Pinned(8));
    assert_eq!(Scroll::Pinned(18).lines_down(1, 30, 10), Scroll::Pinned(19));
    assert_eq!(Scroll::Pinned(19).lines_down(1, 30, 10), Scroll::Follow);
    assert_eq!(Scroll::Pinned(18).lines_down(3, 30, 10), Scroll::Follow);
    assert_eq!(Scroll::Follow.lines_down(3, 30, 10), Scroll::Follow);
}

#[test]
fn lines_down_clamps_a_stale_pin_before_moving() {
    // The transcript shrank: Pinned(40) is really the bottom (top 20).
    assert_eq!(Scroll::Pinned(40).lines_down(1, 30, 10), Scroll::Follow);
    assert_eq!(Scroll::Pinned(40).lines_up(1, 30, 10), Scroll::Pinned(19));
}

#[test]
fn a_pinned_view_stays_on_the_same_content_while_the_transcript_grows() {
    let s = Scroll::Pinned(7);
    assert_eq!(s.top(30, 10), 7);
    assert_eq!(s.top(35, 10), 7, "five more lines: same top line");
    assert_eq!(
        Scroll::Follow.top(35, 10),
        25,
        "following moves with the end"
    );
}

#[test]
fn a_pin_past_the_end_is_clamped_when_the_transcript_shrinks() {
    assert_eq!(Scroll::Pinned(40).top(30, 10), 20);
    assert_eq!(Scroll::Pinned(5).top(3, 10), 0);
}

#[test]
fn a_zero_height_view_does_not_panic() {
    assert_eq!(Scroll::Follow.top(30, 0), 30);
    let _ = Scroll::Follow.page_up(30, 0);
    let _ = Scroll::Pinned(3).page_down(30, 0);
}

#[test]
fn sending_returns_the_view_to_the_bottom() {
    let mut state = chat();
    answered(&mut state, "q", "a");
    state.set_scroll(Scroll::Pinned(1));
    assert_eq!(state.scroll(), Scroll::Pinned(1));
    answered(&mut state, "again", "b");
    assert_eq!(state.scroll(), Scroll::Follow);
}

#[test]
fn a_refused_send_keeps_the_scroll() {
    let mut state = chat();
    state.send("hi", &ready(), "");
    assert_eq!(state.phase(), Phase::Waiting { slow: false });
    state.set_scroll(Scroll::Pinned(1));
    state.send("busy", &ready(), "");
    assert_eq!(state.scroll(), Scroll::Pinned(1));
}

// The cache ----------------------------------------------------------------------

#[test]
fn lines_are_cached_by_width_and_generation() {
    let mut state = chat();
    answered(&mut state, "q", "a");
    let first = state.lines(20);
    assert!(
        Arc::ptr_eq(&first, &state.lines(20)),
        "same width, same generation"
    );
    assert!(
        !Arc::ptr_eq(&first, &state.lines(21)),
        "another width rebuilds"
    );
    let again = state.lines(20);
    assert!(
        !Arc::ptr_eq(&first, &again),
        "the width cache holds one entry"
    );
    answered(&mut state, "r", "b");
    let grown = state.lines(20);
    assert!(!Arc::ptr_eq(&again, &grown), "a change rebuilds");
    assert_eq!(*grown, build(&state, 20));
    assert!(grown.len() > again.len());
}

// Spec (chat-pane, Per-answer usage line): a dim system row below the answer.
#[test]
fn a_usage_line_is_a_system_row_right_below_its_answer() {
    let mut state = chat();
    state.set_usage_config(Default::default());
    let base = Instant::now();
    let SendOutcome::Sent { turn, .. } = state.send_at("hi", &ready(), "", base) else {
        panic!("refused")
    };
    state.apply_at(turn, StreamEvent::Delta("hello".into()), at(base, 1));
    state.apply_at(turn, StreamEvent::Done(Finish::Stop), at(base, 2));
    assert_eq!(
        rows(&build(&state, 40)),
        ["U|› hi", "_|", "A|hello", "_|", "G|• usage: no data"]
    );
}

// Spec (chat-pane): the line is not shown after a resume.
#[test]
fn a_resume_does_not_keep_the_usage_line() {
    use crate::core::ai::{chat::tags::Tags, config::Profile};
    let mut state = chat();
    answered_with_line(&mut state);
    assert!(
        state
            .entries()
            .iter()
            .any(|e| matches!(e, crate::core::ai::chat::Entry::Usage(_)))
    );
    state.resume_into(7, Profile::Programmer, Tags::default(), Vec::new());
    assert!(
        state
            .entries()
            .iter()
            .all(|e| !matches!(e, crate::core::ai::chat::Entry::Usage(_)))
    );
}

fn answered_with_line(state: &mut ChatState) {
    let base = Instant::now();
    let SendOutcome::Sent { turn, .. } = state.send_at("hi", &ready(), "", base) else {
        panic!("refused")
    };
    state.apply_at(turn, StreamEvent::Done(Finish::Stop), at(base, 2));
}
