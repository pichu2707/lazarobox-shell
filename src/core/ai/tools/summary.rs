//! The short texts a tool row shows. Pure.
//!
//! A row is a summary, never content: the argument of a call or one line
//! about its result, flattened to one line, sanitized and capped.

use crate::core::ai::sanitize::clean;

/// The most characters of a summary. A longer text is cut and ends in `…`.
pub const SUMMARY_CAP: usize = 120;

/// `text` as one sanitized line of at most [`SUMMARY_CAP`] characters:
/// control and escape sequences are gone, every run of whitespace (newlines
/// included) is one space.
pub fn clip(text: &str) -> String {
    let flat = clean(text).split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= SUMMARY_CAP {
        return flat;
    }
    let mut cut: String = flat.chars().take(SUMMARY_CAP - 1).collect();
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_plain_text_is_kept() {
        assert_eq!(clip("curso-rust"), "curso-rust");
    }

    #[test]
    fn lines_and_runs_of_blanks_become_single_spaces() {
        assert_eq!(clip("  a\n\n b\t\tc \r\n"), "a b c");
    }

    #[test]
    fn escape_sequences_and_controls_do_not_survive() {
        let out = clip("a\u{1b}[31mred\u{1b}[0m \u{7}b\u{1b}]0;title\u{7}c");
        assert!(!out.contains('\u{1b}') && !out.contains('\u{7}'), "{out:?}");
        assert!(out.starts_with('a') && out.contains("red"), "{out:?}");
    }

    #[test]
    fn a_long_text_is_cut_on_a_character_boundary_with_an_ellipsis() {
        let out = clip(&"é".repeat(SUMMARY_CAP * 2));
        assert_eq!(out.chars().count(), SUMMARY_CAP);
        assert!(out.ends_with('…'));
        let exact = "x".repeat(SUMMARY_CAP);
        assert_eq!(clip(&exact), exact);
    }

    #[test]
    fn nothing_but_blanks_is_empty() {
        assert_eq!(clip(" \n\t"), "");
    }
}
