//! The review view drawn over a chat pane (chat-pane: "Batch review view";
//! code-write: "Batch review and coverage gate"). The clock is a fake one:
//! the view takes `now` as an argument.

use std::sync::Arc;
use std::time::{Duration, Instant};

use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Rect, widgets::Widget};

use super::chat::ChatView;
use crate::core::ai::chat::{
    ChatState,
    approval::ApprovalAsk,
    engine::CallId,
    review::{ARM_DELAY, BatchReview},
    review_view::{
        Move, ReviewView,
        testing::{coarse_of, edit_of, numbered, review_of},
    },
};
use crate::ui::theme::LazaroboxTheme;

fn t0() -> Instant {
    use std::sync::OnceLock;
    static BASE: OnceLock<Instant> = OnceLock::new();
    *BASE.get_or_init(Instant::now)
}

fn at(ms: u64) -> Instant {
    t0() + Duration::from_millis(ms)
}

/// A chat whose pending approval is `review`, open in a `w` x `h` pane.
fn reviewing(review: Arc<BatchReview>, w: usize, h: usize) -> ChatState {
    let mut chat = ChatState::new(None);
    let ask = ApprovalAsk::Batch(review.clone());
    chat.approvals_mut()
        .request_call(ask, 1, CallId(1), None)
        .unwrap();
    chat.approvals_mut()
        .set_review(ReviewView::new(review, (w, h)));
    chat
}

fn page_to_the_end(chat: &mut ChatState, size: (usize, usize)) {
    let (w, h) = (
        u16::try_from(size.0).unwrap(),
        u16::try_from(size.1).unwrap(),
    );
    draw(chat, at(0), w, h);
    let total = chat.approvals().review().unwrap().total();
    for _ in 0..total {
        chat.approvals_mut()
            .review_mut()
            .unwrap()
            .scroll(Move::PageDown, size);
        draw(chat, at(0), w, h);
    }
}

/// Draws the chat as the event loop does: a frame, then the rows it showed
/// count as seen, and another frame while the counter on screen is stale.
fn draw(chat: &mut ChatState, now: Instant, w: u16, h: u16) -> TestBackend {
    loop {
        let shown = frame(chat, now, w, h);
        let again = chat
            .approvals_mut()
            .review_mut()
            .is_some_and(ReviewView::credit_drawn);
        if !again {
            return shown;
        }
    }
}

fn frame(chat: &ChatState, now: Instant, w: u16, h: u16) -> TestBackend {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal
        .draw(|frame| {
            let area = frame.area();
            frame.render_widget(
                ChatView::new(chat, &LazaroboxTheme::default()).now(now),
                area,
            );
        })
        .unwrap();
    terminal.backend().clone()
}

fn rows(backend: &TestBackend) -> Vec<String> {
    let buf = backend.buffer();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

fn flat(backend: &TestBackend) -> String {
    rows(backend).join("\n")
}

#[test]
fn a_fresh_review_counts_what_remains_and_y_reads_as_locked() {
    let mut chat = reviewing(numbered(30), 60, 14);
    insta::assert_snapshot!("review_fresh_60x14", flat(&draw(&mut chat, at(0), 60, 14)));
}

#[test]
fn a_y_on_a_locked_gate_says_what_is_missing_for_two_seconds() {
    let mut chat = reviewing(numbered(30), 60, 14);
    draw(&mut chat, at(0), 60, 14);
    let view = chat.approvals_mut().review_mut().unwrap();
    let left = view.remaining();
    view.y_pressed(at(900));
    let hint = format!("y needs every line seen: {left} left, scroll ↓");
    let shown = draw(&mut chat, at(2_899), 60, 14);
    assert_eq!(rows(&shown)[0].trim_end(), hint);
    let later = draw(&mut chat, at(2_900), 60, 14);
    assert!(
        rows(&later)[0].ends_with("↓ to unlock y"),
        "{:?}",
        rows(&later)
    );
}

#[test]
fn a_y_with_every_line_seen_shows_no_hint() {
    let mut chat = reviewing(numbered(30), 60, 14);
    page_to_the_end(&mut chat, (60, 14));
    chat.approvals_mut()
        .review_mut()
        .unwrap()
        .y_pressed(at(100));
    let shown = draw(&mut chat, at(200), 60, 14);
    assert!(rows(&shown)[0].contains("y in 0.6s"), "{:?}", rows(&shown));
}

#[test]
fn everything_seen_but_the_delay_not_over_counts_down() {
    let mut chat = reviewing(numbered(30), 60, 14);
    page_to_the_end(&mut chat, (60, 14));
    insta::assert_snapshot!(
        "review_countdown_60x14",
        flat(&draw(&mut chat, at(400), 60, 14))
    );
}

#[test]
fn everything_seen_and_the_delay_over_enables_y() {
    let mut chat = reviewing(numbered(30), 60, 14);
    page_to_the_end(&mut chat, (60, 14));
    let shown = draw(&mut chat, at(800), 60, 14);
    assert!(rows(&shown)[0].contains("y ready"), "{:?}", rows(&shown));
    assert!(
        rows(&shown)[13].starts_with("n reject · y approve"),
        "{:?}",
        rows(&shown)
    );
    insta::assert_snapshot!("review_ready_60x14", flat(&shown));
}

#[test]
fn the_countdown_follows_the_clock_it_is_given() {
    let mut chat = reviewing(numbered(3), 60, 14);
    page_to_the_end(&mut chat, (60, 14));
    for (ms, wanted) in [
        (0, "y in 0.8s"),
        (50, "y in 0.8s"),
        (300, "y in 0.5s"),
        (700, "y in 0.1s"),
    ] {
        let first = rows(&draw(&mut chat, at(ms), 60, 14)).remove(0);
        assert!(first.contains(wanted), "{ms}: {first}");
    }
    assert!(ARM_DELAY.as_millis() == 800);
}

#[test]
fn a_scrolled_view_shows_the_rows_it_scrolled_to() {
    let mut chat = reviewing(numbered(30), 60, 14);
    draw(&mut chat, at(0), 60, 14);
    let view = chat.approvals_mut().review_mut().unwrap();
    view.scroll(Move::PageDown, (60, 14));
    insta::assert_snapshot!(
        "review_scrolled_60x14",
        flat(&draw(&mut chat, at(0), 60, 14))
    );
}

#[test]
fn a_build_file_carries_its_warning() {
    let mut chat = reviewing(review_of(&[("Makefile", "all:\n\tcc x.c\n", true)]), 70, 12);
    insta::assert_snapshot!(
        "review_build_file_70x12",
        flat(&draw(&mut chat, at(0), 70, 12))
    );
}

#[test]
fn a_coarse_diff_carries_its_warning() {
    let mut chat = reviewing(coarse_of(), 70, 12);
    let text = rows(&draw(&mut chat, at(0), 70, 12)).join("\n");
    assert!(text.contains("warning: coarse diff"), "{text}");
    insta::assert_snapshot!("review_coarse_70x12", flat(&draw(&mut chat, at(0), 70, 12)));
}

#[test]
fn a_batch_too_large_offers_only_to_discard_and_says_why() {
    let mut chat = reviewing(numbered(2100), 70, 12);
    page_to_the_end(&mut chat, (70, 12));
    let shown = draw(&mut chat, at(60_000), 70, 12);
    let r = rows(&shown);
    assert!(r[0].contains("too large to review"), "{r:?}");
    assert!(!r.iter().any(|l| l.contains("y approve")), "{r:?}");
    assert!(r[11].starts_with("n discard"), "{r:?}");
    insta::assert_snapshot!("review_too_large_70x12", flat(&shown));
}

#[test]
fn an_empty_batch_is_the_same_view() {
    let mut chat = reviewing(review_of(&[]), 70, 8);
    let r = rows(&draw(&mut chat, at(60_000), 70, 8));
    assert!(r[0].contains("too large to review"), "{r:?}");
    assert!(r[7].starts_with("n discard"), "{r:?}");
}

#[test]
fn a_long_line_wraps_with_the_continuation_marker() {
    let long = "x".repeat(50);
    let mut chat = reviewing(review_of(&[("a.txt", &format!("{long}\n"), false)]), 24, 10);
    insta::assert_snapshot!(
        "review_wrapped_24x10",
        flat(&draw(&mut chat, at(0), 24, 10))
    );
}

#[test]
fn hidden_characters_are_drawn_as_escapes_never_raw() {
    let mut chat = reviewing(
        edit_of(
            "a.txt",
            "ok\nold\u{202E}y\n\u{1b}[31m\n",
            "old\u{202E}y",
            "new",
        ),
        60,
        14,
    );
    let shown = draw(&mut chat, at(0), 60, 14);
    let text = rows(&shown).join("\n");
    assert!(text.contains("-old<U+202E>y"), "{text}");
    assert!(
        !text.chars().any(|c| c.is_control() && c != '\n'),
        "{text:?}"
    );
    assert!(!text.contains('\u{202E}') && !text.contains('\u{1b}'));
}

#[test]
fn the_secret_a_diff_holds_is_shown_to_the_user() {
    // The renderer is the one place the rows go: the user must see what
    // `y` would write and what it replaces.
    let mut chat = reviewing(
        edit_of(
            "a.txt",
            "token=hunter2\nkeep\n",
            "token=hunter2",
            "token=next",
        ),
        60,
        14,
    );
    let text = rows(&draw(&mut chat, at(0), 60, 14)).join("\n");
    assert!(
        text.contains("-token=hunter2") && text.contains("+token=next"),
        "{text}"
    );
}

#[test]
fn a_small_pane_sheds_the_body_first_and_keeps_the_way_out() {
    let mut chat = reviewing(numbered(5), 30, 2);
    insta::assert_snapshot!("review_2_rows", flat(&draw(&mut chat, at(0), 30, 2)));
    let mut chat = reviewing(numbered(5), 30, 1);
    insta::assert_snapshot!("review_1_row", flat(&draw(&mut chat, at(0), 30, 1)));
}

#[test]
fn no_size_draws_outside_its_area_or_panics() {
    for review in [numbered(40), numbered(2100), review_of(&[])] {
        for h in 0..=25u16 {
            for w in [0u16, 1, 2, 3, 10, 24, 80] {
                let mut chat = reviewing(review.clone(), usize::from(w), usize::from(h));
                if h > 0 && w > 0 {
                    page_to_the_end(&mut chat, (usize::from(w), usize::from(h)));
                }
                let area = Rect::new(0, 0, w, h);
                let mut buf = Buffer::empty(area);
                ChatView::new(&chat, &LazaroboxTheme::default())
                    .now(at(0))
                    .render(area, &mut buf);
                let _ = ChatView::new(&chat, &LazaroboxTheme::default()).cursor(area);
            }
        }
    }
}

#[test]
fn the_review_takes_the_real_cursor() {
    let chat = reviewing(numbered(5), 60, 14);
    let theme = LazaroboxTheme::default();
    let view = ChatView::new(&chat, &theme);
    assert_eq!(view.cursor(Rect::new(0, 0, 60, 14)), None);
}

#[test]
fn the_keys_row_keeps_n_first_then_y_then_the_scroll_hints() {
    for (w, ready, wanted) in [
        (10u16, true, "n reject ·"),
        (19, true, "n reject · y approv"),
        (24, true, "n reject · y approve · ↑"),
        (10, false, "n reject ·"),
        (19, false, "n reject · y after"),
        (24, false, "n reject · y after scrol"),
    ] {
        let mut chat = reviewing(numbered(3), usize::from(w), 6);
        if ready {
            page_to_the_end(&mut chat, (usize::from(w), 6));
        }
        let now = if ready {
            at(ARM_DELAY.as_millis() as u64)
        } else {
            at(0)
        };
        let last = rows(&draw(&mut chat, now, w, 6)).remove(5);
        assert_eq!(last, wanted, "width {w}, ready {ready}");
    }
}

#[test]
fn the_mode_badge_does_not_cover_the_counter_in_auto_mode() {
    use crate::core::ai::config::ApprovalMode;
    let mut chat = reviewing(numbered(3), 36, 6);
    chat.set_approval_mode(ApprovalMode::Auto);
    chat.set_run_hint(true);
    assert!(chat.mode_badge().is_some());
    page_to_the_end(&mut chat, (36, 6));
    let first = rows(&draw(&mut chat, at(ARM_DELAY.as_millis() as u64), 36, 6)).remove(0);
    assert!(first.ends_with("y ready"), "{first:?}");
    assert!(!first.contains("AUTO"), "{first:?}");
}

// Spec (git-tools, commit: Review shows message and diff): the commit review
// is the batch view with the signing notice and the message above the diff.
#[test]
fn a_commit_review_shows_the_notice_the_message_and_the_staged_diff() {
    use crate::core::ai::chat::review::CommitReview;
    let diff = "diff --git a/src/a.rs b/src/a.rs\nindex 111..222 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1 +1 @@\n-old\n+new\n";
    let commit = Arc::new(CommitReview::new(
        "feat: new\n\nWhy it changes.",
        None,
        &[],
        diff.as_bytes(),
    ));
    let mut chat = ChatState::new(None);
    chat.approvals_mut()
        .request_call(ApprovalAsk::Commit(commit.clone()), 1, CallId(1), None)
        .unwrap();
    chat.approvals_mut()
        .set_review(ReviewView::new(commit.review().clone(), (60, 20)));
    let shown = draw(&mut chat, at(0), 60, 20);
    let text = flat(&shown);
    for needle in [
        "signing disabled for this commit",
        "feat: new",
        "Why it changes.",
        "+new",
        "-old",
    ] {
        assert!(text.contains(needle), "{needle}: {text}");
    }
    insta::assert_snapshot!("commit_review_60x20", text);
}
