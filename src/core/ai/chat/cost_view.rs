//! The `/cost` overlay state of a chat pane: the lines of the report, the
//! scroll over their wrapped rows and a one-key note. Pure: the pane draws it.

use std::cell::Cell;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::dashboard::{Dashboard, Range, TEXT_BELOW, UsageData};
use super::wrap::wrap_words;
use crate::core::ai::chat::usage_view::HEADER;

/// Shown while the store thread reads.
pub const READING_LINE: &str = "reading usage…";
/// The notice for `/cost` followed by text.
pub const NO_ARGUMENTS: &str = "/cost takes no arguments";
/// What Ctrl+C says while the view is open: it is not a cancel.
pub const CLOSE_FIRST: &str = "Esc closes this view; Ctrl+C then cancels the answer";

/// What a range key says when the pane is too narrow for the dashboard.
pub const WIDEN: &str = "widen the pane to change the range";

/// Rows a page key scrolls.
const PAGE: usize = 10;

/// What the overlay shows besides the text lines.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Body {
    /// The text report only (reading, unavailable).
    Text,
    /// The usage the store read and the model of the range shown, rebuilt on
    /// a range change with no new read.
    Data(Box<UsageData>, Box<Dashboard>),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CostView {
    lines: Vec<String>,
    body: Body,
    /// The columns the last draw had.
    width: Cell<u16>,
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
            body: Body::Text,
            width: Cell::new(0),
            scroll: 0,
            layout: Cell::new((0, 0)),
            note: None,
        }
    }

    /// Replaces the lines (the store had no data), keeping the scroll.
    pub fn set_lines(&mut self, lines: Vec<String>) {
        self.lines = lines;
        self.body = Body::Text;
    }

    /// The usage the store read: the dashboard, with `lines` (the text
    /// report) for a pane too narrow for it.
    pub fn with_data(data: UsageData, lines: Vec<String>) -> Self {
        let mut view = Self::with(Vec::new());
        view.set_data(data, lines);
        view
    }

    /// Replaces the text and the usage (the store answered), keeping the
    /// scroll and the range the user chose.
    pub fn set_data(&mut self, data: UsageData, lines: Vec<String>) {
        let range = self.dashboard().map_or_else(Range::default, |d| d.range);
        self.lines = lines;
        let model = Dashboard::build(&data, range);
        self.body = Body::Data(Box::new(data), Box::new(model));
    }

    /// The model of the range shown, when the store answered with data.
    pub fn dashboard(&self) -> Option<&Dashboard> {
        match &self.body {
            Body::Data(_, model) => Some(model),
            Body::Text => None,
        }
    }

    /// The columns a draw had: under 40 the text report is shown.
    pub fn set_width(&self, width: u16) {
        self.width.set(width);
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

    /// A range key: the model is rebuilt from the usage already read, with no
    /// new read. In the text tier there is nothing to change, so it says so.
    fn pick(&mut self, to: impl FnOnce(Range) -> Range) {
        let Body::Data(data, model) = &mut self.body else {
            return;
        };
        if self.width.get() < TEXT_BELOW {
            self.note = Some(WIDEN);
        } else {
            **model = Dashboard::build(data, to(model.range));
        }
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
            KeyCode::Char(c @ '1'..='4') => {
                self.pick(|_| Range::ALL[usize::from(c as u8 - b'1')]);
            }
            KeyCode::Left | KeyCode::Char('h') => self.pick(Range::prev),
            KeyCode::Right | KeyCode::Char('l') => self.pick(Range::next),
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
