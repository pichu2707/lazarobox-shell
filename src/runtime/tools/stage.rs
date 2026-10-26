//! The staged batches of the runtime: one per [`StageKey`], in memory only,
//! never saved, so nothing survives a restart. The store wraps the pure
//! [`Batch`] and adds what a batch cannot check by itself: a generation that
//! is never reused, so an answer for a gone batch cannot hit a new one.
//!
//! Applying goes through [`apply`]; this file holds no write API.

use std::{
    collections::{HashMap, hash_map::Entry},
    path::Path,
};

use super::{
    apply::{self, ApplyResult, Disk, WriteOps},
    staged::StagedView,
};
use crate::core::ai::{
    chat::review::{Badge, BatchReview, ReviewDoc},
    tools::{
        paths::SecretRoots,
        stage::{Batch, BatchId, Outcome, StageError, StageKey, State},
    },
};
use crate::core::layout::PaneId;

/// The generation counter ran out: no new batch can be made.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Exhausted;

/// Why [`Stages::stage_with`] did not stage.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StageFailed<E = StageError> {
    Exhausted,
    Batch(E),
}

/// The disk read of a path could not be made or is not allowed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Unreadable;

/// What the file `real` holds now, for staging: `None` when the name is free.
/// The same checked read the apply preflight makes, so staging and apply judge
/// a path alike. It reads and never writes.
pub fn disk_bytes(roots: &SecretRoots, real: &Path) -> Result<Option<Vec<u8>>, Unreadable> {
    Disk::new(roots).probe(real).map_err(|_| Unreadable)
}

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
        self.open(key).map(|(batch, _)| batch)
    }

    /// Runs `staging` on the batch of `key`. When it fails and this call made
    /// the batch, the empty batch is removed again; a batch that was already
    /// there stays as it was.
    pub fn stage_with<T, E>(
        &mut self,
        key: StageKey,
        staging: impl FnOnce(&mut Batch) -> Result<T, E>,
    ) -> Result<T, StageFailed<E>> {
        let (batch, created) = self.open(key).map_err(|_| StageFailed::Exhausted)?;
        let result = staging(batch);
        if result.is_err() && created {
            self.batches.remove(&key);
        }
        result.map_err(StageFailed::Batch)
    }

    /// The batch of `key` and whether this call made it.
    fn open(&mut self, key: StageKey) -> Result<(&mut Batch, bool), Exhausted> {
        match self.batches.entry(key) {
            Entry::Occupied(slot) => Ok((slot.into_mut(), false)),
            Entry::Vacant(slot) => {
                let generation = self.next;
                self.next = generation.checked_add(1).ok_or(Exhausted)?;
                Ok((slot.insert(Batch::new(BatchId { key, generation })), true))
            }
        }
    }

    /// What each pane with a batch shows: the entries it holds, an orphan
    /// when `live` says no turn of the pane runs. An empty batch shows nothing.
    pub fn badges(&self, live: impl Fn(PaneId) -> bool) -> HashMap<PaneId, Badge> {
        let mut out = HashMap::new();
        for (&StageKey::Pane(pane), batch) in &self.batches {
            let n = batch.entries().len();
            if n > 0 {
                let orphan = batch.orphan(live(pane)).is_some();
                out.insert(
                    pane,
                    if orphan {
                        Badge::Orphan(n)
                    } else {
                        Badge::Staged(n)
                    },
                );
            }
        }
        out
    }

    pub fn batch(&self, key: StageKey) -> Option<&Batch> {
        self.batches.get(&key)
    }

    /// A snapshot of what `key` has staged, for the read tools. Empty when the
    /// key has no batch.
    pub fn view(&self, key: StageKey) -> StagedView {
        self.batches
            .get(&key)
            .map_or_else(StagedView::default, StagedView::from_batch)
    }

    /// Closes staging for the batch `id` and hands it to the user. A stale
    /// id, from a gone batch of the same key, is refused.
    pub fn seal(&mut self, id: BatchId) -> Result<BatchId, StageError> {
        self.current(id)?.propose()
    }

    /// The orphan of `key` for the user (see [`Batch::review_orphan`]): a
    /// batch the turn never proposed is sealed now, a proposed one is shown
    /// as it is. A live turn, an empty batch or no batch is an error.
    pub fn review(
        &mut self,
        key: StageKey,
        turn_live: bool,
    ) -> Result<(BatchId, BatchReview), StageError> {
        let batch = self.batches.get_mut(&key).ok_or(StageError::NotOrphan)?;
        let id = batch.review_orphan(turn_live)?;
        Ok((id, BatchReview::new(ReviewDoc::build(batch))))
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

    /// Drops whatever `key` holds (a pane close or an app quit). How many
    /// entries were lost; `None` when nothing worth a notice was: an empty
    /// batch is no loss.
    pub fn discard_key(&mut self, key: StageKey) -> Option<usize> {
        let lost = self.batches.remove(&key)?.entries().len();
        (lost > 0).then_some(lost)
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
    fn badges_count_the_entries_and_call_an_orphan_only_when_no_turn_runs() {
        use crate::core::ai::chat::review::Badge;
        let mut stages = Stages::default();
        assert!(stages.badges(|_| false).is_empty());
        put(&mut stages, key(1), "a.txt");
        put(&mut stages, key(1), "b.txt");
        let id = put(&mut stages, key(2), "c.txt");
        // An empty batch is nothing to show.
        stages.stage(key(3)).unwrap();
        let live = |pane: PaneId| pane == PaneId::for_test(1);
        let badges = stages.badges(live);
        assert_eq!(badges.len(), 2, "{badges:?}");
        assert_eq!(badges[&PaneId::for_test(1)], Badge::Staged(2));
        assert_eq!(badges[&PaneId::for_test(2)], Badge::Orphan(1));
        // A sealed batch whose ask is gone is an orphan too; answered, it goes.
        stages.seal(id).unwrap();
        assert_eq!(
            stages.badges(|_| false)[&PaneId::for_test(2)],
            Badge::Orphan(1)
        );
        stages.discard(id).unwrap();
        assert!(!stages.badges(|_| false).contains_key(&PaneId::for_test(2)));
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
        let gone = BatchId {
            key: key(1),
            generation: 0,
        };
        assert_eq!(stages.seal(gone), Err(StageError::Stale));
        let empty = stages.stage(key(1)).unwrap().id();
        assert_eq!(stages.seal(empty), Err(StageError::Empty));
        let id = put(&mut stages, key(1), "a.txt");
        assert_eq!(stages.seal(id), Ok(id));
        assert_eq!(stages.batch(key(1)).unwrap().state(), State::Proposed);
        let batch = stages.stage(key(1)).unwrap();
        let again = batch.stage_file(&target("b.txt"), "y\n", Existing::Absent);
        assert_eq!(again, Err(StageError::Proposed));
    }

    #[test]
    fn apply_writes_the_proposed_batch_once_and_closes_it() {
        let mut stages = Stages::default();
        let id = put(&mut stages, key(1), "a.txt");
        stages.seal(id).unwrap();
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
        stages.seal(id).unwrap();
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
        stages.seal(proposed).unwrap();
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
        stages.seal(old).unwrap();
        stages.discard(old).unwrap();
        let new = put(&mut stages, key(1), "b.txt");
        stages.seal(new).unwrap();
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
            stages.seal(id).unwrap();
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
        assert_eq!(stages.discard_key(key(1)), None);
        stages.stage(key(1)).unwrap();
        assert_eq!(
            stages.discard_key(key(1)),
            None,
            "an empty batch is no loss"
        );
        let id = put(&mut stages, key(1), "a.txt");
        put(&mut stages, key(1), "b.txt");
        assert_eq!(stages.discard_key(key(1)), Some(2));
        assert!(stages.batch(key(1)).is_none());
        assert_eq!(stages.discard(id), Err(StageError::Stale));
    }

    #[test]
    fn the_view_of_a_key_shows_its_live_batch_and_nothing_once_it_is_gone() {
        let mut stages = Stages::default();
        let id = put(&mut stages, key(1), "a.txt");
        put(&mut stages, key(2), "b.txt");
        let path = target("a.txt").real;
        assert_eq!(stages.view(key(1)).text(&path), Some("x\n"));
        assert_eq!(stages.view(key(2)).text(&path), None);
        assert_eq!(stages.view(key(3)).text(&path), None);
        stages.discard(id).unwrap();
        assert_eq!(stages.view(key(1)).text(&path), None);
    }

    // Review fix: seal is checked against the generation, like apply.
    #[test]
    fn an_old_id_cannot_seal_the_newer_staging_batch_of_the_same_key() {
        let mut stages = Stages::default();
        let old = put(&mut stages, key(1), "a.txt");
        stages.discard(old).unwrap();
        let new = put(&mut stages, key(1), "b.txt");
        assert_eq!(stages.seal(old), Err(StageError::Stale));
        assert_eq!(stages.batch(key(1)).unwrap().state(), State::Staging);
        assert_eq!(stages.seal(new), Ok(new));
    }

    // Review fix: a batch the failed call just made does not stay behind.
    #[test]
    fn a_failed_first_staging_leaves_no_empty_batch_but_an_old_batch_stays() {
        let mut stages = Stages::default();
        let failed = stages.stage_with(key(1), |_| Err::<(), _>(StageError::FileTooLarge));
        assert_eq!(failed, Err(StageFailed::Batch(StageError::FileTooLarge)));
        assert!(stages.batch(key(1)).is_none());
        put(&mut stages, key(2), "a.txt");
        let failed = stages.stage_with(key(2), |_| Err::<(), _>(StageError::FileTooLarge));
        assert_eq!(failed, Err(StageFailed::Batch(StageError::FileTooLarge)));
        assert_eq!(stages.batch(key(2)).unwrap().entries().len(), 1);
        // An empty batch that was already there is not this call's to remove.
        stages.stage(key(3)).unwrap();
        let _ = stages.stage_with(key(3), |_| Err::<(), _>(StageError::FileTooLarge));
        assert!(stages.batch(key(3)).is_some());
        // A success keeps its batch.
        assert_eq!(stages.stage_with(key(4), |_| Ok::<_, StageError>(7)), Ok(7));
        assert!(stages.batch(key(4)).is_some());
    }
}
