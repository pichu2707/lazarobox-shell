//! Tokens by weekday and local hour. The shade glyphs carry the intensity, so
//! the grid reads with no colour.

use ratatui::{buffer::Buffer, style::Style};

use crate::core::ai::chat::dashboard::{
    Heat, buckets, heat_columns, heat_legend, heat_peak, shade,
};
use crate::ui::theme::LazaroboxTheme;

const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
/// Columns of the weekday label.
const LABEL: u16 = 4;

/// Draws the legend at `y`, the hour axis under it and a row a weekday.
pub(super) fn render(heat: &Heat, y: u16, width: u16, buf: &mut Buffer, theme: &LazaroboxTheme) {
    let base = Style::default().bg(theme.bg_base);
    let w = usize::from(width);
    let peak = heat_peak(heat);
    let empty = heat.iter().flatten().all(Option::is_none);
    buf.set_stringn(0, y, heat_legend(empty, w), w, base.fg(theme.text_muted));
    if empty {
        return;
    }
    let grouped = heat_columns(width) == 8;
    let rows: Vec<Vec<Option<u64>>> = if grouped {
        buckets(heat).iter().map(|r| r.to_vec()).collect()
    } else {
        heat.iter().map(|r| r.to_vec()).collect()
    };
    // Hours a column covers, and its width: a grid of 8 or 24 columns.
    let (hours, cell) = if grouped { (3, 3) } else { (1, 2) };
    // An hour label every 3 hours when grouped, every 6 otherwise.
    let every = if grouped { 1 } else { 6 };
    for n in (0..rows[0].len()).step_by(every) {
        let x = LABEL + u16::try_from(n).unwrap_or(0) * cell;
        let label = format!("{:02}", n * hours);
        buf.set_stringn(x, y + 1, label, w, base.fg(theme.text_muted));
    }
    for (d, (name, row)) in (0u16..).zip(DAYS.iter().zip(&rows)) {
        buf.set_stringn(0, y + 2 + d, name, w, base.fg(theme.text_muted));
        for (n, v) in (0u16..).zip(row) {
            let glyph = shade(*v, peak);
            if glyph == ' ' {
                continue;
            }
            let text: String = std::iter::repeat_n(glyph, usize::from(cell)).collect();
            let style = base.fg(theme.primary_cyan);
            buf.set_stringn(LABEL + n * cell, y + 2 + d, text, usize::from(cell), style);
        }
    }
}
