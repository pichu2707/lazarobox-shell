//! Tokens by day: input grows up from the axis in eighth blocks, output grows
//! down in half blocks. Shape and the legend carry the meaning, so the chart
//! reads with no colour.

use ratatui::{buffer::Buffer, style::Style};

use crate::core::ai::chat::dashboard::{DOWN_ROWS, DayTokens, UP_ROWS, fit, legend, level, peak};
use crate::ui::theme::LazaroboxTheme;

/// Eighth blocks, one step to a full cell.
const EIGHTHS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
/// A day with a record and no tokens.
const ZERO: char = '·';

/// The cell of the input bar `row` rows above the axis (0 is the first).
fn up_cell(steps: u64, row: u64) -> char {
    match steps.saturating_sub(row * 8) {
        0 => ' ',
        n => EIGHTHS[usize::try_from(n.min(8) - 1).unwrap_or(7)],
    }
}

/// The cell of the output bar `row` rows below the axis.
fn down_cell(steps: u64, row: u64) -> char {
    match steps.saturating_sub(row * 2) {
        0 => ' ',
        1 => '▀',
        _ => '█',
    }
}

/// Draws the legend at `y` and the chart under it, `UP_ROWS + DOWN_ROWS`
/// rows, in `width` columns.
pub(super) fn render(
    days: &[DayTokens],
    y: u16,
    width: u16,
    buf: &mut Buffer,
    theme: &LazaroboxTheme,
) {
    let base = Style::default().bg(theme.bg_base);
    let w = usize::from(width);
    let fitted = fit(days, w);
    buf.set_stringn(0, y, legend(fitted.size, w), w, base.fg(theme.text_muted));
    let max = peak(&fitted.days);
    // Each bucket takes a slot of up to 3 columns: a bar and a gap.
    let slot = (w / fitted.days.len().max(1)).clamp(1, 3);
    let bar = slot.saturating_sub(1).max(1);
    let axis = y + 1 + UP_ROWS;
    for (n, day) in fitted.days.iter().enumerate() {
        let x = n * slot;
        let up = day
            .input
            .map_or(0, |v| level(v, max, u64::from(UP_ROWS) * 8));
        let down = day
            .output
            .map_or(0, |v| level(v, max, u64::from(DOWN_ROWS) * 2));
        let known = day.input.is_some() || day.output.is_some();
        for k in 0..u64::from(UP_ROWS) {
            let mut cell = up_cell(up, k);
            if cell == ' ' && k == 0 && known && up == 0 && down == 0 {
                cell = ZERO;
            }
            put(
                buf,
                x,
                axis - 1 - u16::try_from(k).unwrap_or(0),
                cell,
                bar,
                base.fg(theme.primary_cyan),
            );
        }
        for k in 0..u64::from(DOWN_ROWS) {
            let cell = down_cell(down, k);
            put(
                buf,
                x,
                axis + u16::try_from(k).unwrap_or(0),
                cell,
                bar,
                base.fg(theme.warning_orange),
            );
        }
    }
}

fn put(buf: &mut Buffer, x: usize, y: u16, cell: char, bar: usize, style: Style) {
    if cell == ' ' {
        return;
    }
    let text: String = std::iter::repeat_n(cell, bar).collect();
    let x = u16::try_from(x).unwrap_or(u16::MAX);
    buf.set_stringn(x, y, text, bar, style);
}
