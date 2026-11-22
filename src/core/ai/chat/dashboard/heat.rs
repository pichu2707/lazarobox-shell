//! Tokens by weekday and local hour: the model of the heatmap.

use super::{Range, UsageData, daily::level};

/// A gap, the legend, the hour axis and one row a weekday.
pub const HEAT_ROWS: u16 = 3 + 7;
/// Under this many columns the hours are grouped by three.
pub const BUCKETS_BELOW: u16 = 60;

/// Shades from the least to the most: a cell with rows is never blank.
const SHADES: [char; 4] = ['░', '▒', '▓', '█'];
/// A cell with rows and no known tokens.
const ZERO: char = '·';

/// The tokens the records of one weekday and local hour hold, on one local
/// day. The weekday is 0 for Monday.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HourCell {
    pub days_ago: u32,
    pub weekday: u8,
    pub hour: u8,
    pub tokens: u64,
}

/// Weekday (Monday first) by hour. An hour no record falls in is `None`.
pub type Heat = [[Option<u64>; 24]; 7];

/// Hours grouped by three.
pub type Buckets = [[Option<u64>; 8]; 7];

/// The hours of the range, summed by weekday and hour.
pub fn heat(data: &UsageData, range: Range) -> Heat {
    let mut grid: Heat = [[None; 24]; 7];
    let days = range.days().unwrap_or(u32::MAX);
    for c in data.hours.iter().filter(|c| c.days_ago < days) {
        let (Some(row), true) = (grid.get_mut(usize::from(c.weekday)), c.hour < 24) else {
            continue;
        };
        let cell = &mut row[usize::from(c.hour)];
        *cell = Some(cell.unwrap_or(0).saturating_add(c.tokens));
    }
    grid
}

/// The hours grouped by three: a bucket with no hour known stays `None`.
pub fn buckets(heat: &Heat) -> Buckets {
    let mut out: Buckets = [[None; 8]; 7];
    for (day, row) in heat.iter().enumerate() {
        for (hour, v) in row.iter().enumerate() {
            let Some(v) = v else { continue };
            let cell = &mut out[day][hour / 3];
            *cell = Some(cell.unwrap_or(0).saturating_add(*v));
        }
    }
    out
}

/// Columns the grid has at `width`: 24 hours, or 8 buckets when narrow.
pub fn heat_columns(width: u16) -> usize {
    if width < BUCKETS_BELOW { 8 } else { 24 }
}

/// The largest cell of the grid.
pub fn heat_peak(heat: &Heat) -> u64 {
    heat.iter().flatten().flatten().copied().max().unwrap_or(0)
}

/// The glyph of a cell when `max` is the darkest: ` ` for no record,
/// `░▒▓█` by quarter, and `·` for records with no tokens.
pub fn shade(value: Option<u64>, max: u64) -> char {
    match value {
        None => ' ',
        Some(0) => ZERO,
        Some(v) => SHADES[usize::try_from(level(v, max, 4).max(1) - 1).unwrap_or(3)],
    }
}

/// The legend in the longest form that fits `width` columns; with nothing to
/// show it says so.
pub fn heat_legend(empty: bool, width: usize) -> String {
    if empty {
        return "no hourly usage in this range".to_owned();
    }
    let scale = "░ ▒ ▓ █ low to high";
    [
        format!("Busy hours · weekday x local hour · {scale}"),
        format!("Hours · local time · {scale}"),
    ]
    .into_iter()
    .find(|text| text.chars().count() <= width)
    .unwrap_or_else(|| format!("local · {scale}"))
}
