//! What a turn used. Engine-reported only: counts, a timing and, when the
//! engine says so, dollars. A value the engine did not report is `None`,
//! never 0. No message text lives here.

use serde_json::Value;

use crate::core::ai::sanitize::clean;

/// Longest model id kept, in characters.
const MODEL_CAP: usize = 80;
/// Millionths of a dollar in a dollar.
const MICROS: f64 = 1_000_000.0;

/// Where a dollar figure comes from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CostSource {
    /// The engine's own list price for the turn.
    EngineList,
    /// A gateway that bills (reserved).
    Gateway,
}

impl CostSource {
    /// The id the store keeps.
    pub fn id(self) -> &'static str {
        match self {
            Self::EngineList => "engine_list",
            Self::Gateway => "gateway",
        }
    }
}

/// The usage of one turn, or of one request of it. Token buckets are
/// disjoint: `input` is the fresh prompt only, the cache has its own buckets
/// and `reasoning` is part of `output`.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct TurnUsage {
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cache_read: Option<u64>,
    pub cache_write: Option<u64>,
    pub reasoning: Option<u64>,
    /// Millionths of a dollar, as the engine reported them.
    pub cost_micros: Option<u64>,
    pub cost_source: Option<CostSource>,
    pub duration_ms: Option<u64>,
    /// A sanitized model id.
    pub model: Option<String>,
    /// Some of the turn is not in the sums: a request of it reported no
    /// usage, or the turn ended before its end.
    pub partial: bool,
    /// The engine reported more than one model (sub-agents, side models), so
    /// `model` names none of them.
    pub multi_model: bool,
}

/// Adds two optional counts: `None` only when neither side has one.
fn add(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(0).saturating_add(b.unwrap_or(0))),
    }
}

impl TurnUsage {
    /// Adds the usage of another request of the same turn. A field only one
    /// side reported keeps its value; a field neither reported stays `None`.
    /// A field gap is no data, not a partial turn: `partial` is about whole
    /// requests (see [`UsageFold`]).
    pub fn merge(&mut self, other: &TurnUsage) {
        self.partial |= other.partial;
        self.input = add(self.input, other.input);
        self.output = add(self.output, other.output);
        self.cache_read = add(self.cache_read, other.cache_read);
        self.cache_write = add(self.cache_write, other.cache_write);
        self.reasoning = add(self.reasoning, other.reasoning);
        self.cost_micros = add(self.cost_micros, other.cost_micros);
        self.duration_ms = add(self.duration_ms, other.duration_ms);
        self.cost_source = self.cost_source.or(other.cost_source);
        self.multi_model |= other.multi_model
            || matches!((&self.model, &other.model), (Some(a), Some(b)) if a != b);
        if self.multi_model {
            self.model = None;
        } else if self.model.is_none() {
            self.model.clone_from(&other.model);
        }
    }

    /// Nothing was used: every count and the cost are 0 or missing. A failed
    /// request reports this, and it is not a turn worth a record.
    pub fn idle(&self) -> bool {
        [
            self.input,
            self.output,
            self.cache_read,
            self.cache_write,
            self.reasoning,
            self.cost_micros,
        ]
        .iter()
        .all(|v| v.unwrap_or(0) == 0)
    }

    /// This usage if it holds any count or cost, else `None`: an object with
    /// nothing reported is no data, not a record of `None`s.
    pub fn reported(self) -> Option<Self> {
        let any = [
            self.input,
            self.output,
            self.cache_read,
            self.cache_write,
            self.reasoning,
            self.cost_micros,
        ]
        .iter()
        .any(Option::is_some);
        any.then_some(self)
    }
}

/// The usage of one run, waiting for the store. `role` says what ran: `chat`,
/// `compaction` or `summary`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Pending {
    pub role: &'static str,
    pub engine: String,
    pub usage: TurnUsage,
}

/// What a compaction or summary run used so far. Their model calls are real
/// spend the chat turns do not show, so each ends as a row of its own.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SideRun {
    role: &'static str,
    engine: String,
    model: Option<String>,
    usage: Option<TurnUsage>,
}

impl SideRun {
    pub fn new(role: &'static str, engine: &str, model: Option<&str>) -> Self {
        Self {
            role,
            engine: engine.to_owned(),
            model: model.and_then(model_id),
            usage: None,
        }
    }

    /// A `Usage` event of the run.
    pub fn add(&mut self, mut usage: TurnUsage) {
        if usage.model.is_none() && !usage.multi_model {
            usage.model.clone_from(&self.model);
        }
        match self.usage.as_mut() {
            Some(sum) => sum.merge(&usage),
            None => self.usage = Some(usage),
        }
    }

    /// The run ended; `cut` when it did not end well. `None` when it reported
    /// no usage: no data writes no row.
    pub fn end(self, cut: bool) -> Option<Pending> {
        let mut usage = self.usage?;
        usage.partial |= cut;
        Some(Pending {
            role: self.role,
            engine: self.engine,
            usage,
        })
    }
}

/// Folds the requests of one turn into one value.
#[derive(Default)]
pub struct UsageFold {
    requests: usize,
    reported: usize,
    sum: Option<TurnUsage>,
}

impl UsageFold {
    /// One request ended, with its usage if it reported any.
    pub fn add(&mut self, usage: Option<TurnUsage>) {
        self.requests += 1;
        let Some(usage) = usage else { return };
        self.reported += 1;
        match self.sum.as_mut() {
            Some(sum) => sum.merge(&usage),
            None => self.sum = Some(usage),
        }
    }

    /// The turn's usage: `None` when no request reported any. It is partial
    /// when a request reported nothing, or when the turn was `cut` (an error
    /// or the tool limit) after usage had been reported.
    pub fn finish(self, cut: bool) -> Option<TurnUsage> {
        let mut sum = self.sum?;
        sum.partial |= cut || self.reported < self.requests;
        Some(sum)
    }
}

/// A whole, non-negative count. Anything else is no data.
pub fn count(value: &Value) -> Option<u64> {
    value.as_u64()
}

/// The fresh part of a prompt count that includes cached tokens. Saturating:
/// a cached count over the total is the engine's inconsistency, not a debt.
pub fn net_input(total: Option<u64>, cached: Option<u64>) -> Option<u64> {
    total.map(|t| t.saturating_sub(cached.unwrap_or(0)))
}

/// A dollar figure as millionths, rounded. Negative, non-finite, non-number
/// and out-of-range values are no data.
pub fn micros(value: &Value) -> Option<u64> {
    let usd = value.as_f64().filter(|v| v.is_finite() && *v >= 0.0)?;
    let scaled = (usd * MICROS).round();
    (scaled < u64::MAX as f64).then_some(scaled as u64)
}

/// A model id as one safe, short token. Empty is no data.
pub fn model_id(raw: &str) -> Option<String> {
    // A managed llama-server is named by its model file: keep the file stem,
    // never a path from the user's disk.
    let raw = if raw.starts_with(['/', '~']) || raw.contains('\\') || raw.ends_with(".gguf") {
        let name = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
        name.strip_suffix(".gguf").unwrap_or(name)
    } else {
        raw
    };
    let id: String = clean(raw).trim().chars().take(MODEL_CAP).collect();
    (!id.is_empty()).then_some(id)
}

#[cfg(test)]
mod tests;
