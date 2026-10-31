//! The folder picker drawn over a chat pane: a title, one row per candidate,
//! the reason of the last refusal, the typed path and a line of keys.
//! Everything is already sanitized text in styled cells.

use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Modifier, Style},
};

use super::{ChatView, fill};
use crate::core::ai::{
    chat::folder_picker::{FolderPicker, Source},
    sanitize::clean,
};

/// Columns taken by the selection marker, and by the field's prompt.
const MARKER_WIDTH: u16 = 2;
const TITLE: &str = "Choose the chat folder";
const KEYS: &str = "↵ open · Tab complete · Esc keep this folder";
const GATHERING: &str = "looking for folders…";
const NONE: &str = "no suggestions: type a path";

/// Where the parts of the picker sit in an area of `height` rows.
struct Plan {
    title: bool,
    /// The rows of the candidates: first row and how many.
    rows: (u16, u16),
    error: Option<u16>,
    field: Option<u16>,
    keys: Option<u16>,
}

fn plan(height: u16, error: bool) -> Plan {
    let title = height >= 4;
    let keys = (height >= 2).then(|| height - 1);
    let field = (height >= 1).then(|| height.saturating_sub(2));
    // The refusal sits just above the field, when there is room for it.
    let error = (error && height >= 4).then(|| height - 3);
    let top = u16::from(title);
    let bottom = error.or(field).unwrap_or(height);
    let rows = (top, bottom.saturating_sub(top));
    Plan {
        title,
        rows,
        error,
        field,
        keys,
    }
}

fn label(source: Source) -> &'static str {
    match source {
        Source::LastUsed => "last used",
        Source::PaneRoot => "pane root",
        Source::Recent => "recent",
        Source::LastShell => "last shell",
    }
}

/// The room the typed path has: after the prompt, on its row.
fn field_rect(area: Rect, row: u16) -> Option<Rect> {
    let prompt = MARKER_WIDTH.min(area.width);
    let rect = Rect::new(area.x + prompt, area.y + row, area.width - prompt, 1);
    (!rect.is_empty()).then_some(rect)
}

impl ChatView<'_> {
    /// Draws the picker over `area`.
    pub(super) fn render_folder(&self, picker: &FolderPicker, area: Rect, buf: &mut Buffer) {
        let theme = self.theme;
        let base = Style::default().bg(theme.bg_base);
        let muted = base.fg(theme.text_muted);
        fill(buf, area, base);
        let put = |buf: &mut Buffer, x: u16, y: u16, width: u16, text: &str, style: Style| {
            if width > 0 {
                buf.set_stringn(x, area.y + y, text, usize::from(width), style);
            }
        };
        let at = |buf: &mut Buffer, y: u16, text: &str, style: Style| {
            put(buf, area.x, y, area.width, text, style);
        };
        let plan = plan(area.height, picker.error().is_some());
        if plan.title {
            let style = base.fg(theme.primary_cyan).add_modifier(Modifier::BOLD);
            at(buf, 0, TITLE, style);
        }
        let (first, room) = plan.rows;
        if picker.rows().is_empty() {
            let hint = if picker.gathering() { GATHERING } else { NONE };
            if room > 0 {
                at(buf, first, hint, muted);
            }
        } else {
            let room = usize::from(room);
            let top = (picker.selected() + 1).saturating_sub(room);
            let text_room = area.width.saturating_sub(MARKER_WIDTH);
            for (n, row) in picker.rows().iter().skip(top).take(room).enumerate() {
                let y = first + u16::try_from(n).unwrap_or(0);
                let selected = top + n == picker.selected();
                let (marker, style) = if selected {
                    ("▸ ", base.fg(theme.primary_cyan))
                } else {
                    ("  ", base)
                };
                // The source comes first: a long path is cut at its end.
                let line = format!(
                    "{} · {}",
                    label(row.source),
                    clean(&row.path.to_string_lossy())
                );
                at(buf, y, marker, style);
                put(
                    buf,
                    area.x + MARKER_WIDTH.min(area.width),
                    y,
                    text_room,
                    &line,
                    style,
                );
            }
        }
        if let (Some(y), Some(reason)) = (plan.error, picker.error()) {
            at(buf, y, reason, base.fg(theme.warning_orange));
        }
        if let Some(y) = plan.field {
            at(buf, y, "› ", base.fg(theme.primary_cyan));
            if let Some(rect) = field_rect(area, y) {
                let (shown, _) = picker.field().window(usize::from(rect.width));
                put(buf, rect.x, y, rect.width, &clean(&shown), base);
            }
        }
        if let Some(y) = plan.keys {
            at(buf, y, KEYS, muted);
        }
    }

    /// Where the real cursor goes in the typed path, `None` when it has no
    /// room.
    pub(super) fn folder_cursor(&self, picker: &FolderPicker, area: Rect) -> Option<Position> {
        let row = plan(area.height, picker.error().is_some()).field?;
        let rect = field_rect(area, row)?;
        let (_, col) = picker.field().window(usize::from(rect.width));
        let col = u16::try_from(col).ok()?;
        (col < rect.width).then(|| Position::new(rect.x + col, rect.y))
    }
}
