//! Terminal emulator pane.
//!
//! Wraps a `vt100` parser behind our own DTOs so no `vt100` type leaks out of
//! this module: swapping the emulator only rewrites this file. Query replies
//! (DA1, DSR 5n/6n) are produced here and returned from [`Pane::feed`]; the
//! caller decides how to write them back to the child.

use vt100::{Callbacks, Parser, Screen};

/// DA1 reply: VT620-class terminal with no extensions.
const DA1_REPLY: &[u8] = b"\x1b[?62;c";
/// DSR 5n reply: terminal is operating correctly.
const DSR_OK_REPLY: &[u8] = b"\x1b[0n";

/// Size of the pane in character cells.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PaneSize {
    pub rows: u16,
    pub cols: u16,
}

/// Terminal color, independent of any UI toolkit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TermColor {
    Default,
    Idx(u8),
    Rgb(u8, u8, u8),
}

/// Maps a vt100 color to [`TermColor`]. Private on purpose: no vt100 type
/// crosses the module boundary.
fn to_term_color(color: vt100::Color) -> TermColor {
    match color {
        vt100::Color::Default => TermColor::Default,
        vt100::Color::Idx(i) => TermColor::Idx(i),
        vt100::Color::Rgb(r, g, b) => TermColor::Rgb(r, g, b),
    }
}

/// Read-only view of one screen cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CellView<'a> {
    pub text: &'a str,
    pub fg: TermColor,
    pub bg: TermColor,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    /// Right half of a double-width character; draw nothing.
    pub wide_cont: bool,
}

/// Input-relevant modes the child has switched on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TermModes {
    pub application_cursor: bool,
    pub bracketed_paste: bool,
}

/// DECSCUSR cursor kind.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CursorKind {
    #[default]
    Default,
    Block,
    Underline,
    Bar,
}

/// Cursor shape requested by the child through DECSCUSR (`CSI Ps SP q`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CursorShape {
    pub kind: CursorKind,
    pub blinking: bool,
}

impl CursorShape {
    /// Maps a DECSCUSR `Ps`: 0 default, 1/2 block, 3/4 underline, 5/6 bar
    /// (odd values blink). Unknown values yield `None` and must be ignored.
    pub fn from_decscusr(ps: u16) -> Option<Self> {
        let (kind, blinking) = match ps {
            0 => (CursorKind::Default, false),
            1 => (CursorKind::Block, true),
            2 => (CursorKind::Block, false),
            3 => (CursorKind::Underline, true),
            4 => (CursorKind::Underline, false),
            5 => (CursorKind::Bar, true),
            6 => (CursorKind::Bar, false),
            _ => return None,
        };
        Some(Self { kind, blinking })
    }
}

/// vt100 callbacks: answers terminal queries that vt100 does not handle and
/// records the last DECSCUSR cursor shape.
///
/// Known limitation: DSR 6n reports the absolute cursor position. With DECOM
/// (origin mode) on, a real terminal reports it relative to the scroll region,
/// but vt100 exposes no origin-mode or scroll-region accessor, so this is not
/// tracked. Accepted: rare in practice. The reply is always the live cursor,
/// even while the view is scrolled back.
#[derive(Default)]
struct Responder {
    replies: Vec<u8>,
    cursor_shape: CursorShape,
}

impl Callbacks for Responder {
    fn unhandled_csi(
        &mut self,
        screen: &mut Screen,
        i1: Option<u8>,
        i2: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        let first = params.first().and_then(|p| p.first()).copied().unwrap_or(0);
        match (i1, i2, c) {
            // Only the plain request: `\e[>c` (DA2) carries an intermediate,
            // and extra params make it a different sequence.
            (None, None, 'c') if params.len() <= 1 && first == 0 => {
                self.replies.extend_from_slice(DA1_REPLY)
            }
            // DSR takes exactly one param (5 or 6).
            (None, None, 'n') if params.len() == 1 => match first {
                5 => self.replies.extend_from_slice(DSR_OK_REPLY),
                6 => {
                    let (row, col) = screen.cursor_position();
                    let reply = format!("\x1b[{};{}R", row + 1, col + 1);
                    self.replies.extend_from_slice(reply.as_bytes());
                }
                _ => {}
            },
            // DECSCUSR: never replies; unknown Ps leaves the shape unchanged.
            (Some(b' '), None, 'q') => {
                if let Some(shape) = CursorShape::from_decscusr(first) {
                    self.cursor_shape = shape;
                }
            }
            _ => {}
        }
    }
}

/// A terminal emulator screen fed with the child's output bytes.
pub struct Pane {
    parser: Parser<Responder>,
}

impl Pane {
    pub fn new(size: PaneSize, scrollback: usize) -> Self {
        Self {
            parser: Parser::new_with_callbacks(
                size.rows.max(1),
                size.cols.max(1),
                scrollback,
                Responder::default(),
            ),
        }
    }

    /// Processes child output and returns the bytes to answer its queries with.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<u8> {
        self.parser.process(bytes);
        std::mem::take(&mut self.parser.callbacks_mut().replies)
    }

    pub fn resize(&mut self, size: PaneSize) {
        self.parser
            .screen_mut()
            .set_size(size.rows.max(1), size.cols.max(1));
    }

    pub fn cell(&self, row: u16, col: u16) -> Option<CellView<'_>> {
        let cell = self.parser.screen().cell(row, col)?;
        Some(CellView {
            text: cell.contents(),
            fg: to_term_color(cell.fgcolor()),
            bg: to_term_color(cell.bgcolor()),
            bold: cell.bold(),
            italic: cell.italic(),
            underline: cell.underline(),
            inverse: cell.inverse(),
            wide_cont: cell.is_wide_continuation(),
        })
    }

    /// Cursor position `(row, col)`, or `None` when the child hid it or the
    /// view is scrolled back into history.
    pub fn cursor(&self) -> Option<(u16, u16)> {
        let screen = self.parser.screen();
        (!screen.hide_cursor() && screen.scrollback() == 0).then(|| screen.cursor_position())
    }

    /// Last cursor shape the child requested via DECSCUSR.
    pub fn cursor_shape(&self) -> CursorShape {
        self.parser.callbacks().cursor_shape
    }

    /// Scrolls the view `offset` rows into history; clamped by vt100 to the
    /// available history (always 0 on the alternate screen).
    pub fn set_scrollback(&mut self, offset: usize) {
        self.parser.screen_mut().set_scrollback(offset);
    }

    /// Current distance from the bottom, in rows.
    pub fn scrollback_offset(&self) -> usize {
        self.parser.screen().scrollback()
    }

    /// Rows of history available. vt100 has no accessor, so clamp to the
    /// maximum, read it back, and restore the previous offset.
    pub fn scrollback_len(&mut self) -> usize {
        let previous = self.scrollback_offset();
        self.set_scrollback(usize::MAX);
        let len = self.scrollback_offset();
        self.set_scrollback(previous);
        len
    }

    pub fn modes(&self) -> TermModes {
        let screen = self.parser.screen();
        TermModes {
            application_cursor: screen.application_cursor(),
            bracketed_paste: screen.bracketed_paste(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(rows: u16, cols: u16) -> Pane {
        Pane::new(PaneSize { rows, cols }, 100)
    }

    fn text_at(p: &Pane, row: u16, col: u16) -> String {
        p.cell(row, col).expect("cell in range").text.to_string()
    }

    // Spec: pane renders child output (text).
    #[test]
    fn printed_text_lands_in_consecutive_cells() {
        let mut p = pane(3, 10);
        p.feed(b"hi");
        assert_eq!(text_at(&p, 0, 0), "h");
        assert_eq!(text_at(&p, 0, 1), "i");
        assert_eq!(text_at(&p, 0, 2), "");
        p.feed(b"\r\nyo");
        assert_eq!(text_at(&p, 1, 0), "y");
        assert_eq!(text_at(&p, 1, 1), "o");
    }

    #[test]
    fn cell_out_of_range_is_none() {
        let p = pane(3, 10);
        assert!(p.cell(0, 9).is_some());
        assert!(p.cell(3, 0).is_none());
        assert!(p.cell(0, 10).is_none());
    }

    #[test]
    fn sgr_colors_and_attributes_are_mapped() {
        let mut p = pane(3, 10);
        p.feed(b"\x1b[31;1;3;4;7mA\x1b[0mB");
        let a = p.cell(0, 0).unwrap();
        assert_eq!(a.fg, TermColor::Idx(1));
        assert_eq!(a.bg, TermColor::Default);
        assert!(a.bold && a.italic && a.underline && a.inverse);
        let b = p.cell(0, 1).unwrap();
        assert_eq!(b.fg, TermColor::Default);
        assert!(!(b.bold || b.italic || b.underline || b.inverse));
    }

    #[test]
    fn truecolor_and_256_colors_are_mapped() {
        let mut p = pane(3, 10);
        p.feed(b"\x1b[38;2;10;20;30;48;5;200mX");
        let x = p.cell(0, 0).unwrap();
        assert_eq!(x.fg, TermColor::Rgb(10, 20, 30));
        assert_eq!(x.bg, TermColor::Idx(200));
    }

    #[test]
    fn wide_char_marks_its_continuation_cell() {
        let mut p = pane(3, 10);
        p.feed("世a".as_bytes());
        let head = p.cell(0, 0).unwrap();
        assert_eq!(head.text, "世");
        assert!(!head.wide_cont);
        assert!(p.cell(0, 1).unwrap().wide_cont);
        assert_eq!(text_at(&p, 0, 2), "a");
    }

    #[test]
    fn cursor_follows_output_and_hides_on_demand() {
        let mut p = pane(3, 10);
        assert_eq!(p.cursor(), Some((0, 0)));
        p.feed(b"ab");
        assert_eq!(p.cursor(), Some((0, 2)));
        p.feed(b"\x1b[?25l");
        assert_eq!(p.cursor(), None);
        p.feed(b"\x1b[?25h");
        assert_eq!(p.cursor(), Some((0, 2)));
    }

    // Spec: mode tracking.
    #[test]
    fn modes_track_application_cursor_and_bracketed_paste() {
        let mut p = pane(3, 10);
        assert_eq!(
            p.modes(),
            TermModes {
                application_cursor: false,
                bracketed_paste: false
            }
        );
        p.feed(b"\x1b[?1h");
        assert!(p.modes().application_cursor && !p.modes().bracketed_paste);
        p.feed(b"\x1b[?2004h");
        assert!(p.modes().application_cursor && p.modes().bracketed_paste);
        p.feed(b"\x1b[?1l\x1b[?2004l");
        assert!(!p.modes().application_cursor && !p.modes().bracketed_paste);
    }

    #[test]
    fn resize_changes_the_cell_grid() {
        let mut p = pane(3, 10);
        assert!(p.cell(2, 9).is_some());
        p.resize(PaneSize { rows: 2, cols: 5 });
        assert!(p.cell(1, 4).is_some());
        assert!(p.cell(2, 0).is_none());
        assert!(p.cell(0, 5).is_none());
    }

    // Spec: terminal queries are answered (responder).
    #[test]
    fn da1_is_answered() {
        let mut p = pane(24, 80);
        assert_eq!(p.feed(b"\x1b[c"), b"\x1b[?62;c");
        assert_eq!(p.feed(b"\x1b[0c"), b"\x1b[?62;c");
    }

    #[test]
    fn da2_and_other_probes_are_not_answered() {
        let mut p = pane(24, 80);
        let replies = p.feed(b"\x1b[>c\x1b[>0q\x1b[?u\x1b[?6n\x1b]11;?\x07");
        assert!(replies.is_empty(), "got {replies:?}");
    }

    #[test]
    fn dsr_5n_reports_ok() {
        let mut p = pane(24, 80);
        assert_eq!(p.feed(b"\x1b[5n"), b"\x1b[0n");
    }

    #[test]
    fn dsr_6n_reports_one_based_cursor_position() {
        let mut p = pane(24, 80);
        assert_eq!(p.feed(b"\x1b[6n"), b"\x1b[1;1R");
        assert_eq!(p.feed(b"\x1b[5;10H\x1b[6n"), b"\x1b[5;10R");
    }

    #[test]
    fn query_split_across_chunks_replies_exactly_once() {
        let mut p = pane(24, 80);
        assert!(p.feed(b"\x1b[").is_empty());
        assert!(p.feed(b"6").is_empty());
        assert_eq!(p.feed(b"n"), b"\x1b[1;1R");
        assert!(p.feed(b"x").is_empty(), "replies are drained, not repeated");
    }

    #[test]
    fn two_queries_in_one_chunk_reply_in_order() {
        let mut p = pane(24, 80);
        assert_eq!(p.feed(b"\x1b[5n\x1b[6n"), b"\x1b[0n\x1b[1;1R");
    }

    // Spec: DECSCUSR cursor shape (7 scenarios).
    fn shape(kind: CursorKind, blinking: bool) -> CursorShape {
        CursorShape { kind, blinking }
    }

    #[test]
    fn cursor_shape_defaults_to_default_steady() {
        let p = pane(3, 10);
        assert_eq!(p.cursor_shape(), shape(CursorKind::Default, false));
    }

    #[test]
    fn decscusr_6_is_steady_bar() {
        let mut p = pane(3, 10);
        p.feed(b"\x1b[6 q");
        assert_eq!(p.cursor_shape(), shape(CursorKind::Bar, false));
    }

    #[test]
    fn decscusr_5_is_blinking_bar() {
        let mut p = pane(3, 10);
        p.feed(b"\x1b[5 q");
        assert_eq!(p.cursor_shape(), shape(CursorKind::Bar, true));
    }

    #[test]
    fn decscusr_2_is_steady_block() {
        let mut p = pane(3, 10);
        p.feed(b"\x1b[6 q\x1b[2 q");
        assert_eq!(p.cursor_shape(), shape(CursorKind::Block, false));
    }

    #[test]
    fn decscusr_all_known_values_map_per_spec() {
        let expected = [
            (0, CursorKind::Default, false),
            (1, CursorKind::Block, true),
            (2, CursorKind::Block, false),
            (3, CursorKind::Underline, true),
            (4, CursorKind::Underline, false),
            (5, CursorKind::Bar, true),
            (6, CursorKind::Bar, false),
        ];
        for (ps, kind, blinking) in expected {
            assert_eq!(
                CursorShape::from_decscusr(ps),
                Some(shape(kind, blinking)),
                "Ps {ps}"
            );
        }
        assert_eq!(CursorShape::from_decscusr(7), None);
        assert_eq!(CursorShape::from_decscusr(u16::MAX), None);
    }

    #[test]
    fn decscusr_0_and_missing_ps_reset_to_default() {
        let mut p = pane(3, 10);
        p.feed(b"\x1b[6 q\x1b[0 q");
        assert_eq!(p.cursor_shape(), shape(CursorKind::Default, false));
        p.feed(b"\x1b[5 q\x1b[ q");
        assert_eq!(p.cursor_shape(), shape(CursorKind::Default, false));
    }

    #[test]
    fn decscusr_unknown_ps_is_ignored() {
        let mut p = pane(3, 10);
        p.feed(b"\x1b[6 q\x1b[9 q");
        assert_eq!(p.cursor_shape(), shape(CursorKind::Bar, false));
    }

    #[test]
    fn decscusr_split_across_chunks_applies_once_complete() {
        let mut p = pane(3, 10);
        p.feed(b"\x1b[");
        p.feed(b"5 ");
        assert_eq!(p.cursor_shape(), shape(CursorKind::Default, false));
        p.feed(b"q");
        assert_eq!(p.cursor_shape(), shape(CursorKind::Bar, true));
    }

    #[test]
    fn decscusr_never_produces_reply_bytes() {
        let mut p = pane(3, 10);
        assert!(p.feed(b"\x1b[6 q\x1b[2 q\x1b[ q").is_empty());
    }

    // Scrollback API (vt100 has no length accessor; clamp trick).
    fn feed_lines(p: &mut Pane, n: usize) {
        for i in 0..n {
            p.feed(format!("line{i}\r\n").as_bytes());
        }
    }

    #[test]
    fn scrollback_len_counts_history_and_restores_offset() {
        let mut p = Pane::new(PaneSize { rows: 3, cols: 10 }, 100);
        feed_lines(&mut p, 10); // 10 lines on 3 rows: 8 rows in history
        assert_eq!(p.scrollback_len(), 8);
        assert_eq!(p.scrollback_offset(), 0);
        p.set_scrollback(3);
        assert_eq!(p.scrollback_len(), 8);
        assert_eq!(p.scrollback_offset(), 3, "previous offset restored");
    }

    #[test]
    fn set_scrollback_clamps_to_history_and_to_capacity() {
        let mut p = Pane::new(PaneSize { rows: 3, cols: 10 }, 5);
        feed_lines(&mut p, 30);
        p.set_scrollback(2);
        assert_eq!(p.scrollback_offset(), 2);
        p.set_scrollback(1_000);
        assert_eq!(p.scrollback_offset(), 5, "clamped to configured capacity");
        assert_eq!(p.scrollback_len(), 5);
        let mut short = Pane::new(PaneSize { rows: 3, cols: 10 }, 100);
        feed_lines(&mut short, 5); // 3 rows in history
        short.set_scrollback(50);
        assert_eq!(short.scrollback_offset(), 3, "clamped to available history");
    }

    #[test]
    fn offset_larger_than_rows_shows_history_without_panic() {
        let mut p = Pane::new(PaneSize { rows: 3, cols: 10 }, 100);
        feed_lines(&mut p, 20);
        p.set_scrollback(9);
        assert_eq!(p.scrollback_offset(), 9);
        // 20 lines + blank cursor row = 21 rows total; top visible = 21-3-9 = 9.
        assert_eq!(text_at(&p, 0, 0), "l");
        assert_eq!(text_at(&p, 0, 4), "9");
        p.set_scrollback(usize::MAX);
        assert_eq!(p.scrollback_offset(), p.scrollback_len());
        assert!(p.cell(2, 0).is_some());
    }

    #[test]
    fn cursor_is_hidden_while_scrolled_back() {
        let mut p = Pane::new(PaneSize { rows: 3, cols: 10 }, 100);
        feed_lines(&mut p, 10);
        assert!(p.cursor().is_some());
        p.set_scrollback(2);
        assert_eq!(p.cursor(), None);
        p.set_scrollback(0);
        assert!(p.cursor().is_some());
    }

    #[test]
    fn scrolled_view_stays_anchored_when_new_output_arrives() {
        let mut p = Pane::new(PaneSize { rows: 3, cols: 10 }, 100);
        feed_lines(&mut p, 10);
        p.set_scrollback(2);
        let before = text_at(&p, 0, 0);
        feed_lines(&mut p, 2);
        assert_eq!(p.scrollback_offset(), 4);
        assert_eq!(text_at(&p, 0, 0), before);
    }

    #[test]
    fn alternate_screen_has_no_scrollback_and_main_history_survives() {
        let mut p = Pane::new(PaneSize { rows: 3, cols: 10 }, 100);
        feed_lines(&mut p, 10);
        p.feed(b"\x1b[?1049h");
        assert_eq!(p.scrollback_len(), 0);
        p.set_scrollback(5);
        assert_eq!(p.scrollback_offset(), 0);
        p.feed(b"\x1b[?1049l");
        assert_eq!(p.scrollback_len(), 8);
    }

    #[test]
    fn resize_while_scrolled_back_does_not_panic() {
        let mut p = Pane::new(PaneSize { rows: 5, cols: 20 }, 100);
        feed_lines(&mut p, 30);
        p.set_scrollback(10);
        p.resize(PaneSize { rows: 2, cols: 8 });
        assert!(p.cell(1, 7).is_some());
        assert!(p.scrollback_offset() <= p.scrollback_len());
        p.resize(PaneSize { rows: 1, cols: 1 });
        assert!(p.cell(0, 0).is_some());
        assert!(p.scrollback_offset() <= p.scrollback_len());
    }

    #[test]
    fn zero_size_is_clamped_to_one_cell() {
        let p = Pane::new(PaneSize { rows: 0, cols: 0 }, 10);
        assert!(p.cell(0, 0).is_some());
        assert!(p.cell(1, 0).is_none());
        assert!(p.cell(0, 1).is_none());
        let mut p = pane(3, 10);
        p.resize(PaneSize { rows: 0, cols: 0 });
        assert!(p.cell(0, 0).is_some());
        assert!(p.cell(1, 0).is_none());
        assert!(p.cell(0, 1).is_none());
        p.resize(PaneSize { rows: 0, cols: 5 });
        assert!(p.cell(0, 4).is_some());
        assert!(p.cell(1, 0).is_none());
    }

    #[test]
    fn dsr_6n_reports_live_cursor_while_scrolled_back() {
        let mut p = Pane::new(PaneSize { rows: 3, cols: 10 }, 100);
        feed_lines(&mut p, 10);
        p.feed(b"\x1b[2;4H");
        p.set_scrollback(3);
        assert_eq!(p.cursor(), None);
        assert_eq!(p.feed(b"\x1b[6n"), b"\x1b[2;4R");
    }

    #[test]
    fn malformed_da1_and_dsr_variants_are_not_answered() {
        for seq in [
            &b"\x1b[1c"[..],
            b"\x1b[?c",
            b"\x1b[0;1c",
            b"\x1b[?5n",
            b"\x1b[5;6n",
        ] {
            let mut p = pane(24, 80);
            let replies = p.feed(seq);
            assert!(replies.is_empty(), "{seq:?} got {replies:?}");
        }
    }
}
