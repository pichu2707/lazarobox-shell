//! What a run stores in engram (agent-orchestrator D13, D17) and the small
//! state that decides, per run, whether the store works: the notes and their
//! topic keys, and the degraded mode. Pure: the app owns the effects.

use super::RunId;
use super::feature::FeatureDoc;
use super::ledger::{Handoff, Unit};
use super::steps::Step;
use crate::core::ai::memory::{kind::MemKind, port::Note, slug::ORCHESTRA_PREFIX};

/// The pane says this once when memory was off and the run turned it on.
pub const MEMORY_ON: &str = "memory on for this run";

/// Said once, with `NOT_PERSISTED`, when memory was never accepted by the
/// user: a run does not turn on a memory whose privacy note is unanswered.
pub const NEEDS_NOTE: &str = "handoffs need the memory privacy note accepted";

/// The pane says this once when the handoffs stay in the run's memory only.
pub const NOT_PERSISTED: &str = "handoffs not persisted";

/// One note a run stores and the stable key it is stored under.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Item {
    pub key: String,
    pub note: Note,
}

/// The key of a worker's handoff: a repeated handoff for the worker updates
/// the note.
fn worker_key(run: RunId, worker: u32) -> String {
    format!("{ORCHESTRA_PREFIX}{}/worker/{worker}", run.0)
}

/// The key of an ODD step's handoff (S5, D15).
fn step_key(run: RunId, step: Step) -> String {
    format!("{ORCHESTRA_PREFIX}{}/step/{}", run.0, step.label())
}

/// The key of a receipt-driven review's receipt (S7b): the `n`th review of the
/// worker's change; a receipt rebuilt after a late path updates the note.
fn receipt_key(run: RunId, worker: u32, n: u32) -> String {
    format!("{ORCHESTRA_PREFIX}{}/receipt/{worker}/{n}", run.0)
}

/// The key of the run's feature document (S5, D15).
fn feature_key(run: RunId) -> String {
    format!("{ORCHESTRA_PREFIX}{}/feature", run.0)
}

/// The key of the run's final ledger and summary.
fn ledger_key(run: RunId) -> String {
    format!("{ORCHESTRA_PREFIX}{}/ledger", run.0)
}

impl Item {
    /// The note of a handoff. The text is not redacted here: the save call
    /// redacts title and content like any note.
    pub fn of_handoff(handoff: &Handoff) -> Self {
        let (key, title, subject) = match handoff.unit {
            Unit::Worker(worker) => (
                worker_key(handoff.run, worker.0),
                format!("Run {} worker {} handoff", handoff.run.0, worker.0),
                format!("worker: {}", worker.0),
            ),
            Unit::Step(step) => (
                step_key(handoff.run, step),
                format!("Run {} step {} handoff", handoff.run.0, step.label()),
                format!("step: {}", step.label()),
            ),
            Unit::Receipt(worker, n) => (
                receipt_key(handoff.run, worker.0, n),
                format!("Run {} worker {} receipt {n}", handoff.run.0, worker.0),
                format!("receipt: worker {} review {n}", worker.0),
            ),
        };
        let content = format!(
            "run: {}\n{subject}\nkind: {}\nengine: {}\nstatus: {}\nverdict: {}\nreason: {}\n\
             done: {}\nleft: {}\ndecisions: {}",
            handoff.run.0,
            handoff.kind.id(),
            handoff.engine_model(),
            handoff.status.label(),
            handoff.verdict.label(),
            handoff.reason,
            handoff.done,
            handoff.left,
            handoff.decisions,
        );
        Self {
            key,
            note: Note {
                kind: MemKind::Architecture,
                title,
                content,
            },
        }
    }

    /// The note of the run's feature document (S5, D15): one per run, the
    /// same key whenever a correction rewrites it.
    pub fn of_feature(doc: &FeatureDoc) -> Self {
        Self {
            key: feature_key(doc.run()),
            note: Note {
                kind: MemKind::Architecture,
                title: format!("Run {} feature document", doc.run().0),
                content: doc.render(),
            },
        }
    }

    /// The note of the finished run: the orchestrator's summary, then its
    /// ledger as rendered.
    pub fn of_ledger(run: RunId, ledger: &str, summary: &str) -> Self {
        Self {
            key: ledger_key(run),
            note: Note {
                kind: MemKind::Architecture,
                title: format!("Run {} ledger", run.0),
                // The summary first: the save cuts a long text from the end.
                content: format!("Summary: {summary}\n\n{ledger}"),
            },
        }
    }
}

/// Where a run's saves stand.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Stage {
    /// The project is not known: items wait for it.
    Waiting,
    Ready(String),
    /// Engram cannot be used for this run; nothing more is sent.
    Degraded,
}

/// What the app must do after a step.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Action {
    Save {
        item: Item,
        project: String,
    },
    /// The handoffs now stay in memory only: say `NOT_PERSISTED`, once.
    Degrade,
}

/// The persistence of one run's handoffs.
#[derive(Debug)]
pub struct RunSave {
    stage: Stage,
    waiting: Vec<Item>,
    said: bool,
}

impl RunSave {
    /// A run whose project is `project` when already known.
    pub fn new(project: Option<String>) -> Self {
        Self {
            stage: project.map_or(Stage::Waiting, Stage::Ready),
            waiting: Vec::new(),
            said: false,
        }
    }

    /// Whether the project is still awaited.
    pub fn waiting(&self) -> bool {
        self.stage == Stage::Waiting
    }

    /// An item to store: saved at once, held until the project is known, or
    /// dropped (kept in the run's ledger only) when degraded.
    pub fn add(&mut self, item: Item) -> Vec<Action> {
        match &self.stage {
            Stage::Ready(project) => vec![Action::Save {
                item,
                project: project.clone(),
            }],
            Stage::Waiting => {
                self.waiting.push(item);
                Vec::new()
            }
            Stage::Degraded => Vec::new(),
        }
    }

    /// The project came (`Some`) or memory cannot be used (`None`).
    pub fn resolved(&mut self, project: Option<String>) -> Vec<Action> {
        if self.stage != Stage::Waiting {
            return Vec::new();
        }
        let Some(project) = project else {
            return self.degrade();
        };
        self.stage = Stage::Ready(project.clone());
        std::mem::take(&mut self.waiting)
            .into_iter()
            .map(|item| Action::Save {
                item,
                project: project.clone(),
            })
            .collect()
    }

    /// A save failed or got no answer: the run goes on in memory.
    pub fn failed(&mut self) -> Vec<Action> {
        self.degrade()
    }

    fn degrade(&mut self) -> Vec<Action> {
        self.stage = Stage::Degraded;
        self.waiting.clear();
        if std::mem::replace(&mut self.said, true) {
            Vec::new()
        } else {
            vec![Action::Degrade]
        }
    }
}
