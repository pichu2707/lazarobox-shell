//! The `/cost` view drawn over a chat pane: the header, the lines of the
//! report and a last row with the key that closes it. The lines are already
//! plain text; long ones are clipped like the conversation list's rows.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
};

use super::{ChatView, fill};
use crate::core::ai::chat::cost_view::CostView;

/// The keys line.
const HINT: &str = "Esc close · ↑↓ scroll";

impl ChatView<'_> {
    /// Draws the cost view over `area`.
    pub(super) fn render_cost(&self, view: &CostView, area: Rect, buf: &mut Buffer) {
        let theme = self.theme;
        let base = Style::default().bg(theme.bg_base);
        fill(buf, area, base);
        let put = |buf: &mut Buffer, y: u16, text: &str, style: Style| {
            if y < area.height {
                buf.set_stringn(area.x, area.y + y, text, usize::from(area.width), style);
            }
        };
        if let Some(title) = view.lines().first() {
            put(
                buf,
                0,
                title,
                base.fg(theme.primary_cyan).add_modifier(Modifier::BOLD),
            );
        }
        // Row 0 is the header and the last row the keys; the rows between
        // them are the window on the wrapped body.
        let window = usize::from(area.height.saturating_sub(2));
        let rows = view.body_rows(usize::from(area.width));
        view.set_layout(window, rows.len());
        for (n, (row, mine)) in rows.iter().skip(view.scroll()).take(window).enumerate() {
            let y = 1 + u16::try_from(n).unwrap_or(0);
            // This chat stands out from the totals of all of them.
            let style = if *mine {
                base.fg(theme.primary_cyan)
            } else {
                base
            };
            put(buf, y, row, style);
        }
        let (keys, style) = match view.note() {
            Some(note) => (note, base.fg(theme.warning_orange)),
            None => (HINT, base.fg(theme.text_muted)),
        };
        put(buf, area.height.saturating_sub(1).max(1), keys, style);
    }
}
