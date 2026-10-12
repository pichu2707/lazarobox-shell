//! The message box of a chat pane: a small multi-line editor. Pure.

use std::ops::Range;

use super::wrap::{clusters, wrap_ranges};
use crate::core::ai::sanitize::is_format;

/// The most characters the composer holds.
pub const COMPOSER_CAP: usize = 8000;
/// The most rows the composer takes on screen.
pub const MAX_ROWS: usize = 5;

/// What the composer shows: at most `max_rows` rows and where the cursor is
/// among them.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ComposerView {
    pub rows: Vec<String>,
    pub cursor_row: usize,
    pub cursor_col: usize,
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Composer {
    chars: Vec<char>,
    cursor: usize,
}

impl Composer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    /// The cursor as a character index.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn len(&self) -> usize {
        self.chars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    /// Types one character. Control and format characters are dropped, and
    /// so is anything past the cap.
    pub fn insert(&mut self, c: char) {
        if is_text(c) {
            self.put(c);
        }
    }

    pub fn newline(&mut self) {
        self.put('\n');
    }

    /// Inserts pasted text: CRLF becomes a newline, every other control or
    /// format character is dropped, and what does not fit under the cap is
    /// cut off.
    pub fn paste(&mut self, text: &str) {
        for c in text.chars() {
            if c == '\n' {
                self.newline();
            } else {
                self.insert(c);
            }
        }
    }

    fn put(&mut self, c: char) {
        if self.chars.len() < COMPOSER_CAP {
            self.chars.insert(self.cursor, c);
            self.cursor += 1;
            // A joiner typed in the middle can fuse the neighbours into one
            // cluster; the cursor moves to its end.
            if let Some(cluster) = clusters(&self.chars)
                .into_iter()
                .find(|k| k.chars.start < self.cursor && self.cursor < k.chars.end)
            {
                self.cursor = cluster.chars.end;
            }
        }
    }

    /// Char range of the grapheme cluster that ends at the cursor.
    fn cluster_before(&self) -> Option<Range<usize>> {
        clusters(&self.chars)
            .into_iter()
            .map(|k| k.chars)
            .find(|r| r.end >= self.cursor && r.start < self.cursor)
    }

    /// Char range of the grapheme cluster that starts at the cursor.
    fn cluster_after(&self) -> Option<Range<usize>> {
        clusters(&self.chars)
            .into_iter()
            .map(|k| k.chars)
            .find(|r| r.start == self.cursor)
    }

    /// Removes the whole cluster before the cursor.
    pub fn backspace(&mut self) {
        if let Some(r) = self.cluster_before() {
            self.chars.drain(r.clone());
            self.cursor = r.start;
        }
    }

    /// Removes the whole cluster at the cursor.
    pub fn delete(&mut self) {
        if let Some(r) = self.cluster_after() {
            self.chars.drain(r);
        }
    }

    /// One cluster left, never into the middle of one.
    pub fn left(&mut self) {
        if let Some(r) = self.cluster_before() {
            self.cursor = r.start;
        }
    }

    /// One cluster right, never into the middle of one.
    pub fn right(&mut self) {
        if let Some(r) = self.cluster_after() {
            self.cursor = r.end;
        }
    }

    /// Start of the line the cursor is on.
    pub fn home(&mut self) {
        self.cursor = self.line_start();
    }

    /// End of the line the cursor is on, before its newline.
    pub fn end(&mut self) {
        self.cursor = self.line_end();
    }

    fn line_start(&self) -> usize {
        self.chars[..self.cursor]
            .iter()
            .rposition(|&c| c == '\n')
            .map_or(0, |i| i + 1)
    }

    fn line_end(&self) -> usize {
        self.chars[self.cursor..]
            .iter()
            .position(|&c| c == '\n')
            .map_or(self.chars.len(), |i| self.cursor + i)
    }

    pub fn clear(&mut self) {
        self.chars.clear();
        self.cursor = 0;
    }

    /// The text, leaving the composer empty.
    pub fn take(&mut self) -> String {
        let text = self.text();
        self.clear();
        text
    }

    /// Every wrapped row of the text, as character ranges, and the cursor
    /// among them. A cursor past a full row sits on a fresh empty row.
    fn layout(&self, width: usize) -> (Vec<Range<usize>>, usize, usize) {
        let mut rows: Vec<Range<usize>> = Vec::new();
        let mut cursor = (0, 0);
        let mut line_start = 0;
        loop {
            let line_end = self.chars[line_start..]
                .iter()
                .position(|&c| c == '\n')
                .map_or(self.chars.len(), |i| line_start + i);
            let wrapped = wrap_ranges(&self.chars[line_start..line_end], width);
            let last = wrapped.len() - 1;
            for (n, r) in wrapped.into_iter().enumerate() {
                let r = line_start + r.start..line_start + r.end;
                let here = self.cursor >= r.start && self.cursor < r.end;
                let at_line_end = n == last && self.cursor == r.end && self.cursor == line_end;
                if here || at_line_end {
                    let col = clusters(&self.chars[r.start..self.cursor])
                        .iter()
                        .map(|k| k.width)
                        .sum();
                    cursor = (rows.len(), col);
                }
                rows.push(r);
                if at_line_end && cursor.1 >= width.max(1) {
                    rows.push(r_empty(line_end));
                    cursor = (rows.len() - 1, 0);
                }
            }
            if line_end >= self.chars.len() {
                break;
            }
            line_start = line_end + 1;
        }
        (rows, cursor.0, cursor.1)
    }

    /// The rows for a pane `width` columns wide, cut to `max_rows` around
    /// the cursor.
    pub fn view(&self, width: usize, max_rows: usize) -> ComposerView {
        let (rows, cursor_row, cursor_col) = self.layout(width);
        let max_rows = max_rows.max(1);
        let first = (cursor_row + 1)
            .saturating_sub(max_rows)
            .min(rows.len().saturating_sub(max_rows));
        let shown = rows
            .iter()
            .skip(first)
            .take(max_rows)
            .map(|r| self.chars[r.clone()].iter().collect())
            .collect();
        ComposerView {
            rows: shown,
            cursor_row: cursor_row - first,
            cursor_col,
        }
    }

    /// How many rows the text needs at `width`, up to `MAX_ROWS`.
    pub fn height(&self, width: usize) -> usize {
        self.layout(width).0.len().min(MAX_ROWS)
    }
}

fn r_empty(at: usize) -> Range<usize> {
    at..at
}

/// A character the composer accepts: not a control or format character,
/// except the ZWJ, which glues emoji sequences together. The variation
/// selectors (U+FE0E, U+FE0F) are not format characters and pass already.
fn is_text(c: char) -> bool {
    c == '\u{200d}' || (!c.is_control() && !is_format(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(text: &str) -> Composer {
        let mut c = Composer::new();
        for ch in text.chars() {
            if ch == '\n' {
                c.newline();
            } else {
                c.insert(ch);
            }
        }
        c
    }

    #[test]
    fn typing_and_backspace_edit_the_text() {
        let mut c = typed("hi!");
        assert_eq!(c.text(), "hi!");
        assert_eq!(c.cursor(), 3);
        c.backspace();
        assert_eq!(c.text(), "hi");
        c.backspace();
        c.backspace();
        c.backspace();
        assert_eq!(c.text(), "");
        assert_eq!(c.cursor(), 0);
    }

    #[test]
    fn editing_happens_at_the_cursor() {
        let mut c = typed("ac");
        c.left();
        c.insert('b');
        assert_eq!(c.text(), "abc");
        assert_eq!(c.cursor(), 2);
        c.delete();
        assert_eq!(c.text(), "ab");
        c.delete();
        assert_eq!(c.text(), "ab", "Delete at the end does nothing");
        c.home();
        c.backspace();
        assert_eq!(c.text(), "ab", "Backspace at the start does nothing");
    }

    #[test]
    fn left_and_right_clamp_and_step_over_whole_characters() {
        let mut c = typed("é日");
        c.left();
        assert_eq!(c.cursor(), 1);
        c.left();
        c.left();
        assert_eq!(c.cursor(), 0);
        c.right();
        c.right();
        c.right();
        assert_eq!(c.cursor(), 2);
        c.left();
        c.insert('x');
        assert_eq!(c.text(), "éx日");
    }

    #[test]
    fn home_and_end_move_within_the_current_line() {
        let mut c = typed("ab\ncd");
        c.left();
        c.home();
        assert_eq!(c.cursor(), 3);
        c.end();
        assert_eq!(c.cursor(), 5);
        c.left();
        c.left();
        c.left();
        c.left();
        assert_eq!(c.cursor(), 1);
        c.end();
        assert_eq!(c.cursor(), 2, "End stops before the newline");
        c.home();
        assert_eq!(c.cursor(), 0);
    }

    #[test]
    fn a_newline_is_inserted_where_the_cursor_is() {
        let mut c = typed("ab");
        c.left();
        c.newline();
        assert_eq!(c.text(), "a\nb");
        assert_eq!(c.cursor(), 2);
    }

    #[test]
    fn clear_empties_the_composer_and_take_returns_the_text() {
        let mut c = typed("abc");
        c.clear();
        assert_eq!(c.text(), "");
        assert_eq!(c.cursor(), 0);
        let mut d = typed("xyz");
        assert_eq!(d.take(), "xyz");
        assert!(d.is_empty());
        assert_eq!(d.cursor(), 0);
    }

    #[test]
    fn typed_control_and_format_characters_are_dropped() {
        let mut c = Composer::new();
        for ch in [
            'a', '\u{1b}', '\t', '\r', '\u{7f}', '\u{200b}', '\u{9b}', 'b',
        ] {
            c.insert(ch);
        }
        assert_eq!(c.text(), "ab");
    }

    #[test]
    fn a_paste_turns_crlf_into_newline_and_drops_controls() {
        let mut c = Composer::new();
        c.paste("x\r\ny\u{1b}[31m");
        assert_eq!(c.text(), "x\ny[31m");
        assert_eq!(c.cursor(), 7);
    }

    #[test]
    fn a_paste_drops_lone_cr_and_format_characters_and_inserts_at_the_cursor() {
        let mut c = typed("ad");
        c.left();
        c.paste("b\rc\u{200b}\n");
        assert_eq!(c.text(), "abc\nd");
        assert_eq!(c.cursor(), 4);
    }

    #[test]
    fn the_cap_stops_typing() {
        let mut c = Composer::new();
        for _ in 0..COMPOSER_CAP {
            c.insert('a');
        }
        assert_eq!(c.len(), COMPOSER_CAP);
        c.insert('b');
        c.newline();
        assert_eq!(c.len(), COMPOSER_CAP);
        assert!(!c.text().contains('b'));
        c.backspace();
        c.insert('b');
        assert_eq!(c.len(), COMPOSER_CAP);
        assert!(c.text().ends_with('b'));
    }

    #[test]
    fn a_paste_over_the_cap_keeps_what_fits() {
        let mut c = typed("start");
        c.paste(&"z".repeat(COMPOSER_CAP + 50));
        assert_eq!(c.len(), COMPOSER_CAP);
        assert!(c.text().starts_with("startz"));
        assert_eq!(c.cursor(), COMPOSER_CAP);
    }

    #[test]
    fn the_cap_counts_characters_not_bytes() {
        let mut c = Composer::new();
        c.paste(&"日".repeat(COMPOSER_CAP));
        assert_eq!(c.len(), COMPOSER_CAP);
    }

    #[test]
    fn view_wraps_rows_and_places_the_cursor() {
        let c = typed("abcdefg");
        let v = c.view(3, 5);
        assert_eq!(v.rows, ["abc", "def", "g"]);
        assert_eq!((v.cursor_row, v.cursor_col), (2, 1));
    }

    #[test]
    fn a_cursor_after_a_full_row_sits_on_a_fresh_row() {
        let c = typed("abcdef");
        let v = c.view(3, 5);
        assert_eq!(v.rows, ["abc", "def", ""]);
        assert_eq!((v.cursor_row, v.cursor_col), (2, 0));
    }

    #[test]
    fn view_follows_newlines_and_a_cursor_in_the_middle() {
        let mut c = typed("ab\ncd");
        for _ in 0..4 {
            c.left();
        }
        let v = c.view(10, 5);
        assert_eq!(v.rows, ["ab", "cd"]);
        assert_eq!((v.cursor_row, v.cursor_col), (0, 1));
    }

    #[test]
    fn the_cursor_column_counts_display_width() {
        let c = typed("日a");
        let v = c.view(10, 5);
        assert_eq!((v.cursor_row, v.cursor_col), (0, 3));
    }

    #[test]
    fn the_cursor_column_counts_clusters_not_chars() {
        for (text, cols) in [
            ("\u{2764}\u{fe0f}", 2),
            ("\u{1f44d}\u{1f3fd}", 2),
            (
                "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}\u{200d}\u{1f466}",
                2,
            ),
        ] {
            let c = typed(text);
            let v = c.view(10, 5);
            assert_eq!((v.cursor_row, v.cursor_col), (0, cols), "{text:?}");
        }
    }

    #[test]
    fn the_cursor_steps_and_deletes_by_cluster() {
        let mut c = typed("a\u{1f44d}\u{1f3fd}b");
        c.left();
        c.left();
        assert_eq!(c.cursor(), 1, "left jumps over the whole thumb");
        c.right();
        assert_eq!(c.cursor(), 3, "right too");
        c.backspace();
        assert_eq!(c.text(), "ab", "backspace removes the whole cluster");
        let mut d = typed("\u{2764}\u{fe0f}z");
        d.home();
        d.delete();
        assert_eq!(d.text(), "z", "delete removes the whole cluster");
    }

    #[test]
    fn a_typed_joiner_never_leaves_the_cursor_inside_a_cluster() {
        let mut c = typed("\u{1f468}\u{1f469}");
        c.left();
        c.insert('\u{200d}');
        // Man + ZWJ + woman is one cluster now; the cursor sits after it.
        assert_eq!(c.cursor(), 3);
    }

    #[test]
    fn nine_lines_take_five_rows_and_keep_the_cursor_visible() {
        let text = (1..=9)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let mut c = typed(&text);
        assert_eq!(c.height(20), MAX_ROWS);
        let v = c.view(20, MAX_ROWS);
        assert_eq!(v.rows, ["5", "6", "7", "8", "9"]);
        assert_eq!((v.cursor_row, v.cursor_col), (4, 1));
        // Cursor on the first line: the window moves up with it.
        for _ in 0..16 {
            c.left();
        }
        let v = c.view(20, MAX_ROWS);
        assert_eq!(v.rows, ["1", "2", "3", "4", "5"]);
        assert_eq!((v.cursor_row, v.cursor_col), (0, 1));
    }

    #[test]
    fn height_grows_with_the_content() {
        assert_eq!(Composer::new().height(10), 1);
        assert_eq!(typed("a\nb").height(10), 2);
        assert_eq!(typed("abcdefg").height(3), 3);
    }

    #[test]
    fn a_view_with_fewer_rows_than_the_text_needs_is_cut_to_the_limit() {
        let c = typed("a\nb\nc");
        let v = c.view(10, 2);
        assert_eq!(v.rows, ["b", "c"]);
        assert_eq!(v.cursor_row, 1);
    }
}
