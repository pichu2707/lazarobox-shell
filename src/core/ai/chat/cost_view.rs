//! The `/cost` overlay state of a chat pane: the lines of the report, the
//! scroll over their wrapped rows and a one-key note. Pure: the pane draws it.

use std::cell::Cell;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::wrap::wrap_words;
use crate::core::ai::chat::usage_view::HEADER;

/// Shown while the store thread reads.
pub const READING_LINE: &str = "reading usage…";
/// The notice for `/cost` followed by text.
pub const NO_ARGUMENTS: &str = "/cost takes no arguments";
/// What Ctrl+C says while the view is open: it is not a cancel.
pub const CLOSE_FIRST: &str = "Esc closes this view; Ctrl+C then cancels the answer";

/// Rows a page key scrolls.
const PAGE: usize = 10;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CostView {
    lines: Vec<String>,
    scroll: usize,
    /// The body rows the window shows and the body rows there are, as the
    /// last draw measured them (the draw is the only one that knows the width).
    layout: Cell<(usize, usize)>,
    note: Option<&'static str>,
}

impl CostView {
    /// The view while the store thread reads.
    pub fn reading() -> Self {
        Self::with(vec![HEADER.to_owned(), READING_LINE.to_owned()])
    }

    pub fn with(lines: Vec<String>) -> Self {
        Self {
            lines,
            scroll: 0,
            layout: Cell::new((0, 0)),
            note: None,
        }
    }

    /// Replaces the lines (the store answered), keeping the scroll.
    pub fn set_lines(&mut self, lines: Vec<String>) {
        self.lines = lines;
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn note(&self) -> Option<&'static str> {
        self.note
    }

    /// The body rows (every line but the header) wrapped to `width` columns,
    /// each with whether it belongs to the line about this chat.
    pub fn body_rows(&self, width: usize) -> Vec<(String, bool)> {
        self.lines
            .iter()
            .skip(1)
            .flat_map(|line| {
                let mine = line.starts_with("this chat");
                wrap_words(line, width)
                    .into_iter()
                    .map(move |row| (row, mine))
            })
            .collect()
    }

    /// What a draw measured: `window` rows shown of `total`. The scroll is
    /// brought back within what the window hides.
    pub fn set_layout(&self, window: usize, total: usize) {
        self.layout.set((window, total));
    }

    /// The first body row to draw, within what the window hides.
    pub fn scroll(&self) -> usize {
        let (window, total) = self.layout.get();
        self.scroll.min(total.saturating_sub(window))
    }

    /// A key on the view: `true` when it closes.
    pub fn on_key(&mut self, key: &KeyEvent) -> bool {
        self.note = None;
        let at = self.scroll();
        match key.code {
            KeyCode::Esc => return true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.note = Some(CLOSE_FIRST);
            }
            KeyCode::Up | KeyCode::Char('k') => self.scroll = at.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll = at + 1,
            KeyCode::PageUp => self.scroll = at.saturating_sub(PAGE),
            KeyCode::PageDown => self.scroll = at + PAGE,
            _ => {}
        }
        self.scroll = self.scroll();
        false
    }
}

#[cfg(test)]
mod tests;
