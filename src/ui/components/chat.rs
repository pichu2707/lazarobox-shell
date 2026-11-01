//! Draws a chat pane: the transcript above, the composer below.
//!
//! Everything goes through styled cells of already sanitized text; model
//! output never reaches the terminal as raw bytes.

use std::time::Instant;

use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    widgets::Widget,
};

use super::review;
use crate::{
    core::ai::{
        chat::{
            BATCH_BADGE_ROOM, ChatState,
            approval::ApprovalPrompt,
            composer::ComposerPlan,
            copy::ChatMarks,
            guidance::guidance_rows,
            review::Badge,
            transcript::{Line, LineKind},
            wrap::{text_width, wrap_words},
        },
        config::ApprovalMode,
    },
    ui::theme::LazaroboxTheme,
};

/// Columns the batch badge also leaves free for the ORCH badge.
const ORCH_BADGE_ROOM: usize = " ORCH ".len();

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
    copy: Option<ChatMarks>,
    orchestrating: bool,
    quiet_guidance: bool,
}

impl<'a> ChatView<'a> {
    pub fn new(chat: &'a ChatState, theme: &'a LazaroboxTheme) -> Self {
        Self {
            chat,
            theme,
            guidance: None,
            quiet_guidance: false,
            approval: None,
            now: None,
            copy: None,
            orchestrating: false,
        }
    }

    /// Whether the pane orchestrates (the global gate and its ORCH mode both
    /// on): it then wears the ORCH badge beside the mode badge.
    pub fn orchestrating(mut self, on: bool) -> Self {
        self.orchestrating = on;
        self
    }

    /// The chat COPY cursor and selection: the selection is drawn reversed
    /// and the cursor goes on the transcript instead of the composer.
    pub fn copy(mut self, marks: Option<ChatMarks>) -> Self {
        self.copy = marks;
        self
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

    /// Draws the guidance as a quiet hint (muted) instead of a warning.
    pub fn quiet_guidance(mut self, quiet: bool) -> Self {
        self.quiet_guidance = quiet;
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
        // The batch badge owns the top row of the conversation, so it never
        // hides a transcript line; a pane that cannot spare it draws none.
        let top = u16::from(self.chat.badge_row(area.width, area.height - rows));
        let layout = ChatLayout {
            transcript: Rect::new(area.x, area.y + top, area.width, area.height - rows - top),
            composer: Rect::new(area.x, area.y + area.height - rows, area.width, rows),
        };
        (layout, plan)
    }

    /// Where the real cursor goes, or `None` when the composer cannot show it.
    pub fn cursor(&self, area: Rect) -> Option<Position> {
        if let Some(picker) = self.chat.folder_picker() {
            return self.folder_cursor(picker, area);
        }
        if let Some(editor) = self.chat.tag_editor() {
            return self.tag_cursor(editor, area);
        }
        // The prompt owns the bottom rows: the composer is hidden under it.
        if self.approval.is_some() || self.chat.run_view().is_some() {
            return None;
        }
        if self.chat.list().is_some() || self.chat.cost().is_some() {
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
        if let Some(marks) = self.copy {
            return self.copy_cursor(marks, area);
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
            LineKind::Mark | LineKind::Reasoning | LineKind::Usage => base.fg(theme.text_muted),
            LineKind::ReasoningHead => base.fg(theme.ai_purple),
            LineKind::Tool => base.fg(theme.info_blue),
            LineKind::ToolAlert => base.fg(theme.warning_orange),
            LineKind::Hint => base.fg(theme.text_muted).add_modifier(Modifier::ITALIC),
        }
    }

    /// The chat COPY cursor on the screen, `None` when its line is not drawn.
    fn copy_cursor(&self, marks: ChatMarks, area: Rect) -> Option<Position> {
        let transcript = self.transcript_rows(self.plan(area).0.transcript);
        let lines = self.chat.lines(usize::from(transcript.width));
        let height = usize::from(transcript.height);
        let top = self.chat.scroll().top(lines.len(), height);
        let row = u16::try_from(marks.cursor.line.checked_sub(top)?).ok()?;
        (row < transcript.height && marks.cursor.col < transcript.width)
            .then(|| Position::new(transcript.x + marks.cursor.col, transcript.y + row))
    }

    /// The rows of `transcript` the lines are drawn in: the guidance takes
    /// the rows just above the composer.
    fn transcript_rows(&self, mut transcript: Rect) -> Rect {
        if let Some(text) = self.guidance
            && !transcript.is_empty()
        {
            transcript.height -= guidance_rows(text, transcript.width, transcript.height);
        }
        transcript
    }

    /// Reverses the selected cells of transcript `line`, drawn at row `y`. A
    /// wide character is selected from either of its cells.
    fn draw_selection(&self, buf: &mut Buffer, rect: Rect, y: u16, line: usize) {
        let Some(span) = self.copy.and_then(|marks| marks.span) else {
            return;
        };
        for col in 0..rect.width {
            let cell = &mut buf[(rect.x + col, y)];
            let wide = text_width(cell.symbol()) > 1;
            if span.contains(line, col) || (wide && span.contains(line, col + 1)) {
                cell.modifier.insert(Modifier::REVERSED);
            }
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
        let mut right = area.right();
        if let Some(mode) = self.chat.mode_badge()
            && !self.review_shown()
        {
            right = self.draw_mode_badge(mode, area, buf);
        }
        if self.orchestrating && !self.review_shown() {
            self.draw_orch_badge(area, right, buf);
        }
        // The conversation draws it only in the row it reserved.
        let reserved = self.plan(area).0.transcript.y > area.y;
        if let Some(badge) = self.chat.batch_badge()
            && !self.review_shown()
            && (reserved || !self.chat.conversation_shown())
        {
            self.draw_batch_badge(badge, area, buf);
        }
    }
}

impl ChatView<'_> {
    /// ` AUTO ` (amber) or ` RUN ` (red) over the top-right cells of the
    /// pane, `A` or `R` when it is narrower than that: an exception marker,
    /// drawn in every view. Returns the column it starts at.
    fn draw_mode_badge(&self, mode: ApprovalMode, area: Rect, buf: &mut Buffer) -> u16 {
        let (text, short, colour) = match mode {
            ApprovalMode::Run => (" RUN ", "R", self.theme.error_red),
            ApprovalMode::Auto => (" AUTO ", "A", self.theme.warning_orange),
            // `mode_badge` never yields `ask`.
            ApprovalMode::Ask => return area.right(),
        };
        self.draw_badge(text, short, colour, area, buf)
            .unwrap_or(area.right())
    }

    /// ` ORCH ` (purple) just left of the mode badge, whose cells start at
    /// `right`, or `O` when only one column is left: drawn in every view of
    /// an orchestrating pane, as the mode badge.
    fn draw_orch_badge(&self, area: Rect, right: u16, buf: &mut Buffer) {
        let room = Rect::new(area.x, area.y, right - area.x, area.height);
        self.draw_badge(" ORCH ", "O", self.theme.ai_purple, room, buf);
    }

    /// A badge, `text` or else `short`, over the top-right cells of `area`;
    /// `None` when not even `short` fits. Returns the column it starts at.
    fn draw_badge(
        &self,
        text: &str,
        short: &str,
        colour: Color,
        area: Rect,
        buf: &mut Buffer,
    ) -> Option<u16> {
        let style = Style::default()
            .fg(self.theme.bg_base)
            .bg(colour)
            .add_modifier(Modifier::BOLD);
        let text = [text, short]
            .into_iter()
            .find(|text| usize::from(area.width) >= text.len())?;
        let width = u16::try_from(text.len()).ok()?;
        let x = area.right() - width;
        buf.set_stringn(x, area.y, text, usize::from(width), style);
        Some(x)
    }

    /// The staged batch of the chat on the top-left cells of the pane: the
    /// conversation reserves that row for it, the other views are drawn over.
    /// The mode and scroll badges own the right side, so it leaves their room
    /// and gets shorter before it would meet them.
    fn draw_batch_badge(&self, badge: Badge, area: Rect, buf: &mut Buffer) {
        let orch = if self.orchestrating {
            ORCH_BADGE_ROOM
        } else {
            0
        };
        let room = usize::from(area.width).saturating_sub(BATCH_BADGE_ROOM + orch);
        let Some(text) = badge.text(room) else {
            return;
        };
        let colour = match badge {
            Badge::Staged(_) => self.theme.info_blue,
            Badge::Orphan(_) => self.theme.warning_orange,
        };
        let style = Style::default()
            .fg(self.theme.bg_base)
            .bg(colour)
            .add_modifier(Modifier::BOLD);
        buf.set_stringn(area.x, area.y, text, room, style);
    }

    /// Whether the batch review is the view drawn: the tag editor, the list,
    /// the note editor and the summary draft come before it.
    pub fn review_shown(&self) -> bool {
        self.chat.tag_editor().is_none()
            && self.chat.folder_picker().is_none()
            && self.chat.list().is_none()
            && self.chat.cost().is_none()
            && self.chat.note_editor().is_none()
            && self.chat.summary_draft().is_none()
            && self.chat.approvals().review().is_some()
    }

    /// The pane without the badge: the view that is open, or the
    /// conversation.
    fn render_view(&self, area: Rect, buf: &mut Buffer) {
        if let Some(picker) = self.chat.folder_picker() {
            self.render_folder(picker, area, buf);
            return;
        }
        if let Some(editor) = self.chat.tag_editor() {
            self.render_tags(editor, area, buf);
            return;
        }
        if let Some(list) = self.chat.list() {
            self.render_list(list, area, buf);
            return;
        }
        if let Some(view) = self.chat.cost() {
            self.render_cost(view, area, buf);
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
        if self.chat.run_view().is_some() {
            self.render_run_view(area, buf);
            return;
        }
        let (layout, plan) = self.plan(area);
        // The row the badge reserved is drawn over by `render`.
        if layout.transcript.y > area.y {
            let row = Rect::new(area.x, area.y, area.width, layout.transcript.y - area.y);
            fill(buf, row, Style::default().bg(self.theme.bg_base));
        }
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

        let transcript = self.transcript_rows(layout.transcript);
        if let Some(text) = self.guidance {
            // The guidance takes the rows just above the composer.
            let rows = wrap_words(text, usize::from(transcript.width));
            let taken = layout.transcript.height - transcript.height;
            let tone = if self.quiet_guidance {
                self.theme.text_muted
            } else {
                self.theme.warning_orange
            };
            let style = Style::default().fg(tone).bg(self.theme.bg_base);
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
                self.draw_selection(buf, transcript, y, top + row);
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
mod cost;
#[cfg(test)]
mod cost_tests;
mod folder;
#[cfg(test)]
mod folder_tests;
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
#[cfg(test)]
mod orch_tests;
mod run_view;
#[cfg(test)]
mod run_view_tests;
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
