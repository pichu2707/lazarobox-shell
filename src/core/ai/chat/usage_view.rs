//! The `/cost` text view (ai-cost, ai-usage "The `/cost` view"). Pure: the
//! store thread answers with [`UsageRow`]s, already grouped, and these
//! functions fold them into lines. A value an engine did not report stays
//! `None` and reads `no data`; it is never summed or shown as 0.

use std::collections::BTreeMap;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::usage::TurnUsage;
use crate::core::ai::sanitize::clean;

/// First line of every `/cost` view.
pub const HEADER: &str = "AI usage (alpha)";
/// Shown under the header when nothing was recorded.
pub const EMPTY_LINE: &str = "no usage recorded yet";
/// The whole view when the store cannot be read.
pub const UNAVAILABLE_LINE: &str = "usage history is not available";
/// Shown when the session held more engine and model pairs than it keeps.
pub const SESSION_FULL_LINE: &str =
    "session list is full: later engines and models are not counted";

/// The days a window of "last 7 days" covers, today included.
const WINDOW_DAYS: u32 = 7;

/// How a CLI engine is billed. It labels a reported dollar figure and
/// nothing else.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Billing {
    Subscription,
    PerToken,
}

/// `$` and the figure: 4 decimals below one dollar, 2 from one dollar up.
pub fn format_usd(micros: u64) -> String {
    if micros < 1_000_000 {
        let tenth_cents = micros.saturating_add(50) / 100;
        format!("${}.{:04}", tenth_cents / 10_000, tenth_cents % 10_000)
    } else {
        let cents = micros.saturating_add(5_000) / 10_000;
        format!("${}.{:02}", cents / 100, cents % 100)
    }
}

/// The dollar part of a usage line, labelled by the billing mode.
pub fn dollar_part(micros: u64, billing: Billing) -> String {
    match billing {
        Billing::PerToken => format_usd(micros),
        Billing::Subscription => format!("{} list-price equivalent", format_usd(micros)),
    }
}

/// The usage line under an answer (`None`: no record). Dollars are labelled
/// by `billing`, and a local engine (`None`) never shows them.
pub fn usage_line(usage: Option<&TurnUsage>, billing: Option<Billing>) -> String {
    let Some(usage) = usage else {
        return "usage: no data".to_owned();
    };
    let n = |v: Option<u64>| v.map_or_else(|| "no data".to_owned(), |n| n.to_string());
    let mut line = format!("usage: in {} · out {}", n(usage.input), n(usage.output));
    if let Some(cached) = usage.cache_read {
        line.push_str(&format!(" · cached {cached}"));
    }
    if let Some(reasoning) = usage.reasoning {
        line.push_str(&format!(" · reasoning {reasoning}"));
    }
    if let (Some(micros), Some(mode)) = (usage.cost_micros, billing) {
        line.push_str(&format!(" · {}", dollar_part(micros, mode)));
    }
    if usage.partial {
        line.push_str(" · partial");
    }
    line
}

/// The status-bar segment: today's known tokens (`no data` when none) and,
/// when there is any, the spend of the per-token engines.
pub fn today_segment(tokens: Option<u64>, spend_micros: Option<u64>) -> String {
    let mut text = tokens.map_or_else(
        || "today: no data".to_owned(),
        |n| format!("today: {n} tok"),
    );
    // A zero sum is no figure; a sub-cent one is not rounded down to $0.00.
    match spend_micros {
        None | Some(0) => {}
        Some(micros) if micros < 5_000 => text.push_str(" · <$0.01"),
        Some(micros) => {
            let cents = micros.saturating_add(5_000) / 10_000;
            text.push_str(&format!(" · ${}.{:02}", cents / 100, cents % 100));
        }
    }
    text
}

/// The records of one engine and model on one local day, folded by the store
/// thread (or, for a session, by [`Session`]).
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct UsageRow {
    /// Local days before today; 0 is today.
    pub days_ago: u32,
    pub engine: String,
    pub model: Option<String>,
    /// The records are linked to the conversation that asked.
    pub in_chat: bool,
    /// Chat turns; compaction and summary runs are not turns.
    pub turns: u64,
    /// Every record of the row, whatever its role.
    pub records: u64,
    /// How many of those reported a dollar figure.
    pub costed: u64,
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cache_read: Option<u64>,
    /// Cache reads of the records that reported both an input and a cache
    /// read: the numerator of the cache share.
    pub cache_hit: Option<u64>,
    /// Fresh input + cache reads + cache writes of those same records.
    pub cache_base: Option<u64>,
    /// Records of the row that gave no cache figure: no input, no cache read
    /// or, for claude, no cache write. The share leaves them out.
    pub cache_gap: u64,
    /// Prompt-processing time of the local records that reported it, in ms.
    pub prefill_ms: Option<u64>,
    /// Full prompt tokens (fresh input + cache reads) of those same records.
    pub prefill_tokens: Option<u64>,
    /// Records of the row that gave no prefill time or no input.
    pub prefill_gap: u64,
    pub cost_micros: Option<u64>,
    /// Sum of the known totals (`input + output` of each record that has both).
    pub total: Option<u64>,
    /// A record was partial or lacked its input or output.
    pub partial: bool,
    /// How many of the records are worker turns (they are in every figure
    /// above, but they are not turns).
    pub workers: u64,
    /// Sum of the known totals of the worker records alone.
    pub worker_total: Option<u64>,
    /// A worker record was partial or lacked its input or output.
    pub worker_partial: bool,
}

/// `Some(a + b)` when either is known: a gap is skipped, not counted as 0.
fn add(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (None, other) | (other, None) => other,
        (Some(a), Some(b)) => Some(a.saturating_add(b)),
    }
}

/// Which kind of run a usage record belongs to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Run {
    /// A chat turn.
    Chat,
    /// A compaction or summary run: tokens count, but it is not a turn.
    Side,
}

/// What this session used, kept in memory so `/cost` can add a `this
/// session` line to the stored report. One row per engine and model.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Session {
    rows: Vec<UsageRow>,
    full: bool,
}

impl Session {
    /// The most engine and model pairs kept.
    const CAP: usize = 64;

    /// Folds the usage of one finished run in. Worker usage is folded as a
    /// side run: the session view has no `of which workers` figures.
    pub fn add(&mut self, engine: &str, run: Run, usage: &TurnUsage) {
        let model = usage.model.clone();
        let total = usage
            .input
            .zip(usage.output)
            .map(|(i, o)| i.saturating_add(o));
        let partial = usage.partial || usage.input.is_none() || usage.output.is_none();
        let at = self
            .rows
            .iter()
            .position(|r| r.engine == engine && r.model == model);
        let row = match at {
            Some(at) => &mut self.rows[at],
            None if self.rows.len() < Self::CAP => {
                self.rows.push(UsageRow {
                    days_ago: 0,
                    engine: engine.to_owned(),
                    model,
                    in_chat: true,
                    turns: 0,
                    records: 0,
                    costed: 0,
                    input: None,
                    output: None,
                    cache_read: None,
                    cost_micros: None,
                    total: None,
                    partial: false,
                    ..UsageRow::default()
                });
                self.rows.last_mut().expect("just pushed")
            }
            None => {
                self.full = true;
                return;
            }
        };
        row.turns += u64::from(run == Run::Chat);
        row.records += 1;
        row.costed += u64::from(usage.cost_micros.is_some());
        row.input = add(row.input, usage.input);
        row.output = add(row.output, usage.output);
        row.cache_read = add(row.cache_read, usage.cache_read);
        row.cost_micros = add(row.cost_micros, usage.cost_micros);
        row.total = add(row.total, total);
        row.partial |= partial;
    }

    pub fn rows(&self) -> &[UsageRow] {
        &self.rows
    }

    /// The session held more pairs than it keeps.
    pub fn full(&self) -> bool {
        self.full
    }
}

/// `<N> tokens` or `no data`.
fn tokens(total: Option<u64>) -> String {
    total.map_or_else(|| "no data".to_owned(), |n| format!("{n} tokens"))
}

/// A row has a record that was cut or lacks its input or output.
fn incomplete(r: &UsageRow) -> bool {
    r.partial || r.input.is_none() || r.output.is_none()
}

/// `<T> · <K> turns`, marked `partial` when any row is, for the rows picked.
fn summary<'a>(rows: impl Iterator<Item = &'a UsageRow>) -> String {
    let (total, turns, partial) = rows.fold((None, 0u64, false), |(total, turns, partial), r| {
        (
            add(total, r.total),
            turns + r.turns,
            partial || incomplete(r),
        )
    });
    let mark = if partial { " · partial" } else { "" };
    format!("{} · {turns} turns{mark}", tokens(total))
}

/// `of which workers: <T>` for the rows picked, `None` without a worker
/// record. A total nobody reported is `no data`, a cut or incomplete record
/// marks it `partial`.
fn workers_line<'a>(rows: impl Iterator<Item = &'a UsageRow>) -> Option<String> {
    let (records, total, partial) = rows.fold((0u64, None, false), |(n, total, partial), r| {
        (
            n + r.workers,
            add(total, r.worker_total),
            partial || r.worker_partial,
        )
    });
    let mark = if partial { " · partial" } else { "" };
    (records > 0).then(|| format!("of which workers: {}{mark}", tokens(total)))
}

/// `<label> <engine> N% · <engine> N%` for the rows picked, `None` when no
/// engine reported a cache. The share is `cache reads / (fresh input + cache
/// reads + cache writes)`; `TurnUsage` buckets are disjoint for every engine
/// (the codex and gemini adapters store the input net of the cache), so one
/// formula serves all. An engine with no cache report, or an empty base, has
/// no figure: never 0%. Marked `partial` when some record of a shown engine
/// gave no cache figure (no input, no cache read, or for claude no cache
/// write): the share then covers only the records that did.
fn cache_line<'a>(label: &str, rows: impl Iterator<Item = &'a UsageRow>) -> Option<String> {
    // Per engine: cache reads, base and records that gave no figure.
    let mut by_engine: BTreeMap<String, (u64, u64, u64)> = BTreeMap::new();
    for r in rows {
        let e = by_engine.entry(ident(&r.engine)).or_default();
        e.0 = e.0.saturating_add(r.cache_hit.unwrap_or(0));
        e.1 = e.1.saturating_add(r.cache_base.unwrap_or(0));
        e.2 = e.2.saturating_add(r.cache_gap);
    }
    let shown: Vec<_> = by_engine
        .into_iter()
        .filter(|(_, (_, base, _))| *base > 0)
        .collect();
    let partial = shown.iter().any(|(_, (_, _, gap))| *gap > 0);
    let shares: Vec<String> = shown
        .into_iter()
        .map(|(engine, (hit, base, _))| {
            format!("{engine} {}%", u128::from(hit) * 100 / u128::from(base))
        })
        .collect();
    let mark = if partial { " · partial" } else { "" };
    (!shares.is_empty()).then(|| format!("cache {label}: {}{mark}", shares.join(" · ")))
}

/// A usage line as the coordinator's: while a run is live the line under the
/// coordinator's answer is not the run's total.
pub fn coordinator_line(line: String) -> String {
    match line.strip_prefix("usage:") {
        Some(rest) => format!("usage (coordinator):{rest}"),
        None => line,
    }
}

/// An engine or model id as one line of plain text.
fn ident(raw: &str) -> String {
    clean(raw)
        .split(char::is_whitespace)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// One line per engine and model, biggest total first.
fn pair_lines<'a>(
    rows: impl Iterator<Item = &'a UsageRow>,
    billing: &dyn Fn(&str) -> Option<Billing>,
) -> Vec<String> {
    let mut pairs: BTreeMap<(String, String), UsageRow> = BTreeMap::new();
    for r in rows {
        let key = (
            ident(&r.engine),
            r.model.as_deref().map_or_else(|| "unknown".into(), ident),
        );
        let p = pairs.entry(key).or_insert_with(|| UsageRow {
            turns: 0,
            records: 0,
            costed: 0,
            input: None,
            output: None,
            cache_read: None,
            cost_micros: None,
            total: None,
            partial: false,
            workers: 0,
            worker_total: None,
            worker_partial: false,
            ..r.clone()
        });
        p.turns += r.turns;
        p.records += r.records;
        p.costed += r.costed;
        p.input = add(p.input, r.input);
        p.output = add(p.output, r.output);
        p.cache_read = add(p.cache_read, r.cache_read);
        p.cost_micros = add(p.cost_micros, r.cost_micros);
        p.total = add(p.total, r.total);
        p.partial |= incomplete(r);
    }
    let mut pairs: Vec<_> = pairs.into_iter().collect();
    // Biggest first; unknown totals last.
    pairs.sort_by(|a, b| b.1.total.cmp(&a.1.total).then_with(|| a.0.cmp(&b.0)));
    pairs
        .into_iter()
        .map(|((engine, model), p)| {
            let n = |v: Option<u64>| v.map_or_else(|| "no data".to_owned(), |n| n.to_string());
            let mut line = format!(
                "  {engine} › {model}: in {} · out {}",
                n(p.input),
                n(p.output)
            );
            if let Some(cached) = p.cache_read {
                line.push_str(&format!(" · cached {cached}"));
            }
            let mut gap = false;
            if let (Some(micros), Some(mode)) = (p.cost_micros, billing(&engine)) {
                line.push_str(&format!(" · {}", dollar_part(micros, mode)));
                // Some records of the pair reported no dollars: the figure
                // is a floor.
                gap = p.costed < p.records;
            }
            if p.partial || gap {
                line.push_str(" · partial");
            }
            line
        })
        .collect()
}

/// The bar glyphs of the per-day line, lowest first.
const LEVELS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
/// The longest engine bar.
const BAR_MAX: u64 = 20;
/// A day with nothing known.
const GAP: char = '·';

/// The longest engine name a chart shows, in columns.
const NAME_MAX: usize = 16;

/// A record that cannot tell its whole total: the figure it feeds is a minimum.
fn minimum(r: &UsageRow) -> bool {
    incomplete(r) || r.total.is_none()
}

/// An engine id for the chart: `?` when empty, cut with `…` past
/// [`NAME_MAX`] columns.
fn chart_name(engine: &str) -> String {
    if engine.is_empty() {
        return "?".to_owned();
    }
    if engine.width() <= NAME_MAX {
        return engine.to_owned();
    }
    let mut name = String::new();
    for c in engine.chars() {
        if name.width() + c.width().unwrap_or(0) >= NAME_MAX {
            break;
        }
        name.push(c);
    }
    name.push('…');
    name
}

/// The two charts of the last 7 days as plain text lines. They fold the same
/// rows as the text view: a total nobody reported is a gap (`·`, `no data`),
/// never a low bar, and dollars are never charted. A figure that includes a
/// partial or unknown record ends in ` *` (it is a minimum). Empty when no
/// row falls inside the window.
fn chart_lines(rows: &[UsageRow]) -> Vec<String> {
    let window: Vec<&UsageRow> = rows.iter().filter(|r| r.days_ago < WINDOW_DAYS).collect();
    if window.is_empty() {
        return Vec::new();
    }
    let days: Vec<Option<u64>> = (0..WINDOW_DAYS)
        .rev()
        .map(|ago| {
            window
                .iter()
                .filter(|r| r.days_ago == ago)
                .fold(None, |sum, r| add(sum, r.total))
        })
        .collect();
    let max = days.iter().flatten().copied().max();
    let spark: String = days
        .iter()
        .map(|day| match (day, max) {
            (Some(0), _) | (Some(_), Some(0)) => LEVELS[0],
            (Some(n), Some(max)) => {
                // A known non-zero day never looks like a known zero.
                let level = (u128::from(*n) * 7 / u128::from(max)).clamp(1, 7);
                LEVELS[usize::try_from(level).unwrap_or(7)]
            }
            _ => GAP,
        })
        .collect();
    let star = |partial: bool| if partial { " *" } else { "" };
    let day_mark = star(window.iter().any(|r| minimum(r)));
    let mut lines = vec![
        "per day (last 7 days, oldest first):".to_owned(),
        match max {
            Some(max) => format!("{spark} max {max}{day_mark}"),
            None => format!("{spark} no data"),
        },
        "per engine (last 7 days):".to_owned(),
    ];
    let mut engines: BTreeMap<String, (Option<u64>, bool)> = BTreeMap::new();
    for r in &window {
        let (total, partial) = engines.entry(ident(&r.engine)).or_default();
        *total = add(*total, r.total);
        *partial |= minimum(r);
    }
    let mut engines: Vec<_> = engines.into_iter().collect();
    // Biggest first; unknown totals last.
    engines.sort_by(|a, b| b.1.0.cmp(&a.1.0).then_with(|| a.0.cmp(&b.0)));
    let biggest = engines.iter().filter_map(|e| e.1.0).max().unwrap_or(0);
    let names: Vec<String> = engines.iter().map(|e| chart_name(&e.0)).collect();
    let pad = names.iter().map(|n| n.width()).max().unwrap_or(0);
    lines.extend(
        engines
            .iter()
            .zip(&names)
            .map(|((_, (total, partial)), name)| {
                let gap = " ".repeat(pad - name.width());
                match total {
                    None => format!("  {name}{gap} no data"),
                    Some(n) => {
                        let len = if biggest == 0 {
                            1
                        } else {
                            (u128::from(*n) * u128::from(BAR_MAX) / u128::from(biggest)).max(1)
                        };
                        let bar = "█".repeat(usize::try_from(len).unwrap_or(1));
                        format!("  {name}{gap} {bar} {n}{}", star(*partial))
                    }
                }
            }),
    );
    lines
}

/// The `/cost` view of the store's rows. `session` is what this pane used.
pub fn report_lines(
    rows: &[UsageRow],
    session: &Session,
    billing: &dyn Fn(&str) -> Option<Billing>,
) -> Vec<String> {
    if rows.is_empty() {
        return vec![HEADER.to_owned(), EMPTY_LINE.to_owned()];
    }
    let window = || rows.iter().filter(|r| r.days_ago < WINDOW_DAYS);
    let mut lines = vec![
        HEADER.to_owned(),
        format!(
            "today: {}",
            summary(rows.iter().filter(|r| r.days_ago == 0))
        ),
    ];
    lines.extend(cache_line("today", rows.iter().filter(|r| r.days_ago == 0)));
    lines.push(format!("last 7 days: {}", summary(window())));
    lines.extend(cache_line("last 7 days", window()));
    lines.extend(workers_line(window()));
    lines.push("by engine and model (last 7 days):".to_owned());
    lines.extend(pair_lines(window(), billing));
    let mut mine = rows.iter().filter(|r| r.in_chat).peekable();
    lines.push(if mine.peek().is_some() {
        format!("this chat: {}", summary(mine))
    } else {
        "this chat: no data".to_owned()
    });
    lines.extend(cache_line("this chat", rows.iter().filter(|r| r.in_chat)));
    if !session.rows().is_empty() {
        lines.push(format!("this session: {}", summary(session.rows().iter())));
        if session.full() {
            lines.push(SESSION_FULL_LINE.to_owned());
        }
    }
    lines.extend(chart_lines(rows));
    lines
}

#[cfg(test)]
mod tests;
