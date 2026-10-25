//! The review of a staged batch: what the user reads before `y`. Pure: no IO
//! and no clock reads (the caller passes `now`).
//!
//! INVARIANT: the rows of a [`ReviewDoc`] may hold the text of a file the
//! model never saw, secrets included (the `base` of an overwrite or an edit,
//! the removed rows of a diff). They are shown ONLY to the user, in the review
//! view, and NEVER go into a tool result or anything sent to the model.
//! Therefore `ReviewDoc` has no `Display`, no `Serialize`, no conversion into
//! a `String`, and its `Debug` prints counts only. Whatever the model is told
//! about a review (counts, paths) is built from [`ReviewDoc::files`],
//! [`ReviewDoc::added`] and [`ReviewDoc::removed`], never from the lines.
//!
//! Every row is made from [`Line::display_text`] (or `escape` for a path),
//! never from `raw_text`, so no control, bidi or invisible character reaches
//! a terminal: it shows as `<U+XXXX>`.

use std::fmt;
use std::ops::Range;
use std::time::{Duration, Instant};

use crate::core::ai::chat::wrap::{expand_tabs, wrap_ranges};
use crate::core::ai::tools::diff::{self, Diff, Kind, escape};
use crate::core::ai::tools::stage::{Batch, Entry};

/// What a line of the review is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineKind {
    /// Totals of the batch.
    Summary,
    /// A path and what happens to it.
    File,
    /// A warning about the file above it.
    Warning,
    /// `@@ -a,b +c,d @@`.
    Hunk,
    Context,
    Added,
    Removed,
    /// `\ No newline at end of file`.
    Note,
}

/// One logical line of the review. `text` is safe to draw; its first column is
/// the sign of a diff row.
#[derive(Clone, PartialEq, Eq)]
pub struct DocLine {
    pub kind: LineKind,
    pub text: String,
}

/// The whole batch as lines. Built once; the layout and the coverage count
/// work on its lines.
#[derive(Clone, PartialEq, Eq)]
pub struct ReviewDoc {
    lines: Vec<DocLine>,
    files: usize,
    added: usize,
    removed: usize,
    /// A diff left rows out: the user cannot see the whole batch.
    truncated: bool,
}

impl fmt::Debug for ReviewDoc {
    /// Counts only: the rows must never reach a log or a tool result.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReviewDoc")
            .field("lines", &self.lines.len())
            .field("files", &self.files)
            .field("added", &self.added)
            .field("removed", &self.removed)
            .finish()
    }
}

const NO_NEWLINE: &str = "\\ No newline at end of file";

impl ReviewDoc {
    /// The review of `batch`, diffing each file with [`diff::diff`].
    pub fn build(batch: &Batch) -> ReviewDoc {
        ReviewDoc::build_with(batch, diff::diff)
    }

    /// [`ReviewDoc::build`] with the diff function given, so a test can force
    /// a coarse diff.
    pub fn build_with(batch: &Batch, diff: impl Fn(Option<&str>, &str) -> Diff) -> ReviewDoc {
        let mut doc = ReviewDoc {
            lines: Vec::new(),
            files: batch.entries().len(),
            added: 0,
            removed: 0,
            truncated: false,
        };
        let mut body = Vec::new();
        for staged in batch.entries() {
            let path = escape(&staged.target.real.to_string_lossy()).into_owned();
            let diff = match &staged.entry {
                Entry::Dir => {
                    push(&mut body, LineKind::File, format!("create dir {path}"));
                    None
                }
                Entry::File { text, base } => {
                    let verb = if base.is_some() { "modify" } else { "create" };
                    push(&mut body, LineKind::File, format!("{verb} {path}"));
                    Some(diff(base.as_deref(), text))
                }
            };
            if staged.target.build_file {
                push(&mut body, LineKind::Warning, WARN_BUILD.to_owned());
            }
            if let Some(d) = diff {
                doc.add_diff(&mut body, &d);
            }
        }
        let s = if doc.files == 1 { "" } else { "s" };
        let summary = format!("{} file{s}, +{} -{}", doc.files, doc.added, doc.removed);
        push(&mut doc.lines, LineKind::Summary, summary);
        doc.lines.append(&mut body);
        doc
    }

    fn add_diff(&mut self, out: &mut Vec<DocLine>, d: &Diff) {
        self.added += d.added;
        self.removed += d.removed;
        if d.coarse {
            push(out, LineKind::Warning, WARN_COARSE.to_owned());
        }
        if d.truncated {
            self.truncated = true;
            push(out, LineKind::Warning, WARN_TRUNCATED.to_owned());
        }
        for h in &d.hunks {
            let head = format!(
                "@@ -{},{} +{},{} @@",
                h.old_start, h.old_len, h.new_start, h.new_len
            );
            push(out, LineKind::Hunk, head);
            for l in &h.lines {
                let (kind, sign) = match l.kind {
                    Kind::Context => (LineKind::Context, ' '),
                    Kind::Added => (LineKind::Added, '+'),
                    Kind::Removed => (LineKind::Removed, '-'),
                };
                push(out, kind, format!("{sign}{}", l.display_text()));
                if l.no_newline {
                    push(out, LineKind::Note, NO_NEWLINE.to_owned());
                }
            }
        }
    }

    pub fn lines(&self) -> &[DocLine] {
        &self.lines
    }

    /// Whether the batch can be approved at all. A batch with a truncated
    /// diff (rows the user would never see) or with no entry is not: it can
    /// only be rejected.
    pub fn reviewable(&self) -> bool {
        !self.truncated && self.files > 0
    }

    /// Entries of the batch, folders included.
    pub fn files(&self) -> usize {
        self.files
    }

    /// Added lines, exact even when rows were left out.
    pub fn added(&self) -> usize {
        self.added
    }

    pub fn removed(&self) -> usize {
        self.removed
    }
}

const WARN_BUILD: &str = "warning: build file: it can run code when the project is built";
const WARN_COARSE: &str = "warning: coarse diff: the whole file is shown as replaced";
const WARN_TRUNCATED: &str = "warning: too large to review: reject it and ask for smaller changes";

/// How long `y` stays disabled after the view opens. Other asks keep 400 ms.
pub const ARM_DELAY: Duration = Duration::from_millis(800);

/// One drawn row: a slice of one logical line.
#[derive(Clone, PartialEq, Eq)]
pub struct Row {
    /// Index of the logical line in [`ReviewDoc::lines`].
    pub line: usize,
    pub text: String,
}

/// The rows of a doc at one width. The renderer and the coverage gate both
/// take their rows from here: there is no other wrap function.
#[derive(Clone, PartialEq, Eq)]
pub struct Layout {
    width: usize,
    rows: Vec<Row>,
    /// Per logical line, its rows as a range of `rows`.
    spans: Vec<Range<usize>>,
}

impl Layout {
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }
}

/// Wraps every line of `doc` to `width` display columns (wide and CJK
/// characters count two, as the terminal draws them). Every line has at least
/// one row.
pub fn layout(doc: &ReviewDoc, width: usize) -> Layout {
    let mut rows = Vec::new();
    let mut spans = Vec::with_capacity(doc.lines.len());
    for (line, l) in doc.lines.iter().enumerate() {
        let chars: Vec<char> = l.text.chars().collect();
        let first = rows.len();
        for r in wrap_ranges(&chars, width) {
            rows.push(Row {
                line,
                text: chars[r].iter().collect(),
            });
        }
        spans.push(first..rows.len());
    }
    Layout { width, rows, spans }
}

/// What the user has seen, per logical line, and the gate on `y`.
///
/// A line is seen once all its rows have been shown, not before, and it stays
/// seen when the layout changes: the width changes the rows, never the lines.
/// Rows shown at an old width do not carry over (a line half shown at one
/// width is not seen at another).
#[derive(Clone, Debug)]
pub struct Coverage {
    seen: Vec<bool>,
    /// [`ReviewDoc::reviewable`] of the doc; `y` never arms when false.
    reviewable: bool,
    /// Rows shown at `width`, per row of that layout.
    shown: Vec<bool>,
    width: Option<usize>,
    opened: Instant,
}

impl Coverage {
    /// `opened` is when the view opened; `y` arms [`ARM_DELAY`] after it.
    pub fn new(doc: &ReviewDoc, opened: Instant) -> Coverage {
        Coverage {
            seen: vec![false; doc.lines.len()],
            reviewable: doc.reviewable(),
            shown: Vec::new(),
            width: None,
            opened,
        }
    }

    /// Marks the rows `rows` of `layout` as shown (clamped to the layout).
    pub fn show(&mut self, layout: &Layout, rows: Range<usize>) {
        if self.width != Some(layout.width) || self.shown.len() != layout.rows.len() {
            self.width = Some(layout.width);
            self.shown = vec![false; layout.rows.len()];
        }
        let end = rows.end.min(layout.rows.len());
        let start = rows.start.min(end);
        self.shown[start..end].fill(true);
        let (Some(first), Some(last)) = (layout.rows.get(start), end.checked_sub(1)) else {
            return;
        };
        for line in first.line..=layout.rows[last].line {
            let span = layout.spans[line].clone();
            if self.shown[span].iter().all(|s| *s) {
                self.seen[line] = true;
            }
        }
    }

    pub fn total(&self) -> usize {
        self.seen.len()
    }

    /// Lines not seen yet.
    pub fn remaining(&self) -> usize {
        self.seen.iter().filter(|s| !**s).count()
    }

    /// Whether `y` may approve at `now`: the doc is reviewable, every line
    /// seen and [`ARM_DELAY`] elapsed since the view opened. `now` comes from the caller's clock; a
    /// clock that went back does not arm.
    pub fn can_approve(&self, now: Instant) -> bool {
        self.reviewable && self.remaining() == 0 && now.saturating_duration_since(self.opened) >= ARM_DELAY
    }
}

/// Pushes a line with its tabs expanded, so every column is measurable.
fn push(out: &mut Vec<DocLine>, kind: LineKind, text: String) {
    out.push(DocLine {
        kind,
        text: expand_tabs(&text),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ai::tools::diff::{Diff, diff_within};
    use crate::core::ai::tools::paths::WriteTarget;
    use crate::core::ai::tools::stage::{Batch, BatchId, Existing, StageKey};
    use crate::core::layout::PaneId;
    use std::path::PathBuf;
    use std::time::Duration;

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

    fn disk(text: &str) -> Existing<'_> {
        Existing::Present {
            size: text.len() as u64,
            bytes: text.as_bytes(),
        }
    }

    fn texts(doc: &ReviewDoc, kind: LineKind) -> Vec<&str> {
        doc.lines()
            .iter()
            .filter(|l| l.kind == kind)
            .map(|l| l.text.as_str())
            .collect()
    }

    #[test]
    fn a_create_shows_its_path_and_every_new_row_as_added() {
        let mut b = batch();
        b.stage_file(&target("a.txt", false), "one\ntwo\n", Existing::Absent)
            .unwrap();
        let doc = ReviewDoc::build(&b);
        assert_eq!(texts(&doc, LineKind::File), ["create /home/u/proj/a.txt"]);
        assert_eq!(texts(&doc, LineKind::Added), ["+one", "+two"]);
        assert_eq!((doc.files(), doc.added(), doc.removed()), (1, 2, 0));
        assert_eq!(texts(&doc, LineKind::Summary), ["1 file, +2 -0"]);
    }

    #[test]
    fn an_overwrite_shows_context_removed_and_added_rows() {
        let mut b = batch();
        b.stage_file(&target("a.txt", false), "keep\nnew\n", disk("keep\nold\n"))
            .unwrap();
        let doc = ReviewDoc::build(&b);
        assert_eq!(texts(&doc, LineKind::File), ["modify /home/u/proj/a.txt"]);
        assert_eq!(texts(&doc, LineKind::Hunk), ["@@ -1,2 +1,2 @@"]);
        assert_eq!(texts(&doc, LineKind::Context), [" keep"]);
        assert_eq!(texts(&doc, LineKind::Removed), ["-old"]);
        assert_eq!(texts(&doc, LineKind::Added), ["+new"]);
    }

    #[test]
    fn a_dir_shows_its_path_and_no_rows() {
        let mut b = batch();
        b.stage_dir(&target("sub", false)).unwrap();
        b.stage_file(&target("sub/a.txt", false), "x\n", Existing::Absent)
            .unwrap();
        let doc = ReviewDoc::build(&b);
        assert_eq!(
            texts(&doc, LineKind::File),
            [
                "create dir /home/u/proj/sub",
                "create /home/u/proj/sub/a.txt"
            ]
        );
        assert_eq!(doc.files(), 2);
    }

    #[test]
    fn invisible_characters_show_as_escapes_in_every_kind_of_row() {
        let mut b = batch();
        let on_disk = "ok\nold\u{202E}y\n\u{1b}[31m\n";
        b.edit_file(
            &target("a.txt", false),
            "old\u{202E}y",
            "new",
            disk(on_disk),
        )
        .unwrap();
        let doc = ReviewDoc::build(&b);
        assert_eq!(texts(&doc, LineKind::Removed), ["-old<U+202E>y"]);
        assert_eq!(texts(&doc, LineKind::Context), [" ok", " <U+001B>[31m"]);
        for l in doc.lines() {
            assert!(!l.text.chars().any(|c| c.is_control()), "{:?}", l.kind);
        }
    }

    #[test]
    fn a_build_file_carries_a_warning_and_a_plain_file_does_not() {
        let mut b = batch();
        b.stage_file(&target("build.rs", true), "x\n", Existing::Absent)
            .unwrap();
        let w = texts(&ReviewDoc::build(&b), LineKind::Warning)
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("build file"), "{}", w[0]);
        let mut p = batch();
        p.stage_file(&target("a.txt", false), "x\n", Existing::Absent)
            .unwrap();
        assert!(texts(&ReviewDoc::build(&p), LineKind::Warning).is_empty());
    }

    #[test]
    fn a_coarse_diff_is_flagged() {
        let mut b = batch();
        b.stage_file(&target("a.txt", false), "b\n", disk("a\n"))
            .unwrap();
        let coarse =
            |base: Option<&str>, new: &str| -> Diff { diff_within(base, new, Duration::ZERO) };
        let doc = ReviewDoc::build_with(&b, coarse);
        let w = texts(&doc, LineKind::Warning);
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("coarse"), "{}", w[0]);
        assert!(texts(&ReviewDoc::build(&b), LineKind::Warning).is_empty());
    }

    #[test]
    fn a_truncated_diff_is_flagged() {
        let mut b = batch();
        let big: String = (0..2100).map(|i| format!("l{i}\n")).collect();
        b.stage_file(&target("a.txt", false), &big, Existing::Absent)
            .unwrap();
        let doc = ReviewDoc::build(&b);
        let w = texts(&doc, LineKind::Warning);
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("too large to review"), "{}", w[0]);
        assert_eq!(doc.added(), 2100);
        assert!(!doc.reviewable());
    }

    #[test]
    fn a_truncated_diff_is_never_approvable_whatever_was_seen() {
        let mut b = batch();
        let big: String = (0..2100).map(|i| format!("l{i}\n")).collect();
        b.stage_file(&target("a.txt", false), &big, Existing::Absent)
            .unwrap();
        let doc = ReviewDoc::build(&b);
        let l = layout(&doc, 40);
        let t0 = start();
        let mut c = Coverage::new(&doc, t0);
        c.show(&l, 0..l.rows().len());
        assert_eq!(c.remaining(), 0, "every visible row was shown");
        assert!(!c.can_approve(t0 + Duration::from_secs(60)));
    }

    #[test]
    fn an_empty_batch_is_not_reviewable_nor_approvable() {
        let doc = ReviewDoc::build(&batch());
        assert!(!doc.reviewable());
        let l = layout(&doc, 40);
        let t0 = start();
        let mut c = Coverage::new(&doc, t0);
        c.show(&l, 0..l.rows().len());
        assert!(!c.can_approve(t0 + Duration::from_secs(60)));
    }

    #[test]
    fn a_normal_batch_is_reviewable() {
        assert!(doc_of(&["a"]).reviewable());
    }

    #[test]
    fn a_row_without_a_newline_gets_a_note() {
        let mut b = batch();
        b.stage_file(&target("a.txt", false), "x", Existing::Absent)
            .unwrap();
        let doc = ReviewDoc::build(&b);
        assert_eq!(
            texts(&doc, LineKind::Note),
            ["\\ No newline at end of file"]
        );
    }

    #[test]
    fn tabs_are_expanded_so_the_width_is_measurable() {
        let mut b = batch();
        b.stage_file(&target("a.txt", false), "\tx\n", Existing::Absent)
            .unwrap();
        let doc = ReviewDoc::build(&b);
        assert_eq!(texts(&doc, LineKind::Added), ["+   x"]);
    }

    /// Autoref probe: `IS` is true only when `T: Bound` holds (the inherent
    /// const wins over the trait one).
    macro_rules! implements {
        ($t:ty, $($bound:tt)+) => {{
            #[allow(dead_code)]
            trait No {
                const IS: bool = false;
            }
            impl<T: ?Sized> No for T {}
            struct Probe<T: ?Sized>(std::marker::PhantomData<T>);
            #[allow(dead_code)]
            impl<T: ?Sized + $($bound)+> Probe<T> {
                const IS: bool = true;
            }
            <Probe<$t>>::IS
        }};
    }

    #[test]
    fn the_probe_sees_a_bound_that_holds() {
        assert!(implements!(String, std::fmt::Display));
        assert!(!implements!(Vec<u8>, std::fmt::Display));
    }

    #[test]
    fn a_review_has_no_path_into_a_string() {
        assert!(!implements!(ReviewDoc, std::fmt::Display));
        assert!(!implements!(ReviewDoc, ToString));
        assert!(!implements!(ReviewDoc, Into<String>));
        assert!(!implements!(DocLine, std::fmt::Display));
        assert!(!implements!(DocLine, Into<String>));
    }

    #[test]
    fn debug_prints_counts_and_never_a_row() {
        let mut b = batch();
        b.edit_file(
            &target("a.txt", false),
            "sk-SECRET-token",
            "gone",
            disk("keep\nsk-SECRET-token\n"),
        )
        .unwrap();
        let doc = ReviewDoc::build(&b);
        assert!(texts(&doc, LineKind::Removed)[0].contains("SECRET"));
        let shown = format!("{doc:?} {:#?}", doc);
        assert!(
            !shown.contains("SECRET") && !shown.contains("keep"),
            "{shown}"
        );
        assert!(shown.contains("files: 1"), "{shown}");
    }

    // --- layout and coverage ---

    use std::time::Instant;

    /// A doc of one created file whose rows are `rows`; its logical lines are
    /// the summary, the file header, the hunk header and one per row.
    fn doc_of(rows: &[&str]) -> ReviewDoc {
        let mut b = batch();
        let text: String = rows.iter().map(|r| format!("{r}\n")).collect();
        b.stage_file(&target("a.txt", false), &text, Existing::Absent)
            .unwrap();
        ReviewDoc::build(&b)
    }

    fn row_texts(l: &Layout) -> Vec<&str> {
        l.rows().iter().map(|r| r.text.as_str()).collect()
    }

    #[test]
    fn a_long_line_wraps_and_a_short_one_stays_whole() {
        let l = layout(&doc_of(&["abcdefgh", "ab"]), 5);
        let rows = row_texts(&l);
        assert!(
            rows.contains(&"+abcd") && rows.contains(&"efgh"),
            "{rows:?}"
        );
        assert!(rows.contains(&"+ab"), "{rows:?}");
        let long = l.rows().iter().filter(|r| r.text == "efgh").count();
        assert_eq!(long, 1);
    }

    #[test]
    fn rows_of_one_line_rejoin_to_the_line() {
        let doc = doc_of(&["the quick brown fox jumps"]);
        let l = layout(&doc, 7);
        for (i, line) in doc.lines().iter().enumerate() {
            let joined: String = l
                .rows()
                .iter()
                .filter(|r| r.line == i)
                .map(|r| r.text.as_str())
                .collect();
            assert_eq!(joined, line.text);
        }
    }

    #[test]
    fn wide_characters_count_two_columns() {
        let l = layout(&doc_of(&["日本語"]), 5);
        let rows = row_texts(&l);
        assert!(rows.contains(&"+日本") && rows.contains(&"語"), "{rows:?}");
    }

    #[test]
    fn an_escape_counts_by_its_visible_width() {
        let mut b = batch();
        let on_disk = "a\u{200B}b\n";
        b.edit_file(&target("a.txt", false), "a\u{200B}b", "z", disk(on_disk))
            .unwrap();
        let rows = layout(&ReviewDoc::build(&b), 8)
            .rows()
            .iter()
            .map(|r| r.text.clone())
            .collect::<Vec<_>>();
        // `-a<U+200B>b` is 11 columns: the escape is eight, not one.
        assert!(rows.contains(&"-a<U+200".to_owned()), "{rows:?}");
        assert!(rows.contains(&"B>b".to_owned()), "{rows:?}");
    }

    #[test]
    fn a_zero_width_still_makes_progress() {
        let l = layout(&doc_of(&["ab"]), 0);
        let rows = row_texts(&l);
        assert!(rows.windows(3).any(|w| w == ["+", "a", "b"]), "{rows:?}");
    }

    fn start() -> Instant {
        Instant::now()
    }

    #[test]
    fn a_line_is_seen_only_when_all_its_rows_were_shown() {
        let doc = doc_of(&["abcdefghij"]);
        let l = layout(&doc, 4);
        let row = l.rows().iter().position(|r| r.text == "+abc").unwrap();
        let mut c = Coverage::new(&doc, start());
        let before = c.remaining();
        c.show(&l, row..row + 2);
        assert_eq!(c.remaining(), before, "two of three rows is not enough");
        c.show(&l, row + 1..row + 3);
        assert_eq!(c.remaining(), before - 1);
    }

    #[test]
    fn showing_every_row_sees_every_line() {
        let doc = doc_of(&["a", "b", "c"]);
        let l = layout(&doc, 20);
        let mut c = Coverage::new(&doc, start());
        assert_eq!(c.total(), doc.lines().len());
        assert_eq!(c.remaining(), c.total());
        c.show(&l, 0..l.rows().len());
        assert_eq!((c.remaining(), c.total()), (0, doc.lines().len()));
    }

    #[test]
    fn a_range_past_the_end_is_clamped() {
        let doc = doc_of(&["a"]);
        let l = layout(&doc, 20);
        let mut c = Coverage::new(&doc, start());
        c.show(&l, 0..1_000);
        assert_eq!(c.remaining(), 0);
    }

    #[test]
    fn coverage_survives_a_resize() {
        let doc = doc_of(&["aaaaaaaaaaaa", "bbbbbbbbbbbb", "cc"]);
        let wide = layout(&doc, 40);
        let mut c = Coverage::new(&doc, start());
        c.show(&wide, 0..wide.rows().len());
        assert_eq!(c.remaining(), 0);
        let narrow = layout(&doc, 5);
        assert!(narrow.rows().len() > wide.rows().len());
        c.show(&narrow, 0..0);
        assert_eq!(c.remaining(), 0, "nothing seen is lost");
    }

    #[test]
    fn a_resize_keeps_seen_lines_and_drops_half_seen_rows() {
        let doc = doc_of(&["aaaaaaaaaaaa", "b"]);
        let wide = layout(&doc, 40);
        let mut c = Coverage::new(&doc, start());
        let b = wide.rows().iter().position(|r| r.text == "+b").unwrap();
        c.show(&wide, b..b + 1);
        let seen = c.total() - c.remaining();
        assert_eq!(seen, 1);
        let narrow = layout(&doc, 5);
        let a = narrow
            .rows()
            .iter()
            .position(|r| r.text == "+aaaa")
            .unwrap();
        c.show(&narrow, a..a + 1);
        assert_eq!(c.total() - c.remaining(), 1, "one row of three is not seen");
        c.show(&narrow, a + 1..a + 3);
        assert_eq!(c.total() - c.remaining(), 2);
    }

    #[test]
    fn y_needs_every_line_seen_and_the_delay() {
        let t0 = start();
        let doc = doc_of(&["a", "b"]);
        let l = layout(&doc, 20);
        let mut c = Coverage::new(&doc, t0);
        let ms = Duration::from_millis;
        assert!(!c.can_approve(t0 + ms(5_000)), "unseen lines block y");
        c.show(&l, 0..l.rows().len());
        assert!(!c.can_approve(t0), "the delay blocks y");
        assert!(!c.can_approve(t0 + ms(799)));
        assert!(c.can_approve(t0 + ms(800)));
        assert!(c.can_approve(t0 + ms(60_000)));
        assert_eq!(ARM_DELAY, ms(800));
    }

    #[test]
    fn a_clock_that_went_back_does_not_arm() {
        let t0 = start() + Duration::from_secs(10);
        let doc = doc_of(&["a"]);
        let l = layout(&doc, 20);
        let mut c = Coverage::new(&doc, t0);
        c.show(&l, 0..l.rows().len());
        assert!(!c.can_approve(t0 - Duration::from_secs(5)));
    }

    #[test]
    fn rows_shown_at_another_width_with_the_same_row_count_do_not_count() {
        let long = "x".repeat(62);
        let doc = doc_of(&[long.as_str()]);
        let (w30, w31) = (layout(&doc, 30), layout(&doc, 31));
        assert_eq!(w30.rows().len(), w31.rows().len());
        let first = |l: &Layout| l.rows().iter().position(|r| r.line == 3).unwrap();
        let mut c = Coverage::new(&doc, start());
        let before = c.remaining();
        c.show(&w30, first(&w30)..first(&w30) + 1);
        c.show(&w31, first(&w31) + 1..first(&w31) + 3);
        assert_eq!(c.remaining(), before, "the rows were of two layouts");
        c.show(&w31, first(&w31)..first(&w31) + 1);
        assert_eq!(c.remaining(), before - 1);
    }
}
