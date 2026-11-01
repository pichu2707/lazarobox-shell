//! One worker: what it is started from and how it ends.

use std::time::Duration;

use super::Toolset;
use crate::core::ai::chat::engine::Usage;
use crate::core::ai::config::Task;

/// What a worker is started from. The engine and model are those in effect
/// at spawn time; both stay empty when the engine was unavailable.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WorkerSpec {
    pub task: Task,
    pub prompt: String,
    pub engine: String,
    pub model: Option<String>,
    pub toolset: Toolset,
    pub deadline: Duration,
}

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
