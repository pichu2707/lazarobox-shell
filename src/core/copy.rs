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
    pub fn enter(geo: Geometry, live_cursor: Option<(u16, u16)>, generation: u32) -> Self {
        let (row, col) = live_cursor.unwrap_or((geo.rows, 0));
        let row = row.min(geo.rows.saturating_sub(1));
        let col = col.min(geo.cols.saturating_sub(1));
        Self {
            cursor: Point {
                line: geo.first_line + geo.history + usize::from(row),
                col,
            },
            generation,
            ..Self::default()
        }
    }

    /// The selected cells, `None` without a selection.
    pub fn span(&self, _cols: u16) -> Option<LineSpan> {
        None
    }

    /// Moves the cursor (and the viewport only as far as needed).
    /// `line_end` is the last non-blank column of the cursor's row, used by `$`.
    pub fn apply_motion(&mut self, motion: CopyMotion, geo: Geometry, line_end: u16) {
        let last_col = geo.cols.saturating_sub(1);
        match motion {
            CopyMotion::Left => self.cursor.col = self.cursor.col.saturating_sub(1),
            CopyMotion::Right => self.cursor.col = self.cursor.col.saturating_add(1).min(last_col),
            CopyMotion::LineStart => self.cursor.col = 0,
            CopyMotion::LineEnd => self.cursor.col = line_end.min(last_col),
            _ => self.move_vertically(motion, geo),
        }
    }

    fn move_vertically(&mut self, motion: CopyMotion, geo: Geometry) {
        let last_line = geo.first_line + geo.history + usize::from(geo.rows.max(1)) - 1;
        let half = usize::from(geo.rows / 2).max(1);
        match motion {
            CopyMotion::LineUp => self.cursor.line = self.cursor.line.saturating_sub(1),
            CopyMotion::LineDown => self.cursor.line += 1,
            CopyMotion::HalfUp => {
                self.cursor.line = self.cursor.line.saturating_sub(half);
                self.offset += half;
            }
            CopyMotion::HalfDown => {
                self.cursor.line += half;
                self.offset = self.offset.saturating_sub(half);
            }
            CopyMotion::Top => {
                self.cursor = Point {
                    line: geo.first_line,
                    col: 0,
                };
                self.offset = geo.history;
            }
            CopyMotion::Bottom => {
                self.cursor = Point {
                    line: last_line,
                    col: 0,
                };
                self.offset = 0;
            }
            _ => {}
        }
        self.cursor.line = self.cursor.line.clamp(geo.first_line, last_line);
        self.offset = self.offset.min(geo.history);
        self.ensure_visible(geo);
    }

    /// Scrolls the viewport just enough to show the cursor line.
    fn ensure_visible(&mut self, geo: Geometry) {
        let live_top = geo.first_line + geo.history;
        let top = live_top - self.offset;
        let bottom = top + usize::from(geo.rows.max(1)) - 1;
        if self.cursor.line < top {
            self.offset = live_top - self.cursor.line;
        } else if self.cursor.line > bottom {
            self.offset = self.offset.saturating_sub(self.cursor.line - bottom);
        }
    }

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

    // copy-selection: CopyState core.
    fn geo() -> Geometry {
        Geometry {
            first_line: 10,
            history: 100,
            rows: ROWS,
            cols: 80,
        }
    }

    fn pt(line: usize, col: u16) -> Point {
        Point { line, col }
    }

    /// Copy state with the cursor at `(line, col)` and the given offset.
    fn cursor_at(line: usize, col: u16, offset: usize) -> CopyState {
        CopyState {
            offset,
            cursor: pt(line, col),
            ..CopyState::default()
        }
    }

    #[test]
    fn enter_puts_the_cursor_on_the_live_cursor() {
        let s = CopyState::enter(geo(), Some((5, 3)), 7);
        assert_eq!(s.cursor, pt(10 + 100 + 5, 3));
        assert_eq!(s.offset, 0);
        assert_eq!(s.selection, None);
        assert_eq!(s.generation, 7);
    }

    #[test]
    fn enter_clamps_the_live_cursor_into_the_pane() {
        let s = CopyState::enter(geo(), Some((99, 200)), 0);
        assert_eq!(s.cursor, pt(10 + 100 + 23, 79));
    }

    #[test]
    fn enter_with_a_hidden_cursor_uses_the_bottom_row_at_column_zero() {
        let s = CopyState::enter(geo(), None, 0);
        assert_eq!(s.cursor, pt(10 + 100 + 23, 0));
        assert_eq!(s.offset, 0);
    }

    fn cols_after(start: u16, line_end: u16, motions: &[CopyMotion]) -> Vec<u16> {
        let mut s = cursor_at(110, start, 0);
        motions
            .iter()
            .map(|m| {
                s.apply_motion(*m, geo(), line_end);
                s.cursor.col
            })
            .collect()
    }

    #[test]
    fn horizontal_motions_follow_the_spec_sequence() {
        // h 0 h $ l from col 3 on `echo hi` (last non-blank col 6).
        let seq = [
            CopyMotion::Left,
            CopyMotion::LineStart,
            CopyMotion::Left,
            CopyMotion::LineEnd,
            CopyMotion::Right,
        ];
        assert_eq!(cols_after(3, 6, &seq), [2, 0, 0, 6, 7]);
    }

    #[test]
    fn horizontal_motions_clamp_at_both_ends() {
        assert_eq!(cols_after(0, 0, &[CopyMotion::Left]), [0]);
        assert_eq!(cols_after(79, 0, &[CopyMotion::Right]), [79]);
        assert_eq!(cols_after(78, 0, &[CopyMotion::Right]), [79]);
    }

    #[test]
    fn line_end_beyond_the_last_column_clamps_to_it() {
        assert_eq!(cols_after(4, 200, &[CopyMotion::LineEnd]), [79]);
    }

    #[test]
    fn right_from_a_stale_maximal_column_clamps_without_overflow() {
        assert_eq!(cols_after(u16::MAX, 0, &[CopyMotion::Right]), [79]);
    }

    #[test]
    fn line_end_on_a_blank_row_is_column_zero() {
        assert_eq!(cols_after(4, 0, &[CopyMotion::LineEnd]), [0]);
    }

    #[test]
    fn horizontal_motions_leave_line_and_offset_alone() {
        let mut s = cursor_at(110, 3, 4);
        s.apply_motion(CopyMotion::Right, geo(), 0);
        assert_eq!((s.cursor.line, s.offset), (110, 4));
    }

    // Viewport with the geo above: offset 0 shows lines 110..=133.
    #[test]
    fn k_on_the_top_visible_row_scrolls_the_viewport_one_line() {
        let mut s = cursor_at(110, 3, 0);
        s.apply_motion(CopyMotion::LineUp, geo(), 0);
        assert_eq!((s.cursor, s.offset), (pt(109, 3), 1));
        // j moves back inside the viewport; the viewport stays put.
        s.apply_motion(CopyMotion::LineDown, geo(), 0);
        assert_eq!((s.cursor, s.offset), (pt(110, 3), 1));
    }

    #[test]
    fn j_below_the_bottom_row_scrolls_down_only_as_needed() {
        // Offset 2 shows lines 108..=131; the cursor is on the last one.
        let mut s = cursor_at(131, 0, 2);
        s.apply_motion(CopyMotion::LineDown, geo(), 0);
        assert_eq!((s.cursor.line, s.offset), (132, 1));
    }

    #[test]
    fn vertical_motion_inside_the_viewport_leaves_the_offset() {
        let mut s = cursor_at(120, 3, 0);
        s.apply_motion(CopyMotion::LineUp, geo(), 0);
        assert_eq!((s.cursor, s.offset), (pt(119, 3), 0));
    }

    #[test]
    fn vertical_motion_clamps_at_the_first_and_last_line() {
        let mut top = cursor_at(10, 2, 100);
        top.apply_motion(CopyMotion::LineUp, geo(), 0);
        assert_eq!((top.cursor, top.offset), (pt(10, 2), 100));
        let mut bottom = cursor_at(133, 2, 0);
        bottom.apply_motion(CopyMotion::LineDown, geo(), 0);
        assert_eq!((bottom.cursor, bottom.offset), (pt(133, 2), 0));
    }

    #[test]
    fn a_cursor_scrolled_off_screen_returns_with_the_next_motion() {
        // Output scrolled the live screen: the cursor sits above the viewport.
        let mut s = cursor_at(105, 0, 0);
        s.apply_motion(CopyMotion::LineDown, geo(), 0);
        assert_eq!((s.cursor.line, s.offset), (106, 4));
    }

    #[test]
    fn half_pages_move_cursor_and_offset_by_twelve() {
        // Offset 20 shows lines 90..=113.
        let mut s = cursor_at(100, 4, 20);
        s.apply_motion(CopyMotion::HalfUp, geo(), 0);
        assert_eq!((s.cursor, s.offset), (pt(88, 4), 32));
        s.apply_motion(CopyMotion::HalfDown, geo(), 0);
        assert_eq!((s.cursor, s.offset), (pt(100, 4), 20));
    }

    #[test]
    fn half_pages_clamp_at_both_bounds() {
        let mut top = cursor_at(10, 4, 100);
        top.apply_motion(CopyMotion::HalfUp, geo(), 0);
        assert_eq!((top.cursor, top.offset), (pt(10, 4), 100));
        let mut bottom = cursor_at(133, 4, 0);
        bottom.apply_motion(CopyMotion::HalfDown, geo(), 0);
        assert_eq!((bottom.cursor, bottom.offset), (pt(133, 4), 0));
    }

    #[test]
    fn top_and_bottom_jump_with_the_cursor() {
        let mut s = cursor_at(125, 9, 20);
        s.apply_motion(CopyMotion::Top, geo(), 0);
        assert_eq!((s.cursor, s.offset), (pt(10, 0), 100));
        s.apply_motion(CopyMotion::Bottom, geo(), 0);
        assert_eq!((s.cursor, s.offset), (pt(133, 0), 0));
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
