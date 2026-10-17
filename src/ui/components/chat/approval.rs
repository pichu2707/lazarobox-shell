//! The approval prompt drawn over the composer of a chat pane: where the
//! action goes, the exact action, and the keys. Everything is already
//! sanitized text; a tab is drawn as an arrow so nothing is drawn raw. The
//! rows come from `core::ai::chat::approval_rows`.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
};

use super::{ChatView, fill};
use crate::core::ai::chat::approval::ApprovalPrompt;
pub(super) use crate::core::ai::chat::approval_rows::{Part, parts, wanted};

impl ChatView<'_> {
    /// Draws the prompt in `area`, over the composer.
    pub(super) fn render_approval(&self, prompt: &ApprovalPrompt, area: Rect, buf: &mut Buffer) {
        fill(buf, area, Style::default().bg(self.theme.bg_panel));
        let base = Style::default().bg(self.theme.bg_panel);
        for (row, part) in parts(prompt, usize::from(area.width), usize::from(area.height))
            .iter()
            .enumerate()
        {
            let y = area.y + u16::try_from(row).unwrap_or(0);
            let (text, style) = match part {
                Part::Head(t) => (
                    t,
                    base.fg(self.theme.warning_orange)
                        .add_modifier(Modifier::BOLD),
                ),
                Part::Command(t) => (t, base.fg(self.theme.success_green)),
                Part::Marker(t) | Part::Keys(t) => (t, base.fg(self.theme.text_muted)),
            };
            buf.set_stringn(area.x, y, text, usize::from(area.width), style);
        }
    }
}
