//! The state of the review view while a batch waits for the user: what is
//! on screen, what has been seen and when `y` may answer. Pure: the caller
//! passes the clock and the size of the pane.
//!
//! The rows come from [`layout`], the one wrap function, and the rows on
//! screen are the rows the coverage counts, so the counter the user reads is
//! the gate `y` obeys.

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::review::{ARM_DELAY, BatchReview, Coverage, Layout, Row, layout};

/// Rows of the view that are not the body: the counter above and the keys
/// below.
const CHROME: usize = 2;

/// A scrolling key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Move {
    Up,
    Down,
    PageUp,
    PageDown,
    Top,
    Bottom,
}

/// The open review of one proposed batch.
#[derive(Clone)]
pub struct ReviewView {
    review: Arc<BatchReview>,
    coverage: Coverage,
    layout: Layout,
    /// The first row on screen.
    top: usize,
    opened: Instant,
}

impl fmt::Debug for ReviewView {
    /// Counts only, like the doc it shows.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReviewView")
            .field("review", &self.review)
            .field("top", &self.top)
            .field("remaining", &self.remaining())
            .finish()
    }
}

impl PartialEq for ReviewView {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.review, &other.review)
            && self.top == other.top
            && self.opened == other.opened
    }
}

impl Eq for ReviewView {}

impl ReviewView {
    /// Opens the view of `review` at `opened`, in a pane of `size` (columns,
    /// rows). The first screenful counts as seen.
    pub fn new(review: Arc<BatchReview>, opened: Instant, size: (usize, usize)) -> Self {
        let mut view = Self {
            coverage: Coverage::new(review.doc(), opened),
            layout: layout(review.doc(), size.0),
            review,
            top: 0,
            opened,
        };
        view.fit(size);
        view
    }

    /// The rows of the body in a pane `rows` high.
    pub fn body_rows(rows: usize) -> usize {
        rows.saturating_sub(CHROME)
    }

    /// The pane is `size` now: lays the rows out again if the width changed,
    /// keeps `top` in range and counts the rows on screen as seen. What was
    /// seen stays seen.
    pub fn fit(&mut self, (cols, rows): (usize, usize)) {
        if self.layout.width() != cols.max(1) {
            self.layout = layout(self.review.doc(), cols);
        }
        let body = Self::body_rows(rows);
        self.top = self.top.min(self.layout.rows().len().saturating_sub(body));
        if body > 0 {
            self.coverage.show(&self.layout, self.top..self.top + body);
        }
    }

    pub fn scroll(&mut self, by: Move, size: (usize, usize)) {
        let body = Self::body_rows(size.1);
        let page = body.max(1);
        self.top = match by {
            Move::Up => self.top.saturating_sub(1),
            Move::Down => self.top.saturating_add(1),
            Move::PageUp => self.top.saturating_sub(page),
            Move::PageDown => self.top.saturating_add(page),
            Move::Top => 0,
            Move::Bottom => usize::MAX,
        };
        self.fit(size);
    }

    /// The rows on screen in a pane `rows` high, FOR THE REVIEW RENDERER ONLY.
    pub(crate) fn visible(&self, rows: usize) -> &[Row] {
        let all = self.layout.rows();
        let start = self.top.min(all.len());
        &all[start..start.saturating_add(Self::body_rows(rows)).min(all.len())]
    }

    pub fn top(&self) -> usize {
        self.top
    }

    pub fn review(&self) -> &Arc<BatchReview> {
        &self.review
    }

    /// Lines of the doc.
    pub fn total(&self) -> usize {
        self.coverage.total()
    }

    /// Lines not seen yet.
    pub fn remaining(&self) -> usize {
        self.coverage.remaining()
    }

    /// Whether the batch can be approved at all (see `ReviewDoc::reviewable`).
    pub fn reviewable(&self) -> bool {
        self.review.doc().reviewable()
    }

    /// Whether `y` approves at `now`: every line seen, the delay over and the
    /// batch reviewable.
    pub fn can_approve(&self, now: Instant) -> bool {
        self.coverage.can_approve(now)
    }

    /// How long until the delay is over.
    pub fn arm_left(&self, now: Instant) -> Duration {
        ARM_DELAY.saturating_sub(now.saturating_duration_since(self.opened))
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::core::ai::chat::review::{BatchReview, ReviewDoc};
    use crate::core::ai::tools::paths::WriteTarget;
    use crate::core::ai::tools::stage::{Batch, BatchId, Existing, StageKey};
    use crate::core::layout::PaneId;

    fn batch() -> Batch {
        Batch::new(BatchId {
            key: StageKey::Pane(PaneId::for_test(3)),
            generation: 1,
        })
    }

    fn target(path: &str, build_file: bool) -> WriteTarget {
        WriteTarget {
            real: PathBuf::from("/home/u/proj").join(path),
            build_file,
        }
    }

    fn review(b: &Batch) -> Arc<BatchReview> {
        Arc::new(BatchReview::new(ReviewDoc::build(b)))
    }

    /// New files: path, text and whether it is a build file.
    pub fn review_of(files: &[(&str, &str, bool)]) -> Arc<BatchReview> {
        let mut b = batch();
        for (path, text, build_file) in files {
            b.stage_file(&target(path, *build_file), text, Existing::Absent)
                .unwrap();
        }
        review(&b)
    }

    /// An edit of `path`, which holds `on_disk`: `old` becomes `new`.
    pub fn edit_of(path: &str, on_disk: &str, old: &str, new: &str) -> Arc<BatchReview> {
        let mut b = batch();
        let disk = Existing::Present {
            size: on_disk.len() as u64,
            bytes: on_disk.as_bytes(),
        };
        b.edit_file(&target(path, false), old, new, disk).unwrap();
        review(&b)
    }

    /// An edit of `a.txt` that the diff could not minimise in time.
    pub fn coarse_of() -> Arc<BatchReview> {
        use crate::core::ai::tools::diff::{Diff, diff_within};
        let mut b = batch();
        let disk = Existing::Present {
            size: 4,
            bytes: b"old\n",
        };
        b.stage_file(&target("a.txt", false), "new\n", disk)
            .unwrap();
        let coarse = |base: Option<&str>, new: &str| -> Diff {
            diff_within(base, new, std::time::Duration::ZERO)
        };
        Arc::new(BatchReview::new(ReviewDoc::build_with(&b, coarse)))
    }

    /// One new file of `n` lines: the doc has `n + 3` lines (a summary, the
    /// path, a hunk header and the rows).
    pub fn numbered(n: usize) -> Arc<BatchReview> {
        let text: String = (0..n).map(|i| format!("row {i}\n")).collect();
        review_of(&[("a.txt", &text, false)])
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::testing::{numbered, review_of};
    use super::*;
    use crate::core::ai::chat::review::ARM_DELAY;

    const SIZE: (usize, usize) = (40, 12);

    fn open(n: usize) -> (ReviewView, Instant) {
        let t0 = Instant::now();
        (ReviewView::new(numbered(n), t0, SIZE), t0)
    }

    #[test]
    fn opening_counts_the_first_screenful_as_seen() {
        let (v, _) = open(30);
        // 12 rows less the counter row and the keys row.
        assert_eq!(ReviewView::body_rows(12), 10);
        assert_eq!((v.total(), v.remaining()), (33, 23));
        assert_eq!(v.visible(12).len(), 10);
    }

    /// Pages down until the last row is on screen.
    fn read_all(v: &mut ReviewView) {
        for _ in 0..v.total() {
            v.scroll(Move::PageDown, SIZE);
        }
    }

    #[test]
    fn jumping_to_the_end_does_not_count_the_rows_skipped() {
        let (mut v, _) = open(30);
        v.scroll(Move::Bottom, SIZE);
        assert_eq!(v.remaining(), 13);
    }

    #[test]
    fn scrolling_to_the_end_sees_everything_and_y_arms_after_the_delay() {
        let (mut v, t0) = open(30);
        read_all(&mut v);
        assert_eq!(v.remaining(), 0);
        assert!(!v.can_approve(t0 + ARM_DELAY - Duration::from_millis(1)));
        assert!(v.can_approve(t0 + ARM_DELAY));
    }

    #[test]
    fn y_stays_locked_while_a_line_is_unseen_however_long_it_waits() {
        let (v, t0) = open(30);
        assert!(!v.can_approve(t0 + Duration::from_secs(600)));
    }

    #[test]
    fn a_step_moves_one_row_and_clamps_at_both_ends() {
        let (mut v, _) = open(30);
        v.scroll(Move::Up, SIZE);
        assert_eq!(v.top(), 0);
        v.scroll(Move::Down, SIZE);
        assert_eq!((v.top(), v.remaining()), (1, 22));
        v.scroll(Move::Bottom, SIZE);
        assert_eq!(v.top(), 33 - 10);
        v.scroll(Move::Down, SIZE);
        assert_eq!(v.top(), 33 - 10);
        v.scroll(Move::Top, SIZE);
        assert_eq!(v.top(), 0);
    }

    #[test]
    fn a_page_moves_by_the_body_height() {
        let (mut v, _) = open(30);
        v.scroll(Move::PageDown, SIZE);
        assert_eq!(v.top(), 10);
        v.scroll(Move::PageUp, SIZE);
        assert_eq!(v.top(), 0);
    }

    #[test]
    fn a_resize_keeps_the_lines_already_seen() {
        let (mut v, _) = open(30);
        read_all(&mut v);
        assert_eq!(v.remaining(), 0);
        v.fit((20, 6));
        assert_eq!(v.remaining(), 0);
        assert!(v.top() <= 33 - ReviewView::body_rows(6));
    }

    #[test]
    fn a_pane_with_no_room_for_a_body_sees_nothing() {
        for rows in [0, 1, 2] {
            let t0 = Instant::now();
            let mut v = ReviewView::new(numbered(2), t0, (40, rows));
            v.scroll(Move::Bottom, (40, rows));
            assert_eq!(v.remaining(), v.total(), "{rows} rows");
            assert!(!v.can_approve(t0 + Duration::from_secs(60)));
        }
    }

    #[test]
    fn a_batch_that_is_too_large_never_arms() {
        let t0 = Instant::now();
        let mut v = ReviewView::new(numbered(2100), t0, SIZE);
        assert!(!v.reviewable());
        let end = v.total();
        while v.remaining() > 0 && v.top() + ReviewView::body_rows(SIZE.1) < end {
            v.scroll(Move::PageDown, SIZE);
        }
        assert_eq!(v.remaining(), 0, "every row was shown");
        assert!(!v.can_approve(t0 + Duration::from_secs(60)));
    }

    #[test]
    fn the_countdown_runs_from_the_opening_to_the_delay() {
        let (v, t0) = open(1);
        assert_eq!(v.arm_left(t0), ARM_DELAY);
        assert_eq!(
            v.arm_left(t0 + Duration::from_millis(300)),
            ARM_DELAY - Duration::from_millis(300)
        );
        assert_eq!(v.arm_left(t0 + ARM_DELAY), Duration::ZERO);
        assert_eq!(v.arm_left(t0 + Duration::from_secs(9)), Duration::ZERO);
    }

    #[test]
    fn a_view_of_another_batch_is_not_equal() {
        let a = numbered(3);
        let t0 = Instant::now();
        let v = ReviewView::new(a.clone(), t0, SIZE);
        assert_eq!(v, ReviewView::new(a, t0, SIZE));
        assert_ne!(v, ReviewView::new(numbered(3), t0, SIZE));
    }

    #[test]
    fn a_build_file_batch_builds() {
        let r = review_of(&[("Makefile", "all:\n", true)]);
        let v = ReviewView::new(r, Instant::now(), SIZE);
        assert!(v.reviewable());
    }
}
