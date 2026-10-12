//! Draws a chat pane: the transcript above, the composer below.
//!
//! Everything goes through styled cells of already sanitized text; model
//! output never reaches the terminal as raw bytes.

use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Modifier, Style},
    widgets::Widget,
};

use crate::{
    core::ai::chat::{
        ChatState,
        transcript::{Line, LineKind},
    },
    ui::theme::LazaroboxTheme,
};

/// The two areas of a chat pane.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChatLayout {
    pub transcript: Rect,
    pub composer: Rect,
}

/// A chat pane as a widget.
pub struct ChatView<'a> {
    chat: &'a ChatState,
    theme: &'a LazaroboxTheme,
}

impl<'a> ChatView<'a> {
    pub fn new(chat: &'a ChatState, theme: &'a LazaroboxTheme) -> Self {
        Self { chat, theme }
    }

    /// The composer takes the rows its text needs, up to 5, and never the
    /// last row left to the transcript (unless the pane has one row only).
    pub fn layout(&self, area: Rect) -> ChatLayout {
        let wanted = if area.is_empty() {
            0
        } else {
            self.chat.composer().height(usize::from(area.width))
        };
        let wanted = u16::try_from(wanted).unwrap_or(u16::MAX);
        let rows = wanted.min(area.height.saturating_sub(1).max(1).min(area.height));
        ChatLayout {
            transcript: Rect::new(area.x, area.y, area.width, area.height - rows),
            composer: Rect::new(area.x, area.y + area.height - rows, area.width, rows),
        }
    }

    /// Where the real cursor goes, or `None` when the composer cannot show it.
    pub fn cursor(&self, area: Rect) -> Option<Position> {
        let composer = self.layout(area).composer;
        if composer.is_empty() {
            return None;
        }
        let view = self
            .chat
            .composer()
            .view(usize::from(composer.width), usize::from(composer.height));
        let col = u16::try_from(view.cursor_col).ok()?;
        let row = u16::try_from(view.cursor_row).ok()?;
        (col < composer.width && row < composer.height)
            .then(|| Position::new(composer.x + col, composer.y + row))
    }

    fn style(&self, kind: LineKind) -> Style {
        let theme = self.theme;
        let base = Style::default().bg(theme.bg_base);
        match kind {
            LineKind::User => base.fg(theme.primary_cyan),
            LineKind::Assistant | LineKind::Blank => base,
            LineKind::Code => Style::default().fg(theme.success_green).bg(theme.bg_panel),
            LineKind::System => base.fg(theme.warning_orange),
            LineKind::Waiting => base.fg(theme.text_muted).add_modifier(Modifier::ITALIC),
            LineKind::Mark | LineKind::Reasoning => base.fg(theme.text_muted),
            LineKind::ReasoningHead => base.fg(theme.ai_purple),
        }
    }

    fn draw_line(&self, buf: &mut Buffer, rect: Rect, y: u16, line: &Line) {
        let style = self.style(line.kind);
        if line.kind == LineKind::Code {
            // The whole row is the code block, not just its text.
            for x in rect.x..rect.x + rect.width {
                buf[(x, y)].set_style(style);
            }
        }
        buf.set_stringn(rect.x, y, &line.text, usize::from(rect.width), style);
    }
}

impl Widget for ChatView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let area = area.intersection(buf.area);
        if area.is_empty() {
            return;
        }
        let layout = self.layout(area);
        fill(
            buf,
            layout.transcript,
            Style::default().bg(self.theme.bg_base),
        );
        fill(
            buf,
            layout.composer,
            Style::default().bg(self.theme.bg_panel),
        );

        let transcript = layout.transcript;
        if !transcript.is_empty() {
            let lines = self.chat.lines(usize::from(transcript.width));
            let height = usize::from(transcript.height);
            let top = self.chat.scroll().top(lines.len(), height);
            for (row, line) in lines.iter().skip(top).take(height).enumerate() {
                let y = transcript.y + u16::try_from(row).unwrap_or(0);
                self.draw_line(buf, transcript, y, line);
            }
        }

        let composer = layout.composer;
        if !composer.is_empty() {
            let view = self
                .chat
                .composer()
                .view(usize::from(composer.width), usize::from(composer.height));
            let style = Style::default().bg(self.theme.bg_panel);
            for (row, text) in view.rows.iter().enumerate() {
                let y = composer.y + u16::try_from(row).unwrap_or(0);
                buf.set_stringn(composer.x, y, text, usize::from(composer.width), style);
            }
        }
    }
}

/// Blanks `rect` with `style`.
fn fill(buf: &mut Buffer, rect: Rect, style: Style) {
    for y in rect.y..rect.y + rect.height {
        for x in rect.x..rect.x + rect.width {
            buf[(x, y)].set_symbol(" ").set_style(style);
        }
    }
}

#[cfg(test)]
mod tests;
