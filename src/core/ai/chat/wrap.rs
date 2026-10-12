//! Hard wrapping by display width, shared by the composer and the transcript.

use std::ops::Range;

use unicode_width::UnicodeWidthChar;

/// Splits `chars` into rows of at most `width` display columns, as ranges of
/// indices. An empty line is one empty row. Every row holds at least one
/// character, so a character wider than the width still makes progress.
pub fn wrap_ranges(chars: &[char], width: usize) -> Vec<Range<usize>> {
    let mut rows = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let end = row_end(chars, start, width);
        rows.push(start..end);
        start = end;
    }
    if rows.is_empty() {
        rows.push(0..0);
    }
    rows
}

/// Where the row that starts at `start` ends: as many characters as fit in
/// `width` columns, and at least one.
fn row_end(chars: &[char], start: usize, width: usize) -> usize {
    let width = width.max(1);
    let mut used = 0;
    let mut end = start;
    while end < chars.len() {
        let w = char_width(chars[end]);
        if end > start && used + w > width {
            break;
        }
        used += w;
        end += 1;
    }
    end
}

/// Display columns of `c`: 0 for combining marks, 2 for wide characters.
pub fn char_width(c: char) -> usize {
    UnicodeWidthChar::width(c).unwrap_or(0)
}

/// Wraps `line` into rows of at most `width` columns, breaking at spaces
/// where it can. A word longer than a row is broken hard. The spaces at a
/// break are dropped; leading spaces of the line are kept.
pub fn wrap_words(line: &str, width: usize) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut rows = Vec::new();
    let mut start = 0;
    loop {
        // The most the row could hold; it is cut back to a space if it ends
        // inside a word.
        let fit = row_end(&chars, start, width);
        if fit == chars.len() {
            rows.push(chars[start..].iter().collect());
            return rows;
        }
        let first_word = start + chars[start..].iter().take_while(|&&c| c == ' ').count();
        let cut = if chars[fit] == ' ' {
            fit
        } else {
            chars[first_word.min(fit)..fit]
                .iter()
                .rposition(|&c| c == ' ')
                .map_or(fit, |i| first_word.min(fit) + i)
        };
        let row: String = chars[start..cut].iter().collect();
        rows.push(row.trim_end().to_owned());
        start = cut;
        while chars.get(start) == Some(&' ') {
            start += 1;
        }
        if start >= chars.len() {
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
    fn empty_and_tiny_widths_still_yield_rows() {
        assert_eq!(wrap_words("", 5), [""]);
        assert_eq!(wrap_words("a b", 0), ["a", "b"]);
        assert_eq!(wrap_words("日本 語", 4), ["日本", "語"]);
    }
}
