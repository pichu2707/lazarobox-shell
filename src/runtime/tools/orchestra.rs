//! What the tool host sees of the orchestrator: the run manager behind a
//! trait, so the host is tested with a fake and the run manager (S1d-2)
//! plugs in without the host changing.
//!
//! An orchestrator turn carries an [`OrchHandle`]: its orchestration calls go
//! there, and without one they are unknown tools (D3). A worker turn carries
//! a [`WorkerScope`]: its reads are judged against the run's read scope
//! through the same handle (D6).

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use futures::future::BoxFuture;

use crate::{
    core::ai::{
        orchestra::{
            RunId, WorkerId, WorkerKey,
            changes::Change,
            commands::Ran,
            limit::Count,
            review::{self, Assessment},
            tools::OrchCall,
        },
        tools::paths::SecretRoots,
    },
    runtime::{mcp::Outcome, tools::pending::Waiting},
};

/// What the consent ask came back with.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Answered {
    Granted,
    /// The user's `n`.
    Declined,
    /// Nobody answered: the ask was cancelled or expired, `reason` says how.
    Unanswered(String),
    /// Nobody was asked: the author is cheap and its change is always
    /// reviewed (D21).
    Mandatory,
}

/// Why a path was not reviewed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Late {
    /// A path that was not in the set assessed or consented to.
    Staged,
    /// A reviewed path whose content changed after it was reviewed.
    Changed,
}

/// The run manager, as the host calls it.
pub trait Orchestra: Send + Sync {
    /// Runs one orchestration call of the orchestrator turn. What it returns
    /// is the model's text; the host adds the row and bounds a `wait`.
    fn call(&self, call: OrchCall) -> BoxFuture<'static, Outcome>;

    /// The real path `worker` of `run` may read without asking: the run's
    /// read scope (`ReadScope::allows`) judged with `secrets`, a snapshot the
    /// host builds for this one call. The grant is the worker's own and ends
    /// with it: `None` (outside the scope, a denied or expired scope, a
    /// worker that is not running, an unknown run) makes the host ask.
    fn allows(
        &self,
        run: RunId,
        worker: WorkerId,
        secrets: &SecretRoots,
        origin: &Path,
        input: &str,
    ) -> Option<PathBuf>;

    /// Whether `worker` of `run` may stage a write to `real`, the canonical
    /// target the host resolved (`ReadScope::allows_write`). `false` (the
    /// default: outside the scope, a denied or ended scope, a worker that is
    /// not running, an unknown run) refuses the write as a path the policy
    /// refuses; it is never asked.
    fn allows_write(&self, _run: RunId, _worker: WorkerId, _real: &Path) -> bool {
        false
    }

    /// How many workers `run` has so far, for the `n/m` of a worker's label.
    /// `0` (the default) when it is not known: the label then counts only up
    /// to the asking worker.
    fn workers(&self, _run: RunId) -> usize {
        0
    }

    /// The host staged `change` for `worker` of `run`: its change ledger
    /// records it (S7). Nothing is written by it.
    fn staged(&self, _run: RunId, _worker: WorkerId, _change: Change) {}

    /// `worker` of `run` called a command that reached the user's ask, and
    /// it ended as `ran` says (W6): `collect` shows it to the coordinator.
    fn ran(&self, _run: RunId, _worker: WorkerId, _ran: Ran) {}

    /// `worker` of `run` called `needs_specification` with `what` (W8c). The
    /// run records it as the worker's closing. `false` when it was not
    /// recorded (the worker is not running, was not offered the tool, or had
    /// already closed).
    fn clarify(&self, _run: RunId, _worker: WorkerId, _what: &str) -> bool {
        false
    }

    /// Whether `worker` of `run` has called `needs_specification`: its write
    /// and command tools are then refused.
    fn clarifying(&self, _run: RunId, _worker: WorkerId) -> bool {
        false
    }

    /// The risk of the change set `worker` of `run` is about to propose,
    /// with whatever the orchestrator raised for the run (S7b, D16). The
    /// default is the path rules alone.
    fn assess(&self, _run: RunId, changes: &[Change]) -> Assessment {
        review::assess(changes, None)
    }

    /// Runs the receipt-driven review of `changes` (S7b), the user's answer
    /// to the consent being `answered`. Returns when it ended; it never
    /// approves anything, and the worker's batch goes to the user's review
    /// whatever it found. `Ok` holds the set as it was last reviewed: the
    /// hashes of the files a correction changed and the review checked
    /// included, so the host can tell a file re-staged since. `Err` is why it
    /// could not start at all: the host shows it and the batch goes on
    /// unreviewed. The default reviews nothing and returns `changes`.
    fn review(
        &self,
        _run: RunId,
        _worker: WorkerId,
        changes: Vec<Change>,
        _assessment: Assessment,
        _answered: Answered,
    ) -> BoxFuture<'static, Result<Vec<Change>, String>> {
        Box::pin(async { Ok(changes) })
    }

    /// `changes` were staged after the set `worker` consented to or was
    /// assessed on, or changed after it was reviewed (`kind`), and so were
    /// not reviewed. `reviewed` is whether a review ran for the rest. The
    /// default keeps nothing.
    fn late(
        &self,
        _run: RunId,
        _worker: WorkerId,
        _changes: Vec<Change>,
        _assessment: Assessment,
        _reviewed: bool,
        _kind: Late,
    ) {
    }

    /// A review worker could not read `path`, which is outside the run's
    /// scope: its review cannot count as clean.
    fn unread(&self, _run: RunId, _worker: WorkerId, _path: &str) {}

    /// Whether `worker` of `run` is a cheap author (design D20), by the
    /// engine and model it was spawned with, never the menu now: its staged
    /// change is always reviewed, with no consent ask (D21). `false` (the
    /// default) for an unknown run or worker.
    fn cheap(&self, _run: RunId, _worker: WorkerId) -> bool {
        false
    }

    /// Holds `worker`'s own count of asks that wait for the user (its route's,
    /// D5) until the guard is dropped: its deadline does not run while the
    /// consent and the model review of its batch last (D6). `None` (the
    /// default) when the worker has no route here.
    fn pause(&self, _run: RunId, _worker: WorkerId) -> Option<Waiting> {
        None
    }

    /// Whether `worker` is the correction of a review: it stages fixes but
    /// the worker whose change is under review proposes the batch.
    fn corrects(&self, _run: RunId, _worker: WorkerId) -> bool {
        false
    }

    /// The line the user's batch review shows about the receipt of the review
    /// that covers `current`, the change set being sealed (S7b): lazarobox's
    /// own words and counts. `None` when no review of `worker` describes
    /// that set (none ran, or the set is another batch). It informs; it
    /// never approves.
    fn receipt_line(&self, _run: RunId, _worker: WorkerId, _current: &[Change]) -> Option<String> {
        None
    }

    /// The user's batch review applied a batch holding `paths` that `worker`
    /// of `run` proposed.
    fn applied(&self, _run: RunId, _worker: WorkerId, _paths: &[String]) {}

    /// The files of the batch `worker` proposes (`counts`, per project path)
    /// that go over the line limit with what it applied before (W9). Only a
    /// `simple` worker has a cap: the default lists nothing. `None` when the
    /// run cannot be reached: the lines cannot be counted, and the host must
    /// not propose the batch unasked.
    fn over_limit(
        &self,
        _run: RunId,
        _worker: WorkerId,
        _counts: Vec<(String, Count)>,
    ) -> Option<Vec<(String, Count)>> {
        Some(Vec::new())
    }

    /// `worker` chose to split its brief: `files` are the files over the
    /// limit. The run records it as the worker's closing (W9b).
    fn split(&self, _run: RunId, _worker: WorkerId, _files: Vec<String>) {}
}

/// The run manager of a turn.
pub type OrchHandle = Arc<dyn Orchestra>;

/// What makes a turn a worker's: the run whose scope its reads (and writes)
/// are judged against, the worker whose ledger its staged files go to, and
/// the label its asks show.
#[derive(Clone)]
pub struct WorkerScope {
    pub handle: OrchHandle,
    pub run: RunId,
    pub worker: WorkerId,
    /// `<kind> <engine>><model>`, already sanitized for display
    /// (`approval::worker_label`).
    pub label: String,
}

impl WorkerScope {
    /// The session the worker's asks belong to.
    pub fn key(&self) -> WorkerKey {
        WorkerKey {
            run: self.run,
            worker: self.worker,
        }
    }

    /// The label its asks show: `n/m <kind> <engine>><model>`, worker `n` of
    /// the `m` the run has now.
    pub fn ask_label(&self) -> String {
        let of = self.handle.workers(self.run).max(self.worker.0 as usize);
        format!("{}/{of} {}", self.worker.0, self.label)
    }
}
