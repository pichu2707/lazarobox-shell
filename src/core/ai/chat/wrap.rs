//! Hard wrapping by display width, shared by the composer and the transcript.

use std::ops::Range;

use unicode_width::UnicodeWidthChar;

/// Splits `chars` into rows of at most `width` display columns, as ranges of
/// indices. An empty line is one empty row. Every row holds at least one
/// character, so a character wider than the width still makes progress.
pub fn wrap_ranges(chars: &[char], width: usize) -> Vec<Range<usize>> {
    let width = width.max(1);
    let mut rows = Vec::new();
    let mut start = 0;
    while start < chars.len() {
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
        rows.push(start..end);
        start = end;
    }
    if rows.is_empty() {
        rows.push(0..0);
    }
    rows
}

/// Display columns of `c`: 0 for combining marks, 2 for wide characters.
pub fn char_width(c: char) -> usize {
    UnicodeWidthChar::width(c).unwrap_or(0)
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
}
