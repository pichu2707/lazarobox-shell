//! Draws a chat pane: the transcript above, the composer below.
//!
//! Everything goes through styled cells of already sanitized text; model
//! output never reaches the terminal as raw bytes.

use std::time::Instant;

use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Modifier, Style},
    widgets::Widget,
};

use super::review;
use crate::{
    core::ai::{
        chat::{
            ChatState,
            approval::ApprovalPrompt,
            composer::ComposerPlan,
            guidance::guidance_rows,
            transcript::{Line, LineKind},
            wrap::wrap_words,
        },
        config::ApprovalMode,
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
    guidance: Option<&'a str>,
    approval: Option<&'a ApprovalPrompt>,
    now: Option<Instant>,
}

impl<'a> ChatView<'a> {
    pub fn new(chat: &'a ChatState, theme: &'a LazaroboxTheme) -> Self {
        Self {
            chat,
            theme,
            guidance: None,
            approval: None,
            now: None,
        }
    }

    /// The clock the review view counts its delay by.
    pub fn now(mut self, now: Instant) -> Self {
        self.now = Some(now);
        self
    }

    /// The pending run-in-pane confirm, drawn over the composer.
    pub fn approval(mut self, approval: Option<&'a ApprovalPrompt>) -> Self {
        self.approval = approval;
        self
    }

    /// Text shown above the composer while the engine cannot be used.
    pub fn guidance(mut self, guidance: Option<&'a str>) -> Self {
        self.guidance = guidance;
        self
    }

    /// The composer takes the rows its text needs, up to 5, and never the
    /// last row left to the transcript (unless the pane has one row only).
    pub fn layout(&self, area: Rect) -> ChatLayout {
        self.plan(area).0
    }

    /// The layout and the wrapped composer it came from, so that a frame
    /// wraps the composer text once for the height, the rows and the cursor.
    /// The composer is as wide as the area, so one plan serves all three.
    fn plan(&self, area: Rect) -> (ChatLayout, ComposerPlan<'_>) {
        let plan = self.chat.composer().plan(usize::from(area.width));
        let wanted = match self.approval {
            _ if area.is_empty() => 0,
            Some(prompt) => approval::wanted(prompt, usize::from(area.width)),
            None => plan.height(),
        };
        let rows = ChatState::clamp_composer_rows(wanted, area.height);
        let layout = ChatLayout {
            transcript: Rect::new(area.x, area.y, area.width, area.height - rows),
            composer: Rect::new(area.x, area.y + area.height - rows, area.width, rows),
        };
        (layout, plan)
    }

    /// Where the real cursor goes, or `None` when the composer cannot show it.
    pub fn cursor(&self, area: Rect) -> Option<Position> {
        if let Some(editor) = self.chat.tag_editor() {
            return self.tag_cursor(editor, area);
        }
        // The prompt owns the bottom rows: the composer is hidden under it.
        if self.approval.is_some() {
            return None;
        }
        if self.chat.list().is_some() {
            return None;
        }
        if let Some(editor) = self.chat.note_editor() {
            return self.note_cursor(editor, area);
        }
        if let Some(draft) = self.chat.summary_draft() {
            return self.summary_cursor(draft, area);
        }
        if self.chat.approvals().review().is_some() {
            return None;
        }
        let (layout, plan) = self.plan(area);
        let composer = layout.composer;
        if composer.is_empty() {
            return None;
        }
        let view = plan.view(usize::from(composer.height));
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
            LineKind::Tool => base.fg(theme.info_blue),
            LineKind::ToolAlert => base.fg(theme.warning_orange),
            LineKind::Hint => base.fg(theme.text_muted).add_modifier(Modifier::ITALIC),
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
        self.render_view(area, buf);
        // The review keeps its counter row to itself, whatever the mode.
        if let Some(mode) = self.chat.mode_badge()
            && !self.review_shown()
        {
            self.draw_mode_badge(mode, area, buf);
        }
    }
}

impl ChatView<'_> {
    /// ` AUTO ` (amber) or ` RUN ` (red) over the top-right cells of the
    /// pane, `A` or `R` when it is narrower than that: an exception marker,
    /// drawn in every view.
    fn draw_mode_badge(&self, mode: ApprovalMode, area: Rect, buf: &mut Buffer) {
        let (text, short, colour) = match mode {
            ApprovalMode::Run => (" RUN ", "R", self.theme.error_red),
            ApprovalMode::Auto => (" AUTO ", "A", self.theme.warning_orange),
            // `mode_badge` never yields `ask`.
            ApprovalMode::Ask => return,
        };
        let style = Style::default()
            .fg(self.theme.bg_base)
            .bg(colour)
            .add_modifier(Modifier::BOLD);
        let text = if usize::from(area.width) >= text.len() {
            text
        } else {
            short
        };
        let width = u16::try_from(text.len()).unwrap_or(1);
        buf.set_stringn(
            area.right() - width,
            area.y,
            text,
            usize::from(width),
            style,
        );
    }

    /// Whether the batch review is the view drawn: the tag editor, the list,
    /// the note editor and the summary draft come before it.
    pub fn review_shown(&self) -> bool {
        self.chat.tag_editor().is_none()
            && self.chat.list().is_none()
            && self.chat.note_editor().is_none()
            && self.chat.summary_draft().is_none()
            && self.chat.approvals().review().is_some()
    }

    /// The pane without the badge: the view that is open, or the
    /// conversation.
    fn render_view(&self, area: Rect, buf: &mut Buffer) {
        if let Some(editor) = self.chat.tag_editor() {
            self.render_tags(editor, area, buf);
            return;
        }
        if let Some(list) = self.chat.list() {
            self.render_list(list, area, buf);
            return;
        }
        if let Some(editor) = self.chat.note_editor() {
            self.render_note(editor, area, buf);
            return;
        }
        if let Some(draft) = self.chat.summary_draft() {
            self.render_summary(draft, area, buf);
            return;
        }
        if let Some(view) = self.chat.approvals().review() {
            let now = self.now.unwrap_or_else(Instant::now);
            review::render(view, now, self.theme, area, buf);
            return;
        }
        let (layout, plan) = self.plan(area);
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

        let mut transcript = layout.transcript;
        if let Some(text) = self.guidance
            && !transcript.is_empty()
        {
            // The guidance takes the rows just above the composer.
            let rows = wrap_words(text, usize::from(transcript.width));
            let taken = guidance_rows(text, transcript.width, transcript.height);
            transcript.height -= taken;
            let style = Style::default()
                .fg(self.theme.warning_orange)
                .bg(self.theme.bg_base);
            for (n, text) in rows.iter().take(usize::from(taken)).enumerate() {
                let y = transcript.y + transcript.height + u16::try_from(n).unwrap_or(0);
                buf.set_stringn(transcript.x, y, text, usize::from(transcript.width), style);
            }
        }
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
        if let Some(prompt) = self.approval {
            self.render_approval(prompt, composer, buf);
        } else if !composer.is_empty() {
            let view = plan.view(usize::from(composer.height));
            let style = Style::default().bg(self.theme.bg_panel);
            for (row, text) in view.rows.iter().enumerate() {
                let y = composer.y + u16::try_from(row).unwrap_or(0);
                buf.set_stringn(composer.x, y, text, usize::from(composer.width), style);
            }
        }
    }
}

/// Blanks `rect` with `style`.
pub(super) fn fill(buf: &mut Buffer, rect: Rect, style: Style) {
    for y in rect.y..rect.y + rect.height {
        for x in rect.x..rect.x + rect.width {
            buf[(x, y)].set_symbol(" ").set_style(style);
        }
    }
}

mod approval;
#[cfg(test)]
mod approval_tests;
#[cfg(test)]
mod badge_tests;
#[cfg(test)]
mod guidance_tests;
mod list;
#[cfg(test)]
mod list_tests;
#[cfg(test)]
mod memory_tests;
mod note;
#[cfg(test)]
mod note_tests;
mod summary;
#[cfg(test)]
mod summary_tests;
#[cfg(test)]
mod tag_tests;
mod tags;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tool_tests;
