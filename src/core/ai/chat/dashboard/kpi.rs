//! The four summary cards.

use std::collections::HashSet;

use super::{
    super::{
        usage_fmt::{compact, duration, grouped, pct},
        usage_view::add,
    },
    Range, UsageData,
};

/// The most days a sparkline covers; older days only count in the totals.
const SPARK_DAYS: u32 = 400;

/// The change against the previous equal period.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Delta {
    NotAvailable,
    Up(u64),
    Down(u64),
    Flat,
}

impl Delta {
    /// `▲ 12%`, `▼ 3%`, `= 0%` or `n/a`: the arrow carries the meaning, so the
    /// text needs no colour.
    pub fn text(self) -> String {
        match self {
            Self::NotAvailable => "n/a".to_owned(),
            Self::Up(n) => format!("▲ {n}%"),
            Self::Down(n) => format!("▼ {n}%"),
            Self::Flat => "= 0%".to_owned(),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Kpi {
    pub label: &'static str,
    pub value: String,
    pub sub: String,
    pub delta: Delta,
    /// The metric by day, oldest first.
    pub spark: Vec<u64>,
}

/// Sums adjacent values so at most `cols` remain: 90 days fit a narrow pane.
pub fn bucket(values: &[u64], cols: usize) -> Vec<u64> {
    if cols == 0 {
        return Vec::new();
    }
    let size = values.len().div_ceil(cols).max(1);
    values
        .chunks(size)
        .map(|c| c.iter().fold(0u64, |a, v| a.saturating_add(*v)))
        .collect()
}

/// What the rows and tasks of a span of days add up to. A figure no record
/// reported stays `None`, never 0.
#[derive(Default)]
struct Totals {
    input: Option<u64>,
    output: Option<u64>,
    chat_output: Option<u64>,
    turns: u64,
    cache_hit: Option<u64>,
    cache_base: Option<u64>,
    wall: Option<u64>,
    workers: u64,
    timed: u64,
    tasks: u64,
}

/// The days before today in `[from, to)`.
fn totals(data: &UsageData, from: u32, to: u32) -> Totals {
    let days = from..to;
    let mut t = Totals::default();
    for r in data.rows.iter().filter(|r| days.contains(&r.days_ago)) {
        t.input = add(t.input, r.input);
        t.output = add(t.output, r.output);
        t.chat_output = add(t.chat_output, r.chat_output);
        t.turns += r.turns;
        t.cache_hit = add(t.cache_hit, r.cache_hit);
        t.cache_base = add(t.cache_base, r.cache_base);
        t.wall = add(t.wall, r.worker_wall_ms);
        t.workers += r.workers;
        t.timed += r.worker_timed;
    }
    let keys: HashSet<&str> = data
        .tasks
        .iter()
        .filter(|(day, _)| days.contains(day))
        .map(|(_, key)| key.as_str())
        .collect();
    t.tasks = keys.len() as u64;
    t
}

type Metric = fn(&Totals) -> Option<u64>;

/// The metric of each card, in card order.
const METRICS: [Metric; 4] = [|t| t.input, |t| t.output, |t| Some(t.tasks), |t| t.wall];

/// The change of a card against the previous equal period: only for 7 and 30
/// days, and only when both periods have a figure and the previous one is not
/// 0. For agent time the previous period must have wall time on every worker
/// record, or the comparison would mislead.
fn delta(data: &UsageData, range: Range, metric: Metric, timed: bool) -> Delta {
    let Some(n) = range.days().filter(|n| *n <= 30) else {
        return Delta::NotAvailable;
    };
    let prev = totals(data, n, 2 * n);
    let now = totals(data, 0, n);
    let complete = !timed || prev.workers == prev.timed;
    let (Some(now), Some(prev)) = (metric(&now), metric(&prev).filter(|_| complete)) else {
        return Delta::NotAvailable;
    };
    if prev == 0 {
        return Delta::NotAvailable;
    }
    let change = (u128::from(now.abs_diff(prev)) * 100 + u128::from(prev) / 2) / u128::from(prev);
    let change = u64::try_from(change).unwrap_or(u64::MAX);
    match (change, now > prev) {
        (0, _) => Delta::Flat,
        (n, true) => Delta::Up(n),
        (n, false) => Delta::Down(n),
    }
}

fn tokens(n: Option<u64>) -> String {
    n.map_or_else(|| "n/a".to_owned(), compact)
}

/// The agent time: `n/a` when no worker record has a wall time, `~` when only
/// some do, so the sum is a lower bound.
fn agent_time(t: &Totals) -> String {
    match (t.workers, t.timed, t.wall) {
        (0, _, _) => duration(0),
        (_, 0, _) | (_, _, None) => "n/a".to_owned(),
        (w, n, Some(ms)) if n < w => format!("~{}", duration(ms)),
        (_, _, Some(ms)) => duration(ms),
    }
}

pub(super) fn is_empty(data: &UsageData, range: Range) -> bool {
    let to = range.days().unwrap_or(u32::MAX);
    data.rows.iter().all(|r| r.days_ago >= to)
}

pub(super) fn kpis(data: &UsageData, range: Range) -> [Kpi; 4] {
    let now = totals(data, 0, range.days().unwrap_or(u32::MAX));
    let len = range
        .days()
        .or_else(|| data.rows.iter().map(|r| r.days_ago.saturating_add(1)).max())
        .unwrap_or(0)
        .min(SPARK_DAYS);
    let days: Vec<Totals> = (0..len).map(|d| totals(data, d, d + 1)).collect();
    let card = |i: usize, label, value, sub| Kpi {
        label,
        value,
        sub,
        delta: delta(data, range, METRICS[i], i == 3),
        spark: days
            .iter()
            .rev()
            .map(|d| METRICS[i](d).unwrap_or(0))
            .collect(),
    };
    let cache = pct(now.cache_hit.unwrap_or(0), now.cache_base.unwrap_or(0));
    // The average per chat turn, not per record (R1): compaction, summary and
    // worker records are not turns.
    let average = match now.chat_output {
        Some(out) if now.turns > 0 => format!("avg {}/turn", compact(out / now.turns)),
        _ => "avg n/a".to_owned(),
    };
    [
        card(
            0,
            "Input",
            tokens(now.input),
            format!(
                "cache {}",
                cache.map_or("n/a".to_owned(), |p| format!("{p}%"))
            ),
        ),
        card(1, "Output", tokens(now.output), average),
        card(
            2,
            "Tasks",
            grouped(now.tasks),
            format!("{} turns", grouped(now.turns)),
        ),
        card(
            3,
            "Agent time",
            agent_time(&now),
            format!("{} agent turns", grouped(now.workers)),
        ),
    ]
}
