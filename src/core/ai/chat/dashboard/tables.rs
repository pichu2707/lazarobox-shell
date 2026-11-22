//! The Models table and the provider shares: the model of both blocks. The
//! text is formatted here, so the UI only lays it out and colours it.

use std::{cmp::Reverse, collections::BTreeMap};

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{
    super::{
        provider::{Provider, classify},
        usage_fmt::{compact, grouped},
        usage_view::{Billing, UsageRow, add, dollar_part, format_usd, ident, incomplete},
    },
    Range, UsageData,
};

/// Most models listed; the rest are counted on a `+N more models` row.
pub const MODELS_MAX: usize = 8;
/// From these widths the table adds the dollars and then the turns.
const DOLLARS_FROM: usize = 60;
const TURNS_FROM: usize = 80;
/// Columns of `glyph label`, and the least a model name keeps.
const PROVIDER: usize = 11;
const MODEL_MIN: usize = 8;
/// Shown in place of the bar when no model reported tokens.
pub const NO_TOKENS: &str = "no tokens reported in this range";

/// The billing mode of each engine that shows dollars; any engine not listed
/// (a local one) never does.
pub type Billed = Vec<(String, Billing)>;

/// The dollars of a billed pair.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Usd {
    pub micros: Option<u64>,
    pub mode: Billing,
    /// Some records reported no dollars: the figure is a floor.
    pub floor: bool,
}

/// One engine and model pair over the range.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ModelRow {
    pub provider: Provider,
    pub model: String,
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub total: Option<u64>,
    pub turns: u64,
    /// A record was cut or lacked its input or output.
    pub partial: bool,
    /// `None` for an engine that is not billed.
    pub usd: Option<Usd>,
}

/// The pairs of the range, biggest total first (unknown totals last).
pub fn models(data: &UsageData, range: Range, billed: &Billed) -> Vec<ModelRow> {
    let to = range.days().unwrap_or(u32::MAX);
    let mut by: BTreeMap<(String, String), ModelRow> = BTreeMap::new();
    for r in data.rows.iter().filter(|r| r.days_ago < to) {
        let (engine, model) = (
            ident(&r.engine),
            r.model.as_deref().map_or_else(|| "unknown".into(), ident),
        );
        let usd = billed
            .iter()
            .find(|(e, _)| *e == engine)
            .map(|(_, mode)| Usd {
                micros: None,
                mode: *mode,
                floor: false,
            });
        let row = by
            .entry((engine, model.clone()))
            .or_insert_with(|| ModelRow {
                provider: classify(&r.engine, r.model.as_deref()),
                model,
                input: None,
                output: None,
                total: None,
                turns: 0,
                partial: false,
                usd,
            });
        add_row(row, r);
    }
    let mut rows: Vec<ModelRow> = by.into_values().collect();
    rows.sort_by_key(|m| Reverse(m.total));
    rows
}

fn add_row(row: &mut ModelRow, r: &UsageRow) {
    row.input = add(row.input, r.input);
    row.output = add(row.output, r.output);
    row.total = add(row.total, r.total);
    row.turns = row.turns.saturating_add(r.turns);
    row.partial |= incomplete(r);
    if let Some(usd) = &mut row.usd {
        usd.micros = add(usd.micros, r.cost_micros);
        usd.floor |= r.costed < r.records;
    }
}

/// A provider's part of the tokens.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Share {
    pub provider: Provider,
    pub tokens: u64,
    /// Whole percent; the shares add up to exactly 100.
    pub percent: u64,
}

/// The providers with tokens, biggest first. Every model counts, shown or not.
pub fn shares(models: &[ModelRow]) -> Vec<Share> {
    let mut by: Vec<(Provider, u64)> = Provider::ALL.map(|p| (p, 0)).to_vec();
    for m in models {
        if let Some((_, sum)) = by.iter_mut().find(|(p, _)| *p == m.provider) {
            *sum = sum.saturating_add(m.total.unwrap_or(0));
        }
    }
    by.retain(|(_, tokens)| *tokens > 0);
    by.sort_by_key(|(_, tokens)| Reverse(*tokens));
    let percents = apportion(&by.iter().map(|(_, t)| *t).collect::<Vec<_>>(), 100);
    by.into_iter()
        .zip(percents)
        .map(|((provider, tokens), percent)| Share {
            provider,
            tokens,
            percent,
        })
        .collect()
}

/// Splits `units` among `values` in proportion, by the largest remainder, so
/// the parts add up to `units` exactly. A value above 0 keeps at least one
/// unit while there is a unit to take from a bigger part.
pub fn apportion(values: &[u64], units: u64) -> Vec<u64> {
    let sum: u128 = values.iter().map(|v| u128::from(*v)).sum();
    if sum == 0 {
        return vec![0; values.len()];
    }
    let scaled = |v: u64| u128::from(v) * u128::from(units);
    let mut out: Vec<u64> = values
        .iter()
        .map(|v| u64::try_from(scaled(*v) / sum).unwrap_or(units))
        .collect();
    let mut order: Vec<usize> = (0..values.len()).collect();
    order.sort_by_key(|&i| (Reverse(scaled(values[i]) % sum), i));
    let left = usize::try_from(units - out.iter().sum::<u64>()).unwrap_or(0);
    for &i in order.iter().take(left) {
        out[i] += 1;
    }
    for i in 0..values.len() {
        let biggest = (0..values.len()).max_by_key(|&j| out[j]);
        if values[i] > 0
            && out[i] == 0
            && let Some(big) = biggest
            && out[big] > 1
        {
            out[big] -= 1;
            out[i] = 1;
        }
    }
    out
}

/// The cells of each provider in a bar `width` columns wide.
pub fn segments(shares: &[Share], width: usize) -> Vec<(Provider, usize)> {
    let tokens: Vec<u64> = shares.iter().map(|s| s.tokens).collect();
    let cells = apportion(&tokens, u64::try_from(width).unwrap_or(0));
    shares
        .iter()
        .zip(cells)
        .map(|(s, n)| (s.provider, usize::try_from(n).unwrap_or(0)))
        .collect()
}

/// `✻ Anthropic 62%` items packed into lines of at most `width` columns.
pub fn share_legend(shares: &[Share], width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for s in shares {
        let item = format!(
            "{} {} {}%",
            s.provider.glyph(),
            s.provider.label(),
            s.percent
        );
        match lines.last_mut() {
            Some(line) if line.width() + 3 + item.width() <= width => {
                line.push_str(" · ");
                line.push_str(&item);
            }
            _ => lines.push(item),
        }
    }
    lines
}

/// The table as drawn at one width: a header, a row a model (each with its
/// provider, for the colour of its glyph) and the notes under it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Table {
    pub head: String,
    pub rows: Vec<(Provider, String)>,
    pub notes: Vec<String>,
}

/// `text` cut to `width` columns with `…`, or padded to them.
fn pad(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    let cut = text.width() > width;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w + usize::from(cut) > width {
            break;
        }
        out.push(c);
        used += w;
    }
    if cut {
        out.push('…');
        used += 1;
    }
    out + &" ".repeat(width.saturating_sub(used))
}

/// A token figure: `~` when some record lacked it, `n/a` when none had it.
fn count(value: Option<u64>, partial: bool) -> String {
    match value {
        Some(n) if partial => format!("~{}", compact(n)),
        Some(n) => compact(n),
        None => "n/a".to_owned(),
    }
}

/// The dollar cell: the full billing label from `TURNS_FROM`, a `*` mark
/// (explained under the table) before it.
fn dollars(usd: &Usd, full: bool) -> String {
    let Some(micros) = usd.micros else {
        return "n/a".to_owned();
    };
    let floor = if usd.floor { "~" } else { "" };
    match (usd.mode, full) {
        (Billing::Subscription, false) => format!("{floor}{}*", format_usd(micros)),
        (mode, _) => format!("{floor}{}", dollar_part(micros, mode)),
    }
}

/// The table at `width` columns: from 80 it has the turns and the full dollar
/// label, from 60 the dollars with a mark, and under that only the tokens.
/// Under 40 columns the dashboard is not drawn at all.
pub fn table(models: &[ModelRow], width: usize) -> Table {
    let (full, usd) = (width >= TURNS_FROM, width >= DOLLARS_FROM);
    let shown = &models[..models.len().min(MODELS_MAX)];
    let mut head = vec!["in", "out"];
    head.extend(full.then_some("turns"));
    head.extend(usd.then_some("$"));
    let cells: Vec<Vec<String>> = shown
        .iter()
        .map(|m| {
            let mut c = vec![count(m.input, m.partial), count(m.output, m.partial)];
            c.extend(full.then(|| grouped(m.turns)));
            c.extend(usd.then(|| {
                m.usd
                    .as_ref()
                    .map_or_else(String::new, |u| dollars(u, full))
            }));
            c
        })
        .collect();
    let widths: Vec<usize> = (0..head.len())
        .map(|i| {
            cells
                .iter()
                .map(|c| c[i].width())
                .chain([head[i].width()])
                .max()
                .unwrap_or(0)
        })
        .collect();
    let name = width
        .saturating_sub(PROVIDER + 1 + widths.iter().map(|w| w + 1).sum::<usize>())
        .max(MODEL_MIN);
    let line = |first: &str, model: &str, cols: &[String]| {
        let nums = cols.iter().zip(&widths).enumerate().map(|(i, (c, w))| {
            match usd && i + 1 == cols.len() {
                true => format!("{c:<w$}"),
                false => format!("{c:>w$}"),
            }
        });
        let parts = [pad(first, PROVIDER), pad(model, name)];
        let all: Vec<String> = parts.into_iter().chain(nums).collect();
        all.join(" ").trim_end().to_owned()
    };
    let titles: Vec<String> = head.iter().map(|h| (*h).to_owned()).collect();
    let rows = shown
        .iter()
        .zip(&cells)
        .map(|(m, c)| {
            let first = format!("{} {}", m.provider.glyph(), m.provider.label());
            (m.provider, line(&first, &m.model, c))
        })
        .collect();
    let mut notes = Vec::new();
    if models.len() > shown.len() {
        notes.push(format!("+{} more models", models.len() - shown.len()));
    }
    let marked = shown.iter().any(|m| {
        m.usd
            .as_ref()
            .is_some_and(|u| u.micros.is_some() && u.mode == Billing::Subscription)
    });
    if usd && !full && marked {
        notes.push("* list-price equivalent".to_owned());
    }
    Table {
        head: line("provider", "model", &titles),
        rows,
        notes,
    }
}

/// Rows the two blocks take at `width`: each a gap and a title, the table
/// with its header and notes, then the bar and its legend (or one line that
/// says there are no tokens). Nothing without models.
pub fn blocks_rows(models: &[ModelRow], shares: &[Share], width: u16) -> u16 {
    if models.is_empty() {
        return 0;
    }
    let t = table(models, usize::from(width));
    let share_rows = match shares.is_empty() {
        true => 1,
        false => 1 + share_legend(shares, usize::from(width)).len(),
    };
    let n = 2 + 1 + t.rows.len() + t.notes.len() + 2 + share_rows;
    u16::try_from(n).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests;
