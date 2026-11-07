//! The review of a pane's screen: the exact text that goes to the model, as
//! the lines of a [`ReviewDoc`], so the view and the coverage gate are those
//! of a batch. The text is frozen when the review opens; what the user reads
//! is what is sent.

use std::fmt;
use std::sync::{Arc, atomic::Ordering};

use super::{BatchReview, LineKind, NEXT_DOC, ReviewDoc, push};
use crate::core::ai::tools::diff::escape;
use crate::core::ai::tools::screen::{ALT_NOTE as NO_SCROLLBACK, Screen};

const WARN_CUT: &str = "earlier lines were cut: only the tail is sent";

/// The screen a [`ApprovalAsk::Screen`](crate::core::ai::chat::approval::ApprovalAsk)
/// asks the user about: its review and the frozen text it shows.
#[derive(Clone, PartialEq, Eq)]
pub struct ScreenReview {
    review: Arc<BatchReview>,
    text: String,
}

impl fmt::Debug for ScreenReview {
    /// Counts only: the screen never reaches a log.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScreenReview")
            .field("review", &self.review)
            .field("text_bytes", &self.text.len())
            .finish()
    }
}

impl ScreenReview {
    /// The review of `screen`, read from pane number `pane`.
    pub fn new(pane: u32, screen: &Screen) -> Self {
        let mut doc = ReviewDoc {
            id: NEXT_DOC.fetch_add(1, Ordering::Relaxed),
            lines: Vec::new(),
            files: 1,
            added: 0,
            removed: 0,
            truncated: false,
        };
        let s = if screen.lines == 1 { "" } else { "s" };
        let summary = format!("pane {pane} screen, {} line{s}", screen.lines);
        push(&mut doc.lines, LineKind::Summary, summary.clone());
        if screen.cut {
            push(&mut doc.lines, LineKind::Warning, WARN_CUT.to_owned());
        }
        if screen.no_scrollback {
            push(&mut doc.lines, LineKind::Warning, NO_SCROLLBACK.to_owned());
        }
        for line in screen.text.lines() {
            push(
                &mut doc.lines,
                LineKind::Context,
                format!(" {}", escape(line)),
            );
        }
        Self {
            review: Arc::new(BatchReview::new(doc).with_summary(summary)),
            text: screen.text.clone(),
        }
    }

    /// The review the view draws.
    pub fn review(&self) -> &Arc<BatchReview> {
        &self.review
    }

    /// The exact text the user reviewed, and the one to send.
    pub fn text(&self) -> &str {
        &self.text
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ARM_DELAY, Coverage, layout};
    use super::*;
    use std::time::{Duration, Instant};

    fn screen(text: &str, lines: usize, cut: bool, no_scrollback: bool) -> Screen {
        Screen {
            text: text.to_owned(),
            lines,
            cut,
            no_scrollback,
        }
    }

    fn rows(review: &ScreenReview) -> Vec<(LineKind, String)> {
        review
            .review()
            .doc()
            .lines()
            .iter()
            .map(|l| (l.kind, l.text_for_review_renderer_only().to_owned()))
            .collect()
    }

    fn has(rows: &[(LineKind, String)], kind: LineKind, text: &str) -> bool {
        rows.iter().any(|(k, t)| *k == kind && t.contains(text))
    }

    // Spec (chat-pane, read_pane): the preview is the text, under a header.
    #[test]
    fn the_review_has_a_summary_and_one_context_row_per_line() {
        let review = ScreenReview::new(3, &screen("$ ls\nsrc", 2, false, false));
        let rows = rows(&review);
        assert_eq!(
            rows[0],
            (LineKind::Summary, "pane 3 screen, 2 lines".to_owned())
        );
        assert_eq!(rows[1], (LineKind::Context, " $ ls".to_owned()));
        assert_eq!(rows[2], (LineKind::Context, " src".to_owned()));
        assert_eq!(rows.len(), 3);
        assert_eq!(review.review().counts(), "pane 3 screen, 2 lines");
        assert_eq!(review.text(), "$ ls\nsrc");
        assert!(review.review().doc().reviewable());
    }

    #[test]
    fn a_cut_adds_a_warning_and_a_missing_scrollback_adds_another() {
        let cut = ScreenReview::new(3, &screen("a", 1, true, false));
        assert!(has(&rows(&cut), LineKind::Warning, "cut"));
        let alt = ScreenReview::new(3, &screen("a", 1, false, true));
        let rows = rows(&alt);
        assert!(has(
            &rows,
            LineKind::Warning,
            "full-screen app: visible screen only"
        ));
        assert!(!has(&rows, LineKind::Warning, "cut"));
        let plain = ScreenReview::new(3, &screen("a", 1, false, false));
        assert!(!rows_have_warning(&plain));
    }

    fn rows_have_warning(review: &ScreenReview) -> bool {
        rows(review).iter().any(|(k, _)| *k == LineKind::Warning)
    }

    #[test]
    fn a_single_line_is_singular_in_the_header() {
        let review = ScreenReview::new(12, &screen("a", 1, false, false));
        assert_eq!(review.review().counts(), "pane 12 screen, 1 line");
    }

    // The gate is the batch one: every line seen, then the delay.
    #[test]
    fn y_is_locked_until_every_line_is_seen_and_the_delay_passed() {
        let text = (0..30)
            .map(|i| format!("l{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let review = ScreenReview::new(3, &screen(&text, 30, false, false));
        let doc = review.review().doc();
        let layout = layout(doc, 20);
        let t0 = Instant::now();
        let mut coverage = Coverage::started_at(doc, t0);
        let late = t0 + Duration::from_secs(60);
        assert!(!coverage.can_approve(late), "nothing seen yet");
        coverage.show(&layout, 0..5);
        assert!(!coverage.can_approve(late), "most lines unseen");
        coverage.show(&layout, 0..layout.rows().len());
        assert!(!coverage.can_approve(t0), "the delay blocks y");
        assert!(coverage.can_approve(t0 + ARM_DELAY));
    }

    // D5: the rows are what is sent, escaped only if something needed it.
    #[test]
    fn the_rows_equal_the_escape_of_the_lines() {
        let text = "ok ✓\nünï\nplain";
        let review = ScreenReview::new(3, &screen(text, 3, false, false));
        let shown: Vec<String> = rows(&review)
            .into_iter()
            .filter(|(k, _)| *k == LineKind::Context)
            .map(|(_, t)| t)
            .collect();
        let expected: Vec<String> = text.lines().map(|l| format!(" {}", escape(l))).collect();
        assert_eq!(shown, expected);
    }

    #[test]
    fn debug_prints_counts_and_never_the_text() {
        let review = ScreenReview::new(3, &screen("hunter2 is the password", 1, false, false));
        let text = format!("{review:?}");
        assert!(
            !text.contains("hunter2") && !text.contains("password"),
            "{text}"
        );
        assert!(text.contains("23"), "the byte count shows: {text}");
    }
}
