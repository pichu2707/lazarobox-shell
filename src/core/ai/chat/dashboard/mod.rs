//! The `/cost` dashboard model. Pure: it is built from the usage the store
//! read, with no clock and no colour; the UI only lays it out.

mod kpi;

pub use kpi::{Delta, Kpi, bucket};

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

/// What the store read for the dashboard: counts, ids and day offsets only.
/// It is displayed and never leaves the process.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct UsageData {
    pub rows: Vec<UsageRow>,
    /// Distinct `(days ago, task key)` pairs: `c<id>` for a conversation,
    /// `r<run>` for an orchestrator run.
    pub tasks: Vec<(u32, String)>,
}

/// The model of one range.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Dashboard {
    pub range: Range,
    /// No usage falls in the range.
    pub empty: bool,
    pub kpis: [Kpi; 4],
}

impl Dashboard {
    pub fn build(data: &UsageData, range: Range) -> Self {
        Self {
            range,
            empty: kpi::is_empty(data, range),
            kpis: kpi::kpis(data, range),
        }
    }
}

#[cfg(test)]
mod tests;
