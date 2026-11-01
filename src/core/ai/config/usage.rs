//! `[ai.usage]` and `[ai.usage.billing]` (ai-cost, configuration "Usage
//! keys"): what is shown about token use and how a reported dollar figure is
//! labelled.

use toml::{Table, Value};

use crate::core::ai::chat::usage_view::Billing;

/// The CLI engines that have a billing mode, in the order of their problems.
const BILLED: [&str; 3] = ["claude", "codex", "gemini"];

const BILLING_PROBLEM: &str = "expected \"subscription\" or \"per-token\"";

/// The usage settings. A threshold of `None` is off.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UsageConfig {
    line: bool,
    statusline: bool,
    turn_tokens: Option<u64>,
    day_tokens: Option<u64>,
    /// Millionths of a dollar, so the config stays `Eq`.
    day_usd_micros: Option<u64>,
    billing: [Billing; 3],
}

impl Default for UsageConfig {
    fn default() -> Self {
        Self {
            line: true,
            statusline: true,
            turn_tokens: None,
            day_tokens: None,
            day_usd_micros: None,
            billing: [Billing::Subscription; 3],
        }
    }
}

impl UsageConfig {
    /// The defaults with the per-answer line off: for tests that are about
    /// something else than the line.
    #[cfg(test)]
    pub fn without_line() -> Self {
        Self {
            line: false,
            ..Self::default()
        }
    }

    /// The per-answer usage line is shown.
    pub fn line(&self) -> bool {
        self.line
    }

    /// The status-bar usage segment is shown.
    pub fn statusline(&self) -> bool {
        self.statusline
    }

    pub fn set_line(&mut self, on: bool) {
        self.line = on;
    }

    pub fn set_statusline(&mut self, on: bool) {
        self.statusline = on;
    }

    /// The billing mode of the `at`-th CLI engine (claude, codex, gemini).
    pub fn billing_at(&self, at: usize) -> Billing {
        self.billing[at]
    }

    pub fn set_billing_at(&mut self, at: usize, mode: Billing) {
        self.billing[at] = mode;
    }

    pub fn turn_tokens(&self) -> Option<u64> {
        self.turn_tokens
    }

    pub fn day_tokens(&self) -> Option<u64> {
        self.day_tokens
    }

    pub fn day_usd_micros(&self) -> Option<u64> {
        self.day_usd_micros
    }

    /// The billing mode of `engine`: `None` for a local engine, which never
    /// shows dollars.
    pub fn billing(&self, engine: &str) -> Option<Billing> {
        BILLED
            .iter()
            .position(|id| *id == engine)
            .map(|at| self.billing[at])
    }
}

/// A whole number above 0.
fn read_count(table: &Table, key: &str, problems: &mut Vec<String>) -> Option<u64> {
    match table.get(key)? {
        Value::Integer(n) if *n > 0 => u64::try_from(*n).ok(),
        _ => {
            problems.push(format!("ai.usage.{key}: expected a whole number above 0"));
            None
        }
    }
}

fn read_flag(table: &Table, key: &str, problems: &mut Vec<String>) -> bool {
    match table.get(key) {
        None => true,
        Some(Value::Boolean(on)) => *on,
        Some(_) => {
            problems.push(format!("ai.usage.{key}: expected true or false"));
            true
        }
    }
}

/// A number above 0 (a TOML float or integer), as millionths of a dollar.
fn read_usd(table: &Table, key: &str, problems: &mut Vec<String>) -> Option<u64> {
    let dollars = match table.get(key)? {
        Value::Integer(n) if *n > 0 => Some(*n as f64),
        Value::Float(f) if f.is_finite() && *f > 0.0 => Some(*f),
        _ => None,
    };
    if dollars.is_none() {
        problems.push(format!("ai.usage.{key}: expected a number above 0"));
    }
    // Never 0 for a number above 0: a threshold of 0 would always fire.
    dollars.map(|d| ((d * 1_000_000.0).round() as u64).max(1))
}

fn read_billing(usage: &Table, problems: &mut Vec<String>) -> [Billing; 3] {
    let mut modes = [Billing::Subscription; 3];
    let table = match usage.get("billing") {
        None => return modes,
        Some(Value::Table(table)) => table,
        Some(_) => {
            problems.push("ai.usage.billing: expected a table".to_owned());
            return modes;
        }
    };
    for (at, id) in BILLED.iter().enumerate() {
        match table.get(*id) {
            None => {}
            Some(Value::String(mode)) if mode == "subscription" => {}
            Some(Value::String(mode)) if mode == "per-token" => modes[at] = Billing::PerToken,
            Some(_) => problems.push(format!("ai.usage.billing.{id}: {BILLING_PROBLEM}")),
        }
    }
    modes
}

/// Reads `[ai.usage]` out of the `[ai]` table. Each key falls back on its
/// own, with one problem.
pub(super) fn read(ai: &Table, problems: &mut Vec<String>) -> UsageConfig {
    let usage = match ai.get("usage") {
        None => return UsageConfig::default(),
        Some(Value::Table(usage)) => usage,
        Some(_) => {
            problems.push("ai.usage: expected a table".to_owned());
            return UsageConfig::default();
        }
    };
    UsageConfig {
        line: read_flag(usage, "line", problems),
        statusline: read_flag(usage, "statusline", problems),
        turn_tokens: read_count(usage, "turn_tokens", problems),
        day_tokens: read_count(usage, "day_tokens", problems),
        day_usd_micros: read_usd(usage, "day_usd", problems),
        billing: read_billing(usage, problems),
    }
}

#[cfg(test)]
mod tests;
