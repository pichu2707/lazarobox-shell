//! The line diff a batch review shows. Pure: no IO, no clock reads other than
//! the deadline guard below.
//!
//! INVARIANT: the rows of a diff may hold the text of a file the model never
//! saw, secrets included. They are shown ONLY to the user, in the review
//! view, and NEVER go into a tool result or anything sent to the model.
//!
//! Myers through `similar`, 3 lines of context. `diff` is a function of
//! `(base, new)` alone, with one escape hatch: the algorithm may take at most
//! [`DEADLINE`]. If it takes that long, the result is discarded and replaced
//! by a coarse diff (every base line removed, every new line added), flagged
//! with [`Diff::coarse`]. That diff is always correct, only not minimal, and
//! it depends on `(base, new)` alone; only the choice between the minimal and
//! the coarse one depends on the speed of the machine.

use std::fmt;
use std::time::{Duration, Instant};

use std::borrow::Cow;

use similar::{Algorithm, ChangeTag, DiffTag, TextDiff};

use crate::core::ai::tools::content::is_hidden;

/// The most the diff algorithm may run.
pub const DEADLINE: Duration = Duration::from_millis(500);
/// Lines of context around a change.
pub const CONTEXT: usize = 3;
/// The most rows a diff carries. Totals stay exact past it.
pub const MAX_ROWS: usize = 2000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Context,
    Added,
    Removed,
}

/// One row of a hunk, without its line ending. Review-only: see the module
/// invariant; a row never goes into a tool result or to the model.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Line {
    pub kind: Kind,
    text: String,
    /// The line ends the file without a newline.
    pub no_newline: bool,
}

impl Line {
    /// The text exactly as it is on disk or was staged. RAW: it may hold
    /// escape sequences, C1 controls or bidi marks, so it is NOT for
    /// rendering; use [`Line::display_text`] for that.
    pub fn raw_text(&self) -> &str {
        &self.text
    }

    /// The text made safe to show: every control, C1, bidi and format
    /// character becomes a visible `<U+XXXX>`; a tab stays.
    pub fn display_text(&self) -> Cow<'_, str> {
        if !self.text.chars().any(unsafe_char) {
            return Cow::Borrowed(&self.text);
        }
        let mut out = String::with_capacity(self.text.len() + 16);
        for c in self.text.chars() {
            if unsafe_char(c) {
                out.push_str(&format!("<U+{:04X}>", c as u32));
            } else {
                out.push(c);
            }
        }
        Cow::Owned(out)
    }
}

/// Whether a character must not reach a terminal as it is: a control (C0,
/// DEL, C1 including CSI U+009B), a bidi or other invisible format character,
/// or a line or paragraph separator. A tab is shown as it is.
fn unsafe_char(c: char) -> bool {
    c != '\t' && (c.is_control() || is_hidden(c) || matches!(c, '\u{2028}' | '\u{2029}'))
}

/// Rows with their place in both texts. A start is 1-based; an empty side
/// starts at the line before, as in a unified diff.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Hunk {
    pub old_start: usize,
    pub old_len: usize,
    pub new_start: usize,
    pub new_len: usize,
    pub lines: Vec<Line>,
}

/// The diff of one file. Review-only: its rows never go into a tool result or
/// to the model.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Diff {
    pub hunks: Vec<Hunk>,
    pub added: usize,
    pub removed: usize,
    /// The deadline was hit: the diff is whole-file, not minimal.
    pub coarse: bool,
    /// Rows were left out to stay under [`MAX_ROWS`]; the totals are exact.
    pub truncated: bool,
}

/// The diff of a file: `base` is its text on disk, `None` for a create.
pub fn diff(base: Option<&str>, new: &str) -> Diff {
    diff_within(base, new, DEADLINE)
}

/// [`diff`] with the time limit given, so a test can hit it. The limit covers
/// the whole work, row building included. A create has no algorithm to run
/// and at most [`MAX_ROWS`] rows to build, so it is never late.
pub fn diff_within(base: Option<&str>, new: &str, limit: Duration) -> Diff {
    let base = base.unwrap_or("");
    let deadline = (!base.is_empty()).then(|| Instant::now() + limit);
    match fine(base, new, deadline) {
        Some(d) if !late(deadline) => d,
        _ => coarse(base, new),
    }
}

fn late(deadline: Option<Instant>) -> bool {
    deadline.is_some_and(|d| Instant::now() >= d)
}

/// `None` when the deadline passed before the diff was whole.
fn fine(base: &str, new: &str, deadline: Option<Instant>) -> Option<Diff> {
    let mut config = TextDiff::configure();
    config.algorithm(Algorithm::Myers);
    if let Some(d) = deadline {
        config.deadline(d);
    }
    let text = config.diff_lines(base, new);
    let mut out = Diff {
        hunks: Vec::new(),
        added: 0,
        removed: 0,
        coarse: false,
        truncated: false,
    };
    let mut rows = 0;
    for group in text.grouped_ops(CONTEXT) {
        if late(deadline) {
            return None;
        }
        let (Some(first), Some(last)) = (group.first(), group.last()) else {
            continue;
        };
        // Totals come from the ops, so they stay exact past the row cap.
        for op in &group {
            let (_, old, new) = op.as_tag_tuple();
            match op.tag() {
                DiffTag::Delete => out.removed += old.len(),
                DiffTag::Insert => out.added += new.len(),
                DiffTag::Replace => {
                    out.removed += old.len();
                    out.added += new.len();
                }
                DiffTag::Equal => {}
            }
        }
        if rows >= MAX_ROWS {
            out.truncated = true;
            continue;
        }
        let old = first.old_range().start..last.old_range().end;
        let new = first.new_range().start..last.new_range().end;
        let mut lines = Vec::new();
        'ops: for op in &group {
            for change in text.iter_changes(op) {
                if rows >= MAX_ROWS {
                    out.truncated = true;
                    break 'ops;
                }
                if late(deadline) {
                    return None;
                }
                let kind = match change.tag() {
                    ChangeTag::Equal => Kind::Context,
                    ChangeTag::Insert => Kind::Added,
                    ChangeTag::Delete => Kind::Removed,
                };
                lines.push(Line {
                    kind,
                    text: change.value().trim_end_matches('\n').to_owned(),
                    no_newline: change.missing_newline(),
                });
                rows += 1;
            }
        }
        out.hunks.push(hunk(old, new, lines));
    }
    Some(out)
}

/// Every base line removed, then every new line added, in one hunk. It
/// carries at most half of [`MAX_ROWS`] rows of each side; the totals count
/// every line.
fn coarse(base: &str, new: &str) -> Diff {
    let mut lines = Vec::new();
    let mut totals = [0usize; 2];
    for (side, (kind, text)) in [(Kind::Removed, base), (Kind::Added, new)]
        .into_iter()
        .enumerate()
    {
        for row in text.split_inclusive('\n') {
            totals[side] += 1;
            if totals[side] <= MAX_ROWS / 2 {
                lines.push(Line {
                    kind,
                    text: row.trim_end_matches('\n').to_owned(),
                    no_newline: !row.ends_with('\n'),
                });
            }
        }
    }
    let [removed, added] = totals;
    Diff {
        hunks: vec![hunk(0..removed, 0..added, lines)],
        added,
        removed,
        coarse: !base.is_empty(),
        truncated: removed.max(added) > MAX_ROWS / 2,
    }
}

fn hunk(old: std::ops::Range<usize>, new: std::ops::Range<usize>, lines: Vec<Line>) -> Hunk {
    let start = |r: &std::ops::Range<usize>| if r.is_empty() { r.start } else { r.start + 1 };
    Hunk {
        old_start: start(&old),
        old_len: old.len(),
        new_start: start(&new),
        new_len: new.len(),
        lines,
    }
}

impl fmt::Display for Diff {
    /// The unified text: a header per hunk, signed rows, and the
    /// `\ No newline at end of file` marker after a row that ends the file
    /// without one.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.coarse {
            writeln!(f, "# coarse diff: the whole file is shown as replaced")?;
        }
        if self.truncated {
            writeln!(f, "# truncated: rows were left out; the totals are exact")?;
        }
        for h in &self.hunks {
            writeln!(
                f,
                "@@ -{},{} +{},{} @@",
                h.old_start, h.old_len, h.new_start, h.new_len
            )?;
            for l in &h.lines {
                let sign = match l.kind {
                    Kind::Context => ' ',
                    Kind::Added => '+',
                    Kind::Removed => '-',
                };
                writeln!(f, "{sign}{}", l.display_text())?;
                if l.no_newline {
                    writeln!(f, "\\ No newline at end of file")?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn rows(d: &Diff) -> Vec<String> {
        d.hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .map(|l| {
                let sign = match l.kind {
                    Kind::Context => ' ',
                    Kind::Added => '+',
                    Kind::Removed => '-',
                };
                let eol = if l.no_newline { "|" } else { "" };
                format!("{sign}{}{eol}", l.raw_text())
            })
            .collect()
    }

    #[test]
    fn a_create_shows_every_line_as_added() {
        let d = diff(None, "a\nb\nc\n");
        assert_eq!(rows(&d), ["+a", "+b", "+c"]);
        assert_eq!((d.added, d.removed, d.hunks.len()), (3, 0, 1));
        assert!(!d.coarse);
    }

    #[test]
    fn a_create_of_nothing_has_no_hunks() {
        let d = diff(None, "");
        assert!(d.hunks.is_empty());
        assert_eq!((d.added, d.removed), (0, 0));
    }

    #[test]
    fn a_change_keeps_three_lines_of_context_each_side() {
        let base = "1\n2\n3\n4\n5\n6\n7\n8\n9\n";
        let new = "1\n2\n3\n4\nX\n6\n7\n8\n9\n";
        let d = diff(Some(base), new);
        assert_eq!(rows(&d), [" 2", " 3", " 4", "-5", "+X", " 6", " 7", " 8"]);
        let h = &d.hunks[0];
        assert_eq!((h.old_start, h.old_len), (2, 7));
        assert_eq!((h.new_start, h.new_len), (2, 7));
        assert_eq!((d.added, d.removed), (1, 1));
    }

    #[test]
    fn far_apart_changes_make_two_hunks() {
        let base: String = (1..=30).map(|i| format!("{i}\n")).collect();
        let new = base.replace("\n2\n", "\nB\n").replace("\n29\n", "\nY\n");
        let d = diff(Some(&base), &new);
        assert_eq!(d.hunks.len(), 2);
        assert_eq!((d.added, d.removed), (2, 2));
    }

    #[test]
    fn identical_texts_have_no_hunks() {
        let d = diff(Some("a\nb\n"), "a\nb\n");
        assert!(d.hunks.is_empty());
    }

    #[test]
    fn a_missing_final_newline_is_marked_on_the_line() {
        let d = diff(Some("a\nb"), "a\nb\n");
        assert_eq!(rows(&d), [" a", "-b|", "+b"]);
        let d = diff(Some("a\nb\n"), "a\nc");
        assert_eq!(rows(&d), [" a", "-b", "+c|"]);
        // A context line can lack the newline too.
        let d = diff(Some("x\nb"), "y\nb");
        assert_eq!(rows(&d), ["-x", "+y", " b|"]);
        let d = diff(None, "a");
        assert_eq!(rows(&d), ["+a|"]);
    }

    #[test]
    fn the_unified_text_carries_the_marker_line() {
        let d = diff(Some("a\n"), "b");
        assert_eq!(
            d.to_string(),
            "@@ -1,1 +1,1 @@\n-a\n+b\n\\ No newline at end of file\n"
        );
    }

    #[test]
    fn a_deadline_that_is_hit_gives_a_coarse_whole_replace() {
        let base = "a\nb\nc\nd\n";
        let new = "a\nB\nc\nd";
        let d = diff_within(Some(base), new, Duration::ZERO);
        assert!(d.coarse);
        assert_eq!(rows(&d), ["-a", "-b", "-c", "-d", "+a", "+B", "+c", "+d|"]);
        assert_eq!((d.added, d.removed), (4, 4));
        assert_eq!(d.hunks.len(), 1);
    }

    #[test]
    fn a_coarse_diff_of_a_create_equals_the_normal_one() {
        assert_eq!(
            diff_within(None, "a\nb\n", Duration::ZERO).hunks,
            diff(None, "a\nb\n").hunks
        );
    }

    #[test]
    fn the_diff_is_the_same_every_time_for_the_same_inputs() {
        let base: String = (0..400).map(|i| format!("line {i}\n")).collect();
        let new = base.replace("line 200", "changed").replace("line 7\n", "");
        assert_eq!(diff(Some(&base), &new), diff(Some(&base), &new));
    }

    #[test]
    fn a_real_size_diff_is_minimal_with_a_generous_limit() {
        // A limit of an hour: the test is about the result, never the speed,
        // so a slow machine cannot make it flaky.
        let base: String = (0..20_000).map(|i| format!("line {i}\n")).collect();
        let new = base.replace("line 10000\n", "other\n");
        let d = diff_within(Some(&base), &new, Duration::from_secs(3600));
        assert!(!d.coarse && !d.truncated);
        assert_eq!((d.added, d.removed), (1, 1));
    }

    fn row_count(d: &Diff) -> usize {
        d.hunks.iter().map(|h| h.lines.len()).sum()
    }

    #[test]
    fn a_coarse_diff_of_a_big_input_carries_few_rows_and_exact_totals() {
        let base: String = (0..50_000).map(|i| format!("old {i}\n")).collect();
        let new: String = (0..30_000).map(|i| format!("new {i}\n")).collect();
        let d = diff_within(Some(&base), &new, Duration::ZERO);
        assert!(d.coarse && d.truncated);
        assert!(row_count(&d) <= MAX_ROWS);
        assert_eq!((d.removed, d.added), (50_000, 30_000));
        let h = &d.hunks[0];
        assert_eq!((h.old_len, h.new_len), (50_000, 30_000));
    }

    #[test]
    fn a_huge_create_is_capped_with_exact_totals() {
        let new: String = (0..40_000).map(|i| format!("n {i}\n")).collect();
        let d = diff_within(None, &new, Duration::ZERO);
        assert!(!d.coarse && d.truncated);
        assert!(row_count(&d) <= MAX_ROWS);
        assert_eq!((d.added, d.removed), (40_000, 0));
    }

    #[test]
    fn a_huge_fast_diff_is_capped_too() {
        let base: String = (0..3_000).map(|i| format!("keep {i}\n")).collect();
        let top: String = (0..4_000).map(|i| format!("top {i}\n")).collect();
        let new = format!("{top}{base}");
        let d = diff_within(Some(&base), &new, Duration::from_secs(3600));
        assert!(!d.coarse && d.truncated);
        assert!(row_count(&d) <= MAX_ROWS);
        assert_eq!((d.added, d.removed), (4_000, 0));
    }

    #[test]
    fn a_small_diff_is_not_truncated() {
        assert!(!diff(Some("a\n"), "b\n").truncated);
    }

    #[test]
    fn display_escapes_control_c1_and_bidi_in_context_and_removed_rows() {
        let base = "ctx \u{1b}[31m\nold \u{9b}2J\ntail \u{202e}x\ntab\there\n";
        let new = "ctx \u{1b}[31m\nnew\ntail \u{202e}x\ntab\there\n";
        let out = diff(Some(base), new).to_string();
        assert!(out.contains(" ctx <U+001B>[31m\n"), "{out}");
        assert!(out.contains("-old <U+009B>2J\n"), "{out}");
        assert!(out.contains(" tail <U+202E>x\n"), "{out}");
        assert!(out.contains(" tab\there\n"), "{out}");
        assert!(!out.contains('\u{1b}') && !out.contains('\u{9b}') && !out.contains('\u{202e}'));
    }

    #[test]
    fn the_raw_text_stays_disk_exact() {
        let d = diff(Some("a\u{1b}\n"), "b\n");
        let removed = d.hunks[0].lines.iter().find(|l| l.kind == Kind::Removed);
        assert_eq!(removed.unwrap().raw_text(), "a\u{1b}");
        assert_eq!(removed.unwrap().display_text(), "a<U+001B>");
    }

    #[test]
    fn display_says_so_when_the_diff_is_coarse_or_truncated() {
        let d = diff_within(Some("a\n"), "b\n", Duration::ZERO);
        assert!(d.to_string().starts_with("# coarse diff"), "{d}");
        let new: String = (0..MAX_ROWS + 10).map(|i| format!("{i}\n")).collect();
        let d = diff(None, &new);
        assert!(d.to_string().contains("# truncated"), "{d}");
        assert!(!diff(Some("a\n"), "b\n").to_string().contains('#'));
    }
}
