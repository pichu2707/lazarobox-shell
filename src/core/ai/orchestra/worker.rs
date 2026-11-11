//! One worker: what it is started from and how it ends.

use std::time::Duration;

use super::Toolset;
use super::brief::Brief;
use crate::core::ai::chat::engine::Usage;
use crate::core::ai::config::Task;
use crate::core::ai::tools::registry::{Family, FamilySet};

/// What a worker is started from. The engine and model are those in effect
/// at spawn time; both stay empty when the engine was unavailable.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WorkerSpec {
    pub task: Task,
    pub prompt: String,
    pub engine: String,
    pub model: Option<String>,
    pub toolset: Toolset,
    /// The families the worker is offered: its toolset under the menu's
    /// ceiling at spawn time (D17).
    pub families: FamilySet,
    pub deadline: Duration,
    /// The brief a `simple` worker was started from (W8b); `None` for every
    /// other worker. `prompt` is built from it.
    pub brief: Option<Brief>,
}

impl WorkerSpec {
    /// The files the worker may write, when it has a brief.
    pub fn outputs(&self) -> Option<&[String]> {
        self.brief.as_ref().map(Brief::outputs)
    }

    /// Whether the worker may stage changes: what a worker kind is compared
    /// by, not its name (D25).
    pub fn writes(&self) -> bool {
        self.families.contains(Family::Write)
    }

    /// Whether the worker's tool session needs a batch key: it stages
    /// (Write), or runs commands (Exec), whose refusal and output sink are
    /// keyed by its batch (D19).
    pub fn keys_a_batch(&self) -> bool {
        self.writes() || self.families.contains(Family::Exec)
    }
}

/// How lazarobox records that a worker's work ended on a call of its own
/// (D3), not on text: a `simple` worker that asked for a clearer brief.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Closing {
    /// `needs_specification`: the worker's question, cleaned to one line.
    Clarify(String),
}

/// The longest question kept, in characters.
pub const CLARIFY_MAX: usize = 200;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WorkerStatus {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
    TimedOut,
    Refused,
}

impl WorkerStatus {
    /// The id the `wait` result and the worker row use.
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
            Self::Refused => "refused",
        }
    }
}

/// Why a worker failed. The adapters' classifier arrives in S2.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrorClass {
    RateLimit,
    Conflict,
    Auth,
    Quota,
    Timeout,
    JailRefusal,
    /// The worker was refused before it started: its engine or model is not
    /// available (not installed, not downloaded, none assigned, no jail).
    Setup,
    Other,
}

impl ErrorClass {
    /// The spec's id of the class, which a log line may carry.
    pub fn id(self) -> &'static str {
        match self {
            Self::RateLimit => "rate_limit",
            Self::Conflict => "conflict",
            Self::Auth => "auth",
            Self::Quota => "quota",
            Self::Timeout => "timeout",
            Self::JailRefusal => "jail_refusal",
            Self::Setup => "setup",
            Self::Other => "other",
        }
    }
}

/// How a worker ended. `text` is raw: `collect` wraps and caps it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WorkerResult {
    pub status: WorkerStatus,
    pub text: String,
    pub elapsed: Duration,
    /// Set when the status is `Failed`, `TimedOut` or `Refused`.
    pub error: Option<(ErrorClass, String)>,
    /// The engine and model the worker started with.
    pub engine: String,
    pub model: Option<String>,
    /// What the worker's turns reported, summed; `None` when its engine
    /// reported nothing (never an estimate).
    pub usage: Option<Usage>,
}
