//! The change ledger of a `code` worker (S7): the files it staged, each with
//! its size and a hash of its diff, and whether the user's batch review
//! applied it. Pure: the host reports what it staged and what was applied.
//!
//! A worker's writes are staged in its pane's batch and reach the disk only
//! when the user approves that batch: the ledger only records, it never
//! approves anything.

use std::fmt::Write as _;

/// One staged path, as the orchestrator is told about it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Change {
    /// The path relative to the chat's root, one sanitized line.
    pub path: String,
    /// The bytes of the staged text; `None` for a folder.
    pub size: Option<u64>,
    /// [`diff_hash`] of the staged change.
    pub hash: String,
    /// The user's batch review applied it to the disk.
    pub applied: bool,
}

/// The files a worker staged, in the order it first staged them.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ChangeLedger(Vec<Change>);

impl ChangeLedger {
    pub fn changes(&self) -> &[Change] {
        &self.0
    }

    /// Records a staged change. Staging the same path again replaces its
    /// size and hash in place, and it is not applied until a review applies
    /// it again.
    pub fn staged(&mut self, change: Change) {
        let change = Change {
            applied: false,
            ..change
        };
        match self.0.iter_mut().find(|known| known.path == change.path) {
            Some(known) => *known = change,
            None => self.0.push(change),
        }
    }

    /// The user's review applied a batch holding `paths`: those of them this
    /// worker staged are now on disk.
    pub fn applied(&mut self, paths: &[String]) {
        for change in &mut self.0 {
            if paths.contains(&change.path) {
                change.applied = true;
            }
        }
    }

    /// The `files changed` block of the worker's result, `None` when it
    /// staged nothing: one line per path with its size, its diff hash and
    /// whether it was applied.
    pub fn render(&self) -> Option<String> {
        if self.0.is_empty() {
            return None;
        }
        let mut out = String::from("files changed:");
        for change in &self.0 {
            let size = match change.size {
                Some(bytes) => format!("{bytes} bytes"),
                None => "folder".to_owned(),
            };
            let state = if change.applied {
                "applied"
            } else {
                "not applied"
            };
            let _ = write!(
                out,
                "\n- {} ({size}, diff {}): {state}",
                change.path, change.hash
            );
        }
        Some(out)
    }
}

/// A short fingerprint of a staged change: FNV-1a 64 over the text it
/// replaces (none for a create) and its new text, as 16 hex digits. It tells
/// two versions of a change apart; it is no security boundary.
pub fn diff_hash(base: Option<&str>, text: &str) -> String {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    let mut eat = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(PRIME);
        }
    };
    match base {
        None => eat(b"new\0"),
        Some(base) => {
            // Length-prefixed, so no base and text pair reads as another.
            eat(b"old\0");
            eat(&(base.len() as u64).to_le_bytes());
            eat(base.as_bytes());
        }
    }
    eat(text.as_bytes());
    format!("{hash:016x}")
}

/// One staged file as a review reads it: the text it replaces (none for a
/// create) and the text it becomes.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StagedText {
    pub path: String,
    pub base: Option<String>,
    pub text: String,
}

/// The names of skipped paths a diff lists before it says `+N more`.
const SKIPPED_SHOWN: usize = 10;
/// Room kept for the note that the diff was cut.
const CUT_NOTE: usize = 160;

/// What a review may be shown of a staged batch: the files it may read, the
/// paths it may not (or that have no text, like a folder), and whether the
/// batch could be read at all.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct StagedDiff {
    pub files: Vec<StagedText>,
    /// Staged files a review may not read: out of its scope or guarded by the
    /// secret rules.
    pub skipped: Vec<String>,
    /// Staged folders: there is no text to read, and nothing is missing.
    pub dirs: Vec<String>,
    /// The batch could not be read.
    pub unavailable: bool,
}

/// A staged diff rendered within a budget.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Rendered {
    pub text: String,
    /// Why the review did not see the whole change; empty when it did. The
    /// texts are constants, so the same cause is one gap.
    pub gaps: Vec<String>,
    /// Bytes of the diff left out, for the receipt.
    pub cut_bytes: usize,
}

/// The gap of a diff that did not fit the prompt.
pub const GAP_CUT: &str = "diff cut to fit the prompt";
/// The gap of a diff that could not be read.
pub const GAP_UNAVAILABLE: &str = "diff unavailable";
/// The gap of files a review may not read.
pub const GAP_UNREADABLE: &str = "files not readable by a review";
/// The gap of a batch that does not match the change set.
pub const GAP_MISMATCH: &str = "staged batch does not match the change set";

impl StagedDiff {
    /// How many staged entries this holds, whatever became of each.
    pub fn entries(&self) -> usize {
        self.files.len() + self.skipped.len() + self.dirs.len()
    }

    pub fn unavailable() -> Self {
        Self {
            unavailable: true,
            ..Self::default()
        }
    }

    /// The unified diff within `budget` bytes. The notes (skipped paths, a
    /// cut) come before the body, so a cut never hides them. A cut is always
    /// said, with how much is missing, and listed in `gaps`.
    pub fn render(&self, budget: usize) -> Rendered {
        use crate::core::ai::tools::diff::diff;
        if self.unavailable {
            return Rendered {
                text: "staged text: not included (it could not be read); read it with your read tools\n"
                    .to_owned(),
                gaps: vec![GAP_UNAVAILABLE.to_owned()],
                cut_bytes: 0,
            };
        }
        let mut gaps = Vec::new();
        let mut notes = String::new();
        if !self.skipped.is_empty() {
            let shown: Vec<&str> = self
                .skipped
                .iter()
                .take(SKIPPED_SHOWN)
                .map(String::as_str)
                .collect();
            notes.push_str(&format!(
                "not shown (nothing a review can read): {}",
                shown.join(", ")
            ));
            if self.skipped.len() > SKIPPED_SHOWN {
                notes.push_str(&format!(", +{} more", self.skipped.len() - SKIPPED_SHOWN));
            }
            notes.push('\n');
            gaps.push(GAP_UNREADABLE.to_owned());
        }
        if !self.dirs.is_empty() {
            let shown: Vec<&str> = self
                .dirs
                .iter()
                .take(SKIPPED_SHOWN)
                .map(String::as_str)
                .collect();
            notes.push_str(&format!("created dir: {}", shown.join(", ")));
            if self.dirs.len() > SKIPPED_SHOWN {
                notes.push_str(&format!(", +{} more", self.dirs.len() - SKIPPED_SHOWN));
            }
            notes.push('\n');
        }
        let room = budget.saturating_sub(notes.len() + CUT_NOTE);
        let mut body = String::new();
        let mut cut = None;
        let mut cut_bytes = 0;
        for (at, file) in self.files.iter().enumerate() {
            let mut block = format!("diff of {}\n", file.path);
            block.push_str(&diff(file.base.as_deref(), &file.text).to_string());
            if !block.ends_with('\n') {
                block.push('\n');
            }
            let left = room.saturating_sub(body.len());
            if block.len() <= left {
                body.push_str(&block);
                continue;
            }
            let mut end = left;
            while !block.is_char_boundary(end) {
                end -= 1;
            }
            body.push_str(&block[..end]);
            let more: usize = self.files[at + 1..].len();
            cut = Some((block.len() - end, more, file.path.clone()));
            break;
        }
        if let Some((bytes, more, path)) = cut {
            notes.push_str(&format!(
                "[diff cut: {bytes} byte(s) of {path} and {more} more file(s) are not shown]\n"
            ));
            gaps.push(GAP_CUT.to_owned());
            cut_bytes = bytes;
        }
        Rendered {
            text: format!("{notes}{body}"),
            gaps,
            cut_bytes,
        }
    }
}

#[cfg(test)]
mod diff_tests {
    use super::*;

    fn staged(path: &str, base: Option<&str>, text: &str) -> StagedText {
        StagedText {
            path: path.to_owned(),
            base: base.map(str::to_owned),
            text: text.to_owned(),
        }
    }

    fn of(files: Vec<StagedText>, skipped: &[&str]) -> StagedDiff {
        StagedDiff {
            files,
            skipped: skipped.iter().map(|s| (*s).to_owned()).collect(),
            ..StagedDiff::default()
        }
    }

    #[test]
    fn the_diff_holds_the_added_and_removed_lines_and_is_whole() {
        let files = vec![staged(
            "Makefile",
            Some("all:\n\techo build\n"),
            "all:\n\techo lint\n",
        )];
        let out = of(files, &[]).render(8192);
        assert!(out.text.starts_with("diff of Makefile\n"), "{}", out.text);
        assert!(out.text.contains("-\techo build"), "{}", out.text);
        assert!(out.text.contains("+\techo lint"), "{}", out.text);
        assert!(out.gaps.is_empty());
    }

    #[test]
    fn a_create_shows_every_line_as_added() {
        let out = of(vec![staged("a.txt", None, "one\ntwo\n")], &[]).render(8192);
        assert!(out.text.contains("+one") && out.text.contains("+two"));
    }

    #[test]
    fn a_cut_diff_says_so_before_the_body_and_is_a_gap() {
        let big = "x".repeat(40 * 1024);
        let files = vec![staged("big.txt", None, &big), staged("b.txt", None, "b\n")];
        let out = of(files, &[]).render(24 * 1024);
        assert!(out.text.len() <= 24 * 1024, "{}", out.text.len());
        let note = out.text.find("[diff cut:").expect("the cut is said");
        assert!(note < out.text.find("diff of big.txt").unwrap());
        assert!(out.text.contains("1 more file(s)"));
        assert_eq!(out.gaps, [GAP_CUT]);
        assert!(out.cut_bytes > 0);
    }

    #[test]
    fn a_skipped_path_is_named_before_the_body_and_is_a_gap() {
        let out = of(vec![staged("a.txt", None, "a\n")], &[".env"]).render(8192);
        let note = out.text.find("not shown").unwrap();
        assert!(note < out.text.find("diff of a.txt").unwrap());
        assert!(out.text.contains(".env"));
        assert_eq!(out.gaps, [GAP_UNREADABLE]);
    }

    #[test]
    fn the_skipped_list_is_bounded() {
        let names: Vec<String> = (0..150).map(|n| format!("dir{n}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let out = of(Vec::new(), &refs).render(8192);
        assert!(out.text.contains("+140 more"), "{}", out.text);
        assert!(out.text.len() < 400, "{}", out.text.len());
    }

    #[test]
    fn an_unavailable_diff_says_so_and_is_a_gap() {
        let out = StagedDiff::unavailable().render(8192);
        assert!(out.text.contains("not included"));
        assert_eq!(out.gaps, [GAP_UNAVAILABLE]);
    }

    // A staged folder has no text: it is listed and is no gap.
    #[test]
    fn a_staged_folder_is_listed_and_is_not_a_gap() {
        let out = StagedDiff {
            dirs: vec!["src/new".to_owned()],
            ..StagedDiff::default()
        }
        .render(8192);
        assert!(out.text.contains("created dir: src/new"), "{}", out.text);
        assert!(out.gaps.is_empty());
    }
}
