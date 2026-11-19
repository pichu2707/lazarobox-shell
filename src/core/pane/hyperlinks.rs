//! OSC 8 hyperlinks a pane has drawn (osc8-pane-links): where each one was
//! when it ended, and the text its cells showed then. A link whose cells
//! show other text now was drawn over, and is no link any more.
//!
//! Places are stable lines (see [`Pane::first_line`]). The pane forgets
//! every link when they stop being comparable (`bump_generation`).

use std::collections::VecDeque;

use vt100::Screen;

use super::super::links::{Link, Span};
use super::super::osc8::{self, Osc8};
use super::Pane;

/// Links kept per pane; the oldest go first.
const MAX_LINKS: usize = 256;
/// Rows one link may take.
const MAX_ROWS: usize = 32;

/// The cells `start..end` of one row of a link, and the text they showed.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Piece {
    start: u16,
    end: u16,
    text: String,
}

/// A link that ended: from stable line `line` on, one piece per row.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Hyperlink {
    url: String,
    line: usize,
    pieces: Vec<Piece>,
}

impl Hyperlink {
    fn last_line(&self) -> usize {
        self.line + self.pieces.len() - 1
    }

    fn piece_on(&self, line: usize) -> Option<&Piece> {
        self.pieces.get(line.checked_sub(self.line)?)
    }

    /// Whether the two take a cell in common: the newer one was drawn over
    /// the older.
    fn overlaps(&self, other: &Self) -> bool {
        (self.pieces.iter().enumerate()).any(|(index, piece)| {
            other
                .piece_on(self.line + index)
                .is_some_and(|theirs| piece.start < theirs.end && theirs.start < piece.end)
        })
    }
}

/// Where the stable lines stood before a chunk of output was fed.
#[derive(Clone, Copy, Default)]
pub(super) struct Base {
    /// The stable line of screen row 0.
    pub line: usize,
    /// Rows of history.
    pub history: usize,
    /// The view was pinned at offset 1, so the offset counts scrolled rows.
    pub pinned: bool,
}

/// The links of one pane, kept by its vt100 callbacks.
#[derive(Default)]
pub(super) struct Recorder {
    pub base: Base,
    /// The URI and the `(line, col)` of a link still being drawn.
    open: Option<(String, usize, u16)>,
    links: VecDeque<Hyperlink>,
}

impl Recorder {
    /// Takes an OSC sequence; `false` when it is not OSC 8.
    pub fn on_osc(&mut self, screen: &mut Screen, params: &[&[u8]]) -> bool {
        let Some(osc) = osc8::parse(params) else {
            return false;
        };
        let saved = screen.scrollback();
        let top = self.top_line(screen, saved);
        // Cells are read from the screen, not from the history in view.
        screen.set_scrollback(0);
        let (row, col) = screen.cursor_position();
        let here = (top + usize::from(row), col);
        if let Some(open) = self.open.take()
            && let Some(link) = ended(screen, open, here, top)
        {
            self.keep(link);
        }
        if let Osc8::Open(url) = osc {
            let (_, cols) = screen.size();
            // A wrap is pending: the text goes on at the next row.
            let (line, col) = if col >= cols { (here.0 + 1, 0) } else { here };
            self.open = Some((url, line, col));
        }
        screen.set_scrollback(saved);
        true
    }

    /// The stable line of screen row 0 now, part way through a chunk.
    fn top_line(&self, screen: &mut Screen, offset: usize) -> usize {
        let scrolled = if screen.alternate_screen() {
            0
        } else if self.base.pinned {
            offset.saturating_sub(1)
        } else {
            // No history to pin the view in before the chunk: nothing was
            // evicted yet, so the history grew by the rows scrolled.
            screen.set_scrollback(usize::MAX);
            screen.scrollback().saturating_sub(self.base.history)
        };
        self.base.line + scrolled
    }

    fn keep(&mut self, link: Hyperlink) {
        self.links.retain(|old| !old.overlaps(&link));
        if self.links.len() >= MAX_LINKS {
            self.links.pop_front();
        }
        self.links.push_back(link);
    }

    pub fn clear(&mut self) {
        self.open = None;
        self.links.clear();
    }
}

/// The link that started at `(line, col)` and ends at the cursor, with the
/// text it shows; `None` when it is no run of cells a reader can follow.
fn ended(
    screen: &Screen,
    (url, line, col): (String, usize, u16),
    (mut end_line, mut end_col): (usize, u16),
    top: usize,
) -> Option<Hyperlink> {
    let (_, cols) = screen.size();
    // Ended after the line break that followed its text.
    if end_col == 0 && end_line > line {
        end_line -= 1;
        let row = u16::try_from(end_line.checked_sub(top)?).ok()?;
        let from = if end_line == line { col } else { 0 };
        end_col = (from..cols)
            .rev()
            .find(|&at| !cells_text(screen, row, at, at + 1).trim().is_empty())
            .map_or(from, |at| at + 1);
    }
    if (end_line, end_col) <= (line, col) || line < top || end_line - line >= MAX_ROWS {
        return None;
    }
    let mut pieces = Vec::new();
    for at in line..=end_line {
        let row = u16::try_from(at - top).ok()?;
        let last = at == end_line;
        if !last && !screen.row_wrapped(row) {
            return None;
        }
        let start = if at == line { col } else { 0 };
        let end = if last { end_col.min(cols) } else { cols };
        if start >= end {
            return None;
        }
        let text = cells_text(screen, row, start, end);
        pieces.push(Piece { start, end, text });
    }
    Some(Hyperlink { url, line, pieces })
}

/// The text of cells `from..to` of screen row `row` as the view draws it.
fn cells_text(screen: &Screen, row: u16, from: u16, to: u16) -> String {
    (from..to)
        .filter_map(|col| screen.cell(row, col))
        .filter(|cell| !cell.is_wide_continuation())
        .map(|cell| match cell.contents() {
            "" => " ",
            text => text,
        })
        .collect()
}

impl Pane {
    /// The OSC 8 links in the rows [`Pane::link_rows`] reads, with their
    /// spans counted from its first row, in reading order. The scrollback
    /// offset is restored.
    pub fn hyperlinks(&mut self) -> Vec<Link> {
        let (first, last, _) = self.link_window();
        let stored: Vec<Hyperlink> = (self.parser.callbacks().links.links.iter())
            .filter(|link| first <= link.line && link.last_line() <= last)
            .cloned()
            .collect();
        let saved = self.scrollback_offset();
        let mut found: Vec<Link> = (stored.iter())
            .filter_map(|link| self.still_drawn(link, first))
            .collect();
        self.set_scrollback(saved);
        found.sort_by_key(|link| (link.spans[0].row, link.spans[0].start));
        found
    }

    /// `link` as a [`Link`], when its cells still show what they showed.
    fn still_drawn(&mut self, link: &Hyperlink, first: usize) -> Option<Link> {
        let mut spans = Vec::new();
        for (index, piece) in link.pieces.iter().enumerate() {
            let line = link.line + index;
            let row = self.page_to(line)?;
            if cells_text(self.parser.screen(), row, piece.start, piece.end) != piece.text {
                return None;
            }
            spans.push(Span {
                row: line - first,
                start: usize::from(piece.start),
                end: usize::from(piece.end),
            });
        }
        let shown: String = link
            .pieces
            .iter()
            .map(|piece| piece.text.as_str())
            .collect();
        Some(Link {
            hidden: shown.trim() != link.url,
            url: link.url.clone(),
            spans,
        })
    }
}

#[cfg(test)]
mod tests;
