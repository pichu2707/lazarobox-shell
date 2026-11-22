//! Tokens by day: the model of the mirrored chart.

use super::{super::usage_view::add, Range, UsageData, kpi::series_len};

/// Rows of the chart above its axis (input) and below it (output).
pub const UP_ROWS: u16 = 3;
pub const DOWN_ROWS: u16 = 3;
/// A gap, the legend, and the two halves of the chart.
pub const CHART_ROWS: u16 = 2 + UP_ROWS + DOWN_ROWS;

/// The tokens of one local day. A figure no record reported is `None`, so a
/// day with no row is empty and never a known zero.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct DayTokens {
    pub input: Option<u64>,
    pub output: Option<u64>,
}

/// The days the chart covers, oldest first: the same span as the sparklines,
/// so the chart adds up to the cards.
pub fn daily(data: &UsageData, range: Range) -> Vec<DayTokens> {
    let len = series_len(data, range);
    let mut days = vec![DayTokens::default(); len as usize];
    for r in &data.rows {
        // Oldest first: today is the last day.
        let Some(at) = len.checked_sub(r.days_ago.saturating_add(1)) else {
            continue;
        };
        let day = &mut days[at as usize];
        day.input = add(day.input, r.input);
        day.output = add(day.output, r.output);
    }
    days
}

/// Days summed into buckets so they fit a width.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Fit {
    pub days: Vec<DayTokens>,
    /// Days per bucket.
    pub size: usize,
}

/// At most `cols` buckets of equal size, counted back from today so the
/// newest bucket is whole. A bucket of unknown days stays unknown.
pub fn fit(days: &[DayTokens], cols: usize) -> Fit {
    if cols == 0 {
        return Fit {
            days: Vec::new(),
            size: 1,
        };
    }
    let size = days.len().div_ceil(cols).max(1);
    let mut fitted: Vec<DayTokens> = days
        .rchunks(size)
        .map(|c| {
            c.iter().fold(DayTokens::default(), |a, d| DayTokens {
                input: add(a.input, d.input),
                output: add(a.output, d.output),
            })
        })
        .collect();
    fitted.reverse();
    Fit { days: fitted, size }
}

/// Steps of a bar for `value` when `max` fills `steps`: never 0 for a
/// non-zero value.
pub fn level(value: u64, max: u64, steps: u64) -> u64 {
    if max == 0 {
        return 0;
    }
    let n = (u128::from(value) * u128::from(steps)).div_ceil(u128::from(max));
    u64::try_from(n).unwrap_or(steps).min(steps)
}

/// The largest figure of the chart: both halves share one scale.
pub fn peak(days: &[DayTokens]) -> u64 {
    days.iter()
        .flat_map(|d| [d.input, d.output])
        .flatten()
        .max()
        .unwrap_or(0)
}

/// The legend, in the longest form that fits `width` columns; it always says
/// the bucket size.
pub fn legend(size: usize, width: usize) -> String {
    let unit = match size {
        1 => "1 bar = 1 day".to_owned(),
        n => format!("1 bar = {n} days"),
    };
    [
        format!("Tokens by day · in above, out below · {unit}"),
        format!("in above · out below · {unit}"),
        format!("in ↑ out ↓ · {size}d/bar"),
    ]
    .into_iter()
    .find(|text| text.chars().count() <= width)
    .unwrap_or_else(|| format!("in out {size}d"))
}
