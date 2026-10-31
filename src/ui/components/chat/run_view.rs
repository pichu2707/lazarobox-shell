//! The full-pane transcript of a worker (agent-orchestrator S9): a title row,
//! the transcript rows (the end of them unless scrolled) and a key line. An
//! approval prompt keeps the rows it always takes at the bottom, and the keys
//! of it, so the view never hides a question.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
};

use super::{ChatView, fill};
use crate::core::ai::chat::transcript::LineKind;

const KEYS: &str = "Esc/q close \u{b7} \u{2191}\u{2193} PgUp/PgDn scroll \u{b7} End follow";
const SCROLLED: &str = "scrolled \u{b7} End follows \u{b7} Esc/q close";

impl ChatView<'_> {
    pub(super) fn render_run_view(&self, area: Rect, buf: &mut Buffer) {
        fill(buf, area, Style::default().bg(self.theme.bg_base));
        let (body, footer) = match self.approval {
            Some(prompt) => {
                let layout = self.plan(area).0;
                self.render_approval(prompt, layout.composer, buf);
                (layout.transcript, None)
            }
            None if area.height > 1 => (
                Rect::new(area.x, area.y, area.width, area.height - 1),
                Some(Rect::new(area.x, area.bottom() - 1, area.width, 1)),
            ),
            None => (area, None),
        };
        if body.is_empty() {
            return;
        }
        let head = self.chat.run_view_head().unwrap_or_default();
        let style = self.style(LineKind::Tool).add_modifier(Modifier::BOLD);
        buf.set_stringn(body.x, body.y, head, usize::from(body.width), style);
        let rows = Rect::new(body.x, body.y + 1, body.width, body.height - 1);
        let lines = self.chat.run_view_rows(usize::from(rows.width));
        let height = usize::from(rows.height);
        let top = self.chat.run_view_top(lines.len(), height);
        for (n, line) in lines.iter().skip(top).take(height).enumerate() {
            let y = rows.y + u16::try_from(n).unwrap_or(0);
            self.draw_line(buf, rows, y, line);
        }
        if let Some(footer) = footer {
            let scrolled = top + height < lines.len();
            let text = if scrolled { SCROLLED } else { KEYS };
            let style = self.style(LineKind::Hint);
            buf.set_stringn(footer.x, footer.y, text, usize::from(footer.width), style);
        }
    }
}
