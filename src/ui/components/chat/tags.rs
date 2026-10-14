//! The tag editor drawn over a chat pane: a title, the line of tags, a line
//! that counts the extras and a line of keys. Everything is already sanitized
//! text in styled cells.

use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Modifier, Style},
};

use super::{ChatView, fill};
use crate::core::ai::chat::{tag_editor::TagEditor, tags::EXTRA_MAX};

/// Columns taken by the marker before the line.
const MARKER_WIDTH: u16 = 2;
const TITLE: &str = "Edit tags";
const KEYS: &str = "↵ save · Esc discard";

/// What a row of the editor holds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Part {
    Title,
    Line,
    Count,
    Keys,
}

/// The parts that fit in `height` rows, top to bottom. The line is the last to
/// go.
fn parts(height: u16) -> &'static [Part] {
    match height {
        0 => &[],
        1 => &[Part::Line],
        2 => &[Part::Line, Part::Keys],
        3 => &[Part::Title, Part::Line, Part::Keys],
        _ => &[Part::Title, Part::Line, Part::Count, Part::Keys],
    }
}

/// The row of `part` in an area of `height` rows, and its place in the area.
fn row_of(height: u16, part: Part) -> Option<u16> {
    let shown = parts(height);
    let at = shown.iter().position(|p| *p == part)?;
    if part == Part::Keys {
        // The keys sit on the last row, whatever the height.
        return Some(height - 1);
    }
    u16::try_from(at).ok()
}

/// The room the line has: after the marker, on its row.
fn line_rect(area: Rect) -> Option<Rect> {
    let y = row_of(area.height, Part::Line)?;
    let marker = MARKER_WIDTH.min(area.width);
    let rect = Rect::new(area.x + marker, area.y + y, area.width - marker, 1);
    (!rect.is_empty()).then_some(rect)
}

impl ChatView<'_> {
    /// Draws the editor over `area`.
    pub(super) fn render_tags(&self, editor: &TagEditor, area: Rect, buf: &mut Buffer) {
        let theme = self.theme;
        let base = Style::default().bg(theme.bg_base);
        let muted = base.fg(theme.text_muted);
        fill(buf, area, base);
        let put = |buf: &mut Buffer, x: u16, y: u16, width: u16, text: &str, style: Style| {
            if width > 0 {
                buf.set_stringn(x, y, text, usize::from(width), style);
            }
        };
        if let Some(y) = row_of(area.height, Part::Title) {
            put(
                buf,
                area.x,
                area.y + y,
                area.width,
                TITLE,
                base.fg(theme.primary_cyan).add_modifier(Modifier::BOLD),
            );
        }
        if let Some(y) = row_of(area.height, Part::Line) {
            let marker = MARKER_WIDTH.min(area.width);
            put(
                buf,
                area.x,
                area.y + y,
                marker,
                "▸ ",
                base.fg(theme.primary_cyan),
            );
        }
        if let Some(rect) = line_rect(area) {
            let (shown, _) = editor.window(usize::from(rect.width));
            put(buf, rect.x, rect.y, rect.width, &shown, base);
        }
        if let Some(y) = row_of(area.height, Part::Count) {
            let count = format!(
                "project tag first · extras {}/{EXTRA_MAX}",
                editor.preview().extras.len()
            );
            put(buf, area.x, area.y + y, area.width, &count, muted);
        }
        if let Some(y) = row_of(area.height, Part::Keys) {
            put(buf, area.x, area.y + y, area.width, KEYS, muted);
        }
    }

    /// Where the real cursor goes in the editor, `None` when the line has no
    /// room.
    pub(super) fn tag_cursor(&self, editor: &TagEditor, area: Rect) -> Option<Position> {
        let rect = line_rect(area)?;
        let (_, col) = editor.window(usize::from(rect.width));
        let col = u16::try_from(col).ok()?;
        (col < rect.width).then(|| Position::new(rect.x + col, rect.y))
    }
}
