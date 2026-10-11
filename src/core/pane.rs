//! Terminal emulator pane.
//!
//! Wraps a `vt100` parser behind our own DTOs so no `vt100` type leaks out of
//! this module: swapping the emulator only rewrites this file. Query replies
//! (DA1, DSR 5n/6n) are produced here and returned from [`Pane::feed`]; the
//! caller decides how to write them back to the child.

use std::path::{Path, PathBuf};

use vt100::{Callbacks, Parser, Screen};

use super::copy::{LineSpan, LineText};
use super::osc7;

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
    /// Last validated OSC 7 path; a rejected report leaves it untouched.
    cwd: Option<PathBuf>,
    /// This machine's hostname, accepted in OSC 7 URIs besides `localhost`.
    host: Option<String>,
}

impl Callbacks for Responder {
    // OSC 7 never replies: the payload is validated and kept, or dropped.
    fn unhandled_osc(&mut self, _: &mut Screen, params: &[&[u8]]) {
        if let Some(path) = osc7::parse(params, self.host.as_deref()) {
            self.cwd = Some(path);
        }
    }

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
    /// Rows of history, refreshed whenever the screen changes (`feed`, `resize`).
    history_len: usize,
    /// Configured scrollback capacity (rows).
    cap: usize,
    /// Rows evicted from the top of the scrollback so far.
    first_line: usize,
    line_generation: u32,
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
            history_len: 0,
            cap: scrollback,
            first_line: 0,
            line_generation: 0,
        }
    }

    /// Processes child output and returns the bytes to answer its queries with.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<u8> {
        self.process_counting_evictions(bytes);
        std::mem::take(&mut self.parser.callbacks_mut().replies)
    }

    /// Names this machine for OSC 7 host checks (`localhost` and an empty host
    /// are always local).
    pub fn with_host(mut self, host: Option<&str>) -> Self {
        self.set_host(host);
        self
    }

    /// In-place variant of [`Pane::with_host`].
    pub fn set_host(&mut self, host: Option<&str>) {
        self.parser.callbacks_mut().host = host.map(str::to_owned);
    }

    /// The last valid OSC 7 working directory the child reported.
    pub fn cwd(&self) -> Option<&Path> {
        self.parser.callbacks().cwd.as_deref()
    }

    /// Feeds `bytes` while counting rows evicted at the scrollback cap.
    ///
    /// On the main screen the view is pinned at offset 1 first: vt100 then
    /// moves the offset once per scrolled row, so `offset - 1` is the exact
    /// scroll count. The user's offset is restored as vt100 would have left it
    /// (`o0 + scrolled`). When the count cannot be known (screen flip, empty
    /// history, offset saturated at the cap) `line_generation` is bumped.
    fn process_counting_evictions(&mut self, bytes: &[u8]) {
        let was_alt = self.parser.screen().alternate_screen();
        let (h0, o0) = (self.history_len, self.scrollback_offset());
        if !was_alt {
            self.set_scrollback(1);
        }
        let pinned = self.scrollback_offset() == 1;
        self.parser.process(bytes);
        self.refresh_history_len();
        let h1 = self.history_len;
        if self.parser.screen().alternate_screen() != was_alt {
            self.bump_generation();
            return;
        }
        if was_alt {
            return;
        }
        let at_cap = self.cap > 0 && h1 >= self.cap;
        let scrolled = self.scrollback_offset().saturating_sub(1);
        if pinned {
            let evicted = (h0 + scrolled).saturating_sub(h1);
            self.first_line += evicted;
            // Probe lost: vt100 reset the offset (RIS, alt round-trip inside
            // one chunk) or history shrank by more than the scrolled rows can
            // account for. Leave the offset at 0 like native vt100.
            let probe_lost = self.scrollback_offset() == 0 || evicted > scrolled;
            if probe_lost {
                self.bump_generation();
                return;
            }
            self.set_scrollback(if o0 == 0 { 0 } else { o0 + scrolled });
            // Saturated: the offset hit the cap, so `scrolled` is a lower bound.
            // Pushing exactly cap - 1 rows while pinned also lands here: a
            // conservative bump.
            if at_cap && h1 == scrolled + 1 {
                self.bump_generation();
            }
        } else if at_cap {
            self.bump_generation();
        }
    }

    fn bump_generation(&mut self) {
        self.line_generation = self.line_generation.wrapping_add(1);
    }

    pub fn resize(&mut self, size: PaneSize) {
        self.parser
            .screen_mut()
            .set_size(size.rows.max(1), size.cols.max(1));
        self.refresh_history_len();
        self.bump_generation();
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

    /// Rows of history available (0 on the alternate screen).
    pub fn scrollback_len(&self) -> usize {
        self.history_len
    }

    /// vt100 has no accessor for the history length, so clamp to the maximum,
    /// read it back, and restore the previous offset.
    fn refresh_history_len(&mut self) {
        let previous = self.scrollback_offset();
        self.set_scrollback(usize::MAX);
        self.history_len = self.scrollback_offset();
        self.set_scrollback(previous);
    }

    /// Stable number of the oldest retained row; counts evicted rows.
    pub fn first_line(&self) -> usize {
        self.first_line
    }

    /// Changes whenever line numbers stop being comparable (screen flip,
    /// resize, lost eviction count).
    pub fn line_generation(&self) -> u32 {
        self.line_generation
    }

    /// The child's cursor `(row, col)` on the live screen, ignoring the
    /// scrollback offset; `None` when the child hid it.
    pub fn live_cursor(&self) -> Option<(u16, u16)> {
        let screen = self.parser.screen();
        (!screen.hide_cursor()).then(|| screen.cursor_position())
    }

    /// Last non-blank column of stable line `line`; 0 for a blank or
    /// unavailable line. The scrollback offset is restored.
    pub fn line_end(&mut self, line: usize) -> u16 {
        let saved = self.scrollback_offset();
        let end = self.page_to(line).map_or(0, |row| {
            let (_, cols) = self.parser.screen().size();
            (0..cols)
                .rev()
                .find(|&col| {
                    self.cell(row, col)
                        .is_some_and(|c| !matches!(c.text, "" | " "))
                })
                .unwrap_or(0)
        });
        self.set_scrollback(saved);
        end
    }

    /// Text of the cells of `span`, one entry per retained line. Both end
    /// cells are included, empty cells read as `" "`, wide characters appear
    /// once. The scrollback offset is restored.
    pub fn read_lines(&mut self, span: LineSpan) -> Vec<LineText> {
        let saved = self.scrollback_offset();
        let (rows, cols) = self.parser.screen().size();
        let end = self.first_line + self.history_len + usize::from(rows);
        let mut out = Vec::new();
        let mut line = span.first.max(self.first_line);
        let last = span.last.min(end.saturating_sub(1));
        // One page per pane height: each `page_to` exposes `rows` lines.
        while line <= last {
            let Some(mut row) = self.page_to(line) else {
                break;
            };
            while row < rows && line <= last {
                let from = if line == span.first {
                    span.first_col
                } else {
                    0
                };
                let to = if line == span.last {
                    span.last_col
                } else {
                    cols
                };
                out.push(self.row_text(row, from, to.min(cols.saturating_sub(1))));
                line += 1;
                row += 1;
            }
        }
        self.set_scrollback(saved);
        out
    }

    /// Cells `from..=to` of a viewport row. A range starting on the right half
    /// of a wide character starts at its head; continuation cells add nothing.
    fn row_text(&self, row: u16, from: u16, to: u16) -> LineText {
        let wide_cont = |col| self.cell(row, col).is_some_and(|c| c.wide_cont);
        let from = if from > 0 && wide_cont(from) {
            from - 1
        } else {
            from
        };
        let text = (from..=to)
            .filter_map(|col| self.cell(row, col))
            .filter(|c| !c.wide_cont)
            .map(|c| if c.text.is_empty() { " " } else { c.text })
            .collect();
        LineText {
            text,
            wrapped: self.parser.screen().row_wrapped(row),
        }
    }

    /// Scrolls so stable line `line` is visible and returns its viewport row,
    /// or `None` if the line was evicted or is past the screen. The caller
    /// restores the offset.
    fn page_to(&mut self, line: usize) -> Option<u16> {
        let (rows, _) = self.parser.screen().size();
        let base = self.first_line + self.history_len;
        if line < self.first_line || line >= base + usize::from(rows) {
            return None;
        }
        self.set_scrollback(base.saturating_sub(line));
        let top = base - self.scrollback_offset();
        u16::try_from(line - top).ok()
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
    fn scrollback_len_is_readable_through_a_shared_reference() {
        fn len_of(p: &Pane) -> usize {
            p.scrollback_len()
        }
        let mut p = Pane::new(PaneSize { rows: 3, cols: 10 }, 100);
        assert_eq!(len_of(&p), 0);
        feed_lines(&mut p, 10);
        assert_eq!(len_of(&p), 8);
        p.set_scrollback(4);
        assert_eq!(
            len_of(&p),
            8,
            "scrolling does not change the history length"
        );
        p.feed(b"\x1b[?1049h");
        assert_eq!(len_of(&p), 0, "alternate screen has no history");
        p.feed(b"\x1b[?1049l");
        assert_eq!(len_of(&p), 8);
        p.resize(PaneSize { rows: 6, cols: 10 });
        let after_resize = len_of(&p);
        p.set_scrollback(usize::MAX);
        assert_eq!(after_resize, p.scrollback_offset(), "cache follows resize");
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

    // Spec (terminal-emulation): OSC 7 working directory.
    #[test]
    fn osc7_with_bel_sets_the_cwd() {
        let mut p = pane(3, 10);
        p.feed(b"\x1b]7;file:///tmp\x07");
        assert_eq!(p.cwd(), Some(Path::new("/tmp")));
    }

    #[test]
    fn osc7_with_st_sets_the_cwd() {
        let mut p = pane(3, 10);
        p.feed(b"\x1b]7;file:///tmp\x1b\\");
        assert_eq!(p.cwd(), Some(Path::new("/tmp")));
    }

    #[test]
    fn osc7_semicolons_and_escapes_survive_the_parser_split() {
        let mut p = pane(3, 10);
        p.feed(b"\x1b]7;file:///tmp/a;b%20c\x07");
        assert_eq!(p.cwd(), Some(Path::new("/tmp/a;b c")));
    }

    #[test]
    fn osc7_split_across_chunks_is_reassembled() {
        let mut p = pane(3, 10);
        p.feed(b"\x1b]7;file:///t");
        assert_eq!(p.cwd(), None);
        p.feed(b"mp\x07");
        assert_eq!(p.cwd(), Some(Path::new("/tmp")));
    }

    #[test]
    fn a_rejected_osc7_keeps_the_previous_cwd() {
        let mut p = pane(3, 10).with_host(Some("myhost"));
        p.feed(b"\x1b]7;file:///tmp\x07");
        for bad in [
            &b"\x1b]7;file://other.example/etc\x07"[..],
            b"\x1b]7;file://host\x07",
            b"\x1b]7;http:///x\x07",
            b"\x1b]7;file:///a%zz\x07",
            b"\x1b]7;file:///a%00b\x07",
        ] {
            p.feed(bad);
            assert_eq!(p.cwd(), Some(Path::new("/tmp")));
        }
    }

    #[test]
    fn osc7_accepts_the_configured_hostname_only() {
        let mut p = pane(3, 10).with_host(Some("myhost"));
        p.feed(b"\x1b]7;file://MyHost/a\x07");
        assert_eq!(p.cwd(), Some(Path::new("/a")));
        let mut q = pane(3, 10);
        q.feed(b"\x1b]7;file://myhost/a\x07");
        assert_eq!(q.cwd(), None);
    }

    #[test]
    fn osc7_produces_no_reply_bytes() {
        let mut p = pane(3, 10);
        assert_eq!(p.feed(b"\x1b]7;file:///tmp\x07"), Vec::<u8>::new());
    }

    // copy-selection: line model (first_line, line_generation, live_cursor).
    fn small(rows: u16) -> Pane {
        Pane::new(PaneSize { rows, cols: 10 }, 5)
    }

    // Characterization test: pins current behavior, not new logic.
    #[test]
    fn first_line_is_zero_below_the_cap() {
        let mut p = small(3);
        feed_lines(&mut p, 7); // 5 rows in history: exactly at the cap
        assert_eq!(p.scrollback_len(), 5);
        assert_eq!(p.first_line(), 0);
    }

    #[test]
    fn first_line_counts_evictions_at_offset_zero() {
        let mut p = small(3);
        feed_lines(&mut p, 12); // 10 rows pushed, 5 evicted
        assert_eq!(p.first_line(), 5);
        assert_eq!(p.scrollback_offset(), 0);
        p.feed(b"a\r\nb\r\nc\r\n"); // three more lines in one chunk
        assert_eq!(p.first_line(), 8);
        assert_eq!(p.scrollback_offset(), 0);
    }

    #[test]
    fn first_line_counts_evictions_with_a_scrolled_view_and_keeps_vt100_pin() {
        let mut p = small(3);
        feed_lines(&mut p, 7);
        p.set_scrollback(1);
        feed_lines(&mut p, 2);
        assert_eq!(p.first_line(), 2);
        assert_eq!(p.scrollback_offset(), 3, "pinned as vt100 does natively");
        assert_eq!(text_at(&p, 0, 4), "4", "viewport stays on the same content");
    }

    #[test]
    fn first_line_ignores_scrolls_that_stay_in_history() {
        let mut p = small(3);
        feed_lines(&mut p, 4); // 2 rows of history, cap 5
        p.set_scrollback(1);
        feed_lines(&mut p, 2); // 4 rows of history
        assert_eq!(p.first_line(), 0);
        assert_eq!(p.scrollback_offset(), 3);
    }

    #[test]
    fn line_generation_ignores_plain_output() {
        let mut p = small(3);
        assert_eq!(p.line_generation(), 0);
        feed_lines(&mut p, 4);
        assert_eq!(p.line_generation(), 0);
        p.set_scrollback(1);
        feed_lines(&mut p, 2);
        assert_eq!(p.line_generation(), 0);
    }

    #[test]
    fn line_generation_bumps_on_each_screen_flip() {
        let mut p = small(3);
        feed_lines(&mut p, 4);
        p.feed(b"\x1b[?1049h");
        assert_eq!(p.line_generation(), 1);
        p.feed(b"\x1b[?1049l");
        assert_eq!(p.line_generation(), 2);
    }

    #[test]
    fn line_generation_bumps_on_resize() {
        let mut p = small(3);
        p.resize(PaneSize { rows: 4, cols: 10 });
        assert_eq!(p.line_generation(), 1);
    }

    #[test]
    fn line_generation_bumps_when_the_eviction_count_is_lost() {
        let mut p = small(3);
        // One chunk with empty history at the start: the probe cannot pin.
        p.feed("x\r\n".repeat(20).as_bytes());
        assert_eq!(p.line_generation(), 1);
    }

    #[test]
    fn line_generation_bumps_when_an_alt_round_trip_hides_the_eviction_count() {
        let mut p = small(3);
        feed_lines(&mut p, 12);
        p.set_scrollback(2);
        let gen_before = p.line_generation();
        p.feed(("n\r\n".repeat(3) + "\x1b[?1049h\x1b[?1049l").as_bytes());
        assert_eq!(p.line_generation(), gen_before + 1);
        assert_eq!(p.scrollback_offset(), 0, "left at 0 like native vt100");
    }

    #[test]
    fn line_generation_bumps_on_ris() {
        let mut p = small(3);
        feed_lines(&mut p, 4);
        p.set_scrollback(1);
        p.feed(b"\x1bc");
        assert_eq!(p.line_generation(), 1);
    }

    #[test]
    fn line_generation_bumps_when_the_cap_saturates_the_pinned_offset() {
        let mut p = small(3);
        feed_lines(&mut p, 12); // history at the cap (5), first_line 5
        assert_eq!(p.line_generation(), 0);
        p.set_scrollback(1);
        p.feed("n\r\n".repeat(8).as_bytes()); // more lines than the cap
        assert_eq!(p.line_generation(), 1);
        assert_eq!(p.first_line(), 9, "documented lower bound (5 + cap - 1)");
    }

    #[test]
    fn a_zero_capacity_pane_never_bumps_the_line_generation() {
        let mut p = Pane::new(PaneSize { rows: 3, cols: 10 }, 0);
        feed_lines(&mut p, 5);
        assert_eq!(p.line_generation(), 0);
    }

    #[test]
    fn live_cursor_ignores_the_scrollback_offset() {
        let mut p = small(3);
        feed_lines(&mut p, 7);
        p.feed(b"\x1b[2;4H");
        assert_eq!(p.live_cursor(), Some((1, 3)));
        p.set_scrollback(2);
        assert_eq!(p.cursor(), None);
        assert_eq!(p.live_cursor(), Some((1, 3)));
        assert_eq!(p.scrollback_offset(), 2);
    }

    #[test]
    fn line_end_is_the_last_non_blank_column() {
        let mut p = pane(5, 20);
        p.feed(b"echo hi\r\n\r\n  x  ");
        assert_eq!(p.line_end(0), 6);
        assert_eq!(p.line_end(1), 0, "blank row");
        assert_eq!(p.line_end(2), 2, "trailing blanks are not content");
        assert_eq!(p.line_end(99), 0, "unavailable line");
    }

    #[test]
    fn line_end_reads_history_and_restores_the_offset() {
        let mut p = small(3);
        feed_lines(&mut p, 12); // lines 5..=9 are history: line5 .. line9
        assert_eq!(p.first_line(), 5);
        p.set_scrollback(2);
        let top = text_at(&p, 0, 4);
        assert_eq!(p.line_end(5), 4);
        assert_eq!(p.line_end(9), 4);
        assert_eq!(p.scrollback_offset(), 2);
        assert_eq!(text_at(&p, 0, 4), top);
        p.set_scrollback(0);
        assert_eq!(p.line_end(6), 4);
        assert_eq!(p.scrollback_offset(), 0);
    }

    fn padded(text: &str) -> String {
        format!("{text:<10}")
    }

    fn lt(text: &str, wrapped: bool) -> LineText {
        LineText {
            text: text.to_owned(),
            wrapped,
        }
    }

    #[test]
    fn read_lines_spans_history_and_screen_and_restores_the_offset() {
        let mut p = pane(3, 10);
        feed_lines(&mut p, 10); // lines 0..=7 history, 8 and 9 on screen
        p.set_scrollback(1);
        let span = LineSpan::new((6, 0), (9, 3));
        assert_eq!(
            p.read_lines(span),
            [
                lt(&padded("line6"), false),
                lt(&padded("line7"), false),
                lt(&padded("line8"), false),
                lt("line", false),
            ]
        );
        assert_eq!(p.scrollback_offset(), 1);
        p.set_scrollback(0);
        let above = p.read_lines(LineSpan::new((0, 0), (0, 4)));
        assert_eq!(above, [lt("line0", false)]);
        assert_eq!(p.scrollback_offset(), 0);
    }

    #[test]
    fn read_lines_pages_across_more_rows_than_the_pane_height() {
        let mut p = pane(3, 10);
        feed_lines(&mut p, 30);
        p.set_scrollback(4);
        let got = p.read_lines(LineSpan::new((0, 0), (20, 9)));
        let want: Vec<LineText> = (0..=20)
            .map(|i| lt(&padded(&format!("line{i}")), false))
            .collect();
        assert_eq!(got, want);
        assert_eq!(p.scrollback_offset(), 4);
    }

    #[test]
    fn read_lines_includes_both_end_cells_and_reads_empty_cells_as_spaces() {
        let mut p = pane(5, 10);
        p.feed(b"hello");
        assert_eq!(
            p.read_lines(LineSpan::new((0, 1), (0, 3))),
            [lt("ell", false)]
        );
        assert_eq!(
            p.read_lines(LineSpan::new((0, 3), (0, 6))),
            [lt("lo  ", false)]
        );
    }

    #[test]
    fn read_lines_reports_the_soft_wrap_flag_of_each_row() {
        let mut p = pane(3, 5);
        p.feed(b"abcdefgh");
        assert_eq!(
            p.read_lines(LineSpan::new((0, 0), (1, 4))),
            [lt("abcde", true), lt("fgh  ", false)]
        );
    }

    #[test]
    fn read_lines_skips_evicted_lines() {
        let mut p = small(3);
        feed_lines(&mut p, 12);
        assert_eq!(p.first_line(), 5);
        assert_eq!(
            p.read_lines(LineSpan::new((3, 1), (6, 2))),
            [lt(&padded("line5"), false), lt("lin", false)]
        );
    }

    #[test]
    fn read_lines_emits_a_wide_character_once() {
        let mut p = pane(3, 10);
        p.feed("a漢b".as_bytes()); // a=0, 漢 head=1 continuation=2, b=3
        assert_eq!(
            p.read_lines(LineSpan::new((0, 0), (0, 3))),
            [lt("a漢b", false)]
        );
        assert_eq!(
            p.read_lines(LineSpan::new((0, 2), (0, 3))),
            [lt("漢b", false)]
        );
        assert_eq!(
            p.read_lines(LineSpan::new((0, 0), (0, 1))),
            [lt("a漢", false)]
        );
        assert_eq!(
            p.read_lines(LineSpan::new((0, 1), (0, 2))),
            [lt("漢", false)]
        );
    }

    #[test]
    fn live_cursor_is_none_when_the_child_hides_it() {
        let mut p = small(3);
        p.feed(b"\x1b[?25l");
        assert_eq!(p.live_cursor(), None);
    }
}
