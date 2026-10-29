//! The review of a commit: the message, the signing notice and the whole
//! staged diff as the lines of a [`ReviewDoc`], so the view and the coverage
//! gate are those of a batch. Same invariant as the doc: the rows are for the
//! user only; the model is told counts.

use std::fmt;
use std::sync::{Arc, atomic::Ordering};

use super::{BatchReview, DocLine, LineKind, NEXT_DOC, ReviewDoc, push};
use crate::core::ai::tools::diff::escape;

/// Said in the review: the commit is made unsigned, whatever the repo asks.
pub const SIGNING_NOTICE: &str = "signing disabled for this commit";

/// The commit a [`ApprovalAsk::Commit`](crate::core::ai::chat::approval::ApprovalAsk)
/// asks the user about: its review and the exact staged diff it shows, so the
/// host can refuse a commit when the index changed after the question.
#[derive(Clone, PartialEq, Eq)]
pub struct CommitReview {
    review: Arc<BatchReview>,
    staged: Vec<u8>,
}

impl fmt::Debug for CommitReview {
    /// Counts only: the message and the diff never reach a log.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommitReview")
            .field("review", &self.review)
            .field("staged_bytes", &self.staged.len())
            .finish()
    }
}

impl CommitReview {
    /// `hooks` is the notice about a `core.hooksPath` set elsewhere; `running`
    /// names the hooks the commit will run (none: the review says so).
    pub fn new(message: &str, hooks: Option<&str>, running: &[&str], diff: &[u8]) -> Self {
        let mut doc = ReviewDoc {
            id: NEXT_DOC.fetch_add(1, Ordering::Relaxed),
            lines: Vec::new(),
            files: 0,
            added: 0,
            removed: 0,
            truncated: false,
        };
        let mut body = Vec::new();
        push(&mut body, LineKind::Warning, SIGNING_NOTICE.to_owned());
        if let Some(hooks) = hooks {
            push(&mut body, LineKind::Warning, escape(hooks).into_owned());
        }
        if running.is_empty() {
            push(
                &mut body,
                LineKind::Note,
                "no commit hooks found".to_owned(),
            );
        } else {
            let line = format!("hooks that will run: {}", running.join(", "));
            push(&mut body, LineKind::Warning, line);
        }
        push(&mut body, LineKind::File, "commit message".to_owned());
        for line in message.lines() {
            push(&mut body, LineKind::Context, format!(" {}", escape(line)));
        }
        doc.add_git_diff(&mut body, &String::from_utf8_lossy(diff));
        let s = if doc.files == 1 { "" } else { "s" };
        let summary = format!("{} file{s}, +{} -{}", doc.files, doc.added, doc.removed);
        push(&mut doc.lines, LineKind::Summary, summary);
        doc.lines.append(&mut body);
        Self {
            review: Arc::new(BatchReview::new(doc)),
            staged: diff.to_vec(),
        }
    }

    /// The review the view draws.
    pub fn review(&self) -> &Arc<BatchReview> {
        &self.review
    }

    /// Whether `diff` is the one the user read.
    pub fn same_staged(&self, diff: &[u8]) -> bool {
        self.staged == diff
    }
}

impl ReviewDoc {
    /// Adds the rows of a `git diff` text. A header line is a note; inside a
    /// hunk a leading `+` or `-` is content, not a header.
    fn add_git_diff(&mut self, out: &mut Vec<DocLine>, diff: &str) {
        let mut in_hunk = false;
        for line in diff.lines() {
            let kind = if line.starts_with("diff --git ") {
                in_hunk = false;
                self.files += 1;
                LineKind::File
            } else if line.starts_with("@@") {
                in_hunk = true;
                LineKind::Hunk
            } else if in_hunk && line.starts_with('+') {
                self.added += 1;
                LineKind::Added
            } else if in_hunk && line.starts_with('-') {
                self.removed += 1;
                LineKind::Removed
            } else if in_hunk && line.starts_with(' ') {
                LineKind::Context
            } else {
                LineKind::Note
            };
            push(out, kind, escape(line).into_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIFF: &str = "diff --git a/src/a.rs b/src/a.rs\nindex 111..222 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,2 +1,2 @@\n keep\n-old\n+new\n";

    fn rows(review: &CommitReview) -> Vec<(LineKind, String)> {
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

    // Spec (git-tools, commit: Review shows message and diff).
    #[test]
    fn the_review_holds_the_message_the_signing_notice_and_the_diff() {
        let review = CommitReview::new("fix: a\n\nbody line", None, &[], DIFF.as_bytes());
        let rows = rows(&review);
        assert_eq!(rows[0], (LineKind::Summary, "1 file, +1 -1".to_owned()));
        assert!(has(
            &rows,
            LineKind::Warning,
            "signing disabled for this commit"
        ));
        assert!(has(&rows, LineKind::Context, " fix: a"));
        assert!(has(&rows, LineKind::Context, " body line"));
        assert!(has(&rows, LineKind::File, "diff --git a/src/a.rs"));
        assert!(has(&rows, LineKind::Hunk, "@@ -1,2 +1,2 @@"));
        assert!(has(&rows, LineKind::Added, "+new"));
        assert!(has(&rows, LineKind::Removed, "-old"));
        // The header lines `--- ` and `+++ ` are not content rows.
        assert!(has(&rows, LineKind::Note, "+++ b/src/a.rs"));
        assert!(!has(&rows, LineKind::Added, "+++"));
        assert_eq!(review.review().counts(), "1 file, +1 -1");
        assert!(review.review().doc().reviewable());
    }

    // The review says which hooks will run, or that none will.
    #[test]
    fn the_review_lists_the_hooks_that_will_run_or_says_none_will() {
        let none = CommitReview::new("m", None, &[], DIFF.as_bytes());
        assert!(has(&rows(&none), LineKind::Note, "no commit hooks found"));
        let some = CommitReview::new("m", None, &["pre-commit", "commit-msg"], DIFF.as_bytes());
        let rows = rows(&some);
        assert!(has(
            &rows,
            LineKind::Warning,
            "hooks that will run: pre-commit, commit-msg"
        ));
        assert!(!has(&rows, LineKind::Note, "no commit hooks found"));
    }

    #[test]
    fn a_hooks_notice_is_a_warning_row() {
        let review = CommitReview::new(
            "m",
            Some("this repo sets core.hooksPath to x"),
            &[],
            DIFF.as_bytes(),
        );
        assert!(has(&rows(&review), LineKind::Warning, "core.hooksPath"));
    }

    // The rows come from `escape`: no control or bidi character is drawn.
    #[test]
    fn control_and_bidi_characters_in_the_message_and_the_diff_are_escaped() {
        let diff = "diff --git a/x b/x\n@@ -0,0 +1 @@\n+a\u{1b}[2Jb\u{202e}c\n";
        let review = CommitReview::new("m\u{1b}[31m", None, &[], diff.as_bytes());
        for (_, text) in rows(&review) {
            assert!(
                !text.chars().any(|c| c.is_control() || c == '\u{202e}'),
                "{text:?}"
            );
        }
        assert!(has(&rows(&review), LineKind::Added, "<U+001B>"));
        assert!(has(&rows(&review), LineKind::Context, "<U+001B>"));
    }

    #[test]
    fn a_review_with_no_diff_cannot_be_approved() {
        let review = CommitReview::new("m", None, &[], b"");
        assert!(!review.review().doc().reviewable());
    }

    #[test]
    fn the_commit_compares_the_staged_diff_byte_for_byte() {
        let review = CommitReview::new("m", None, &[], DIFF.as_bytes());
        assert!(review.same_staged(DIFF.as_bytes()));
        assert!(!review.same_staged(DIFF.replace("new", "newer").as_bytes()));
    }

    #[test]
    fn debug_prints_counts_and_never_the_text() {
        let review = CommitReview::new("secret message", None, &[], DIFF.as_bytes());
        let text = format!("{review:?}");
        assert!(
            !text.contains("secret message") && !text.contains("old"),
            "{text}"
        );
    }
}
