//! The state of the review view while a batch waits for the user: what is
//! on screen, what has been seen and when `y` may answer. Pure: the caller
//! passes the clock and the size of the pane.
//!
//! The rows come from [`layout`], the one wrap function, and the rows on
//! screen are the rows the coverage counts, so the counter the user reads is
//! the gate `y` obeys.

use std::cell::Cell;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::review::{ARM_DELAY, BatchReview, Coverage, Layout, Row, layout};

/// Rows of the view that are not the body: the counter above and the keys
/// below.
const CHROME: usize = 2;

/// How long the hint for a `y` pressed with lines unseen stays in the counter.
pub const HINT_FOR: Duration = Duration::from_secs(2);

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
    /// What the last frame drew, until [`ReviewView::credit_drawn`] takes it.
    drawn: Cell<Option<Drawn>>,
    /// Whether the wake-up for the end of the delay was given.
    woke: bool,
    /// Until when the counter explains the `y` that found the gate locked.
    hint_until: Option<Instant>,
}

/// A frame of the view: when, how big the area was and the first row on it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Drawn {
    at: Instant,
    cols: usize,
    rows: usize,
    top: usize,
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
            && self.coverage.started() == other.coverage.started()
    }
}

impl Eq for ReviewView {}

impl ReviewView {
    /// Opens the view of `review` for a pane of `size` (columns, rows). No
    /// row counts as seen and the delay does not run until a frame draws it
    /// (see [`ReviewView::credit_drawn`]).
    pub fn new(review: Arc<BatchReview>, size: (usize, usize)) -> Self {
        Self {
            coverage: Coverage::new(review.doc()),
            layout: layout(review.doc(), size.0),
            review,
            top: 0,
            drawn: Cell::new(None),
            woke: false,
            hint_until: None,
        }
    }

    /// A `y` was pressed at `now`. When the gate or the batch will not let it
    /// through it starts a hint in the counter and returns true; it never
    /// approves, scrolls or counts anything.
    pub fn y_pressed(&mut self, now: Instant) -> bool {
        let hint = !self.reviewable() || self.remaining() > 0;
        if hint {
            self.hint_until = Some(now + HINT_FOR);
        }
        hint
    }

    fn hint_live(&self, now: Instant) -> bool {
        self.hint_until.is_some_and(|until| now < until)
    }

    /// The lines still unseen while the hint of a locked `y` shows at `now`.
    pub fn locked_hint(&self, now: Instant) -> Option<usize> {
        let live = self.hint_live(now) && self.reviewable();
        (live && self.remaining() > 0).then(|| self.remaining())
    }

    /// Whether the hint of a `y` this batch can never answer shows at `now`.
    pub fn refused_hint(&self, now: Instant) -> bool {
        self.hint_live(now) && !self.reviewable()
    }

    /// The rows of the body in a pane `rows` high.
    pub fn body_rows(rows: usize) -> usize {
        rows.saturating_sub(CHROME)
    }

    /// The pane is `size` now: lays the rows out again if the width changed,
    /// keeps `top` in range. It counts nothing as seen: only a drawn row is
    /// (see [`ReviewView::credit_drawn`]). What was seen stays seen.
    pub fn fit(&mut self, (cols, rows): (usize, usize)) {
        if self.layout.width() != cols.max(1) {
            self.layout = layout(self.review.doc(), cols);
        }
        let body = Self::body_rows(rows);
        self.top = self.top.min(self.layout.rows().len().saturating_sub(body));
    }

    /// FOR THE REVIEW RENDERER ONLY: this frame drew the body into an area of
    /// `cols` x `rows` at `now`.
    pub(crate) fn note_drawn(&self, now: Instant, (cols, rows): (usize, usize)) {
        self.drawn.set(Some(Drawn {
            at: now,
            cols,
            rows,
            top: self.top,
        }));
    }

    /// After a frame: the rows it drew count as seen and the first frame
    /// starts the delay. A frame nobody drew, or of another width than the
    /// layout, counts nothing. Returns whether another frame is needed: the
    /// counter drawn in this one is out of date, or the layout had to follow
    /// the area.
    pub fn credit_drawn(&mut self) -> bool {
        let Some(frame) = self.drawn.take() else {
            return false;
        };
        if self.layout.width() != frame.cols.max(1) {
            self.fit((frame.cols, frame.rows));
            return true;
        }
        self.coverage.start(frame.at);
        let before = self.coverage.remaining();
        let body = Self::body_rows(frame.rows);
        if body > 0 {
            self.coverage
                .show(&self.layout, frame.top..frame.top + body);
        }
        self.coverage.remaining() != before
    }

    /// Whether the screen must be drawn again for the countdown: while it
    /// runs, and once more when it ends, so `y in 0.1s` becomes `y ready` by
    /// itself.
    pub fn wake(&mut self, now: Instant) -> bool {
        // The hint needs a frame while it shows and one when it ends.
        if self.hint_until.is_some() {
            if self.hint_live(now) {
                return true;
            }
            self.hint_until = None;
            return true;
        }
        if self.coverage.started().is_none() {
            return false;
        }
        if !self.arm_left(now).is_zero() {
            return true;
        }
        !std::mem::replace(&mut self.woke, true)
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

    /// How long until the delay is over; all of it until the first frame.
    pub fn arm_left(&self, now: Instant) -> Duration {
        self.coverage.started().map_or(ARM_DELAY, |opened| {
            ARM_DELAY.saturating_sub(now.saturating_duration_since(opened))
        })
    }

    /// Whether `n` and Esc answer at `now`: a batch that can be reviewed
    /// waits for the same delay as `y`; one that cannot (too large, empty)
    /// has no other way out, so they answer at once.
    pub fn answerable(&self, now: Instant) -> bool {
        !self.reviewable() || (self.coverage.started().is_some() && self.arm_left(now).is_zero())
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

    fn t0() -> Instant {
        use std::sync::OnceLock;
        static BASE: OnceLock<Instant> = OnceLock::new();
        *BASE.get_or_init(Instant::now)
    }

    /// A frame at `at` that drew the view into an area of `size`.
    fn draw(v: &mut ReviewView, at: Instant, size: (usize, usize)) -> bool {
        v.note_drawn(at, size);
        v.credit_drawn()
    }

    /// Scrolls and draws the frame, at t0.
    fn go(v: &mut ReviewView, by: Move, size: (usize, usize)) {
        v.scroll(by, size);
        draw(v, t0(), size);
    }

    /// Opened and drawn once at t0.
    fn open(n: usize) -> (ReviewView, Instant) {
        let mut v = ReviewView::new(numbered(n), SIZE);
        draw(&mut v, t0(), SIZE);
        (v, t0())
    }

    #[test]
    fn opening_counts_nothing_and_the_first_frame_counts_the_first_screenful() {
        let mut v = ReviewView::new(numbered(30), SIZE);
        assert_eq!((v.total(), v.remaining()), (33, 33));
        // 12 rows less the counter row and the keys row.
        assert_eq!(ReviewView::body_rows(12), 10);
        assert!(draw(&mut v, t0(), SIZE), "the counter on screen is stale");
        assert_eq!((v.total(), v.remaining()), (33, 23));
        assert!(
            !draw(&mut v, t0(), SIZE),
            "a second frame shows nothing new"
        );
        assert_eq!(v.visible(12).len(), 10);
    }

    #[test]
    fn a_frame_is_credited_once() {
        let mut v = ReviewView::new(numbered(30), SIZE);
        v.note_drawn(t0(), SIZE);
        v.scroll(Move::Bottom, SIZE);
        // The scroll moved the top after the frame; the frame showed the old one.
        v.credit_drawn();
        assert_eq!(v.remaining(), 23);
        v.credit_drawn();
        assert_eq!(v.remaining(), 23, "nothing left to credit");
    }

    /// Pages down until the last row is on screen.
    fn read_all(v: &mut ReviewView) {
        for _ in 0..v.total() {
            go(v, Move::PageDown, SIZE);
        }
    }

    #[test]
    fn jumping_to_the_end_does_not_count_the_rows_skipped() {
        let (mut v, _) = open(30);
        go(&mut v, Move::Bottom, SIZE);
        assert_eq!(v.remaining(), 13);
    }

    #[test]
    fn scrolling_without_a_frame_counts_nothing() {
        let (mut v, _) = open(30);
        v.scroll(Move::PageDown, SIZE);
        assert_eq!(v.remaining(), 23);
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
        go(&mut v, Move::Up, SIZE);
        assert_eq!(v.top(), 0);
        go(&mut v, Move::Down, SIZE);
        assert_eq!((v.top(), v.remaining()), (1, 22));
        go(&mut v, Move::Bottom, SIZE);
        assert_eq!(v.top(), 33 - 10);
        go(&mut v, Move::Down, SIZE);
        assert_eq!(v.top(), 33 - 10);
        go(&mut v, Move::Top, SIZE);
        assert_eq!(v.top(), 0);
    }

    #[test]
    fn a_page_moves_by_the_body_height() {
        let (mut v, _) = open(30);
        go(&mut v, Move::PageDown, SIZE);
        assert_eq!(v.top(), 10);
        go(&mut v, Move::PageUp, SIZE);
        assert_eq!(v.top(), 0);
    }

    #[test]
    fn a_resize_keeps_the_lines_already_seen_and_counts_nothing_new() {
        let (mut v, _) = open(30);
        read_all(&mut v);
        assert_eq!(v.remaining(), 0);
        v.fit((20, 6));
        assert_eq!(v.remaining(), 0);
        assert!(v.top() <= 33 - ReviewView::body_rows(6));
        let (mut fresh, _) = open(30);
        fresh.fit((20, 6));
        assert_eq!(fresh.remaining(), 23, "a resize shows no row by itself");
    }

    #[test]
    fn a_frame_of_another_width_than_the_layout_counts_nothing_and_asks_for_another() {
        let (mut v, _) = open(30);
        assert!(draw(&mut v, t0(), (30, 12)), "the layout follows the area");
        assert_eq!(v.remaining(), 23);
        assert!(!draw(&mut v, t0(), (30, 12)), "the rows were seen already");
        assert_eq!(v.remaining(), 23);
    }

    #[test]
    fn a_pane_with_no_room_for_a_body_sees_nothing() {
        for rows in [0, 1, 2] {
            let mut v = ReviewView::new(numbered(2), (40, rows));
            draw(&mut v, t0(), (40, rows));
            v.scroll(Move::Bottom, (40, rows));
            draw(&mut v, t0(), (40, rows));
            assert_eq!(v.remaining(), v.total(), "{rows} rows");
            assert!(!v.can_approve(t0() + Duration::from_secs(60)));
        }
    }

    #[test]
    fn a_batch_that_is_too_large_never_arms() {
        let mut v = ReviewView::new(numbered(2100), SIZE);
        draw(&mut v, t0(), SIZE);
        assert!(!v.reviewable());
        let end = v.total();
        while v.remaining() > 0 && v.top() + ReviewView::body_rows(SIZE.1) < end {
            go(&mut v, Move::PageDown, SIZE);
        }
        assert_eq!(v.remaining(), 0, "every row was shown");
        assert!(!v.can_approve(t0() + Duration::from_secs(60)));
    }

    #[test]
    fn the_countdown_runs_from_the_first_frame_to_the_delay() {
        let mut v = ReviewView::new(numbered(1), SIZE);
        let later = t0() + Duration::from_secs(5);
        assert_eq!(v.arm_left(later), ARM_DELAY, "not drawn yet");
        draw(&mut v, t0(), SIZE);
        assert_eq!(v.arm_left(t0()), ARM_DELAY);
        assert_eq!(
            v.arm_left(t0() + Duration::from_millis(300)),
            ARM_DELAY - Duration::from_millis(300)
        );
        assert_eq!(v.arm_left(t0() + ARM_DELAY), Duration::ZERO);
        assert_eq!(v.arm_left(t0() + Duration::from_secs(9)), Duration::ZERO);
    }

    #[test]
    fn only_the_first_frame_starts_the_delay() {
        let mut v = ReviewView::new(numbered(1), SIZE);
        draw(&mut v, t0(), SIZE);
        draw(&mut v, t0() + Duration::from_millis(500), SIZE);
        assert_eq!(v.arm_left(t0() + ARM_DELAY), Duration::ZERO);
    }

    #[test]
    fn rejecting_waits_for_the_delay_unless_the_batch_cannot_be_reviewed() {
        let mut v = ReviewView::new(numbered(5), SIZE);
        assert!(!v.answerable(t0() + Duration::from_secs(60)), "not drawn");
        draw(&mut v, t0(), SIZE);
        assert!(!v.answerable(t0() + ARM_DELAY - Duration::from_millis(1)));
        assert!(v.answerable(t0() + ARM_DELAY));
        let big = ReviewView::new(numbered(2100), SIZE);
        assert!(big.answerable(t0()), "its only exit");
    }

    #[test]
    fn wake_rests_again_once_the_hint_expires() {
        let (mut v, now) = open(30);
        // The delay was already announced: only the hint is left to wake for.
        assert!(v.wake(now + ARM_DELAY));
        assert!(!v.wake(now + ARM_DELAY));
        let pressed = now + Duration::from_secs(5);
        v.y_pressed(pressed);
        assert!(
            v.wake(pressed + Duration::from_millis(500)),
            "while it shows"
        );
        assert!(v.wake(pressed + HINT_FOR), "the frame that clears it");
        assert!(!v.wake(pressed + HINT_FOR + Duration::from_millis(16)));
        assert_eq!(v.locked_hint(pressed + HINT_FOR), None);
    }

    #[test]
    fn y_on_a_batch_that_cannot_be_reviewed_hints_and_then_rests() {
        let mut v = ReviewView::new(numbered(2100), SIZE);
        draw(&mut v, t0(), SIZE);
        let at = t0() + Duration::from_secs(10);
        v.y_pressed(at);
        assert!(v.refused_hint(at + Duration::from_millis(1_999)));
        assert_eq!(v.locked_hint(at), None, "no lines to scroll to");
        assert!(!v.refused_hint(at + HINT_FOR));
        assert!(v.wake(at + Duration::from_millis(500)));
        assert!(v.wake(at + HINT_FOR));
        let after = at + HINT_FOR + Duration::from_millis(16);
        v.wake(after); // the delay's own last frame, if it was not given yet
        assert!(!v.wake(after), "then it rests");
    }

    #[test]
    fn the_wake_runs_while_counting_down_and_once_when_it_ends() {
        let mut v = ReviewView::new(numbered(1), SIZE);
        assert!(!v.wake(t0()), "not drawn");
        draw(&mut v, t0(), SIZE);
        assert!(v.wake(t0() + Duration::from_millis(100)));
        assert!(v.wake(t0() + ARM_DELAY - Duration::from_millis(1)));
        assert!(v.wake(t0() + ARM_DELAY), "the frame that shows `y ready`");
        assert!(!v.wake(t0() + ARM_DELAY + Duration::from_millis(16)));
    }

    #[test]
    fn a_view_of_another_batch_is_not_equal() {
        let a = numbered(3);
        let v = ReviewView::new(a.clone(), SIZE);
        assert_eq!(v, ReviewView::new(a, SIZE));
        assert_ne!(v, ReviewView::new(numbered(3), SIZE));
    }

    #[test]
    fn a_build_file_batch_builds() {
        let r = review_of(&[("Makefile", "all:\n", true)]);
        let v = ReviewView::new(r, SIZE);
        assert!(v.reviewable());
    }
}
