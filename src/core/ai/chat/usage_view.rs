//! The `/cost` text view (ai-cost, ai-usage "The `/cost` view"). Pure: the
//! store thread answers with [`UsageRow`]s, already grouped, and these
//! functions fold them into lines. A value an engine did not report stays
//! `None` and reads `no data`; it is never summed or shown as 0.

use std::collections::BTreeMap;

use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::usage::TurnUsage;
use crate::core::ai::sanitize::clean;

/// First line of every `/cost` view.
pub const HEADER: &str = "AI usage (alpha)";
/// Shown under the header when nothing was recorded.
pub const EMPTY_LINE: &str = "no usage recorded yet";
/// The whole view when the store cannot be read.
pub const UNAVAILABLE_LINE: &str = "usage history is not available";
/// Shown while the store thread reads.
pub const READING_LINE: &str = "reading usage…";
/// Shown when history is off: only this session can be told.
pub const HISTORY_OFF_LINE: &str = "chat history is off: showing this session only";
/// The notice for `/cost` followed by text.
pub const NO_ARGUMENTS: &str = "/cost takes no arguments";

/// The days a window of "last 7 days" covers, today included.
const WINDOW_DAYS: u32 = 7;

/// How a CLI engine is billed. It labels a reported dollar figure and
/// nothing else.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Billing {
    Subscription,
    PerToken,
}

/// The default billing mode of `engine`: `subscription` for the CLI engines,
/// none for the local ones (they never show dollars).
pub fn default_billing(engine: &str) -> Option<Billing> {
    matches!(engine, "claude" | "codex" | "gemini").then_some(Billing::Subscription)
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

/// The records of one engine and model on one local day, folded by the store
/// thread (or, for a session, by [`Session`]).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UsageRow {
    /// Local days before today; 0 is today.
    pub days_ago: u32,
    pub engine: String,
    pub model: Option<String>,
    /// The records are linked to the conversation that asked.
    pub in_chat: bool,
    pub turns: u64,
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cache_read: Option<u64>,
    pub cost_micros: Option<u64>,
    /// Sum of the known totals (`input + output` of each record that has both).
    pub total: Option<u64>,
    /// A record was partial or lacked its input or output.
    pub partial: bool,
}

/// `Some(a + b)` when either is known: a gap is skipped, not counted as 0.
fn add(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (None, other) | (other, None) => other,
        (Some(a), Some(b)) => Some(a.saturating_add(b)),
    }
}

/// What this session used, kept in memory so `/cost` can tell it with the
/// history off. One row per engine and model.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Session {
    rows: Vec<UsageRow>,
}

impl Session {
    /// The most engine and model pairs kept.
    const CAP: usize = 64;

    /// Folds the usage of one finished run in.
    pub fn add(&mut self, engine: &str, usage: &TurnUsage) {
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
                    input: None,
                    output: None,
                    cache_read: None,
                    cost_micros: None,
                    total: None,
                    partial: false,
                });
                self.rows.last_mut().expect("just pushed")
            }
            None => return,
        };
        row.turns += 1;
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
}

/// `<N> tokens` or `no data`.
fn tokens(total: Option<u64>) -> String {
    total.map_or_else(|| "no data".to_owned(), |n| format!("{n} tokens"))
}

/// `<T> · <K> turns` for the rows picked.
fn summary<'a>(rows: impl Iterator<Item = &'a UsageRow>) -> String {
    let (total, turns) = rows.fold((None, 0u64), |(total, turns), r| {
        (add(total, r.total), turns + r.turns)
    });
    format!("{} · {turns} turns", tokens(total))
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
            input: None,
            output: None,
            cache_read: None,
            cost_micros: None,
            total: None,
            partial: false,
            ..r.clone()
        });
        p.turns += r.turns;
        p.input = add(p.input, r.input);
        p.output = add(p.output, r.output);
        p.cache_read = add(p.cache_read, r.cache_read);
        p.cost_micros = add(p.cost_micros, r.cost_micros);
        p.total = add(p.total, r.total);
        p.partial |= r.partial || r.input.is_none() || r.output.is_none();
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
            if let (Some(micros), Some(mode)) = (p.cost_micros, billing(&engine)) {
                line.push_str(&format!(" · {}", dollar_part(micros, mode)));
            }
            if p.partial {
                line.push_str(" · partial");
            }
            line
        })
        .collect()
}

/// The `/cost` view of the store's rows. `session` is what this pane used.
pub fn report_lines(
    rows: &[UsageRow],
    session: &[UsageRow],
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
        format!("last 7 days: {}", summary(window())),
        "by engine and model (last 7 days):".to_owned(),
    ];
    lines.extend(pair_lines(window(), billing));
    let mut mine = rows.iter().filter(|r| r.in_chat).peekable();
    lines.push(if mine.peek().is_some() {
        format!("this chat: {}", summary(mine))
    } else {
        "this chat: no data".to_owned()
    });
    if !session.is_empty() {
        lines.push(format!("this session: {}", summary(session.iter())));
    }
    lines
}

/// The `/cost` view with the history off: this session, from memory.
pub fn session_lines(
    session: &[UsageRow],
    billing: &dyn Fn(&str) -> Option<Billing>,
) -> Vec<String> {
    let mut lines = vec![HEADER.to_owned(), HISTORY_OFF_LINE.to_owned()];
    if session.is_empty() {
        lines.push(EMPTY_LINE.to_owned());
        return lines;
    }
    lines.push(format!("this session: {}", summary(session.iter())));
    lines.push("by engine and model:".to_owned());
    lines.extend(pair_lines(session.iter(), billing));
    lines
}

/// The `/cost` overlay of a chat pane: the lines, and how far they scroll.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CostView {
    lines: Vec<String>,
    scroll: usize,
}

impl CostView {
    /// The view while the store thread reads.
    pub fn reading() -> Self {
        Self::with(vec![HEADER.to_owned(), READING_LINE.to_owned()])
    }

    pub fn with(lines: Vec<String>) -> Self {
        Self { lines, scroll: 0 }
    }

    /// Replaces the lines (the store answered), keeping the scroll.
    pub fn set_lines(&mut self, lines: Vec<String>) {
        self.lines = lines;
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }

    /// A key on the view: `true` when it closes.
    pub fn on_key(&mut self, key: &KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => return true,
            KeyCode::Up | KeyCode::Char('k') => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll += 1,
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(PAGE),
            KeyCode::PageDown => self.scroll += PAGE,
            _ => {}
        }
        self.scroll = self.scroll.min(self.lines.len().saturating_sub(1));
        false
    }
}

/// Rows a page key scrolls.
const PAGE: usize = 10;

#[cfg(test)]
mod tests;
