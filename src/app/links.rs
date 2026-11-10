//! Opening a link a pane shows (pane-links): the rows of the pane go through
//! the detector, and a click is looked up in what it found.

use super::{scroll::rect_has, *};
use crate::core::ai::chat::transcript::{Line, Wrap};
use crate::core::links::{self, Link, Row};

/// The links of a pane and how to find a drawn row among them.
struct PaneLinks {
    links: Vec<Link>,
    /// Rows the detector read before the first drawn one.
    above: usize,
    /// The pane row of the first row of text.
    first_row: usize,
    /// How many rows of text are drawn.
    drawn: usize,
}

impl PaneLinks {
    /// The link drawn at the cell `(col, row)` of the pane.
    fn at(&self, col: usize, row: usize) -> Option<&Link> {
        let row = row
            .checked_sub(self.first_row)
            .filter(|r| *r < self.drawn)?
            + self.above;
        self.links.iter().find(|link| {
            link.spans
                .iter()
                .any(|span| span.row == row && (span.start..span.end).contains(&col))
        })
    }
}

impl PaneLinks {
    /// The links with at least one drawn row, in reading order.
    fn visible(self) -> Vec<String> {
        let drawn = self.above..self.above + self.drawn;
        (self.links.into_iter())
            .filter(|link| link.spans.iter().any(|span| drawn.contains(&span.row)))
            .map(|link| link.url)
            .collect()
    }
}

impl App {
    /// The left button came up: a click opens the link under it when the
    /// press was on the same cell, so a drag never opens one.
    pub(super) fn on_link_release(&mut self, mouse: MouseEvent) -> Vec<Effect> {
        let press = self.link_press.take();
        if press != Some((mouse.column, mouse.row)) || !self.mouse_reads_panes() {
            return Vec::new();
        }
        let tiling = self.tiling();
        let Some((id, rect)) = tiling
            .panes
            .iter()
            .find(|(_, rect)| rect_has(rect, mouse.column, mouse.row))
            .copied()
        else {
            return Vec::new();
        };
        // What covers the pane is not the text the links were found in.
        if self.run_view_open(id) || self.batch_pending(id) {
            return Vec::new();
        }
        let (col, row) = (mouse.column - rect.x, mouse.row - rect.y);
        let url = self
            .pane_links(id)
            .and_then(|found| found.at(usize::from(col), usize::from(row)).cloned());
        url.map_or_else(Vec::new, |link| self.open_link(link.url))
    }

    /// The links pane `id` shows; `None` for a chat with no transcript.
    fn pane_links(&mut self, id: PaneId) -> Option<PaneLinks> {
        if self.panes.get(&id)?.chat_state().is_some() {
            return self.chat_links(id);
        }
        let state = self.panes.get_mut(&id)?;
        let drawn = usize::from(state.size.rows);
        let pty = state.pty_mut()?;
        let (texts, above) = pty.emu.link_rows();
        let rows: Vec<Row> = texts
            .iter()
            .map(|row| Row {
                text: &row.text,
                lead: 0,
                joins_next: row.wrapped,
            })
            .collect();
        Some(PaneLinks {
            links: links::detect(&rows),
            above,
            first_row: 0,
            drawn,
        })
    }

    fn chat_links(&self, id: PaneId) -> Option<PaneLinks> {
        if !self.chat_scrollable(id) {
            return None;
        }
        let drawn = self.chat_viewport(id);
        let state = self.panes.get(&id)?;
        let chat = state.chat_state()?;
        let lines = chat.lines(usize::from(state.size.cols));
        let top = chat.scroll().top(lines.len(), drawn);
        let (first, end) = chat_link_window(&lines, top, drawn);
        let rows: Vec<Row> = lines[first..end]
            .iter()
            .map(|line| Row {
                text: &line.text,
                lead: line.lead,
                joins_next: line.wrap == Wrap::Cut,
            })
            .collect();
        Some(PaneLinks {
            links: links::detect(&rows),
            above: top - first,
            first_row: usize::from(chat.transcript_first_row(state.size.cols, state.size.rows)),
            drawn,
        })
    }

    /// The list of the links the focused pane shows; a notice with none.
    pub(super) fn open_link_list(&mut self) -> Vec<Effect> {
        let id = self.tab().focus;
        let covered = self.run_view_open(id) || self.batch_pending(id);
        let urls = (!covered)
            .then(|| self.pane_links(id))
            .flatten()
            .map_or_else(Vec::new, PaneLinks::visible);
        match LinkList::new(urls) {
            Some(list) => {
                self.link_list = Some(list);
                self.input = InputMode::Links;
            }
            None => self.notice = Some("No links in this pane".to_owned()),
        }
        Vec::new()
    }

    pub(super) fn on_link_list_key(&mut self, key: &KeyEvent, held: bool) -> Vec<Effect> {
        let Some(list) = self.link_list.as_mut() else {
            self.input = InputMode::Terminal;
            return Vec::new();
        };
        let action = list.on_key(key, held);
        if action == ListAction::None {
            return Vec::new();
        }
        self.link_list = None;
        self.input = InputMode::Terminal;
        match action {
            ListAction::Open(url) => self.open_link(url),
            ListAction::Copy(url) => self.copy_to_clipboard(&url),
            ListAction::None | ListAction::Close => Vec::new(),
        }
    }

    /// The open list of links, for the renderer.
    pub fn link_list(&self) -> Option<&LinkList> {
        self.link_list.as_ref()
    }

    /// Opens `url`, or copies it on a host with nothing to open it with.
    fn open_link(&mut self, url: String) -> Vec<Effect> {
        if !self.opener {
            return self.copy_unopened_link(&url);
        }
        self.notice = Some(format!("Opened {url}"));
        self.dirty = true;
        vec![Effect::OpenUrl(url)]
    }
}

/// The lines of the view, with the ones a wrapped link crosses its edges to.
fn chat_link_window(lines: &[Line], top: usize, drawn: usize) -> (usize, usize) {
    let mut first = top.min(lines.len());
    while first > 0 && lines[first - 1].wrap == Wrap::Cut {
        first -= 1;
    }
    let mut end = (top + drawn).min(lines.len());
    while end > 0 && end < lines.len() && lines[end - 1].wrap == Wrap::Cut {
        end += 1;
    }
    (first, end)
}
