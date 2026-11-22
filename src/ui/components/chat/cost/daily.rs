//! Tokens by day: input grows up from the axis in eighth blocks, output grows
//! down in half blocks. Shape and the legend carry the meaning, so the chart
//! reads with no colour.

use ratatui::{buffer::Buffer, style::Style};

use crate::core::ai::chat::dashboard::{
    DOWN_ROWS, DayTokens, UP_ROWS, fit, legend, level, notes, peak,
};
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

/// Draws the legend at `y`, the chart under it, `UP_ROWS + DOWN_ROWS` rows,
/// and a row of notes (`last` is the days of a truncated series).
pub(super) fn render(
    days: &[DayTokens],
    last: Option<u32>,
    y: u16,
    width: u16,
    buf: &mut Buffer,
    theme: &LazaroboxTheme,
) {
    let base = Style::default().bg(theme.bg_base);
    let w = usize::from(width);
    let fitted = fit(days, w);
    buf.set_stringn(0, y, legend(fitted.size, w), w, base.fg(theme.text_muted));
    if let Some(note) = notes(&fitted, last) {
        let row = y + 1 + UP_ROWS + DOWN_ROWS;
        buf.set_stringn(0, row, note, w, base.fg(theme.text_muted));
    }
    let max = peak(&fitted.days);
    // Each bucket takes a slot of up to 8 columns: a bar and a gap.
    let slot = slot(w, fitted.days.len());
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

/// Columns of one bucket, bar and gap.
fn slot(width: usize, buckets: usize) -> usize {
    (width / buckets.max(1)).clamp(1, 8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_range_uses_the_width_up_to_eight_columns_a_bar() {
        assert_eq!(slot(100, 7), 8);
        assert_eq!(slot(100, 30), 3);
        assert_eq!(slot(100, 50), 2);
        assert_eq!(slot(100, 100), 1);
        assert_eq!(slot(40, 90), 1);
        assert_eq!(slot(100, 0), 8);
        assert_eq!(slot(100, 12), 8);
        assert_eq!(slot(100, 13), 7);
    }

    #[test]
    fn the_output_bar_is_halves_down_from_the_axis() {
        // 5 half steps: two full rows, then a half block, then nothing.
        let rows: Vec<char> = (0..3).map(|r| down_cell(5, r)).collect();
        assert_eq!(rows, ['█', '█', '▀']);
        let rows: Vec<char> = (0..3).map(|r| down_cell(4, r)).collect();
        assert_eq!(rows, ['█', '█', ' ']);
        assert_eq!(down_cell(3, 1), '▀');
        assert_eq!(down_cell(6, 2), '█');
        assert_eq!(down_cell(0, 0), ' ');
    }

    #[test]
    fn the_input_bar_is_eighths_up_from_the_axis() {
        let rows: Vec<char> = (0..3).map(|r| up_cell(11, r)).collect();
        assert_eq!(rows, ['█', '▃', ' ']);
        assert_eq!(up_cell(24, 2), '█');
    }
}
