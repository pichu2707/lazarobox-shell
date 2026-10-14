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

#[cfg(test)]
mod tests;
