//! The line diff a batch review shows. Pure: no IO, no clock reads other than
//! the deadline guard below.
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

use similar::{Algorithm, ChangeTag, TextDiff};

/// The most the diff algorithm may run.
pub const DEADLINE: Duration = Duration::from_millis(500);
/// Lines of context around a change.
pub const CONTEXT: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Context,
    Added,
    Removed,
}

/// One row of a hunk, without its line ending.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Line {
    pub kind: Kind,
    pub text: String,
    /// The line ends the file without a newline.
    pub no_newline: bool,
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

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Diff {
    pub hunks: Vec<Hunk>,
    pub added: usize,
    pub removed: usize,
    /// The deadline was hit: the diff is whole-file, not minimal.
    pub coarse: bool,
}

/// The diff of a file: `base` is its text on disk, `None` for a create.
pub fn diff(base: Option<&str>, new: &str) -> Diff {
    diff_within(base, new, DEADLINE)
}

/// [`diff`] with the time limit given, so a test can hit it.
pub fn diff_within(base: Option<&str>, new: &str, limit: Duration) -> Diff {
    let base = base.unwrap_or("");
    let start = Instant::now();
    let fine = fine(base, new, start + limit);
    if start.elapsed() >= limit && !base.is_empty() {
        return coarse(base, new);
    }
    fine
}

fn fine(base: &str, new: &str, deadline: Instant) -> Diff {
    let text = TextDiff::configure()
        .algorithm(Algorithm::Myers)
        .deadline(deadline)
        .diff_lines(base, new);
    let mut out = Diff {
        hunks: Vec::new(),
        added: 0,
        removed: 0,
        coarse: false,
    };
    for group in text.grouped_ops(CONTEXT) {
        let (Some(first), Some(last)) = (group.first(), group.last()) else {
            continue;
        };
        let old = first.old_range().start..last.old_range().end;
        let new = first.new_range().start..last.new_range().end;
        let mut lines = Vec::new();
        for op in &group {
            for change in text.iter_changes(op) {
                let kind = match change.tag() {
                    ChangeTag::Equal => Kind::Context,
                    ChangeTag::Insert => Kind::Added,
                    ChangeTag::Delete => Kind::Removed,
                };
                match kind {
                    Kind::Added => out.added += 1,
                    Kind::Removed => out.removed += 1,
                    Kind::Context => {}
                }
                lines.push(Line {
                    kind,
                    text: change.value().trim_end_matches('\n').to_owned(),
                    no_newline: change.missing_newline(),
                });
            }
        }
        out.hunks.push(hunk(old, new, lines));
    }
    out
}

/// Every base line removed, then every new line added, in one hunk.
fn coarse(base: &str, new: &str) -> Diff {
    let mut lines = Vec::new();
    for (kind, text) in [(Kind::Removed, base), (Kind::Added, new)] {
        for row in text.split_inclusive('\n') {
            lines.push(Line {
                kind,
                text: row.trim_end_matches('\n').to_owned(),
                no_newline: !row.ends_with('\n'),
            });
        }
    }
    let count = |k| lines.iter().filter(|l| l.kind == k).count();
    let (removed, added) = (count(Kind::Removed), count(Kind::Added));
    Diff {
        hunks: vec![hunk(0..removed, 0..added, lines)],
        added,
        removed,
        coarse: true,
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
                writeln!(f, "{sign}{}", l.text)?;
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
                format!("{sign}{}{eol}", l.text)
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
    fn a_real_size_diff_stays_well_inside_the_deadline() {
        let base: String = (0..20_000).map(|i| format!("line {i}\n")).collect();
        let new = base.replace("line 10000\n", "other\n");
        let d = diff(Some(&base), &new);
        assert!(!d.coarse);
        assert_eq!((d.added, d.removed), (1, 1));
    }
}
