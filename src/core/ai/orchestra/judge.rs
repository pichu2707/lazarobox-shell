//! Acceptance judges (acceptance-judges S1a): how many judges a staged
//! change gets, the standard they rule against, the judge prompt and the
//! verdict parser. Pure: no process, no file.

use super::changes::Change;
use super::review::Tier;
use super::steps::Size;

/// Staged text at or above this many bytes counts as a big change.
pub const BIG_BYTES: u64 = 16 * 1024;

/// How many judges a change gets.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JudgeLevel {
    None,
    One,
    Two,
}

/// The judge level (D1): none for a read-only goal; two for a Large goal,
/// more than one file, a tier of medium or more, or a big staged text; one
/// otherwise. An unknown size reads as small.
pub fn level(
    read_only: bool,
    size: Option<Size>,
    changes: &[Change],
    tier: Tier,
) -> JudgeLevel {
    if read_only {
        return JudgeLevel::None;
    }
    if size == Some(Size::Large)
        || changes.len() > 1
        || tier >= Tier::Medium
        || staged_bytes(changes) >= BIG_BYTES
    {
        return JudgeLevel::Two;
    }
    JudgeLevel::One
}

fn staged_bytes(changes: &[Change]) -> u64 {
    changes.iter().filter_map(|change| change.size).sum()
}

/// The transcript line that says the level and its inputs; none for `None`.
pub fn level_line(
    level: JudgeLevel,
    size: Option<Size>,
    changes: &[Change],
    tier: Tier,
) -> Option<String> {
    let judges = match level {
        JudgeLevel::None => return None,
        JudgeLevel::One => "one judge",
        JudgeLevel::Two => "two judges",
    };
    let size = size.map_or("unknown", Size::label);
    Some(format!(
        "judge: {judges} \u{b7} {} file(s), {} B \u{b7} tier {} \u{b7} size {size}",
        changes.len(),
        staged_bytes(changes),
        tier.as_str(),
    ))
}

#[cfg(test)]
mod tests;
