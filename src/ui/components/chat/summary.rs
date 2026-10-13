//! The summary draft drawn over a chat pane: its title line, the text, a line
//! for the hint or notice and a line of keys. Everything is already sanitized
//! text in styled cells.

use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::Style,
};

use super::{ChatView, fill};
use crate::core::ai::chat::summary::SummaryDraft;

/// The line above the text: nothing is kept until the user approves.
const TITLE: &str = "Summary (not saved until you approve)";
/// The keys, shown on the last row.
const KEYS: &str = "Ctrl+S approve · Esc discard";

/// Where the parts of the draft sit in a pane.
struct SummaryLayout {
    title: Rect,
    content: Rect,
    status: Rect,
    keys: Rect,
}

fn layout(area: Rect) -> SummaryLayout {
    let row = |y: u16| {
        Rect::new(
            area.x,
            area.y + y.min(area.height),
            area.width,
            u16::from(y < area.height),
        )
    };
    // The two last rows are the status and the keys; the text is what is
    // left under the title.
    let content_top = 1u16.min(area.height);
    let content_bottom = area.height.saturating_sub(2).max(content_top);
    let indent = 2u16.min(area.width);
    SummaryLayout {
        title: row(0),
        content: Rect::new(
            area.x + indent,
            area.y + content_top,
            area.width - indent,
            content_bottom - content_top,
        ),
        status: row(area.height.saturating_sub(2).max(2)),
        keys: row(area.height.saturating_sub(1).max(3)),
    }
}

impl ChatView<'_> {
    /// Draws the draft over `area`.
    pub(super) fn render_summary(&self, draft: &SummaryDraft, area: Rect, buf: &mut Buffer) {
        let theme = self.theme;
        let base = Style::default().bg(theme.bg_base);
        fill(buf, area, base);
        let layout = layout(area);
        let muted = base.fg(theme.text_muted);
        let put = |buf: &mut Buffer, rect: Rect, text: &str, style: Style| {
            if !rect.is_empty() {
                buf.set_stringn(rect.x, rect.y, text, usize::from(rect.width), style);
            }
        };
        put(buf, layout.title, TITLE, base.fg(theme.primary_cyan));
        if !layout.content.is_empty() {
            let plan = draft
                .content_composer()
                .plan(usize::from(layout.content.width));
            let view = plan.view(usize::from(layout.content.height));
            for (n, text) in view.rows.iter().enumerate() {
                let rect = Rect::new(
                    layout.content.x,
                    layout.content.y + u16::try_from(n).unwrap_or(0),
                    layout.content.width,
                    1,
                );
                put(buf, rect, text, base);
            }
        }
        if let Some(notice) = draft.notice() {
            put(buf, layout.status, notice, base.fg(theme.warning_orange));
        } else if let Some(hint) = draft.hint() {
            put(buf, layout.status, &hint, muted);
        }
        put(buf, layout.keys, KEYS, muted);
    }

    /// Where the real cursor goes in the draft, `None` when the text has no
    /// room.
    pub(super) fn summary_cursor(&self, draft: &SummaryDraft, area: Rect) -> Option<Position> {
        let rect = layout(area).content;
        if rect.is_empty() {
            return None;
        }
        let view = draft
            .content_composer()
            .plan(usize::from(rect.width))
            .view(usize::from(rect.height));
        let col = u16::try_from(view.cursor_col).ok()?;
        let row = u16::try_from(view.cursor_row).ok()?;
        (col < rect.width && row < rect.height).then(|| Position::new(rect.x + col, rect.y + row))
    }
}
