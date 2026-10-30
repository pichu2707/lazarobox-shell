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
        orchestra::{RunId, WorkerId, changes::Change, tools::OrchCall},
        tools::paths::SecretRoots,
    },
    runtime::mcp::Outcome,
};

/// The run manager, as the host calls it.
pub trait Orchestra: Send + Sync {
    /// Runs one orchestration call of the orchestrator turn. What it returns
    /// is the model's text; the host adds the row and bounds a `wait`.
    fn call(&self, call: OrchCall) -> BoxFuture<'static, Outcome>;

    /// The real path a worker of `run` may read without asking: the run's
    /// read scope (`ReadScope::allows`) judged with `secrets`, a snapshot the
    /// host builds for this one call. `None` (outside the scope, a denied or
    /// expired scope, an unknown run) makes the host ask.
    fn allows(
        &self,
        run: RunId,
        secrets: &SecretRoots,
        origin: &Path,
        input: &str,
    ) -> Option<PathBuf>;

    /// Whether a worker of `run` may stage a write to `real`, the canonical
    /// target the host resolved (`ReadScope::allows_write`). `false` (the
    /// default: outside the scope, a denied or ended scope, an unknown run)
    /// refuses the write as a path the policy refuses; it is never asked.
    fn allows_write(&self, _run: RunId, _real: &Path) -> bool {
        false
    }

    /// The host staged `change` for `worker` of `run`: its change ledger
    /// records it (S7). Nothing is written by it.
    fn staged(&self, _run: RunId, _worker: WorkerId, _change: Change) {}

    /// The user's batch review applied a batch holding `paths` that `worker`
    /// of `run` proposed.
    fn applied(&self, _run: RunId, _worker: WorkerId, _paths: &[String]) {}
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
