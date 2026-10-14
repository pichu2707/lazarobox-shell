//! Copy mode: vim-style, read-only navigation of the scrollback.
//!
//! The offset counts rows above the live screen (0 = bottom), matching
//! `Pane::set_scrollback`. This module is pure; the app applies the offset.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::mouse::WHEEL_STEP;

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
    /// One mouse wheel tick up: the viewport goes back `WHEEL_STEP` rows.
    WheelUp,
    /// One mouse wheel tick down.
    WheelDown,
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
    /// Copy mode entered at view `offset` (rows above the live screen). At 0
    /// the cursor is the pane's live cursor; scrolled back it is on the last
    /// visible row so the view does not move on the first key.
    pub fn enter(
        geo: Geometry,
        offset: usize,
        live_cursor: Option<(u16, u16)>,
        generation: u32,
    ) -> Self {
        let offset = offset.min(geo.history);
        let (row, col) = match (offset, live_cursor) {
            // Scrolled back the live cursor is off screen: take the last
            // visible row, keeping its column.
            (1.., live) => (geo.rows.saturating_sub(1), live.map_or(0, |(_, col)| col)),
            (0, live) => live.unwrap_or((geo.rows, 0)),
        };
        let row = row.min(geo.rows.saturating_sub(1));
        let col = col.min(geo.cols.saturating_sub(1));
        Self {
            offset,
            cursor: Point {
                line: geo.first_line + geo.history - offset + usize::from(row),
                col,
            },
            generation,
            ..Self::default()
        }
    }

    /// The selected cells, `None` without a selection.
    pub fn span(&self, cols: u16) -> Option<LineSpan> {
        let sel = self.selection?;
        let (a, b) = (sel.anchor, self.cursor);
        Some(match sel.kind {
            SelectionKind::Char => LineSpan::new((a.line, a.col), (b.line, b.col)),
            SelectionKind::Line => LineSpan::new(
                (a.line.min(b.line), 0),
                (a.line.max(b.line), cols.saturating_sub(1)),
            ),
        })
    }

    /// `v`/`V`: start, clear (same key) or switch kind (other key).
    fn toggle_selection(&mut self, kind: SelectionKind) {
        self.selection = match self.selection {
            Some(s) if s.kind == kind => None,
            Some(s) => Some(Selection { kind, ..s }),
            None => Some(Selection {
                anchor: self.cursor,
                kind,
            }),
        };
    }

    /// Moves the cursor (and the viewport only as far as needed).
    /// `line_end` is the last non-blank column of the cursor's row, used by `$`.
    pub fn apply_motion(&mut self, motion: CopyMotion, geo: Geometry, line_end: u16) {
        let last_col = geo.cols.saturating_sub(1);
        if matches!(motion, CopyMotion::WheelUp | CopyMotion::WheelDown) {
            // The wheel moves the viewport and the cursor follows it; running
            // `ensure_visible` would drag the viewport back to the cursor.
            self.wheel(motion, geo);
            return;
        }
        match motion {
            CopyMotion::Left => self.cursor.col = self.cursor.col.saturating_sub(1),
            CopyMotion::Right => self.cursor.col = self.cursor.col.saturating_add(1).min(last_col),
            CopyMotion::LineStart => self.cursor.col = 0,
            CopyMotion::LineEnd => self.cursor.col = line_end.min(last_col),
            _ => self.move_vertically(motion, geo),
        }
        // Any motion brings an off-screen cursor back into view.
        self.ensure_visible(geo);
    }

    /// One wheel tick: the viewport moves `WHEEL_STEP` rows and the cursor only
    /// as far as needed to stay on a visible row (nearest edge, same column).
    /// Unlike the other motions the viewport never follows the cursor.
    fn wheel(&mut self, motion: CopyMotion, geo: Geometry) {
        self.offset = match motion {
            CopyMotion::WheelUp => self.offset.saturating_add(WHEEL_STEP),
            _ => self.offset.saturating_sub(WHEEL_STEP),
        }
        .min(geo.history);
        let top = geo.first_line + geo.history - self.offset;
        let bottom = top + usize::from(geo.rows.max(1)) - 1;
        self.cursor.line = self.cursor.line.clamp(top, bottom);
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
    pub fn on_key(&mut self, key: &KeyEvent, repeat: bool) -> CopyCommand {
        let was_pending = std::mem::take(&mut self.pending_g);
        let plain = key.modifiers.difference(KeyModifiers::SHIFT).is_empty();
        let ctrl = key.modifiers == KeyModifiers::CONTROL;
        // A held selection or exit key must not toggle, yank or leave COPY
        // (the leftover repeats would reach the shell as typed text).
        let one_shot = matches!(
            key.code,
            KeyCode::Char('v' | 'V' | 'y' | 'q' | 'i') | KeyCode::Esc
        );
        if repeat && plain && one_shot {
            return CopyCommand::Ignore;
        }
        match key.code {
            KeyCode::Char('g') if plain => {
                if was_pending {
                    CopyCommand::Move(CopyMotion::Top)
                } else {
                    self.pending_g = true;
                    CopyCommand::Ignore
                }
            }
            KeyCode::Char('v') if plain => self.select(SelectionKind::Char),
            KeyCode::Char('V') if plain => self.select(SelectionKind::Line),
            KeyCode::Char('h') if plain => CopyCommand::Move(CopyMotion::Left),
            KeyCode::Char('l') if plain => CopyCommand::Move(CopyMotion::Right),
            KeyCode::Char('0') if plain => CopyCommand::Move(CopyMotion::LineStart),
            KeyCode::Char('$') if plain => CopyCommand::Move(CopyMotion::LineEnd),
            KeyCode::Char('j') if plain => CopyCommand::Move(CopyMotion::LineDown),
            KeyCode::Char('k') if plain => CopyCommand::Move(CopyMotion::LineUp),
            KeyCode::Char('G') if plain => CopyCommand::Move(CopyMotion::Bottom),
            KeyCode::Char('d') if ctrl => CopyCommand::Move(CopyMotion::HalfDown),
            KeyCode::Char('u') if ctrl => CopyCommand::Move(CopyMotion::HalfUp),
            KeyCode::Char('q' | 'i') if plain => self.exit(),
            KeyCode::Char('y') if plain && self.selection.is_some() => CopyCommand::Yank,
            KeyCode::Esc if plain && self.selection.take().is_some() => CopyCommand::Ignore,
            KeyCode::Esc if plain => self.exit(),
            _ => CopyCommand::Ignore,
        }
    }

    fn select(&mut self, kind: SelectionKind) -> CopyCommand {
        self.toggle_selection(kind);
        CopyCommand::Ignore
    }

    fn exit(&mut self) -> CopyCommand {
        self.offset = 0;
        self.selection = None;
        CopyCommand::Exit
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

/// What to paint as selected in a pane's viewport: the span and the stable
/// line number shown on the top row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Highlight {
    pub span: LineSpan,
    pub top_line: usize,
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
/// on the last row. Trailing blank rows are dropped, so the text never ends
/// in `\n`.
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
    out.truncate(out.trim_end_matches('\n').len());
    out
}

/// Largest base64 payload (in bytes) sent in one OSC 52 sequence.
pub const OSC52_MAX_PAYLOAD: usize = 100_000;

/// The selection does not fit in one OSC 52 sequence.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TooLarge;

/// Standard base64 (RFC 4648 section 4) with padding.
pub fn encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, b)| acc | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `ESC ] 52 ; c ; <base64> BEL`, or `TooLarge` when the base64 payload would
/// exceed `OSC52_MAX_PAYLOAD`. The size is checked before encoding.
pub fn osc52(text: &str) -> Result<Vec<u8>, TooLarge> {
    if text.len().div_ceil(3) * 4 > OSC52_MAX_PAYLOAD {
        return Err(TooLarge);
    }
    let mut out = b"\x1b]52;c;".to_vec();
    out.extend_from_slice(encode(text.as_bytes()).as_bytes());
    out.push(0x07);
    Ok(out)
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
    fn encode_matches_the_rfc_4648_vectors() {
        let cases: [(&str, &str); 7] = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ];
        for (input, want) in cases {
            assert_eq!(encode(input.as_bytes()), want, "{input:?}");
        }
    }

    #[test]
    fn encode_handles_multibyte_utf8_and_high_bytes() {
        assert_eq!(encode("漢".as_bytes()), "5ryi");
        assert_eq!(encode("é".as_bytes()), "w6k=");
        assert_eq!(encode(&[0xfb, 0xff, 0xfe]), "+//+");
    }

    #[test]
    fn osc52_wraps_the_base64_in_the_clipboard_sequence() {
        assert_eq!(osc52("hello"), Ok(b"\x1b]52;c;aGVsbG8=\x07".to_vec()));
    }

    #[test]
    fn osc52_accepts_exactly_the_cap_and_rejects_one_more_group() {
        // 75_000 bytes -> 100_000 base64 bytes.
        let at_cap = "a".repeat(75_000);
        let ok = osc52(&at_cap).expect("at the cap");
        assert_eq!(ok.len(), 7 + 100_000 + 1);
        // 75_001 bytes -> 100_004 base64 bytes.
        assert_eq!(osc52(&"a".repeat(75_001)), Err(TooLarge));
        // Payload lengths just below the cap: 74_999 -> 100_000 (padded).
        assert_eq!(
            osc52(&"a".repeat(74_999)).expect("padded").len(),
            7 + 100_000 + 1
        );
        // 74_996 bytes -> 24_999 groups -> 99_996 base64 bytes.
        assert_eq!(
            osc52(&"a".repeat(74_996)).expect("below").len(),
            7 + 99_996 + 1
        );
    }

    #[test]
    fn osc52_rejects_on_size_without_needing_valid_encoding_work() {
        assert_eq!(osc52(&"é".repeat(60_000)), Err(TooLarge));
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

    // Characterization (ported from the viewport-only `apply` tests).
    #[test]
    fn half_page_moves_at_least_one_row_on_a_tiny_pane() {
        let geo = Geometry { rows: 1, ..geo() };
        let mut s = cursor_at(110, 0, 0);
        s.apply_motion(CopyMotion::HalfUp, geo, 0);
        assert_eq!((s.cursor.line, s.offset), (109, 1));
    }

    #[test]
    fn no_history_keeps_every_vertical_motion_at_zero() {
        let geo = Geometry {
            history: 0,
            first_line: 0,
            ..geo()
        };
        for motion in [
            CopyMotion::LineUp,
            CopyMotion::LineDown,
            CopyMotion::HalfUp,
            CopyMotion::HalfDown,
            CopyMotion::Top,
            CopyMotion::Bottom,
        ] {
            let mut s = cursor_at(0, 0, 0);
            s.apply_motion(motion, geo, 0);
            assert_eq!(s.offset, 0, "{motion:?}");
        }
    }

    #[test]
    fn stale_offset_beyond_history_is_clamped_on_move() {
        let mut s = cursor_at(20, 0, 500);
        s.apply_motion(CopyMotion::LineDown, geo(), 0);
        assert_eq!(s.offset, 100);
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
        let s = CopyState::enter(geo(), 0, Some((5, 3)), 7);
        assert_eq!(s.cursor, pt(10 + 100 + 5, 3));
        assert_eq!(s.offset, 0);
        assert_eq!(s.selection, None);
        assert_eq!(s.generation, 7);
    }

    #[test]
    fn enter_clamps_the_live_cursor_into_the_pane() {
        let s = CopyState::enter(geo(), 0, Some((99, 200)), 0);
        assert_eq!(s.cursor, pt(10 + 100 + 23, 79));
    }

    // COPY keeps a wheel-scrolled view: the cursor lands on the last visible row.
    #[test]
    fn enter_on_a_scrolled_view_keeps_the_offset_and_uses_the_last_visible_row() {
        // Offset 5 shows lines 105..=128.
        let s = CopyState::enter(geo(), 5, Some((2, 7)), 0);
        assert_eq!((s.offset, s.cursor), (5, pt(128, 7)));
        let s = CopyState::enter(geo(), 5, None, 0);
        assert_eq!((s.offset, s.cursor), (5, pt(128, 0)));
    }

    #[test]
    fn enter_on_a_scrolled_view_clamps_the_column_and_the_offset() {
        let s = CopyState::enter(geo(), 5, Some((2, 200)), 0);
        assert_eq!(s.cursor, pt(128, 79));
        let s = CopyState::enter(geo(), 1000, Some((2, 3)), 0);
        assert_eq!((s.offset, s.cursor), (100, pt(33, 3)));
    }

    #[test]
    fn enter_with_a_hidden_cursor_uses_the_bottom_row_at_column_zero() {
        let s = CopyState::enter(geo(), 0, None, 0);
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

    // mouse-input, Wheel in COPY: the viewport moves 3 rows.
    #[test]
    fn the_wheel_moves_the_viewport_by_three_rows() {
        // Offset 20 shows lines 90..=113; the cursor on 100 stays in view.
        let mut s = cursor_at(100, 4, 20);
        s.apply_motion(CopyMotion::WheelUp, geo(), 0);
        assert_eq!((s.cursor, s.offset), (pt(100, 4), 23));
        s.apply_motion(CopyMotion::WheelDown, geo(), 0);
        s.apply_motion(CopyMotion::WheelDown, geo(), 0);
        assert_eq!((s.cursor, s.offset), (pt(100, 4), 17));
    }

    // mouse-input: the cursor is pulled to the nearest edge row, keeping its column.
    #[test]
    fn the_wheel_keeps_the_cursor_inside_the_viewport() {
        // Cursor on the bottom visible row (133), offset 0: up 3 shows 107..=130.
        let mut s = cursor_at(133, 7, 0);
        s.apply_motion(CopyMotion::WheelUp, geo(), 0);
        assert_eq!((s.cursor, s.offset), (pt(130, 7), 3));
        // Cursor on the top visible row (107), offset 3: down 3 shows 110..=133.
        let mut s = cursor_at(107, 7, 3);
        s.apply_motion(CopyMotion::WheelDown, geo(), 0);
        assert_eq!((s.cursor, s.offset), (pt(110, 7), 0));
    }

    // mouse-input: clamp at the top of the history.
    #[test]
    fn the_wheel_clamps_at_the_top() {
        let mut s = cursor_at(10, 0, 100);
        s.apply_motion(CopyMotion::WheelUp, geo(), 0);
        assert_eq!((s.cursor, s.offset), (pt(10, 0), 100));
        // Two rows short of the top: clamp to the top, not past it.
        let mut s = cursor_at(15, 0, 98);
        s.apply_motion(CopyMotion::WheelUp, geo(), 0);
        assert_eq!(s.offset, 100);
        assert!((10..=33).contains(&s.cursor.line));
    }

    // mouse-input: clamp at the bottom (live).
    #[test]
    fn the_wheel_clamps_at_the_bottom() {
        let mut s = cursor_at(133, 0, 0);
        s.apply_motion(CopyMotion::WheelDown, geo(), 0);
        assert_eq!((s.cursor, s.offset), (pt(133, 0), 0));
        let mut s = cursor_at(120, 0, 2);
        s.apply_motion(CopyMotion::WheelDown, geo(), 0);
        assert_eq!((s.cursor, s.offset), (pt(120, 0), 0));
    }

    // mouse-input: the selection survives and follows the cursor like j/k.
    #[test]
    fn the_wheel_preserves_and_extends_the_selection() {
        let mut s = cursor_at(133, 2, 0);
        s.selection = sel(pt(120, 1), SelectionKind::Char);
        s.apply_motion(CopyMotion::WheelUp, geo(), 0);
        assert_eq!(s.selection, sel(pt(120, 1), SelectionKind::Char));
        assert_eq!(
            s.span(80),
            Some(LineSpan::new((120, 1), (130, 2))),
            "the cursor edge moved, so the span follows it"
        );
    }

    // mouse-input: with no history the wheel does nothing and does not panic.
    #[test]
    fn the_wheel_without_history_does_nothing() {
        let geo = Geometry {
            history: 0,
            first_line: 0,
            ..geo()
        };
        for motion in [CopyMotion::WheelUp, CopyMotion::WheelDown] {
            let mut s = cursor_at(5, 3, 0);
            s.apply_motion(motion, geo, 0);
            assert_eq!((s.cursor, s.offset), (pt(5, 3), 0), "{motion:?}");
        }
    }

    // A tiny pane still has a viewport row to keep the cursor in.
    #[test]
    fn the_wheel_on_a_one_row_pane_keeps_the_cursor_on_it() {
        let geo = Geometry { rows: 1, ..geo() };
        let mut s = cursor_at(110, 0, 0);
        s.apply_motion(CopyMotion::WheelUp, geo, 0);
        assert_eq!((s.cursor.line, s.offset), (107, 3));
    }

    // No key is bound to the wheel motions.
    #[test]
    fn no_key_maps_to_a_wheel_motion() {
        for c in 'a'..='z' {
            let mut s = CopyState::default();
            let got = s.on_key(&press(c), false);
            assert!(
                !matches!(
                    got,
                    CopyCommand::Move(CopyMotion::WheelUp | CopyMotion::WheelDown)
                ),
                "{c}"
            );
        }
    }

    fn sel(anchor: Point, kind: SelectionKind) -> Option<Selection> {
        Some(Selection { anchor, kind })
    }

    #[test]
    fn v_starts_a_character_selection_at_the_cursor() {
        let mut s = cursor_at(115, 3, 0);
        assert_eq!(s.on_key(&press('v'), false), CopyCommand::Ignore);
        assert_eq!(s.selection, sel(pt(115, 3), SelectionKind::Char));
    }

    #[test]
    fn shift_v_starts_a_line_selection_at_the_cursor() {
        let mut s = cursor_at(115, 3, 0);
        let key = KeyEvent::new(KeyCode::Char('V'), KeyModifiers::SHIFT);
        assert_eq!(s.on_key(&key, false), CopyCommand::Ignore);
        assert_eq!(s.selection, sel(pt(115, 3), SelectionKind::Line));
    }

    #[test]
    fn the_same_selection_key_again_clears() {
        for key in [press('v'), press('V')] {
            let mut s = cursor_at(115, 3, 0);
            s.on_key(&key, false);
            s.on_key(&key, false);
            assert_eq!(s.selection, None, "{key:?}");
        }
    }

    #[test]
    fn the_other_selection_key_switches_kind_and_keeps_the_anchor() {
        let mut s = cursor_at(115, 3, 0);
        s.on_key(&press('v'), false);
        s.cursor = pt(118, 9);
        s.on_key(&press('V'), false);
        assert_eq!(s.selection, sel(pt(115, 3), SelectionKind::Line));
        s.on_key(&press('v'), false);
        assert_eq!(s.selection, sel(pt(115, 3), SelectionKind::Char));
    }

    #[test]
    fn span_is_none_without_a_selection() {
        assert_eq!(cursor_at(115, 3, 0).span(80), None);
    }

    #[test]
    fn moving_left_extends_a_character_span_backwards() {
        let mut s = cursor_at(115, 3, 0);
        s.on_key(&press('v'), false);
        s.apply_motion(CopyMotion::Left, geo(), 0);
        s.apply_motion(CopyMotion::Left, geo(), 0);
        assert_eq!(s.span(80), Some(LineSpan::new((115, 1), (115, 3))));
        assert_eq!(
            s.span(80),
            Some(LineSpan {
                first: 115,
                last: 115,
                first_col: 1,
                last_col: 3,
            })
        );
    }

    #[test]
    fn moving_past_the_anchor_reorders_the_span() {
        let mut s = cursor_at(115, 3, 0);
        s.on_key(&press('v'), false);
        s.apply_motion(CopyMotion::LineDown, geo(), 0);
        assert_eq!(s.span(80), Some(LineSpan::new((115, 3), (116, 3))));
        s.apply_motion(CopyMotion::LineUp, geo(), 0);
        s.apply_motion(CopyMotion::LineUp, geo(), 0);
        assert_eq!(
            s.span(80),
            Some(LineSpan {
                first: 114,
                last: 115,
                first_col: 3,
                last_col: 3,
            })
        );
    }

    #[test]
    fn a_line_selection_spans_whole_rows_in_either_direction() {
        let mut s = cursor_at(115, 3, 0);
        s.on_key(&press('V'), false);
        s.apply_motion(CopyMotion::LineDown, geo(), 0);
        s.apply_motion(CopyMotion::LineDown, geo(), 0);
        let whole = LineSpan {
            first: 115,
            last: 117,
            first_col: 0,
            last_col: 79,
        };
        assert_eq!(s.span(80), Some(whole));
        s.cursor = pt(113, 5);
        assert_eq!(
            s.span(80),
            Some(LineSpan {
                first: 113,
                last: 115,
                first_col: 0,
                last_col: 79,
            })
        );
    }

    fn selecting(kind: SelectionKind) -> CopyState {
        CopyState {
            selection: sel(pt(115, 3), kind),
            ..cursor_at(116, 4, 6)
        }
    }

    fn esc() -> KeyEvent {
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
    }

    #[test]
    fn horizontal_keys_map_to_motions() {
        let mut s = CopyState::default();
        let cases = [
            (press('h'), CopyMotion::Left),
            (press('l'), CopyMotion::Right),
            (press('0'), CopyMotion::LineStart),
            (
                KeyEvent::new(KeyCode::Char('$'), KeyModifiers::SHIFT),
                CopyMotion::LineEnd,
            ),
            (press('$'), CopyMotion::LineEnd),
        ];
        for (key, motion) in cases {
            assert_eq!(s.on_key(&key, false), CopyCommand::Move(motion), "{key:?}");
        }
    }

    #[test]
    fn y_with_a_selection_yanks_and_without_is_ignored() {
        let mut s = selecting(SelectionKind::Char);
        assert_eq!(s.on_key(&press('y'), false), CopyCommand::Yank);
        let mut none = cursor_at(116, 4, 6);
        assert_eq!(none.on_key(&press('y'), false), CopyCommand::Ignore);
        assert_eq!(none, cursor_at(116, 4, 6));
    }

    #[test]
    fn esc_with_a_selection_clears_it_and_stays() {
        let mut s = selecting(SelectionKind::Line);
        assert_eq!(s.on_key(&esc(), false), CopyCommand::Ignore);
        assert_eq!(s.selection, None);
        assert_eq!((s.cursor, s.offset), (pt(116, 4), 6));
        // A second Esc, with nothing left to clear, leaves copy mode.
        assert_eq!(s.on_key(&esc(), false), CopyCommand::Exit);
    }

    #[test]
    fn q_and_i_exit_even_with_a_selection_and_drop_it() {
        for key in [press('q'), press('i')] {
            let mut s = selecting(SelectionKind::Char);
            assert_eq!(s.on_key(&key, false), CopyCommand::Exit, "{key:?}");
            assert_eq!((s.selection, s.offset), (None, 0), "{key:?}");
        }
    }

    #[test]
    fn unlisted_keys_stay_ignored_with_a_selection() {
        let mut s = selecting(SelectionKind::Char);
        for key in [press('x'), ctrl('x'), press('p')] {
            assert_eq!(s.on_key(&key, false), CopyCommand::Ignore, "{key:?}");
        }
        assert_eq!(s, selecting(SelectionKind::Char));
    }

    #[test]
    fn repeat_of_selection_and_exit_keys_changes_nothing() {
        let held = [
            press('v'),
            press('V'),
            press('y'),
            esc(),
            press('q'),
            press('i'),
        ];
        for key in held {
            let mut with = selecting(SelectionKind::Char);
            assert_eq!(with.on_key(&key, true), CopyCommand::Ignore, "{key:?}");
            assert_eq!(with, selecting(SelectionKind::Char), "{key:?}");
            let mut without = cursor_at(116, 4, 6);
            assert_eq!(without.on_key(&key, true), CopyCommand::Ignore, "{key:?}");
            assert_eq!(without, cursor_at(116, 4, 6), "{key:?}");
        }
    }

    #[test]
    fn repeat_of_motions_moves_like_press() {
        let cases = [
            (press('j'), CopyMotion::LineDown),
            (press('k'), CopyMotion::LineUp),
            (press('h'), CopyMotion::Left),
            (press('l'), CopyMotion::Right),
            (ctrl('d'), CopyMotion::HalfDown),
        ];
        for (key, motion) in cases {
            let mut s = CopyState::default();
            assert_eq!(s.on_key(&key, true), CopyCommand::Move(motion), "{key:?}");
        }
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
    fn join_of_nothing_is_empty_and_trailing_blank_rows_are_dropped() {
        assert_eq!(join(&[]), "");
        assert_eq!(join(&[row("a", false), row("   ", false)]), "a");
    }
}
