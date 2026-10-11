//! Copy mode: vim-style, read-only navigation of the scrollback.
//!
//! The offset counts rows above the live screen (0 = bottom), matching
//! `Pane::set_scrollback`. This module is pure; the app applies the offset.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CopyMotion {
    LineUp,
    LineDown,
    HalfUp,
    HalfDown,
    Top,
    Bottom,
    Left,
    Right,
    LineStart,
    LineEnd,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CopyCommand {
    Move(CopyMotion),
    Yank,
    Exit,
    Ignore,
}

/// A cell addressed by stable line number (see `Pane::first_line`) and column.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Point {
    pub line: usize,
    pub col: u16,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelectionKind {
    Char,
    Line,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Selection {
    pub anchor: Point,
    pub kind: SelectionKind,
}

/// What the pane offers to copy mode: stable line numbers
/// `first_line..first_line + history + rows`, `rows` of them on screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Geometry {
    pub first_line: usize,
    pub history: usize,
    pub rows: u16,
    pub cols: u16,
}

/// Viewport position in copy mode plus the half-typed `gg`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CopyState {
    /// Rows above the live screen (0 = bottom).
    pub offset: usize,
    pub cursor: Point,
    pub selection: Option<Selection>,
    /// `Pane::line_generation` at entry; a different value drops the selection.
    pub generation: u32,
    pending_g: bool,
}

impl CopyState {
    /// Copy mode entered with the cursor on the pane's live cursor.
    pub fn enter(_geo: Geometry, _live_cursor: Option<(u16, u16)>, _generation: u32) -> Self {
        Self::default()
    }

    /// The selected cells, `None` without a selection.
    pub fn span(&self, _cols: u16) -> Option<LineSpan> {
        None
    }

    /// Moves the cursor (and the viewport only as far as needed).
    /// `line_end` is the last non-blank column of the cursor's row, used by `$`.
    pub fn apply_motion(&mut self, _motion: CopyMotion, _geo: Geometry, _line_end: u16) {}

    /// Interprets a key. A pending `g` is dropped by any key other than `g`,
    /// which is then handled on its own.
    pub fn on_key(&mut self, key: &KeyEvent, _repeat: bool) -> CopyCommand {
        let was_pending = std::mem::take(&mut self.pending_g);
        let plain = key.modifiers.difference(KeyModifiers::SHIFT).is_empty();
        let ctrl = key.modifiers == KeyModifiers::CONTROL;
        match key.code {
            KeyCode::Char('g') if plain => {
                if was_pending {
                    CopyCommand::Move(CopyMotion::Top)
                } else {
                    self.pending_g = true;
                    CopyCommand::Ignore
                }
            }
            KeyCode::Char('j') if plain => CopyCommand::Move(CopyMotion::LineDown),
            KeyCode::Char('k') if plain => CopyCommand::Move(CopyMotion::LineUp),
            KeyCode::Char('G') if plain => CopyCommand::Move(CopyMotion::Bottom),
            KeyCode::Char('d') if ctrl => CopyCommand::Move(CopyMotion::HalfDown),
            KeyCode::Char('u') if ctrl => CopyCommand::Move(CopyMotion::HalfUp),
            KeyCode::Char('q' | 'i') if plain => self.exit(),
            KeyCode::Esc if plain => self.exit(),
            _ => CopyCommand::Ignore,
        }
    }

    fn exit(&mut self) -> CopyCommand {
        self.offset = 0;
        CopyCommand::Exit
    }

    /// Moves the viewport, clamped to `0..=max` (`max` = rows of history).
    /// `rows` is the pane height; a half page is `rows / 2`, at least 1.
    pub fn apply(&mut self, motion: CopyMotion, max: usize, rows: u16) {
        let half = usize::from(rows / 2).max(1);
        let target = match motion {
            CopyMotion::LineUp => self.offset.saturating_add(1),
            CopyMotion::LineDown => self.offset.saturating_sub(1),
            CopyMotion::HalfUp => self.offset.saturating_add(half),
            CopyMotion::HalfDown => self.offset.saturating_sub(half),
            CopyMotion::Top => max,
            CopyMotion::Bottom => 0,
            _ => self.offset,
        };
        self.offset = target.min(max);
    }
}

/// Inclusive range of cells over stable line numbers (see `Pane::first_line`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LineSpan {
    pub first: usize,
    pub last: usize,
    pub first_col: u16,
    pub last_col: u16,
}

impl LineSpan {
    /// Span between two `(line, col)` points given in any order.
    pub fn new(a: (usize, u16), b: (usize, u16)) -> Self {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        Self {
            first: lo.0,
            last: hi.0,
            first_col: lo.1,
            last_col: hi.1,
        }
    }

    /// Whether the cell `(line, col)` is inside the span, both ends included.
    pub fn contains(&self, line: usize, col: u16) -> bool {
        (self.first..=self.last).contains(&line)
            && (line != self.first || col >= self.first_col)
            && (line != self.last || col <= self.last_col)
    }
}

/// Text of one screen row plus whether it soft-wraps into the next one.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LineText {
    pub text: String,
    pub wrapped: bool,
}

/// Joins extracted rows into the text to copy.
///
/// A soft-wrapped row joins the next one with no separator and keeps its
/// trailing spaces (real content at the wrap point); any other row is
/// followed by `\n`. Trailing blanks are trimmed where a logical line ends and
/// on the last row. No trailing `\n` is added.
pub fn join(rows: &[LineText]) -> String {
    let mut out = String::new();
    for (i, row) in rows.iter().enumerate() {
        let last = i + 1 == rows.len();
        if row.wrapped && !last {
            out.push_str(&row.text);
        } else {
            out.push_str(row.text.trim_end_matches(' '));
            if !last {
                out.push('\n');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    const ROWS: u16 = 24;

    fn press(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn at(offset: usize) -> CopyState {
        CopyState {
            offset,
            ..CopyState::default()
        }
    }

    #[test]
    fn keys_map_to_motions() {
        let mut s = CopyState::default();
        let cases = [
            (press('j'), CopyMotion::LineDown),
            (press('k'), CopyMotion::LineUp),
            (ctrl('d'), CopyMotion::HalfDown),
            (ctrl('u'), CopyMotion::HalfUp),
            (
                KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT),
                CopyMotion::Bottom,
            ),
            (press('G'), CopyMotion::Bottom),
        ];
        for (key, motion) in cases {
            assert_eq!(s.on_key(&key, false), CopyCommand::Move(motion), "{key:?}");
        }
    }

    #[test]
    fn exit_keys_are_q_esc_and_i() {
        for key in [
            press('q'),
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            press('i'),
        ] {
            let mut s = at(7);
            assert_eq!(s.on_key(&key, false), CopyCommand::Exit, "{key:?}");
            assert_eq!(s.offset, 0, "exit resets the offset for {key:?}");
        }
    }

    #[test]
    fn unrelated_keys_are_ignored() {
        let mut s = CopyState::default();
        assert_eq!(s.on_key(&press('x'), false), CopyCommand::Ignore);
        assert_eq!(
            s.on_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), false),
            CopyCommand::Ignore
        );
        assert_eq!(s.on_key(&ctrl('x'), false), CopyCommand::Ignore);
    }

    #[test]
    fn double_g_goes_to_the_top() {
        let mut s = CopyState::default();
        assert_eq!(s.on_key(&press('g'), false), CopyCommand::Ignore);
        assert_eq!(
            s.on_key(&press('g'), false),
            CopyCommand::Move(CopyMotion::Top)
        );
        // The pending state is consumed: a third g starts over.
        assert_eq!(s.on_key(&press('g'), false), CopyCommand::Ignore);
    }

    #[test]
    fn lone_g_is_discarded_by_the_next_key() {
        let mut s = CopyState::default();
        s.on_key(&press('g'), false);
        assert_eq!(
            s.on_key(&press('j'), false),
            CopyCommand::Move(CopyMotion::LineDown)
        );
        // The g was dropped, so a single g afterwards must not reach the top.
        assert_eq!(s.on_key(&press('g'), false), CopyCommand::Ignore);
    }

    #[test]
    fn line_motions_move_one_row_and_return() {
        let mut s = at(0);
        s.apply(CopyMotion::LineUp, 100, ROWS);
        assert_eq!(s.offset, 1);
        s.apply(CopyMotion::LineDown, 100, ROWS);
        assert_eq!(s.offset, 0);
    }

    #[test]
    fn half_page_moves_half_the_pane_height() {
        let mut s = at(0);
        s.apply(CopyMotion::HalfUp, 100, ROWS);
        assert_eq!(s.offset, 12);
        s.apply(CopyMotion::HalfDown, 100, ROWS);
        assert_eq!(s.offset, 0);
        s.apply(CopyMotion::HalfUp, 100, 10);
        assert_eq!(s.offset, 5);
    }

    #[test]
    fn half_page_moves_at_least_one_row_on_a_tiny_pane() {
        let mut s = at(0);
        s.apply(CopyMotion::HalfUp, 100, 1);
        assert_eq!(s.offset, 1);
    }

    #[test]
    fn top_and_bottom_jump_to_the_bounds() {
        let mut s = at(40);
        s.apply(CopyMotion::Top, 100, ROWS);
        assert_eq!(s.offset, 100);
        s.apply(CopyMotion::Bottom, 100, ROWS);
        assert_eq!(s.offset, 0);
    }

    #[test]
    fn motions_clamp_at_the_top() {
        for motion in [CopyMotion::LineUp, CopyMotion::HalfUp, CopyMotion::Top] {
            let mut s = at(100);
            s.apply(motion, 100, ROWS);
            assert_eq!(s.offset, 100, "{motion:?}");
        }
        let mut s = at(95);
        s.apply(CopyMotion::HalfUp, 100, ROWS);
        assert_eq!(s.offset, 100);
    }

    #[test]
    fn motions_clamp_at_the_bottom() {
        for motion in [
            CopyMotion::LineDown,
            CopyMotion::HalfDown,
            CopyMotion::Bottom,
        ] {
            let mut s = at(0);
            s.apply(motion, 100, ROWS);
            assert_eq!(s.offset, 0, "{motion:?}");
        }
        let mut s = at(5);
        s.apply(CopyMotion::HalfDown, 100, ROWS);
        assert_eq!(s.offset, 0);
    }

    #[test]
    fn no_history_keeps_every_motion_at_zero() {
        // Alternate screen: scrollback is empty, so max == 0.
        for motion in [
            CopyMotion::LineUp,
            CopyMotion::LineDown,
            CopyMotion::HalfUp,
            CopyMotion::HalfDown,
            CopyMotion::Top,
            CopyMotion::Bottom,
        ] {
            let mut s = at(0);
            s.apply(motion, 0, ROWS);
            assert_eq!(s.offset, 0, "{motion:?}");
        }
    }

    #[test]
    fn stale_offset_beyond_history_is_clamped_on_move() {
        let mut s = at(50);
        s.apply(CopyMotion::LineDown, 10, ROWS);
        assert_eq!(s.offset, 10);
    }

    // copy-selection: LineSpan and join.
    #[test]
    fn span_contains_is_inclusive_at_both_end_cells() {
        // (line, col) -> inside, for the span (2,3)..=(4,5).
        let span = LineSpan::new((2, 3), (4, 5));
        let table = [
            ((1, 9), false, "line before first"),
            ((2, 2), false, "before first cell"),
            ((2, 3), true, "first cell"),
            ((2, 79), true, "rest of the first row"),
            ((3, 0), true, "middle row, col 0"),
            ((3, 79), true, "middle row, last col"),
            ((4, 0), true, "last row start"),
            ((4, 5), true, "last cell"),
            ((4, 6), false, "after last cell"),
            ((5, 0), false, "line after last"),
        ];
        for ((line, col), inside, why) in table {
            assert_eq!(span.contains(line, col), inside, "{why}");
        }
    }

    #[test]
    fn single_row_span_is_bounded_on_both_sides() {
        let span = LineSpan::new((7, 2), (7, 4));
        let cols: Vec<bool> = (0..7).map(|c| span.contains(7, c)).collect();
        assert_eq!(cols, [false, false, true, true, true, false, false]);
        assert!(!span.contains(6, 3));
        assert!(!span.contains(8, 3));
    }

    #[test]
    fn span_constructor_orders_reversed_points() {
        let forward = LineSpan::new((5, 1), (5, 3));
        let reversed = LineSpan::new((5, 3), (5, 1));
        let expected = LineSpan {
            first: 5,
            last: 5,
            first_col: 1,
            last_col: 3,
        };
        assert_eq!(forward, expected);
        assert_eq!(reversed, expected);
        let up = LineSpan::new((9, 2), (4, 7));
        assert_eq!(
            up,
            LineSpan {
                first: 4,
                last: 9,
                first_col: 7,
                last_col: 2,
            }
        );
    }

    fn row(text: &str, wrapped: bool) -> LineText {
        LineText {
            text: text.to_owned(),
            wrapped,
        }
    }

    #[test]
    fn join_separates_logical_lines_with_a_newline() {
        let rows = [row("foo", false), row("bar", false)];
        assert_eq!(join(&rows), "foo\nbar");
    }

    #[test]
    fn join_glues_soft_wrapped_rows_without_a_separator() {
        let rows = [row("abcdefg", true), row("hijklmn", false)];
        assert_eq!(join(&rows), "abcdefghijklmn");
    }

    #[test]
    fn join_trims_trailing_blanks_at_logical_line_ends_and_the_last_row() {
        let rows = [row("foo   ", false), row("bar  ", false)];
        assert_eq!(join(&rows), "foo\nbar");
        assert_eq!(join(&[row("only  ", true)]), "only");
    }

    #[test]
    fn join_keeps_trailing_spaces_of_a_soft_wrapped_row() {
        let rows = [row("hello     ", true), row("world", false)];
        assert_eq!(join(&rows), "hello     world");
    }

    #[test]
    fn join_of_nothing_is_empty_and_a_blank_last_row_is_an_empty_line() {
        assert_eq!(join(&[]), "");
        assert_eq!(join(&[row("a", false), row("   ", false)]), "a\n");
    }
}
