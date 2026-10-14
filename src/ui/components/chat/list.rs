//! The conversation list drawn over a chat pane: a title, one row per
//! conversation, and a last row for the delete prompt, a line or the keys.
//! Everything is already sanitized text in styled cells.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
};

use super::{ChatView, fill};
use crate::core::ai::chat::list::{ListView, row_text};

/// Columns taken by the cursor marker.
const MARKER_WIDTH: u16 = 2;

impl ChatView<'_> {
    /// Draws the list over `area`.
    pub(super) fn render_list(&self, list: &ListView, area: Rect, buf: &mut Buffer) {
        let theme = self.theme;
        let base = Style::default().bg(theme.bg_base);
        let muted = base.fg(theme.text_muted);
        fill(buf, area, base);
        let put = |buf: &mut Buffer, y: u16, text: &str, style: Style| {
            if y < area.height {
                buf.set_stringn(area.x, area.y + y, text, usize::from(area.width), style);
            }
        };
        put(
            buf,
            0,
            &list.title(),
            base.fg(theme.primary_cyan).add_modifier(Modifier::BOLD),
        );

        // Row 0 is the title and the last row the prompt, a line or the keys;
        // the rows between them are the window on the list.
        let window = usize::from(area.height.saturating_sub(2));
        if let Some(empty) = list.empty_text() {
            put(buf, 1, &empty, muted);
        } else if window > 0 {
            let cursor = list.cursor();
            let top = (cursor + 1).saturating_sub(window);
            let width = usize::from(area.width.saturating_sub(MARKER_WIDTH));
            for (n, row) in list
                .visible()
                .into_iter()
                .skip(top)
                .take(window)
                .enumerate()
            {
                let y = 1 + u16::try_from(n).unwrap_or(0);
                let selected = top + n == cursor;
                let (marker, style) = if selected {
                    ("▸ ", base.fg(theme.primary_cyan))
                } else {
                    ("  ", base)
                };
                put(buf, y, &format!("{marker}{}", row_text(row, width)), style);
            }
        }

        let last = area.height.saturating_sub(1).max(1);
        if let Some(prompt) = list.prompt() {
            put(buf, last, &prompt, base.fg(theme.warning_orange));
        } else if let Some(line) = list.line() {
            put(buf, last, line, base.fg(theme.warning_orange));
        } else {
            let hint = if list.project().is_some() {
                "↵ open · n new · d delete · a all projects"
            } else {
                "↵ open · n new · d delete"
            };
            put(buf, last, hint, muted);
        }
    }
}
