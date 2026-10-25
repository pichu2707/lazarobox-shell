//! The state machine of a staged write batch. Pure: no IO, nothing here
//! touches the disk. Apply is a later slice; this only decides what may be
//! staged, when a batch may be proposed and what an answer does to it.
//!
//! Lifecycle: `Staging -> Proposed -> Applied | Discarded`. While a batch is
//! `Proposed`, staging is refused. The review and the apply carry a
//! [`BatchId`], and an answer for another generation is refused as stale.

use std::fmt;

use crate::core::ai::tools::paths::WriteTarget;
use crate::core::layout::PaneId;

/// Whose batch it is. One batch per key; the enum is meant to grow (worker
/// keys come with the orchestrator).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum StageKey {
    Pane(PaneId),
}

/// A batch of one key at one generation. A newer batch of the same key has a
/// higher generation, so an answer meant for an older one cannot hit it.
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
    /// Never proposed: the user may only discard it.
    Staging,
    /// Proposed but never answered: the user may review and apply it.
    Proposed,
}

/// What a staged path will become.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Entry {
    Dir,
    File { text: String },
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

    /// Stages a file with its whole new text.
    pub fn stage_file(&mut self, target: &WriteTarget, text: &str) -> Result<(), StageError> {
        self.put(target, Entry::File { text: text.into() })
    }

    /// Adds an entry, or replaces the earlier one for the same path in place.
    fn put(&mut self, target: &WriteTarget, entry: Entry) -> Result<(), StageError> {
        self.open()?;
        let staged = Staged {
            target: target.clone(),
            entry,
        };
        match self
            .entries
            .iter_mut()
            .find(|s| s.target.real == target.real)
        {
            Some(slot) => *slot = staged,
            None => self.entries.push(staged),
        }
        Ok(())
    }

    /// Whether staging is allowed in the current state.
    fn open(&self) -> Result<(), StageError> {
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

    /// Drops the batch whatever its state: a pane close or an app quit.
    pub fn discard(&mut self) {
        self.state = State::Discarded;
    }

    /// Whether the batch is an orphan: still alive, with entries, and no live
    /// turn behind it.
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
        b.stage_file(&target("src/main.rs"), "fn main() {}\n")
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
                text: "fn main() {}\n".into()
            }
        );
    }

    #[test]
    fn restaging_a_path_replaces_the_earlier_entry() {
        let mut b = batch(1);
        b.stage_file(&target("a.txt"), "one\n").unwrap();
        b.stage_file(&target("b.txt"), "b\n").unwrap();
        b.stage_file(&target("a.txt"), "two\n").unwrap();
        assert_eq!(b.entries().len(), 2);
        assert_eq!(
            b.entries()[0].entry,
            Entry::File {
                text: "two\n".into()
            }
        );
    }

    #[test]
    fn restaging_a_dir_path_as_a_file_replaces_it() {
        let mut b = batch(1);
        b.stage_dir(&target("x")).unwrap();
        b.stage_file(&target("x"), "t\n").unwrap();
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
        assert_eq!(b.stage_file(&target("f"), "x\n"), Err(StageError::Proposed));
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
}
