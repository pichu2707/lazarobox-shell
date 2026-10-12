//! Hard wrapping by display width, shared by the composer and the transcript.
//!
//! Widths are measured per grapheme cluster, as ratatui draws them: a VS16
//! heart or a skin-toned thumb is one 2-column cell, not the sum of its
//! chars.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// One grapheme cluster of a char slice.
#[derive(Clone, Debug)]
pub struct Cluster {
    /// Char indices of the cluster in the slice.
    pub chars: Range<usize>,
    /// Display columns.
    pub width: usize,
    /// A lone space.
    pub space: bool,
}

/// The grapheme clusters of `chars`, in order.
pub fn clusters(chars: &[char]) -> Vec<Cluster> {
    let text: String = chars.iter().collect();
    let mut at = 0;
    text.graphemes(true)
        .map(|g| {
            let n = g.chars().count();
            let cluster = Cluster {
                chars: at..at + n,
                width: UnicodeWidthStr::width(g),
                space: g == " ",
            };
            at += n;
            cluster
        })
        .collect()
}

/// Display columns of `s`, summed per grapheme cluster like ratatui draws it.
pub fn text_width(s: &str) -> usize {
    s.graphemes(true).map(UnicodeWidthStr::width).sum()
}

/// Columns between tab stops. Terminals draw nothing for a tab cell, so tabs
/// are turned into spaces before anything is measured or drawn. 4 keeps tab
/// indented code (Go, Makefiles) readable in a narrow pane, where 8 would eat
/// the width after two levels.
pub const TAB_WIDTH: usize = 4;

/// The spaces a tab at display column `col` stands for: up to the next stop,
/// and at least one.
pub fn tab_pad(col: usize) -> usize {
    TAB_WIDTH - col % TAB_WIDTH
}

/// `s` with every tab replaced by spaces up to the next tab stop. Columns
/// count display width and restart after a newline.
pub fn expand_tabs(s: &str) -> String {
    if !s.contains('\t') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut col = 0;
    for g in s.graphemes(true) {
        match g {
            "\t" => {
                let pad = tab_pad(col);
                out.extend(std::iter::repeat_n(' ', pad));
                col += pad;
            }
            "\n" | "\r\n" => {
                out.push_str(g);
                col = 0;
            }
            _ => {
                out.push_str(g);
                col += UnicodeWidthStr::width(g);
            }
        }
    }
    out
}

/// Splits `chars` into rows of at most `width` display columns, as ranges of
/// char indices. An empty line is one empty row. Every row holds at least one
/// cluster, so a cluster wider than the width still makes progress, and no
/// row ends inside a cluster.
pub fn wrap_ranges(chars: &[char], width: usize) -> Vec<Range<usize>> {
    let cl = clusters(chars);
    let mut rows = Vec::new();
    let mut start = 0;
    while start < cl.len() {
        let end = row_end(&cl, start, width);
        rows.push(cl[start].chars.start..cl[end - 1].chars.end);
        start = end;
    }
    if rows.is_empty() {
        rows.push(0..0);
    }
    rows
}

/// Where the row that starts at cluster `start` ends: as many clusters as
/// fit in `width` columns, and at least one.
fn row_end(cl: &[Cluster], start: usize, width: usize) -> usize {
    let width = width.max(1);
    let mut used = 0;
    let mut end = start;
    while end < cl.len() {
        let w = cl[end].width;
        if end > start && used + w > width {
            break;
        }
        used += w;
        end += 1;
    }
    end
}

/// Wraps `line` into rows of at most `width` columns, breaking at spaces
/// where it can. A word longer than a row is broken hard. The spaces at a
/// break are dropped; leading spaces of the line are kept.
pub fn wrap_words(line: &str, width: usize) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let cl = clusters(&chars);
    let text = |a: usize, b: usize| -> String {
        if a >= b {
            return String::new();
        }
        chars[cl[a].chars.start..cl[b - 1].chars.end]
            .iter()
            .collect()
    };
    let mut rows = Vec::new();
    let mut start = 0;
    loop {
        // The most the row could hold; it is cut back to a space if it ends
        // inside a word.
        let fit = row_end(&cl, start, width);
        if fit == cl.len() {
            rows.push(text(start, cl.len()));
            return rows;
        }
        let first_word = start + cl[start..].iter().take_while(|c| c.space).count();
        let cut = if cl[fit].space {
            fit
        } else {
            cl[first_word.min(fit)..fit]
                .iter()
                .rposition(|c| c.space)
                .map_or(fit, |i| first_word.min(fit) + i)
        };
        // An indent wider than the row leaves nothing to show: no empty row.
        let row = text(start, cut).trim_end().to_owned();
        if !row.is_empty() {
            rows.push(row);
        }
        start = cut;
        while cl.get(start).is_some_and(|c| c.space) {
            start += 1;
        }
        if start >= cl.len() {
            if rows.is_empty() {
                rows.push(String::new());
            }
            return rows;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(text: &str, width: usize) -> Vec<String> {
        let chars: Vec<char> = text.chars().collect();
        wrap_ranges(&chars, width)
            .into_iter()
            .map(|r| chars[r].iter().collect())
            .collect()
    }

    #[test]
    fn a_short_line_is_one_row() {
        assert_eq!(rows("hello", 10), ["hello"]);
    }

    #[test]
    fn a_long_line_breaks_at_the_width() {
        assert_eq!(rows("abcdefg", 3), ["abc", "def", "g"]);
        assert_eq!(rows("abcdef", 3), ["abc", "def"]);
    }

    #[test]
    fn an_empty_line_is_one_empty_row() {
        assert_eq!(rows("", 5), [""]);
    }

    #[test]
    fn wide_characters_count_two_columns() {
        assert_eq!(rows("日本語", 4), ["日本", "語"]);
        // A wide character that does not fit the row starts the next one.
        assert_eq!(rows("a日", 2), ["a", "日"]);
    }

    #[test]
    fn a_character_wider_than_the_width_still_makes_progress() {
        assert_eq!(rows("日日", 1), ["日", "日"]);
        assert_eq!(rows("ab", 0), ["a", "b"]);
    }

    #[test]
    fn zero_width_marks_stay_with_their_base() {
        assert_eq!(rows("e\u{301}x", 1), ["e\u{301}", "x"]);
    }

    #[test]
    fn a_cluster_is_measured_as_a_whole_not_per_char() {
        // VS16 makes the heart an emoji: 2 columns, not 1.
        assert_eq!(rows("\u{2764}\u{fe0f}x", 2), ["\u{2764}\u{fe0f}", "x"]);
        // A skin tone modifier joins the thumb: 2 columns, not 4.
        assert_eq!(rows("\u{1f44d}\u{1f3fd}\u{1f44d}\u{1f3fd}", 2).len(), 2);
        assert_eq!(rows("\u{1f44d}\u{1f3fd}ab", 4), ["\u{1f44d}\u{1f3fd}ab"]);
        // A ZWJ family is one 2-column cluster.
        let family = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}\u{200d}\u{1f466}";
        assert_eq!(rows(&format!("{family}ab"), 4), [format!("{family}ab")]);
        assert_eq!(rows(&format!("{family}{family}"), 2).len(), 2);
    }

    #[test]
    fn text_width_sums_cluster_widths() {
        assert_eq!(text_width("\u{2764}\u{fe0f}"), 2);
        assert_eq!(text_width("\u{1f44d}\u{1f3fd}"), 2);
        assert_eq!(
            text_width("\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}\u{200d}\u{1f466}"),
            2
        );
        assert_eq!(text_width("日a"), 3);
        assert_eq!(text_width("e\u{301}"), 1);
    }

    #[test]
    fn tabs_expand_to_the_next_stop() {
        assert_eq!(expand_tabs("\tx"), "    x");
        assert_eq!(expand_tabs("ab\tc"), "ab  c");
        assert_eq!(expand_tabs("abcd\te"), "abcd    e");
        assert_eq!(expand_tabs("a\t\tb"), "a       b");
        // Stops count display columns, not chars.
        assert_eq!(expand_tabs("日\tx"), "日  x");
        assert_eq!(expand_tabs("no tabs"), "no tabs");
    }

    #[test]
    fn words_break_at_spaces() {
        assert_eq!(
            wrap_words("alpha beta gamma delta", 16),
            ["alpha beta gamma", "delta"]
        );
        assert_eq!(wrap_words("alpha beta gamma", 12), ["alpha beta", "gamma"]);
        assert_eq!(wrap_words("one two", 7), ["one two"]);
    }

    #[test]
    fn a_long_word_is_broken_hard_and_the_rest_continues() {
        assert_eq!(wrap_words("abcdefghij kl", 4), ["abcd", "efgh", "ij", "kl"]);
        assert_eq!(wrap_words("ab cdefghij", 4), ["ab", "cdef", "ghij"]);
    }

    #[test]
    fn runs_of_spaces_at_a_break_are_dropped_but_indent_stays() {
        assert_eq!(wrap_words("aaa    bbb", 4), ["aaa", "bbb"]);
        assert_eq!(wrap_words("  indented text", 8), ["  indent", "ed text"]);
        assert_eq!(wrap_words("    x", 10), ["    x"]);
    }

    #[test]
    fn an_indent_wider_than_the_width_does_not_leave_an_empty_row() {
        assert_eq!(wrap_words("    x", 2), ["x"]);
        assert_eq!(wrap_words("      abc def", 3), ["abc", "def"]);
        assert_eq!(wrap_words("     ", 2), [""], "a blank line stays one row");
    }

    #[test]
    fn empty_and_tiny_widths_still_yield_rows() {
        assert_eq!(wrap_words("", 5), [""]);
        assert_eq!(wrap_words("a b", 0), ["a", "b"]);
        assert_eq!(wrap_words("日本 語", 4), ["日本", "語"]);
    }
}
