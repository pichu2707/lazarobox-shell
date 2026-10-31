//! The review view drawn over the whole chat pane: a counter on top, the rows
//! of the batch below it and the keys on the last row.
//!
//! The rows are the only place the text of a diff is drawn. They are taken
//! from `ReviewView::visible`, the rows the coverage counts, and each one
//! comes from `text_for_review_renderer_only`: already escaped, so no control,
//! bidi or invisible character reaches the terminal.

use std::time::{Duration, Instant};

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
};

use super::chat::fill;
use crate::{
    core::ai::chat::{
        review::{LineKind, WARN_TRUNCATED},
        review_view::ReviewView,
    },
    ui::theme::LazaroboxTheme,
};

// The way out first: a narrow pane cuts the row from the right, so `n` is the
// last key to go, then `y`, then the scroll hints.
const KEYS_READY: &str = "n reject · y approve · ↑↓ PgUp PgDn scroll";
const KEYS_LOCKED: &str = "n reject · y after scrolling · ↑↓ PgUp PgDn scroll";
// Where `y after scrolling` no longer fits, `y locked` still says what it is.
const KEYS_LOCKED_NARROW: &str = "n reject · y locked · ↑↓ PgUp PgDn scroll";
const KEYS_LOCKED_HEAD: &str = "n reject · y after scrolling";
const KEYS_DISCARD: &str = "n discard · ↑↓ PgUp PgDn scroll";

/// `0.4s`, rounded up to a tenth.
fn seconds(left: Duration) -> String {
    let tenths = left.as_millis().div_ceil(100);
    format!("{}.{}s", tenths / 10, tenths % 10)
}

/// The counter row, and whether it is a warning.
fn header(view: &ReviewView, now: Instant, width: usize) -> (String, bool) {
    // A worker's batch names the worker first, then the line of its review's
    // receipt when it had one (S7b: a note, never a gate); a chat's batch is
    // unchanged.
    let review = view.review();
    let by = review.label().map_or_else(String::new, |label| {
        let receipt = review
            .receipt()
            .map_or_else(String::new, |r| format!("{r} · "));
        format!("{label} · {receipt}")
    });
    if !view.reviewable() {
        let text = if view.refused_hint(now) {
            "y can't approve this batch · press n"
        } else if view.review().doc().files() == 0 {
            "empty batch: nothing to approve · press n"
        } else {
            WARN_TRUNCATED
        };
        return (format!("{by}{text}"), true);
    }
    let (total, remaining) = (view.total(), view.remaining());
    let left = view.arm_left(now);
    if let Some(left) = view.locked_hint(now) {
        // The action first: a narrow pane cuts the row from the right.
        let full = format!("scroll ↓ · {left} lines unseen · y needs all");
        let text = if full.chars().count() <= width {
            full
        } else {
            format!("scroll ↓ · {left} unseen")
        };
        return (text, false);
    }
    let gate = if remaining > 0 {
        format!("{remaining} remaining · ↓ to unlock y")
    } else if left > Duration::ZERO {
        format!("y in {}", seconds(left))
    } else {
        "y ready".to_owned()
    };
    let counts = view.review().counts();
    (
        format!("{by}{counts} · seen {}/{total} · {gate}", total - remaining),
        false,
    )
}

fn style_of(kind: LineKind, theme: &LazaroboxTheme) -> Style {
    let base = Style::default().bg(theme.bg_base);
    match kind {
        LineKind::Summary => base.fg(theme.primary_cyan).add_modifier(Modifier::BOLD),
        LineKind::File => base.add_modifier(Modifier::BOLD),
        LineKind::Warning => base.fg(theme.warning_orange).add_modifier(Modifier::BOLD),
        LineKind::Hunk => base.fg(theme.info_blue),
        LineKind::Added => base.fg(theme.success_green),
        LineKind::Removed => base.fg(theme.error_red),
        LineKind::Context => base,
        LineKind::Note => base.fg(theme.text_muted),
    }
}

/// Draws the view in `area`. A pane of one row shows the keys only, of two
/// rows the counter and the keys: the way out is the last to go.
pub fn render(
    view: &ReviewView,
    now: Instant,
    theme: &LazaroboxTheme,
    area: Rect,
    buf: &mut Buffer,
) {
    let base = Style::default().bg(theme.bg_base);
    fill(buf, area, base);
    view.note_drawn(now, (usize::from(area.width), usize::from(area.height)));
    let width = usize::from(area.width);
    let put = |buf: &mut Buffer, row: u16, text: &str, style: Style| {
        buf.set_stringn(area.x, area.y + row, text, width, style);
    };
    if area.height >= 2 {
        let (text, warn) = header(view, now, width);
        let colour = if warn {
            theme.warning_orange
        } else {
            theme.primary_cyan
        };
        put(buf, 0, &text, base.fg(colour).add_modifier(Modifier::BOLD));
    }
    for (n, row) in view.visible(usize::from(area.height)).iter().enumerate() {
        if row.fits(width) {
            let y = u16::try_from(n + 1).unwrap_or(u16::MAX);
            put(
                buf,
                y,
                row.text_for_review_renderer_only(),
                style_of(row.kind, theme),
            );
        }
    }
    let keys = if !view.reviewable() {
        KEYS_DISCARD
    } else if view.can_approve(now) {
        KEYS_READY
    } else if width >= KEYS_LOCKED_HEAD.chars().count() {
        KEYS_LOCKED
    } else {
        KEYS_LOCKED_NARROW
    };
    put(buf, area.height - 1, keys, base.fg(theme.text_muted));
}

#[cfg(test)]
mod label_tests {
    use std::sync::Arc;

    use super::*;
    use crate::core::ai::chat::{review::BatchReview, review_view::testing::review_of};

    fn header_of(review: Arc<BatchReview>) -> String {
        let view = ReviewView::new(review, (80, 20));
        // Long after the arm delay, so the gate reads the same each time.
        header(&view, Instant::now() + Duration::from_secs(3600), 80).0
    }

    // Spec (agent-orchestrator, Writes wait for the batch review [S7]): a
    // worker's batch is labelled with the worker in the review header, and
    // a chat's own batch keeps its header as it was.
    #[test]
    fn a_worker_batch_names_the_worker_and_a_chat_batch_is_unchanged() {
        let plain = review_of(&[("a.txt", "one\n", false)]);
        let chat = header_of(plain.clone());
        assert!(chat.starts_with(&plain.counts()), "{chat}");
        let doc = plain.doc().clone();
        let worker = Arc::new(BatchReview::labelled(
            doc,
            "code codex>default".into(),
            crate::core::ai::orchestra::WorkerKey::for_test(1, 1),
        ));
        let shown = header_of(worker);
        assert_eq!(shown, format!("code codex>default · {chat}"));
    }

    // Spec (agent-orchestrator, A receipt never approves [S7b]): the receipt's
    // line sits in a worker's header and nothing more; the gate, the keys and
    // the answer at any moment are those of a batch with no receipt.
    #[test]
    fn a_receipt_line_heads_a_worker_batch_and_never_changes_what_y_does() {
        let plain = review_of(&[("a.txt", "one\n", false)]);
        let key = crate::core::ai::orchestra::WorkerKey::for_test(1, 1);
        let line = "review: high \u{b7} consent granted \u{b7} clean";
        let doc = plain.doc().clone();
        let bare = Arc::new(BatchReview::labelled(doc.clone(), "code x".into(), key));
        let with = Arc::new(
            BatchReview::labelled(doc, "code x".into(), key).with_receipt(Some(line.into())),
        );
        let counts = header_of(plain.clone());
        assert_eq!(
            header_of(with.clone()),
            format!("code x \u{b7} {line} \u{b7} {counts}")
        );
        // A chat's batch ignores a receipt: its header is as it was.
        let chat = Arc::new(BatchReview::new(plain.doc().clone()).with_receipt(Some(line.into())));
        assert_eq!(header_of(chat), counts);
        // The gate is the same: `y` is locked until the lines are seen and
        // the delay is over, whatever the receipt says.
        let (mut a, mut b) = (
            ReviewView::new(bare, (80, 20)),
            ReviewView::new(with, (80, 20)),
        );
        let now = Instant::now();
        assert!(!b.can_approve(now) && !b.answerable(now));
        assert_eq!(a.y_pressed(now), b.y_pressed(now));
        assert_eq!(a.can_approve(now), b.can_approve(now));
        assert_eq!(a.remaining(), b.remaining());
        assert_eq!(a.arm_left(now), b.arm_left(now));
    }
}
