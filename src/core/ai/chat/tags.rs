//! Conversation tags: how a tag is cleaned. Pure.

use crate::core::ai::sanitize::{clean, is_format};

/// The longest tag, in characters. The rest is cut.
pub const TAG_MAX: usize = 64;

/// A tag as it is kept: sanitized, trimmed, without control or format
/// characters (bidi overrides, zero-width marks), a comma turned into `_`
/// (a comma separates tags in the editor) and at most `TAG_MAX` characters.
/// A tag that is empty afterwards is no tag.
pub fn clean_tag(raw: &str) -> Option<String> {
    let kept: String = clean(raw)
        .chars()
        .filter(|c| !c.is_control() && !is_format(*c))
        .map(|c| if c == ',' { '_' } else { c })
        .collect();
    let cut: String = kept.trim().chars().take(TAG_MAX).collect();
    let tag = cut.trim_end();
    (!tag.is_empty()).then(|| tag.to_owned())
}

/// The most extra tags a conversation has.
pub const EXTRA_MAX: usize = 8;

/// The tags of a conversation: the project tag and up to `EXTRA_MAX` extras.
/// Every tag is cleaned, none repeats (ignoring case) and none is empty.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Tags {
    pub project: Option<String>,
    pub extras: Vec<String>,
}

impl Tags {
    /// Tags from raw entries: each cleaned with [`clean_tag`], an empty one
    /// dropped, a repeat (ignoring case, the project tag included) kept once
    /// and the extras cut to `EXTRA_MAX`. Normalizing is idempotent.
    pub fn new(project: Option<String>, extras: Vec<String>) -> Self {
        let project = project.as_deref().and_then(clean_tag);
        let mut seen: Vec<String> = project.iter().map(|tag| tag.to_lowercase()).collect();
        let mut kept = Vec::new();
        for tag in extras.iter().filter_map(|raw| clean_tag(raw)) {
            if kept.len() == EXTRA_MAX {
                break;
            }
            let key = tag.to_lowercase();
            if !seen.contains(&key) {
                seen.push(key);
                kept.push(tag);
            }
        }
        Self {
            project,
            extras: kept,
        }
    }

    /// Tags from the editor line: comma separated, the first entry is the
    /// project tag (empty means none), the rest are extras.
    pub fn parse(line: &str) -> Self {
        let mut entries = line.split(',');
        let project = entries.next().map(str::to_owned);
        Self::new(project, entries.map(str::to_owned).collect())
    }

    /// The editor line for these tags: `project, extra, extra`.
    pub fn line(&self) -> String {
        if self.project.is_none() && self.extras.is_empty() {
            return String::new();
        }
        let first = self.project.as_deref().unwrap_or("");
        std::iter::once(first)
            .chain(self.extras.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[cfg(test)]
mod tests;
