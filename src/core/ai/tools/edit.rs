//! `edit_file`: replace one exact piece of text in a file. Pure: no IO.
//!
//! The edit applies to the text staged for the path, or else to the text the
//! caller read from disk. `check_new` runs on the `new` text only (design
//! D23): a file that already holds an odd character stays editable, while a
//! model cannot add one. `check_existing` guards the disk text.

use std::fmt;

use crate::core::ai::tools::content::{ContentError, check_existing, check_new};
use crate::core::ai::tools::paths::WriteTarget;
use crate::core::ai::tools::stage::{Batch, Entry, Existing, StageError};

/// The largest `old` or `new` text of one edit, in bytes.
pub const FIELD_MAX: usize = 16 * 1024;
/// The largest file an edit may start from, in bytes.
pub const FILE_MAX: usize = 1024 * 1024;

/// Why an edit was refused. Each text goes back to the model.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditError {
    EmptyOld,
    /// `old` is over [`FIELD_MAX`].
    OldTooLarge,
    /// `new` is over [`FIELD_MAX`].
    NewTooLarge,
    /// The existing file holds CR: edit_file does not handle CRLF.
    FileHasCrlf,
    /// The existing file is not UTF-8 text without NUL.
    FileNotText,
    NoMatch,
    Ambiguous,
    FileTooLarge,
    /// The path is neither staged nor on disk.
    Missing,
    /// The path is staged as a folder.
    IsFolder,
    /// The disk read is not the whole file as measured.
    Unreadable,
    Content(ContentError),
    Stage(StageError),
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            EditError::EmptyOld => "old must not be empty",
            EditError::OldTooLarge => "old is over 16 KiB",
            EditError::NewTooLarge => "new is over 16 KiB",
            EditError::FileHasCrlf => {
                "the file uses CRLF line endings, which edit_file does not support"
            }
            EditError::FileNotText => "the file is not UTF-8 text",
            EditError::NoMatch => "old does not occur in the file",
            EditError::Ambiguous => {
                "old occurs more than once: add context so that it occurs exactly once"
            }
            EditError::FileTooLarge => "the file is over 1 MiB and cannot be edited",
            EditError::Missing => "the file does not exist: use write_file to create it",
            EditError::IsFolder => "the path is staged as a folder",
            EditError::Unreadable => "the file could not be read in full: try again",
            EditError::Content(why) => return why.fmt(f),
            EditError::Stage(why) => return why.fmt(f),
        })
    }
}

fn check_fields(old: &str, new: &str) -> Result<(), EditError> {
    if old.len() > FIELD_MAX {
        return Err(EditError::OldTooLarge);
    }
    if new.len() > FIELD_MAX {
        return Err(EditError::NewTooLarge);
    }
    Ok(())
}

/// Replaces the one occurrence of `old` in `current` by `new`. Occurrences
/// may overlap: `aa` in `aaa` is two.
pub fn apply_edit(current: &str, old: &str, new: &str) -> Result<String, EditError> {
    if old.is_empty() {
        return Err(EditError::EmptyOld);
    }
    check_fields(old, new)?;
    let first = current.find(old).ok_or(EditError::NoMatch)?;
    // `old` is not empty, so its first char exists: a second match, even an
    // overlapping one, starts after that char.
    let step = first + old.chars().next().map_or(1, char::len_utf8);
    if current[step..].contains(old) {
        return Err(EditError::Ambiguous);
    }
    let mut out = String::with_capacity(current.len() - old.len() + new.len());
    out.push_str(&current[..first]);
    out.push_str(new);
    out.push_str(&current[first + old.len()..]);
    Ok(out)
}

impl Batch {
    /// Stages an edit of one file. The text edited is the staged one when the
    /// path is staged (its `base` stays), otherwise the disk text in
    /// `existing`, which becomes the `base` so the review can diff against it
    /// and apply can tell when the file drifted. Nothing touches the disk.
    ///
    /// INVARIANT: the `base` is the user's own file text and may hold
    /// secrets. It, and every diff row made from it, is shown ONLY to the
    /// user in the review view, and NEVER goes into a tool result or
    /// anything sent to the model.
    pub fn edit_file(
        &mut self,
        target: &WriteTarget,
        old: &str,
        new: &str,
        existing: Existing<'_>,
    ) -> Result<(), EditError> {
        self.open().map_err(EditError::Stage)?;
        if old.is_empty() {
            return Err(EditError::EmptyOld);
        }
        check_fields(old, new)?;
        check_new(new).map_err(EditError::Content)?;
        let (text, base) = match self.staged(target) {
            Some(Entry::Dir) => return Err(EditError::IsFolder),
            Some(Entry::File { text, base }) => (apply_edit(text, old, new)?, base.clone()),
            None => match existing {
                Existing::Absent => return Err(EditError::Missing),
                // The 1 MiB rule is on the file being edited. The result may
                // grow past it, bounded by the batch byte cap in `put`.
                Existing::Present { size, .. } if size > FILE_MAX as u64 => {
                    return Err(EditError::FileTooLarge);
                }
                Existing::Present { size, bytes } if bytes.len() as u64 != size => {
                    return Err(EditError::Unreadable);
                }
                Existing::Present { bytes, .. } => {
                    let disk = check_existing(bytes).map_err(|why| match why {
                        ContentError::CarriageReturn => EditError::FileHasCrlf,
                        ContentError::NotText => EditError::FileNotText,
                        other => EditError::Content(other),
                    })?;
                    (apply_edit(disk, old, new)?, Some(disk.to_owned()))
                }
            },
        };
        self.put(target, Entry::File { text, base })
            .map_err(EditError::Stage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ai::tools::content::ContentError;
    use crate::core::ai::tools::paths::WriteTarget;
    use crate::core::ai::tools::stage::{
        Batch, BatchId, Entry, Existing, StageError, StageKey, State,
    };
    use crate::core::layout::PaneId;
    use std::path::PathBuf;

    fn batch() -> Batch {
        Batch::new(BatchId {
            key: StageKey::Pane(PaneId::for_test(3)),
            generation: 1,
        })
    }

    fn target(path: &str) -> WriteTarget {
        WriteTarget {
            real: PathBuf::from("/home/u/proj").join(path),
            build_file: false,
        }
    }

    fn disk(text: &str) -> Existing<'_> {
        Existing::Present {
            size: text.len() as u64,
            bytes: text.as_bytes(),
        }
    }

    fn file(b: &Batch, i: usize) -> (&str, Option<&str>) {
        match &b.entries()[i].entry {
            Entry::File { text, base } => (text, base.as_deref()),
            Entry::Dir => panic!("a folder"),
        }
    }

    #[test]
    fn a_unique_match_is_replaced_and_the_disk_text_is_the_base() {
        let mut b = batch();
        b.edit_file(&target("a.txt"), "bar", "baz", disk("foo bar"))
            .unwrap();
        assert_eq!(file(&b, 0), ("foo baz", Some("foo bar")));
    }

    #[test]
    fn no_match_and_many_matches_stage_nothing() {
        let mut b = batch();
        let t = target("a.txt");
        assert_eq!(
            b.edit_file(&t, "y", "z", disk("x x")),
            Err(EditError::NoMatch)
        );
        assert_eq!(
            b.edit_file(&t, "x", "z", disk("x x")),
            Err(EditError::Ambiguous)
        );
        assert!(b.entries().is_empty());
    }

    #[test]
    fn overlapping_matches_count_as_many() {
        let mut b = batch();
        assert_eq!(
            b.edit_file(&target("a"), "aa", "b", disk("aaa")),
            Err(EditError::Ambiguous)
        );
    }

    #[test]
    fn an_empty_old_is_refused() {
        let mut b = batch();
        assert_eq!(
            b.edit_file(&target("a"), "", "b", disk("abc")),
            Err(EditError::EmptyOld)
        );
    }

    #[test]
    fn a_second_edit_matches_the_staged_text_and_keeps_the_disk_base() {
        let mut b = batch();
        let t = target("a.txt");
        b.edit_file(&t, "bar", "baz", disk("foo bar")).unwrap();
        // The disk text passed now is ignored: the staged text wins.
        b.edit_file(&t, "baz", "qux", disk("something else"))
            .unwrap();
        assert_eq!(b.entries().len(), 1);
        assert_eq!(file(&b, 0), ("foo qux", Some("foo bar")));
        // The old text of the disk no longer matches the staged version.
        assert_eq!(
            b.edit_file(&t, "bar", "x", disk("foo bar")),
            Err(EditError::NoMatch)
        );
    }

    #[test]
    fn an_edit_of_a_staged_create_has_no_base() {
        let mut b = batch();
        let t = target("new.txt");
        b.stage_file(&t, "hello world\n", Existing::Absent).unwrap();
        b.edit_file(&t, "world", "there", Existing::Absent).unwrap();
        assert_eq!(file(&b, 0), ("hello there\n", None));
    }

    #[test]
    fn a_missing_file_that_is_not_staged_cannot_be_edited() {
        let mut b = batch();
        assert_eq!(
            b.edit_file(&target("a"), "x", "y", Existing::Absent),
            Err(EditError::Missing)
        );
    }

    #[test]
    fn a_staged_folder_cannot_be_edited() {
        let mut b = batch();
        let t = target("d");
        b.stage_dir(&t).unwrap();
        assert_eq!(
            b.edit_file(&t, "x", "y", Existing::Absent),
            Err(EditError::IsFolder)
        );
    }

    #[test]
    fn each_field_is_capped_at_16_kib() {
        let mut b = batch();
        let t = target("a.txt");
        let text = format!("start {}", "z".repeat(10));
        let at = "a".repeat(FIELD_MAX);
        let over = "a".repeat(FIELD_MAX + 1);
        let on_disk = format!("{at} tail");
        b.edit_file(&target("at.txt"), &at, "x", disk(&on_disk))
            .unwrap();
        assert_eq!(
            b.edit_file(&t, &over, "x", disk(&text)),
            Err(EditError::OldTooLarge)
        );
        assert_eq!(
            b.edit_file(&t, "start", &over, disk(&text)),
            Err(EditError::NewTooLarge)
        );
        b.edit_file(&t, "start", &at, disk(&text)).unwrap();
    }

    #[test]
    fn the_file_is_capped_at_1_mib() {
        let mut b = batch();
        let t = target("big.txt");
        let at = format!("needle{}", "a".repeat(FILE_MAX - 6));
        assert_eq!(at.len(), FILE_MAX);
        b.edit_file(&t, "needle", "n", disk(&at)).unwrap();
        let over = format!("needle{}", "a".repeat(FILE_MAX - 5));
        let mut b = batch();
        assert_eq!(
            b.edit_file(&t, "needle", "n", disk(&over)),
            Err(EditError::FileTooLarge)
        );
        // Judged by size alone: the bytes of a huge file are not needed.
        let huge = Existing::Present {
            size: 2 * 1024 * 1024,
            bytes: &[],
        };
        assert_eq!(
            b.edit_file(&t, "a", "b", huge),
            Err(EditError::FileTooLarge)
        );
    }

    #[test]
    fn a_read_that_is_not_the_whole_file_is_refused() {
        let mut b = batch();
        let short = Existing::Present {
            size: 10,
            bytes: b"abc",
        };
        assert_eq!(
            b.edit_file(&target("a"), "a", "b", short),
            Err(EditError::Unreadable)
        );
    }

    #[test]
    fn a_file_that_is_not_text_or_has_crlf_is_refused() {
        let mut b = batch();
        let t = target("a");
        let latin1 = Existing::Present {
            size: 3,
            bytes: &[0x63, 0xe9, 0x21],
        };
        assert_eq!(
            b.edit_file(&t, "c", "d", latin1),
            Err(EditError::FileNotText)
        );
        assert_eq!(
            b.edit_file(&t, "a", "d", disk("a\r\nb")),
            Err(EditError::FileHasCrlf)
        );
        assert!(b.entries().is_empty());
    }

    #[test]
    fn a_file_holding_a_control_or_a_bom_can_be_edited_elsewhere() {
        let mut b = batch();
        let with_bom = "\u{feff}fn a() {}\n";
        b.edit_file(&target("bom.rs"), "a()", "b()", disk(with_bom))
            .unwrap();
        assert_eq!(file(&b, 0).0, "\u{feff}fn b() {}\n");
        let with_esc = "keep \u{1b}[0m\nchange me\n";
        b.edit_file(&target("esc.txt"), "change me", "done", disk(with_esc))
            .unwrap();
        assert_eq!(file(&b, 1).0, "keep \u{1b}[0m\ndone\n");
    }

    #[test]
    fn a_new_text_with_a_bom_or_a_control_is_refused() {
        let mut b = batch();
        let t = target("a.txt");
        assert_eq!(
            b.edit_file(&t, "abc", "\u{feff}abc", disk("abc")),
            Err(EditError::Content(ContentError::Invisible))
        );
        assert_eq!(
            b.edit_file(&t, "abc", "a\u{7}c", disk("abc")),
            Err(EditError::Content(ContentError::Control))
        );
        assert_eq!(
            b.edit_file(&t, "abc", "a\r\nc", disk("abc")),
            Err(EditError::Content(ContentError::CarriageReturn))
        );
        assert!(b.entries().is_empty());
    }

    #[test]
    fn the_batch_rules_still_apply_to_an_edit() {
        let mut b = batch();
        let t = target("a.txt");
        b.edit_file(&t, "a", "b", disk("a")).unwrap();
        b.propose().unwrap();
        assert_eq!(b.state(), State::Proposed);
        assert_eq!(
            b.edit_file(&t, "b", "c", disk("a")),
            Err(EditError::Stage(StageError::Proposed))
        );
        assert_eq!(file(&b, 0).0, "b");
    }

    #[test]
    fn an_edit_that_would_pass_the_batch_cap_leaves_the_batch_unchanged() {
        let mut b = batch();
        let big = format!("x{}", "a".repeat(900 * 1024));
        b.edit_file(&target("one"), "x", "y", disk(&big)).unwrap();
        assert_eq!(
            b.edit_file(&target("two"), "x", "y", disk(&big)),
            Err(EditError::Stage(StageError::Full))
        );
        assert_eq!(b.entries().len(), 1);
    }

    #[test]
    fn crlf_and_non_utf8_get_their_own_messages() {
        let mut b = batch();
        let t = target("a");
        let crlf = b.edit_file(&t, "a", "d", disk("a\r\nb")).unwrap_err();
        assert!(crlf.to_string().contains("CRLF line endings"), "{crlf}");
        assert!(crlf.to_string().contains("not support"), "{crlf}");
        assert!(!crlf.to_string().contains("use LF"), "{crlf}");
        let latin1 = Existing::Present {
            size: 3,
            bytes: &[0x63, 0xe9, 0x21],
        };
        let bad = b.edit_file(&t, "c", "d", latin1).unwrap_err();
        assert!(bad.to_string().contains("not UTF-8 text"), "{bad}");
        // A CR in the new text still tells the model to use LF.
        let new_cr = b.edit_file(&t, "abc", "a\r", disk("abc")).unwrap_err();
        assert!(new_cr.to_string().contains("LF"), "{new_cr}");
    }

    #[test]
    fn the_too_large_message_names_the_field() {
        let mut b = batch();
        let over = "a".repeat(FIELD_MAX + 1);
        let old = b
            .edit_file(&target("a"), &over, "x", disk("a"))
            .unwrap_err();
        assert_eq!(old.to_string(), "old is over 16 KiB");
        let new = b
            .edit_file(&target("a"), "a", &over, disk("a"))
            .unwrap_err();
        assert_eq!(new.to_string(), "new is over 16 KiB");
    }

    #[test]
    fn growth_past_the_batch_cap_is_full() {
        let mut b = batch();
        // 1 MiB file at the file cap: legal to edit, but not to grow.
        let at = format!("x{}", "a".repeat(FILE_MAX - 1));
        let grow = "b".repeat(FIELD_MAX);
        assert_eq!(
            b.edit_file(&target("big"), "x", &grow, disk(&at)),
            Err(EditError::Stage(StageError::Full))
        );
        assert!(b.entries().is_empty());
    }

    #[test]
    fn a_multi_byte_old_matches_once() {
        let mut b = batch();
        b.edit_file(&target("a"), "é", "e", disk("caf\u{e9} \u{1f600}!"))
            .unwrap();
        assert_eq!(file(&b, 0).0, "cafe \u{1f600}!");
        b.edit_file(&target("b"), "\u{1f600}", "x", disk("a\u{1f600}b"))
            .unwrap();
        assert_eq!(file(&b, 1).0, "axb");
    }

    #[test]
    fn overlapping_multi_byte_matches_are_ambiguous() {
        assert_eq!(
            apply_edit("\u{e9}\u{e9}\u{e9}", "\u{e9}\u{e9}", "x"),
            Err(EditError::Ambiguous)
        );
        assert_eq!(
            apply_edit("\u{1f600}\u{1f600}\u{1f600}", "\u{1f600}\u{1f600}", "x"),
            Err(EditError::Ambiguous)
        );
        assert_eq!(
            apply_edit("\u{e9}\u{e9}", "\u{e9}\u{e9}", "x").unwrap(),
            "x"
        );
    }

    #[test]
    fn an_edit_after_a_write_keeps_the_disk_text_captured_by_the_write() {
        let mut b = batch();
        let t = target("a.txt");
        b.stage_file(&t, "one two\n", disk("disk text\n")).unwrap();
        b.edit_file(&t, "two", "2", disk("changed again\n"))
            .unwrap();
        assert_eq!(file(&b, 0), ("one 2\n", Some("disk text\n")));
    }

    #[test]
    fn apply_edit_is_a_pure_replace_of_the_single_occurrence() {
        assert_eq!(apply_edit("a b c", "b", "X").unwrap(), "a X c");
        assert_eq!(apply_edit("a b c", " b ", "").unwrap(), "ac");
    }
}
