//! The note editor drawn over a chat pane: category, title and content, a
//! line for the hint or notice and a line of keys. Everything is already
//! sanitized text in styled cells.

use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::Style,
};

use super::{ChatView, fill};
use crate::core::ai::chat::note::{Field, NoteEditor};

/// Columns taken by the marker and the label before a field's text.
const LABEL_WIDTH: u16 = 12;
/// The keys, shown on the last row.
const KEYS: &str = "Ctrl+S save · Esc discard · Tab next field";
const SAVING: &str = "saving… · Esc close";

/// Where the parts of the editor sit in a pane.
struct NoteLayout {
    kind: Rect,
    title: Rect,
    label: Rect,
    content: Rect,
    status: Rect,
    keys: Rect,
}

fn layout(area: Rect) -> NoteLayout {
    let row = |y: u16| {
        Rect::new(
            area.x,
            area.y + y.min(area.height),
            area.width,
            u16::from(y < area.height),
        )
    };
    let label_w = LABEL_WIDTH.min(area.width);
    let text_row = |y: u16| {
        let base = row(y);
        Rect::new(base.x + label_w, base.y, base.width - label_w, base.height)
    };
    // The two last rows are the status and the keys; the content region is
    // what is left under the three field rows.
    let content_top = 3u16.min(area.height);
    let content_bottom = area.height.saturating_sub(2).max(content_top);
    let indent = 2u16.min(area.width);
    NoteLayout {
        kind: text_row(0),
        title: text_row(1),
        label: row(2),
        content: Rect::new(
            area.x + indent,
            area.y + content_top,
            area.width - indent,
            content_bottom - content_top,
        ),
        status: row(area.height.saturating_sub(2).max(3)),
        keys: row(area.height.saturating_sub(1).max(4)),
    }
}

impl ChatView<'_> {
    /// Draws the editor over `area`.
    pub(super) fn render_note(&self, editor: &NoteEditor, area: Rect, buf: &mut Buffer) {
        let theme = self.theme;
        let base = Style::default().bg(theme.bg_base);
        fill(buf, area, base);
        let layout = layout(area);
        let muted = base.fg(theme.text_muted);
        let label = |field: Field| {
            if editor.field() == field {
                base.fg(theme.primary_cyan)
            } else {
                muted
            }
        };
        let marker = |field: Field| {
            if editor.field() == field {
                "▸ "
            } else {
                "  "
            }
        };
        let put = |buf: &mut Buffer, rect: Rect, text: &str, style: Style| {
            if !rect.is_empty() {
                buf.set_stringn(rect.x, rect.y, text, usize::from(rect.width), style);
            }
        };

        for (field, name, y) in [
            (Field::Kind, "Category:", 0u16),
            (Field::Title, "Title:", 1),
        ] {
            let row = Rect::new(area.x, area.y + y, area.width, 1);
            if y < area.height {
                put(buf, row, &format!("{}{name}", marker(field)), label(field));
            }
        }
        put(
            buf,
            layout.label,
            &format!("{}Content:", marker(Field::Content)),
            label(Field::Content),
        );
        put(
            buf,
            layout.kind,
            &format!("< {} >", editor.kind().as_str()),
            base,
        );
        if !layout.title.is_empty() {
            let plan = editor
                .title_composer()
                .plan(usize::from(layout.title.width));
            if let Some(row) = plan.view(1).rows.first() {
                put(buf, layout.title, row, base);
            }
        }
        if !layout.content.is_empty() {
            let plan = editor
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
        if let Some(notice) = editor.notice() {
            put(buf, layout.status, notice, base.fg(theme.warning_orange));
        } else if let Some(hint) = editor.hint() {
            put(buf, layout.status, &hint, muted);
        }
        let keys = if editor.is_saving() { SAVING } else { KEYS };
        put(buf, layout.keys, keys, muted);
    }

    /// Where the real cursor goes in the editor, `None` on the kind or when
    /// the field has no room.
    pub(super) fn note_cursor(&self, editor: &NoteEditor, area: Rect) -> Option<Position> {
        let layout = layout(area);
        let (rect, composer) = match editor.field() {
            Field::Kind => return None,
            Field::Title => (layout.title, editor.title_composer()),
            Field::Content => (layout.content, editor.content_composer()),
        };
        if rect.is_empty() {
            return None;
        }
        let height = if editor.field() == Field::Title {
            1
        } else {
            usize::from(rect.height)
        };
        let view = composer.plan(usize::from(rect.width)).view(height);
        let col = u16::try_from(view.cursor_col).ok()?;
        let row = u16::try_from(view.cursor_row).ok()?;
        (col < rect.width && row < rect.height).then(|| Position::new(rect.x + col, rect.y + row))
    }
}
