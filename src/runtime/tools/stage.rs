//! The staged batches of the runtime: one per [`StageKey`], in memory only,
//! never saved, so nothing survives a restart. The store wraps the pure
//! [`Batch`] and adds what a batch cannot check by itself: a generation that
//! is never reused, so an answer for a gone batch cannot hit a new one.
//!
//! Applying goes through [`apply`]; this file holds no write API.

use std::collections::{HashMap, hash_map::Entry};

use super::apply::{self, ApplyResult, Disk, WriteOps};
use crate::core::ai::tools::{
    paths::SecretRoots,
    stage::{Batch, BatchId, Outcome, StageError, StageKey, State},
};

/// The generation counter ran out: no new batch can be made.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Exhausted;

/// The live batches, by key.
#[derive(Debug, Default)]
pub struct Stages {
    batches: HashMap<StageKey, Batch>,
    /// The generation of the next batch. One counter for every key: it only
    /// grows, so a generation is never handed out twice.
    next: u64,
}

impl Stages {
    /// The batch of `key` open for staging, or a new one when the key has none.
    /// A proposed batch is returned as it is: the batch itself refuses staging.
    pub fn stage(&mut self, key: StageKey) -> Result<&mut Batch, Exhausted> {
        match self.batches.entry(key) {
            Entry::Occupied(slot) => Ok(slot.into_mut()),
            Entry::Vacant(slot) => {
                let generation = self.next;
                self.next = generation.checked_add(1).ok_or(Exhausted)?;
                Ok(slot.insert(Batch::new(BatchId { key, generation })))
            }
        }
    }

    pub fn batch(&self, key: StageKey) -> Option<&Batch> {
        self.batches.get(&key)
    }

    /// Closes staging for `key` and hands its batch to the user.
    pub fn seal(&mut self, key: StageKey) -> Result<BatchId, StageError> {
        self.batches
            .get_mut(&key)
            .ok_or(StageError::Empty)?
            .propose()
    }

    /// Applies the proposed batch `id` to the disk below `roots`.
    pub fn apply(&mut self, id: BatchId, roots: &SecretRoots) -> Result<ApplyResult, StageError> {
        self.apply_with(id, &Disk::new(roots))
    }

    fn apply_with(&mut self, id: BatchId, ops: &dyn WriteOps) -> Result<ApplyResult, StageError> {
        let batch = self.current(id)?;
        if batch.state() != State::Proposed {
            return Err(StageError::NotProposed);
        }
        let result = apply::apply(ops, batch.entries());
        // Drift, a refusal or a clean failure left the disk as it was: the
        // batch stays proposed. An apply or a failed undo closes it.
        let closed = match result {
            ApplyResult::Applied(_) => Some(Outcome::Applied),
            ApplyResult::Partial { .. } => Some(Outcome::Discarded),
            _ => None,
        };
        if let Some(outcome) = closed {
            batch.resolve(id, outcome)?;
            self.batches.remove(&id.key);
        }
        Ok(result)
    }

    /// Drops the batch `id`, staged or proposed.
    pub fn discard(&mut self, id: BatchId) -> Result<(), StageError> {
        self.current(id)?.discard();
        self.batches.remove(&id.key);
        Ok(())
    }

    /// Drops whatever `key` holds (a pane close or an app quit). Whether
    /// something worth a notice was dropped: an empty batch is no loss.
    pub fn discard_key(&mut self, key: StageKey) -> bool {
        self.batches
            .remove(&key)
            .is_some_and(|batch| !batch.entries().is_empty())
    }

    /// The live batch with exactly this id.
    fn current(&mut self, id: BatchId) -> Result<&mut Batch, StageError> {
        match self.batches.get_mut(&id.key) {
            Some(batch) if batch.id() == id => Ok(batch),
            _ => Err(StageError::Stale),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ai::tools::{
        paths::WriteTarget,
        stage::{Existing, State},
    };
    use crate::core::layout::PaneId;
    use crate::runtime::tools::write::WriteError;
    use std::{
        cell::RefCell,
        path::{Path, PathBuf},
    };

    fn key(n: u32) -> StageKey {
        StageKey::Pane(PaneId::for_test(n))
    }

    fn target(path: &str) -> WriteTarget {
        WriteTarget {
            real: PathBuf::from("/home/u/proj").join(path),
            build_file: false,
        }
    }

    /// Stages one new file in the batch of `key`.
    fn put(stages: &mut Stages, key: StageKey, path: &str) -> BatchId {
        let batch = stages.stage(key).unwrap();
        batch
            .stage_file(&target(path), "x\n", Existing::Absent)
            .unwrap();
        batch.id()
    }

    /// A disk where every name is free and every write succeeds; it records
    /// the writes it was asked for.
    #[derive(Default)]
    struct Rec {
        log: RefCell<Vec<String>>,
        drift: bool,
    }

    impl WriteOps for Rec {
        fn probe(&self, _: &Path) -> Result<Option<Vec<u8>>, WriteError> {
            if self.drift {
                return Err(WriteError::Drifted);
            }
            Ok(None)
        }
        fn make_dir(&self, real: &Path) -> Result<(), WriteError> {
            self.log
                .borrow_mut()
                .push(format!("mkdir {}", real.display()));
            Ok(())
        }
        fn create_file(&self, real: &Path, _: &[u8]) -> Result<(), WriteError> {
            self.log
                .borrow_mut()
                .push(format!("create {}", real.display()));
            Ok(())
        }
        fn replace_file(&self, _: &Path, _: &[u8], _: &[u8]) -> Result<(), WriteError> {
            Ok(())
        }
        fn delete_file(&self, _: &Path, _: &[u8]) -> Result<(), WriteError> {
            Ok(())
        }
        fn delete_dir(&self, _: &Path) -> Result<(), WriteError> {
            Ok(())
        }
    }

    #[test]
    fn a_key_holds_one_batch_and_staging_again_reaches_the_same_one() {
        let mut stages = Stages::default();
        let first = put(&mut stages, key(1), "a.txt");
        let second = put(&mut stages, key(1), "b.txt");
        assert_eq!(first, second);
        assert_eq!(stages.batch(key(1)).unwrap().entries().len(), 2);
        assert!(stages.batch(key(2)).is_none());
    }

    #[test]
    fn two_keys_hold_two_independent_batches() {
        let mut stages = Stages::default();
        let a = put(&mut stages, key(1), "a.txt");
        let b = put(&mut stages, key(2), "b.txt");
        assert_ne!(a, b);
        stages.discard(a).unwrap();
        assert!(stages.batch(key(1)).is_none());
        assert_eq!(stages.batch(key(2)).unwrap().entries().len(), 1);
    }

    #[test]
    fn seal_proposes_the_batch_and_an_empty_one_is_refused() {
        let mut stages = Stages::default();
        assert_eq!(stages.seal(key(1)), Err(StageError::Empty));
        stages.stage(key(1)).unwrap();
        assert_eq!(stages.seal(key(1)), Err(StageError::Empty));
        let id = put(&mut stages, key(1), "a.txt");
        assert_eq!(stages.seal(key(1)), Ok(id));
        assert_eq!(stages.batch(key(1)).unwrap().state(), State::Proposed);
        let batch = stages.stage(key(1)).unwrap();
        let again = batch.stage_file(&target("b.txt"), "y\n", Existing::Absent);
        assert_eq!(again, Err(StageError::Proposed));
    }

    #[test]
    fn apply_writes_the_proposed_batch_once_and_closes_it() {
        let mut stages = Stages::default();
        let id = put(&mut stages, key(1), "a.txt");
        stages.seal(key(1)).unwrap();
        let disk = Rec::default();
        let result = stages.apply_with(id, &disk).unwrap();
        assert!(matches!(result, ApplyResult::Applied(ref c) if c.len() == 1));
        assert_eq!(*disk.log.borrow(), ["create /home/u/proj/a.txt"]);
        assert!(stages.batch(key(1)).is_none());
        // The same answer again is stale: nothing is written twice.
        assert_eq!(stages.apply_with(id, &disk).err(), Some(StageError::Stale));
        assert_eq!(disk.log.borrow().len(), 1);
    }

    #[test]
    fn apply_refuses_a_batch_that_was_not_proposed_and_writes_nothing() {
        let mut stages = Stages::default();
        let id = put(&mut stages, key(1), "a.txt");
        let disk = Rec::default();
        assert_eq!(
            stages.apply_with(id, &disk).err(),
            Some(StageError::NotProposed)
        );
        assert!(disk.log.borrow().is_empty());
        assert_eq!(stages.batch(key(1)).unwrap().state(), State::Staging);
    }

    #[test]
    fn a_drifted_apply_leaves_the_batch_proposed_so_the_user_can_discard_it() {
        let mut stages = Stages::default();
        let id = put(&mut stages, key(1), "a.txt");
        stages.seal(key(1)).unwrap();
        let disk = Rec {
            drift: true,
            ..Rec::default()
        };
        let result = stages.apply_with(id, &disk).unwrap();
        assert!(matches!(result, ApplyResult::Drifted(_)));
        assert!(disk.log.borrow().is_empty());
        assert_eq!(stages.batch(key(1)).unwrap().state(), State::Proposed);
        assert_eq!(stages.discard(id), Ok(()));
        assert!(stages.batch(key(1)).is_none());
    }

    #[test]
    fn discard_drops_a_staging_or_a_proposed_batch_by_its_id() {
        let mut stages = Stages::default();
        let staging = put(&mut stages, key(1), "a.txt");
        assert_eq!(stages.discard(staging), Ok(()));
        assert!(stages.batch(key(1)).is_none());
        let proposed = put(&mut stages, key(1), "a.txt");
        stages.seal(key(1)).unwrap();
        assert_eq!(stages.discard(proposed), Ok(()));
        assert!(stages.batch(key(1)).is_none());
        assert_eq!(stages.discard(proposed), Err(StageError::Stale));
    }

    // Obligation from the Wc review: a generation is never reused, so an
    // answer for a gone batch cannot hit the new batch of the same key.
    #[test]
    fn an_old_batch_id_is_refused_by_the_new_batch_of_the_same_key() {
        let mut stages = Stages::default();
        let old = put(&mut stages, key(1), "a.txt");
        stages.seal(key(1)).unwrap();
        stages.discard(old).unwrap();
        let new = put(&mut stages, key(1), "b.txt");
        stages.seal(key(1)).unwrap();
        assert_eq!(old.key, new.key);
        assert!(new.generation > old.generation);
        let disk = Rec::default();
        assert_eq!(stages.apply_with(old, &disk).err(), Some(StageError::Stale));
        assert_eq!(stages.discard(old), Err(StageError::Stale));
        assert!(disk.log.borrow().is_empty());
        // The new batch took none of it.
        assert_eq!(stages.batch(key(1)).unwrap().state(), State::Proposed);
        assert!(stages.apply_with(new, &disk).is_ok());
    }

    #[test]
    fn generations_grow_across_applied_batches_and_across_keys() {
        let mut stages = Stages::default();
        let mut seen = Vec::new();
        for n in 0..3 {
            let id = put(&mut stages, key(n % 2), "a.txt");
            stages.seal(key(n % 2)).unwrap();
            stages.apply_with(id, &Rec::default()).unwrap();
            seen.push(id.generation);
        }
        assert!(seen.windows(2).all(|w| w[0] < w[1]), "{seen:?}");
    }

    #[test]
    fn a_counter_at_its_end_refuses_a_new_batch_instead_of_wrapping() {
        let mut stages = Stages {
            next: u64::MAX,
            ..Stages::default()
        };
        assert!(stages.stage(key(1)).is_err());
        assert!(stages.batch(key(1)).is_none());
        stages.next = u64::MAX - 1;
        let id = stages.stage(key(1)).unwrap().id();
        assert_eq!(id.generation, u64::MAX - 1);
        assert!(stages.stage(key(2)).is_err());
    }

    #[test]
    fn a_pane_close_discards_the_batch_and_says_whether_there_was_one() {
        let mut stages = Stages::default();
        assert!(!stages.discard_key(key(1)));
        stages.stage(key(1)).unwrap();
        assert!(!stages.discard_key(key(1)), "an empty batch is no loss");
        let id = put(&mut stages, key(1), "a.txt");
        assert!(stages.discard_key(key(1)));
        assert!(stages.batch(key(1)).is_none());
        assert_eq!(stages.discard(id), Err(StageError::Stale));
    }
}
