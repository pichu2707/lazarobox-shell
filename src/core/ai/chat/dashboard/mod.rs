//! The `/cost` dashboard model. Pure: it is built from the usage the store
//! read, with no clock and no colour; the UI only lays it out.

mod daily;
mod heat;
mod kpi;

pub use daily::{
    CHART_ROWS, DOWN_ROWS, DayTokens, Fit, UP_ROWS, daily, fit, last_days, legend, level, notes,
    peak,
};
pub use heat::{
    BUCKETS_BELOW, Buckets, HEAT_ROWS, Heat, HourCell, buckets, heat, heat_columns, heat_legend,
    heat_rows, rows_peak, shade,
};
pub use kpi::{Delta, Kpi, bucket, groups};

use super::usage_view::UsageRow;

/// The span the dashboard shows.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Range {
    #[default]
    Week,
    Month,
    Quarter,
    All,
}

impl Range {
    pub const ALL: [Range; 4] = [Self::Week, Self::Month, Self::Quarter, Self::All];

    /// The selector text: `7d`, `30d`, `90d`, `All`.
    pub fn label(self) -> &'static str {
        match self {
            Self::Week => "7d",
            Self::Month => "30d",
            Self::Quarter => "90d",
            Self::All => "All",
        }
    }

    /// The next range, wrapping from `All` to the week.
    pub fn next(self) -> Self {
        match self {
            Self::Week => Self::Month,
            Self::Month => Self::Quarter,
            Self::Quarter => Self::All,
            Self::All => Self::Week,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Week => Self::All,
            Self::Month => Self::Week,
            Self::Quarter => Self::Month,
            Self::All => Self::Quarter,
        }
    }

    /// The local days it covers, today included; `None` is every stored day.
    pub fn days(self) -> Option<u32> {
        match self {
            Self::Week => Some(7),
            Self::Month => Some(30),
            Self::Quarter => Some(90),
            Self::All => None,
        }
    }
}

/// Under this many columns the old text report is shown.
pub const TEXT_BELOW: u16 = 40;

/// Rows of one card: label and delta, value, sub-line, sparkline.
pub const CARD_ROWS: u16 = 4;

/// How the cards are laid out for a width.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// Four cards in a row.
    Row,
    /// A 2x2 grid.
    Grid,
    /// One column.
    Column,
    /// The text report.
    Text,
}

impl Tier {
    pub fn of(width: u16) -> Self {
        match width {
            80.. => Self::Row,
            60..80 => Self::Grid,
            TEXT_BELOW..60 => Self::Column,
            _ => Self::Text,
        }
    }
}

/// What the store read for the dashboard: counts, ids and day offsets only.
/// It is displayed and never leaves the process.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct UsageData {
    pub rows: Vec<UsageRow>,
    /// Distinct `(days ago, task key)` pairs: `c<id>` for a conversation,
    /// `r<run>` for an orchestrator run.
    pub tasks: Vec<(u32, String)>,
    /// Tokens by local weekday and hour, placed by the end time of the record.
    pub hours: Vec<HourCell>,
}

/// The model of one range.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Dashboard {
    pub range: Range,
    /// No usage falls in the range.
    pub empty: bool,
    pub kpis: [Kpi; 4],
    /// Tokens by local day, oldest first.
    pub daily: Vec<DayTokens>,
    /// Set when the chart covers only the last N days of an `All` range that
    /// reaches further back than the chart can show.
    pub last_days: Option<u32>,
    /// Tokens by weekday (Monday first) and local hour.
    pub heat: Heat,
}

impl Dashboard {
    pub fn build(data: &UsageData, range: Range) -> Self {
        Self {
            range,
            empty: kpi::is_empty(data, range),
            kpis: kpi::kpis(data, range),
            daily: daily(data, range),
            last_days: last_days(data, range),
            heat: heat(data, range),
        }
    }

    /// Rows the dashboard takes at `width`: the selector, a gap, the cards,
    /// the chart and the heatmap.
    pub fn height(&self, width: u16) -> u16 {
        let cards = match Tier::of(width) {
            Tier::Text => return 0,
            _ if self.empty => 1,
            Tier::Row => CARD_ROWS,
            Tier::Grid => 2 * CARD_ROWS + 1,
            Tier::Column => 4 * CARD_ROWS + 3,
        };
        let blocks = if self.empty {
            0
        } else {
            CHART_ROWS + HEAT_ROWS
        };
        2 + cards + blocks
    }
}

#[cfg(test)]
mod tests;
