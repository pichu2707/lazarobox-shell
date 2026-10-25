//! The state machine of a staged write batch. Pure: no IO, nothing here
//! touches the disk. Apply is a later slice; this only decides what may be
//! staged, when a batch may be proposed and what an answer does to it.
//!
//! Lifecycle: `Staging -> Proposed -> Applied | Discarded`. While a batch is
//! `Proposed`, staging is refused. The review and the apply carry a
//! [`BatchId`], and an answer for another generation is refused as stale.

use std::fmt;

use crate::core::ai::memory::redact::redact;
use crate::core::ai::sanitize::clean;
use crate::core::ai::tools::content::{ContentError, check_existing, check_new};
use crate::core::ai::tools::paths::WriteTarget;
use crate::core::layout::PaneId;

/// Entries a batch may hold.
pub const MAX_ENTRIES: usize = 50;
/// New content a batch may hold, in bytes.
pub const MAX_BYTES: usize = 1024 * 1024;
/// The text of one `write_file` call, in bytes.
pub const WRITE_FILE_MAX: usize = 32 * 1024;

/// The largest existing file `write_file` may overwrite: the read cap.
pub const READ_CAP: usize = 64 * 1024;

/// Whose batch it is. One batch per key; the enum is meant to grow (worker
/// keys come with the orchestrator).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum StageKey {
    Pane(PaneId),
}

/// A batch of one key at one generation. A newer batch of the same key has a
/// higher generation, so an answer meant for an older one cannot hit it.
/// That holds only if the owner (the `Stages` store) hands out generations
/// from a monotonic counter that is never reused (`u64`, `checked_add`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct BatchId {
    pub key: StageKey,
    pub generation: u64,
}

/// Where a batch is in its life.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Staging,
    Proposed,
    Applied,
    Discarded,
}

/// What the user decided about a proposed batch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    Applied,
    Discarded,
}

/// A live batch whose turn is gone, so nobody is waiting on it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Orphan {
    /// Never proposed: [`Batch::review_orphan`] proposes it, then the user
    /// may apply or discard it.
    Staging,
    /// Proposed but never answered: the user may review and apply it.
    Proposed,
}

/// What a staged path will become.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Entry {
    Dir,
    /// `base` is the text on disk for an overwrite, `None` for a create.
    File {
        text: String,
        base: Option<String>,
    },
}

/// What the caller found at a file's path on disk.
#[derive(Clone, Copy, Debug)]
pub enum Existing<'a> {
    Absent,
    /// A file of `size` bytes. `bytes` is read only when `size` is within
    /// [`READ_CAP`], so a huge file is judged by its size alone.
    Present {
        size: u64,
        bytes: &'a [u8],
    },
}

impl Entry {
    /// The new content this entry carries.
    fn bytes(&self) -> usize {
        match self {
            Entry::Dir => 0,
            Entry::File { text, .. } => text.len(),
        }
    }
}

/// One staged path.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Staged {
    pub target: WriteTarget,
    pub entry: Entry,
}

/// Why a stage or a lifecycle call was refused. Each text goes back to the
/// model.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StageError {
    Empty,
    Proposed,
    Closed,
    Stale,
    NotProposed,
    FileTooLarge,
    /// The file on disk cannot be overwritten as shown: the model must edit it.
    UseEditFile,
    /// The text was refused by the content rules.
    Content(ContentError),
    /// Staging this would pass a batch cap.
    Full,
    /// The batch passes a cap at propose time.
    TooLarge,
    /// A path would be a file and a folder in one batch.
    TreeConflict,
    /// The batch is not an orphan, so there is nothing to review.
    NotOrphan,
}

impl fmt::Display for StageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            StageError::Empty => "nothing is staged",
            StageError::Proposed => {
                "a batch is waiting for review: no staging until the user answers"
            }
            StageError::Closed => "the batch is closed: nothing can be staged in it",
            StageError::Stale => "that batch is no longer the current one",
            StageError::NotProposed => "the batch was not proposed",
            StageError::UseEditFile => "use edit_file",
            StageError::Content(why) => return why.fmt(f),
            StageError::FileTooLarge => "write_file content is over 32 KiB",
            StageError::Full => "batch is full: propose it, then stage the rest",
            StageError::TooLarge => {
                "batch is over 50 entries or 1 MiB: split it into smaller batches"
            }
            StageError::TreeConflict => "a path is staged as a file and as a folder",
            StageError::NotOrphan => "the batch is not an orphan",
        })
    }
}

/// The staged writes of one key.
#[derive(Debug)]
pub struct Batch {
    id: BatchId,
    state: State,
    entries: Vec<Staged>,
}

impl Batch {
    /// The owner MUST give each new batch of a key a generation never used
    /// before for that key (a monotonic `u64` counter, `checked_add`); the
    /// batch cannot check it.
    pub fn new(id: BatchId) -> Batch {
        Batch {
            id,
            state: State::Staging,
            entries: Vec::new(),
        }
    }

    pub fn id(&self) -> BatchId {
        self.id
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn entries(&self) -> &[Staged] {
        &self.entries
    }

    /// Stages a folder. Nothing touches the disk.
    pub fn stage_dir(&mut self, target: &WriteTarget) -> Result<(), StageError> {
        self.put(target, Entry::Dir)
    }

    /// Stages a file with its whole new text. `existing` is what the caller
    /// found on disk. An overwrite is allowed only when the file is within
    /// [`READ_CAP`] and redaction leaves its text as it is, so what the review
    /// shows is what is on disk; otherwise the model must use `edit_file`.
    pub fn stage_file(
        &mut self,
        target: &WriteTarget,
        text: &str,
        existing: Existing<'_>,
    ) -> Result<(), StageError> {
        if text.len() > WRITE_FILE_MAX {
            return Err(StageError::FileTooLarge);
        }
        check_new(text).map_err(StageError::Content)?;
        let base = match existing {
            Existing::Absent => None,
            Existing::Present { size, .. } if size > READ_CAP as u64 => {
                return Err(StageError::UseEditFile);
            }
            // A read that is not the whole file as measured (truncated or
            // inconsistent) cannot be shown as the base. With `size` within
            // the cap this also bounds the slice by [`READ_CAP`]. Refused as
            // `UseEditFile`: `edit_file` reads the file again by itself.
            Existing::Present { size, bytes } if bytes.len() as u64 != size => {
                return Err(StageError::UseEditFile);
            }
            Existing::Present { bytes, .. } => {
                let disk = check_existing(bytes).map_err(StageError::Content)?;
                if redact(&clean(disk)).text != disk {
                    return Err(StageError::UseEditFile);
                }
                Some(disk.to_owned())
            }
        };
        self.put(
            target,
            Entry::File {
                text: text.into(),
                base,
            },
        )
    }

    /// Adds an entry, or replaces the earlier one for the same path in place.
    /// Entries key on `WriteTarget.real`, which `resolve_write` builds
    /// canonical (it refuses a path that differs from its canonical form), so
    /// two spellings of one target are one key. No case folding is done: on
    /// macOS `resolve_write` refuses writes, so it never reaches this.
    pub(super) fn put(&mut self, target: &WriteTarget, entry: Entry) -> Result<(), StageError> {
        self.open()?;
        // One path cannot be a file and a folder in the same batch: a file
        // with an entry below it, or an entry below a staged file.
        let clash = self.entries.iter().any(|s| {
            let (old, new) = (&s.target.real, &target.real);
            (old != new)
                && ((new.starts_with(old) && matches!(s.entry, Entry::File { .. }))
                    || (old.starts_with(new) && matches!(entry, Entry::File { .. })))
        });
        if clash {
            return Err(StageError::TreeConflict);
        }
        let slot = self
            .entries
            .iter()
            .position(|s| s.target.real == target.real);
        let replaced = slot.map_or(0, |i| self.entries[i].entry.bytes());
        let entries = self.entries.len() + usize::from(slot.is_none());
        if entries > MAX_ENTRIES || self.bytes() - replaced + entry.bytes() > MAX_BYTES {
            return Err(StageError::Full);
        }
        let staged = Staged {
            target: target.clone(),
            entry,
        };
        match slot {
            Some(i) => self.entries[i] = staged,
            None => self.entries.push(staged),
        }
        Ok(())
    }

    /// The entry staged for a path, if any.
    pub(super) fn staged(&self, target: &WriteTarget) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|s| s.target.real == target.real)
            .map(|s| &s.entry)
    }

    /// The new content held, in bytes.
    pub fn bytes(&self) -> usize {
        self.entries.iter().map(|s| s.entry.bytes()).sum()
    }

    /// Whether staging is allowed in the current state.
    pub(super) fn open(&self) -> Result<(), StageError> {
        match self.state {
            State::Staging => Ok(()),
            State::Proposed => Err(StageError::Proposed),
            State::Applied | State::Discarded => Err(StageError::Closed),
        }
    }

    /// Closes staging and hands the batch to the user. An empty batch is
    /// refused and stays as it is.
    pub fn propose(&mut self) -> Result<BatchId, StageError> {
        self.open()?;
        if self.entries.is_empty() {
            return Err(StageError::Empty);
        }
        if self.entries.len() > MAX_ENTRIES || self.bytes() > MAX_BYTES {
            return Err(StageError::TooLarge);
        }
        self.state = State::Proposed;
        Ok(self.id)
    }

    /// Records the user's answer. Only the proposed batch of this very id
    /// takes it.
    pub fn resolve(&mut self, id: BatchId, outcome: Outcome) -> Result<(), StageError> {
        if id != self.id {
            return Err(StageError::Stale);
        }
        if self.state != State::Proposed {
            return Err(StageError::NotProposed);
        }
        self.state = match outcome {
            Outcome::Applied => State::Applied,
            Outcome::Discarded => State::Discarded,
        };
        Ok(())
    }

    /// Hands an orphan to the user for review. The spec ("Orphan, close and
    /// quit": "review and apply it, or discard it") covers every batch left
    /// staged by a cancelled turn, so a `Staging` orphan is proposed here and
    /// then resolved as any proposed batch; a `Proposed` orphan returns its
    /// id again. A batch that is not an orphan is refused.
    pub fn review_orphan(&mut self, turn_live: bool) -> Result<BatchId, StageError> {
        match self.orphan(turn_live) {
            Some(Orphan::Staging) => self.propose(),
            Some(Orphan::Proposed) => Ok(self.id),
            None => Err(StageError::NotOrphan),
        }
    }

    /// Drops the batch whatever its state: a pane close or an app quit.
    /// An answered batch (`Applied` or `Discarded`) keeps its state.
    pub fn discard(&mut self) {
        if matches!(self.state, State::Staging | State::Proposed) {
            self.state = State::Discarded;
        }
    }

    /// Whether the batch is an orphan: still alive, with entries, and no live
    /// turn behind it. An empty batch is never one, whatever the turn: there
    /// is nothing to review, so the answer is `None`.
    pub fn orphan(&self, turn_live: bool) -> Option<Orphan> {
        if turn_live || self.entries.is_empty() {
            return None;
        }
        match self.state {
            State::Staging => Some(Orphan::Staging),
            State::Proposed => Some(Orphan::Proposed),
            State::Applied | State::Discarded => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::layout::PaneId;
    use std::path::PathBuf;

    fn key() -> StageKey {
        StageKey::Pane(PaneId::for_test(3))
    }

    fn batch(generation: u64) -> Batch {
        Batch::new(BatchId {
            key: key(),
            generation,
        })
    }

    fn target(path: &str) -> WriteTarget {
        WriteTarget {
            real: PathBuf::from("/home/u/proj").join(path),
            build_file: false,
        }
    }

    #[test]
    fn a_new_batch_stages_and_lists_entries_in_order() {
        let mut b = batch(1);
        b.stage_dir(&target("src/util")).unwrap();
        b.stage_file(&target("src/main.rs"), "fn main() {}\n", Existing::Absent)
            .unwrap();
        assert_eq!(b.state(), State::Staging);
        let paths: Vec<_> = b.entries().iter().map(|s| s.target.real.clone()).collect();
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/home/u/proj/src/util"),
                PathBuf::from("/home/u/proj/src/main.rs")
            ]
        );
        assert_eq!(b.entries()[0].entry, Entry::Dir);
        assert_eq!(
            b.entries()[1].entry,
            Entry::File {
                text: "fn main() {}\n".into(),
                base: None
            }
        );
    }

    #[test]
    fn restaging_a_path_replaces_the_earlier_entry() {
        let mut b = batch(1);
        b.stage_file(&target("a.txt"), "one\n", Existing::Absent)
            .unwrap();
        b.stage_file(&target("b.txt"), "b\n", Existing::Absent)
            .unwrap();
        b.stage_file(&target("a.txt"), "two\n", Existing::Absent)
            .unwrap();
        assert_eq!(b.entries().len(), 2);
        assert_eq!(
            b.entries()[0].entry,
            Entry::File {
                text: "two\n".into(),
                base: None
            }
        );
    }

    #[test]
    fn restaging_a_dir_path_as_a_file_replaces_it() {
        let mut b = batch(1);
        b.stage_dir(&target("x")).unwrap();
        b.stage_file(&target("x"), "t\n", Existing::Absent).unwrap();
        assert_eq!(b.entries().len(), 1);
        assert!(matches!(b.entries()[0].entry, Entry::File { .. }));
    }

    #[test]
    fn propose_moves_staging_to_proposed_with_the_batch_id() {
        let mut b = batch(7);
        b.stage_dir(&target("d")).unwrap();
        let id = b.propose().unwrap();
        assert_eq!(
            id,
            BatchId {
                key: key(),
                generation: 7
            }
        );
        assert_eq!(b.state(), State::Proposed);
    }

    #[test]
    fn proposing_an_empty_batch_is_refused_and_stays_staging() {
        let mut b = batch(1);
        assert_eq!(b.propose(), Err(StageError::Empty));
        assert_eq!(b.state(), State::Staging);
        assert_eq!(StageError::Empty.to_string(), "nothing is staged");
    }

    #[test]
    fn staging_is_refused_while_proposed() {
        let mut b = batch(1);
        b.stage_dir(&target("d")).unwrap();
        b.propose().unwrap();
        assert_eq!(b.stage_dir(&target("e")), Err(StageError::Proposed));
        assert_eq!(
            b.stage_file(&target("f"), "x\n", Existing::Absent),
            Err(StageError::Proposed)
        );
        assert_eq!(b.entries().len(), 1);
        assert_eq!(b.propose(), Err(StageError::Proposed));
    }

    #[test]
    fn the_user_answer_ends_the_batch_and_it_takes_no_more_staging() {
        for (outcome, state) in [
            (Outcome::Applied, State::Applied),
            (Outcome::Discarded, State::Discarded),
        ] {
            let mut b = batch(2);
            b.stage_dir(&target("d")).unwrap();
            let id = b.propose().unwrap();
            b.resolve(id, outcome).unwrap();
            assert_eq!(b.state(), state);
            assert_eq!(b.stage_dir(&target("e")), Err(StageError::Closed));
        }
    }

    #[test]
    fn a_stale_generation_is_refused_and_changes_nothing() {
        let mut b = batch(2);
        b.stage_dir(&target("d")).unwrap();
        b.propose().unwrap();
        let stale = BatchId {
            key: key(),
            generation: 1,
        };
        assert_eq!(b.resolve(stale, Outcome::Applied), Err(StageError::Stale));
        let other = BatchId {
            key: StageKey::Pane(PaneId::for_test(4)),
            generation: 2,
        };
        assert_eq!(b.resolve(other, Outcome::Applied), Err(StageError::Stale));
        assert_eq!(b.state(), State::Proposed);
    }

    #[test]
    fn resolving_a_batch_that_was_not_proposed_is_refused() {
        let mut b = batch(1);
        b.stage_dir(&target("d")).unwrap();
        let id = BatchId {
            key: key(),
            generation: 1,
        };
        assert_eq!(
            b.resolve(id, Outcome::Applied),
            Err(StageError::NotProposed)
        );
        assert_eq!(b.state(), State::Staging);
    }

    #[test]
    fn pane_close_discards_a_staging_or_a_proposed_batch() {
        let mut staging = batch(1);
        staging.stage_dir(&target("d")).unwrap();
        staging.discard();
        assert_eq!(staging.state(), State::Discarded);
        let mut proposed = batch(1);
        proposed.stage_dir(&target("d")).unwrap();
        proposed.propose().unwrap();
        proposed.discard();
        assert_eq!(proposed.state(), State::Discarded);
    }

    #[test]
    fn an_orphan_is_a_live_batch_whose_turn_is_gone() {
        let mut b = batch(1);
        b.stage_dir(&target("d")).unwrap();
        assert_eq!(b.orphan(true), None);
        assert_eq!(b.orphan(false), Some(Orphan::Staging));
        b.propose().unwrap();
        assert_eq!(b.orphan(true), None);
        assert_eq!(b.orphan(false), Some(Orphan::Proposed));
        let id = BatchId {
            key: key(),
            generation: 1,
        };
        b.resolve(id, Outcome::Discarded).unwrap();
        assert_eq!(b.orphan(false), None);
    }

    #[test]
    fn an_empty_staging_batch_is_not_an_orphan() {
        assert_eq!(batch(1).orphan(false), None);
    }

    fn numbered(n: usize) -> WriteTarget {
        target(&format!("f{n}.txt"))
    }

    #[test]
    fn write_file_content_is_capped_at_32_kib() {
        let mut b = batch(1);
        let at_cap = "a".repeat(WRITE_FILE_MAX);
        b.stage_file(&target("ok.txt"), &at_cap, Existing::Absent)
            .unwrap();
        let over = "a".repeat(WRITE_FILE_MAX + 1);
        assert_eq!(
            b.stage_file(&target("big.txt"), &over, Existing::Absent),
            Err(StageError::FileTooLarge)
        );
        assert_eq!(b.entries().len(), 1);
        assert_eq!(
            StageError::FileTooLarge.to_string(),
            "write_file content is over 32 KiB"
        );
    }

    #[test]
    fn the_fifty_first_entry_is_refused_with_the_full_message() {
        let mut b = batch(1);
        for n in 0..MAX_ENTRIES {
            b.stage_file(&numbered(n), "x\n", Existing::Absent).unwrap();
        }
        assert_eq!(b.entries().len(), 50);
        assert_eq!(b.stage_dir(&target("extra")), Err(StageError::Full));
        assert_eq!(
            b.stage_file(&numbered(50), "x\n", Existing::Absent),
            Err(StageError::Full)
        );
        assert_eq!(b.entries().len(), 50);
        assert_eq!(
            StageError::Full.to_string(),
            "batch is full: propose it, then stage the rest"
        );
    }

    #[test]
    fn restaging_an_existing_path_at_fifty_entries_is_allowed() {
        let mut b = batch(1);
        for n in 0..MAX_ENTRIES {
            b.stage_file(&numbered(n), "x\n", Existing::Absent).unwrap();
        }
        b.stage_file(&numbered(7), "y\n", Existing::Absent).unwrap();
        assert_eq!(b.entries().len(), 50);
    }

    #[test]
    fn the_total_new_content_is_capped_at_one_mib() {
        let mut b = batch(1);
        let full = "a".repeat(WRITE_FILE_MAX);
        for n in 0..32 {
            b.stage_file(&numbered(n), &full, Existing::Absent).unwrap();
        }
        assert_eq!(b.bytes(), MAX_BYTES);
        assert_eq!(
            b.stage_file(&numbered(32), "x", Existing::Absent),
            Err(StageError::Full)
        );
        assert_eq!(b.entries().len(), 32);
        assert_eq!(b.bytes(), MAX_BYTES);
    }

    #[test]
    fn a_restage_counts_the_replacement_not_the_old_text() {
        let mut b = batch(1);
        let full = "a".repeat(WRITE_FILE_MAX);
        for n in 0..32 {
            b.stage_file(&numbered(n), &full, Existing::Absent).unwrap();
        }
        b.stage_file(&numbered(0), "tiny\n", Existing::Absent)
            .unwrap();
        assert_eq!(b.bytes(), MAX_BYTES - WRITE_FILE_MAX + 5);
        let rest = "a".repeat(WRITE_FILE_MAX - 5);
        b.stage_file(&numbered(32), &rest, Existing::Absent)
            .unwrap();
        assert_eq!(b.entries().len(), 33);
        assert_eq!(b.bytes(), MAX_BYTES);
    }

    #[test]
    fn propose_rechecks_the_caps_and_leaves_the_batch_unchanged() {
        let over_entries = |b: &mut Batch| {
            for n in 0..=MAX_ENTRIES {
                b.entries.push(Staged {
                    target: numbered(n),
                    entry: Entry::Dir,
                });
            }
        };
        let over_bytes = |b: &mut Batch| {
            let text = "a".repeat(WRITE_FILE_MAX);
            for n in 0..32 {
                b.entries.push(Staged {
                    target: numbered(n),
                    entry: Entry::File {
                        text: text.clone(),
                        base: None,
                    },
                });
            }
            b.entries.push(Staged {
                target: numbered(32),
                entry: Entry::File {
                    text: "x".into(),
                    base: None,
                },
            });
        };
        for fill in [&over_entries as &dyn Fn(&mut Batch), &over_bytes] {
            let mut b = batch(1);
            fill(&mut b);
            let before = b.entries().to_vec();
            assert_eq!(b.propose(), Err(StageError::TooLarge));
            assert_eq!(b.state(), State::Staging);
            assert_eq!(b.entries(), &before[..]);
        }
        assert_eq!(
            StageError::TooLarge.to_string(),
            "batch is over 50 entries or 1 MiB: split it into smaller batches"
        );
    }

    #[test]
    fn a_batch_exactly_at_the_caps_can_be_proposed() {
        let mut b = batch(1);
        let full = "a".repeat(WRITE_FILE_MAX);
        for n in 0..32 {
            b.stage_file(&numbered(n), &full, Existing::Absent).unwrap();
        }
        for n in 32..MAX_ENTRIES {
            b.stage_dir(&numbered(n)).unwrap();
        }
        assert_eq!(b.entries().len(), 50);
        assert_eq!(b.bytes(), MAX_BYTES);
        assert!(b.propose().is_ok());
    }

    const KEY_LINE: &str = "token = ghp_0123456789abcdefghijklmnopqrstuvwxyz01\n";

    fn present(text: &str) -> Existing<'_> {
        Existing::Present {
            size: text.len() as u64,
            bytes: text.as_bytes(),
        }
    }

    #[test]
    fn overwriting_an_unredacted_file_stages_it_with_its_base() {
        let mut b = batch(1);
        let disk = "old line\n".repeat(200);
        b.stage_file(&target("a.txt"), "new\n", present(&disk))
            .unwrap();
        assert_eq!(
            b.entries()[0].entry,
            Entry::File {
                text: "new\n".into(),
                base: Some(disk.clone())
            }
        );
        assert!(disk.len() > 1024);
    }

    #[test]
    fn overwriting_a_file_whose_text_redaction_changes_is_refused() {
        let mut b = batch(1);
        let disk = format!("keep\n{KEY_LINE}");
        assert_ne!(redact(&clean(&disk)).text, disk);
        assert_eq!(
            b.stage_file(&target("a.txt"), "new\n", present(&disk)),
            Err(StageError::UseEditFile)
        );
        assert!(b.entries().is_empty());
        assert_eq!(StageError::UseEditFile.to_string(), "use edit_file");
    }

    #[test]
    fn overwriting_a_file_over_the_read_cap_is_refused() {
        let mut b = batch(1);
        let at_cap = "a".repeat(READ_CAP);
        b.stage_file(&target("ok.txt"), "new\n", present(&at_cap))
            .unwrap();
        let over = "a".repeat(READ_CAP + 1);
        assert_eq!(
            b.stage_file(&target("big.txt"), "new\n", present(&over)),
            Err(StageError::UseEditFile)
        );
        // A huge file is judged by its size alone, its bytes are not needed.
        let huge = Existing::Present {
            size: 2 * 1024 * 1024,
            bytes: b"",
        };
        assert_eq!(
            b.stage_file(&target("huge.txt"), "new\n", huge),
            Err(StageError::UseEditFile)
        );
        assert_eq!(b.entries().len(), 1);
    }

    #[test]
    fn an_existing_file_that_clean_would_rewrite_is_refused() {
        let mut b = batch(1);
        for disk in ["esc \u{1b}[31mred\n", "bidi \u{202e}text\n", "bell \u{7}\n"] {
            assert_eq!(
                b.stage_file(&target("a.txt"), "new\n", present(disk)),
                Err(StageError::UseEditFile),
                "{disk:?}"
            );
        }
        assert!(b.entries().is_empty());
    }

    #[test]
    fn an_existing_file_that_is_not_plain_text_keeps_its_content_refusal() {
        let mut b = batch(1);
        let crlf = Existing::Present {
            size: 4,
            bytes: b"a\r\nb",
        };
        assert_eq!(
            b.stage_file(&target("a.txt"), "new\n", crlf),
            Err(StageError::Content(ContentError::CarriageReturn))
        );
        let binary = Existing::Present {
            size: 3,
            bytes: &[0xff, 0x00, 0x01],
        };
        assert_eq!(
            b.stage_file(&target("b.bin"), "new\n", binary),
            Err(StageError::Content(ContentError::NotText))
        );
        assert!(b.entries().is_empty());
    }

    #[test]
    fn the_new_text_goes_through_the_content_rules_for_a_create_and_an_overwrite() {
        let mut b = batch(1);
        assert_eq!(
            b.stage_file(&target("a.txt"), "\u{feff}bom\n", Existing::Absent),
            Err(StageError::Content(ContentError::Invisible))
        );
        assert_eq!(
            b.stage_file(&target("b.txt"), "tab\u{7}\n", present("old\n")),
            Err(StageError::Content(ContentError::Control))
        );
        assert!(b.entries().is_empty());
    }

    #[test]
    fn a_refused_overwrite_leaves_the_earlier_staged_entry_alone() {
        let mut b = batch(1);
        b.stage_file(&target("a.txt"), "first\n", Existing::Absent)
            .unwrap();
        let disk = KEY_LINE.to_string();
        assert_eq!(
            b.stage_file(&target("a.txt"), "second\n", present(&disk)),
            Err(StageError::UseEditFile)
        );
        assert_eq!(
            b.entries()[0].entry,
            Entry::File {
                text: "first\n".into(),
                base: None
            }
        );
    }

    #[test]
    fn a_read_that_does_not_match_its_size_is_refused() {
        let mut b = batch(1);
        let truncated = Existing::Present {
            size: 10,
            bytes: b"short",
        };
        assert_eq!(
            b.stage_file(&target("a.txt"), "new\n", truncated),
            Err(StageError::UseEditFile)
        );
        let long = "a".repeat(100);
        let oversized = Existing::Present {
            size: 10,
            bytes: long.as_bytes(),
        };
        assert_eq!(
            b.stage_file(&target("b.txt"), "new\n", oversized),
            Err(StageError::UseEditFile)
        );
        let over_cap = "a".repeat(READ_CAP + 1);
        let past_cap = Existing::Present {
            size: READ_CAP as u64,
            bytes: over_cap.as_bytes(),
        };
        assert_eq!(
            b.stage_file(&target("c.txt"), "new\n", past_cap),
            Err(StageError::UseEditFile)
        );
        assert!(b.entries().is_empty());
    }

    #[test]
    fn a_base_with_a_nul_or_bad_utf8_is_refused_even_when_its_size_matches() {
        let mut b = batch(1);
        for bytes in [&b"a\0b"[..], &[0xff, 0xfe][..]] {
            let disk = Existing::Present {
                size: bytes.len() as u64,
                bytes,
            };
            assert!(matches!(
                b.stage_file(&target("a.txt"), "new\n", disk),
                Err(StageError::Content(_))
            ));
        }
        assert!(b.entries().is_empty());
    }

    #[test]
    fn discard_leaves_a_terminal_state_as_it_is() {
        for outcome in [Outcome::Applied, Outcome::Discarded] {
            let mut b = batch(1);
            b.stage_dir(&target("d")).unwrap();
            let id = b.propose().unwrap();
            b.resolve(id, outcome).unwrap();
            let state = b.state();
            b.discard();
            assert_eq!(b.state(), state);
        }
    }

    #[test]
    fn proposing_again_after_an_answer_is_closed() {
        for outcome in [Outcome::Applied, Outcome::Discarded] {
            let mut b = batch(1);
            b.stage_dir(&target("d")).unwrap();
            let id = b.propose().unwrap();
            b.resolve(id, outcome).unwrap();
            assert_eq!(b.propose(), Err(StageError::Closed));
        }
    }

    #[test]
    fn resolving_after_a_discard_or_an_answer_is_not_proposed() {
        let id = BatchId {
            key: key(),
            generation: 1,
        };
        let mut b = batch(1);
        b.stage_dir(&target("d")).unwrap();
        b.propose().unwrap();
        b.discard();
        assert_eq!(
            b.resolve(id, Outcome::Applied),
            Err(StageError::NotProposed)
        );
        assert_eq!(b.state(), State::Discarded);
        let mut b = batch(1);
        b.stage_dir(&target("d")).unwrap();
        b.propose().unwrap();
        b.resolve(id, Outcome::Applied).unwrap();
        assert_eq!(
            b.resolve(id, Outcome::Discarded),
            Err(StageError::NotProposed)
        );
        assert_eq!(b.state(), State::Applied);
    }

    #[test]
    fn two_spellings_of_one_target_replace_one_entry() {
        let mut b = batch(1);
        let one = target("src/a.rs");
        let two = WriteTarget {
            real: PathBuf::from("/home/u/proj/src/a.rs"),
            build_file: false,
        };
        b.stage_file(&one, "one\n", Existing::Absent).unwrap();
        b.stage_file(&two, "two\n", Existing::Absent).unwrap();
        assert_eq!(b.entries().len(), 1);
    }

    #[test]
    fn a_path_staged_as_a_file_and_as_a_folder_is_refused_in_both_orders() {
        let mut b = batch(1);
        b.stage_file(&target("src"), "x\n", Existing::Absent)
            .unwrap();
        assert_eq!(
            b.stage_dir(&target("src/util")),
            Err(StageError::TreeConflict)
        );
        assert_eq!(
            b.stage_file(&target("src/a.rs"), "x\n", Existing::Absent),
            Err(StageError::TreeConflict)
        );
        assert_eq!(b.entries().len(), 1);
        let mut b = batch(1);
        b.stage_file(&target("src/a.rs"), "x\n", Existing::Absent)
            .unwrap();
        b.stage_dir(&target("other/deep")).unwrap();
        assert_eq!(
            b.stage_file(&target("src"), "x\n", Existing::Absent),
            Err(StageError::TreeConflict)
        );
        assert_eq!(b.entries().len(), 2);
        // A folder with files below it, and a sibling with a shared prefix, are fine.
        b.stage_dir(&target("lib")).unwrap();
        b.stage_file(&target("lib/a.rs"), "x\n", Existing::Absent)
            .unwrap();
        b.stage_file(&target("src2"), "x\n", Existing::Absent)
            .unwrap();
        assert_eq!(
            StageError::TreeConflict.to_string(),
            "a path is staged as a file and as a folder"
        );
    }

    #[test]
    fn a_staging_orphan_can_be_proposed_for_review() {
        let mut b = batch(4);
        b.stage_dir(&target("d")).unwrap();
        assert_eq!(b.review_orphan(true), Err(StageError::NotOrphan));
        assert_eq!(b.state(), State::Staging);
        let id = b.review_orphan(false).unwrap();
        assert_eq!(b.state(), State::Proposed);
        assert_eq!(b.review_orphan(false), Ok(id));
        b.resolve(id, Outcome::Applied).unwrap();
        assert_eq!(b.review_orphan(false), Err(StageError::NotOrphan));
        assert_eq!(batch(1).review_orphan(false), Err(StageError::NotOrphan));
    }

    #[test]
    fn an_empty_batch_is_never_an_orphan_whatever_the_turn() {
        for state in [State::Staging, State::Proposed] {
            let mut b = batch(1);
            b.state = state;
            assert_eq!(b.orphan(true), None);
            assert_eq!(b.orphan(false), None);
        }
    }
}
