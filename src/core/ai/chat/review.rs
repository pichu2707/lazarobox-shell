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

use crate::core::ai::chat::wrap::expand_tabs;
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
const WARN_TRUNCATED: &str = "warning: truncated: rows were left out; the totals are exact";

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
        assert!(w[0].contains("truncated"), "{}", w[0]);
        assert_eq!(doc.added(), 2100);
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
}
