//! The Agents table and the peak of runs at once: orchestrator workers only.
//! Their time is summed, so parallel agents add up; the peak says how many
//! ran at the same moment.

use std::collections::BTreeMap;

use unicode_width::UnicodeWidthStr;

use super::{
    super::{
        provider::{Provider, classify},
        usage_fmt::duration,
        usage_view::{add, ident},
    },
    Range, UsageData,
    tables::{Table, count, pad},
};

/// Shown in place of the table when no worker ran in the range.
pub const NO_AGENTS: &str = "no agent runs in this range";
/// The least an engine name keeps.
const ENGINE_MIN: usize = 8;

/// When one worker run took place, in unix seconds, and the local day of its
/// end. The start is set only for a record that has a wall time.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Span {
    pub days_ago: u32,
    pub start: i64,
    pub end: i64,
}

/// What the worker records of one engine added up to on one local day.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct AgentDay {
    pub days_ago: u32,
    pub engine: String,
    pub workers: u64,
    pub input: Option<u64>,
    pub output: Option<u64>,
    /// A record was cut or lacked its input or output.
    pub partial: bool,
    pub wall_ms: Option<u64>,
    /// How many of the records have a wall time.
    pub timed: u64,
}

/// One engine over the range.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AgentRow {
    pub provider: Provider,
    pub engine: String,
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub partial: bool,
    pub wall_ms: Option<u64>,
    pub workers: u64,
    pub timed: u64,
}

/// The most runs that overlapped in the range. A span is `[start, end)`, so
/// one that ends as another starts does not overlap it, and a run of no
/// seconds still takes one. `None` when no run has a start.
pub fn overlap(data: &UsageData, range: Range) -> Option<u32> {
    let to = range.days().unwrap_or(u32::MAX);
    let mut events: Vec<(i64, i8)> = Vec::new();
    for s in data.spans.iter().filter(|s| s.days_ago < to) {
        events.push((s.start, 1));
        events.push((s.end.max(s.start.saturating_add(1)), -1));
    }
    // At a tie `-1` sorts first: the end comes before the start.
    events.sort_unstable();
    let (mut now, mut most) = (0i64, 0i64);
    for (_, change) in &events {
        now += i64::from(*change);
        most = most.max(now);
    }
    u32::try_from(most).ok().filter(|n| *n > 0)
}

/// The engines that ran workers in the range, most tokens first.
pub fn agents(data: &UsageData, range: Range) -> Vec<AgentRow> {
    let to = range.days().unwrap_or(u32::MAX);
    let mut by: BTreeMap<String, AgentRow> = BTreeMap::new();
    for d in data.agents.iter().filter(|d| d.days_ago < to) {
        let engine = ident(&d.engine);
        let row = by.entry(engine.clone()).or_insert_with(|| AgentRow {
            provider: classify(&d.engine, None),
            engine,
            input: None,
            output: None,
            partial: false,
            wall_ms: None,
            workers: 0,
            timed: 0,
        });
        row.input = add(row.input, d.input);
        row.output = add(row.output, d.output);
        row.partial |= d.partial;
        row.wall_ms = add(row.wall_ms, d.wall_ms);
        row.workers = row.workers.saturating_add(d.workers);
        row.timed = row.timed.saturating_add(d.timed);
    }
    let mut rows: Vec<AgentRow> = by.into_values().collect();
    // The map is in engine order and the sort is stable.
    rows.sort_by_key(|r| std::cmp::Reverse(add(r.input, r.output)));
    rows
}

/// `n/a` when no worker has a wall time, `~` when only some do.
fn time(r: &AgentRow) -> String {
    match r.wall_ms {
        Some(ms) if r.timed >= r.workers => duration(ms),
        Some(ms) => format!("~{}", duration(ms)),
        None => "n/a".to_owned(),
    }
}

/// The table at `width` columns: the engine with its provider glyph, then
/// `input`, `output` and `time`. No dollars and no turns.
pub fn agent_table(rows: &[AgentRow], width: usize) -> Table {
    let cells: Vec<[String; 3]> = rows
        .iter()
        .map(|r| {
            [
                count(r.input, r.partial),
                count(r.output, r.partial),
                time(r),
            ]
        })
        .collect();
    let head = ["input", "output", "time"];
    let widths: Vec<usize> = (0..3)
        .map(|i| {
            let longest = cells.iter().map(|c| c[i].width()).max().unwrap_or(0);
            longest.max(head[i].width())
        })
        .collect();
    let engines: Vec<String> = rows
        .iter()
        .map(|r| format!("{} {}", r.provider.glyph(), r.engine))
        .collect();
    let longest = engines.iter().map(|e| e.width()).max().unwrap_or(0);
    let name = longest
        .max("engine".width())
        .min(width.saturating_sub(widths.iter().map(|w| w + 1).sum::<usize>()))
        .max(ENGINE_MIN);
    let line = |first: &str, cols: &[String]| {
        let nums = cols.iter().zip(&widths).map(|(c, w)| format!("{c:>w$}"));
        let all: Vec<String> = std::iter::once(pad(first, name)).chain(nums).collect();
        all.join(" ").trim_end().to_owned()
    };
    let titles: Vec<String> = head.iter().map(|h| (*h).to_owned()).collect();
    Table {
        head: line("engine", &titles),
        rows: rows
            .iter()
            .zip(&engines)
            .zip(&cells)
            .map(|((r, e), c)| (r.provider, line(e, c)))
            .collect(),
        notes: Vec::new(),
    }
}

/// Rows of the block: a gap, the title, then the table with its header or the
/// one line that says no worker ran.
pub fn agents_rows(rows: &[AgentRow]) -> u16 {
    let body = if rows.is_empty() { 1 } else { 1 + rows.len() };
    u16::try_from(2 + body).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests;
