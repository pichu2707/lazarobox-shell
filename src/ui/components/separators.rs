//! Draws the 1-cell lines between tiled panes.

use ratatui::{buffer::Buffer, layout::Rect, style::Color, widgets::Widget};

use crate::core::layout::{Axis, Separator};

/// The separators of a tab. Cells next to the focused pane use `accent`, the
/// rest use `muted`. Cells outside the area are clipped.
pub struct SeparatorView<'a> {
    separators: &'a [Separator],
    focused: crate::core::layout::Rect,
    accent: Color,
    muted: Color,
    background: Color,
}

impl<'a> SeparatorView<'a> {
    pub fn new(
        separators: &'a [Separator],
        focused: crate::core::layout::Rect,
        accent: Color,
        muted: Color,
        background: Color,
    ) -> Self {
        Self {
            separators,
            focused,
            accent,
            muted,
            background,
        }
    }
}

impl Widget for SeparatorView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for line in self.separators {
            let (glyph, step) = match line.axis {
                Axis::X => ("\u{2502}", (0, 1)),
                Axis::Y => ("\u{2500}", (1, 0)),
            };
            let run = line.highlight(self.focused);
            for offset in 0..line.len {
                let (x, y) = (line.x + step.0 * offset, line.y + step.1 * offset);
                if !area.contains((x, y).into()) {
                    continue;
                }
                let touches = run.is_some_and(|(from, len)| (from..from + len).contains(&offset));
                let fg = if touches { self.accent } else { self.muted };
                buf[(x, y)]
                    .set_symbol(glyph)
                    .set_fg(fg)
                    .set_bg(self.background);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::layout::Rect as CellRect;

    const ACCENT: Color = Color::Rgb(1, 2, 3);
    const MUTED: Color = Color::Rgb(9, 9, 9);
    const BG: Color = Color::Rgb(0, 0, 0);

    fn rect(x: u16, y: u16, width: u16, height: u16) -> CellRect {
        CellRect {
            x,
            y,
            width,
            height,
        }
    }

    fn vertical(x: u16, len: u16) -> Separator {
        Separator {
            axis: Axis::X,
            x,
            y: 0,
            len,
        }
    }

    fn draw(area: Rect, separators: &[Separator], focused: CellRect) -> Buffer {
        let mut buf = Buffer::empty(area);
        SeparatorView::new(separators, focused, ACCENT, MUTED, BG).render(area, &mut buf);
        buf
    }

    fn fg(buf: &Buffer, x: u16, y: u16) -> Color {
        buf[(x, y)].style().fg.unwrap_or(Color::Reset)
    }

    #[test]
    fn a_vertical_line_uses_the_vertical_glyph() {
        let buf = draw(Rect::new(0, 0, 5, 3), &[vertical(2, 3)], rect(0, 0, 2, 3));
        assert_eq!(buf[(2, 0)].symbol(), "│");
        assert_eq!(buf[(2, 2)].symbol(), "│");
        assert_eq!(buf[(1, 0)].symbol(), " ");
    }

    #[test]
    fn a_horizontal_line_uses_the_horizontal_glyph() {
        let line = Separator {
            axis: Axis::Y,
            x: 0,
            y: 2,
            len: 4,
        };
        let buf = draw(Rect::new(0, 0, 4, 5), &[line], rect(0, 0, 4, 2));
        assert_eq!(buf[(0, 2)].symbol(), "─");
        assert_eq!(buf[(3, 2)].symbol(), "─");
        assert_eq!(buf[(0, 1)].symbol(), " ");
    }

    #[test]
    fn the_separator_next_to_the_focused_pane_gets_the_accent() {
        let buf = draw(Rect::new(0, 0, 5, 3), &[vertical(2, 3)], rect(0, 0, 2, 3));
        assert_eq!(fg(&buf, 2, 1), ACCENT);
        assert_eq!(buf[(2, 1)].style().bg, Some(BG));
    }

    #[test]
    fn separators_away_from_the_focus_are_muted_and_focus_change_recolors() {
        // Three panes in a row: | at x=2 and x=5.
        let lines = [vertical(2, 3), vertical(5, 3)];
        let area = Rect::new(0, 0, 8, 3);
        let left = draw(area, &lines, rect(0, 0, 2, 3));
        assert_eq!((fg(&left, 2, 0), fg(&left, 5, 0)), (ACCENT, MUTED));
        let right = draw(area, &lines, rect(6, 0, 2, 3));
        assert_eq!((fg(&right, 2, 0), fg(&right, 5, 0)), (MUTED, ACCENT));
    }

    #[test]
    fn only_the_run_touching_the_focused_pane_is_accented() {
        // Left column split in two rows by a separate line; the line at x=3 is
        // shared by two rows of panes, only the focused (top) half is accented.
        let area = Rect::new(0, 0, 6, 5);
        let buf = draw(area, &[vertical(3, 5)], rect(0, 0, 3, 2));
        assert_eq!(fg(&buf, 3, 0), ACCENT);
        assert_eq!(fg(&buf, 3, 1), ACCENT);
        assert_eq!(fg(&buf, 3, 2), MUTED);
        assert_eq!(fg(&buf, 3, 4), MUTED);
    }

    #[test]
    fn cells_outside_the_area_and_empty_lines_are_skipped() {
        let area = Rect::new(0, 0, 3, 2);
        let lines = [vertical(2, 9), vertical(1, 0), vertical(40, 3)];
        let buf = draw(area, &lines, rect(0, 0, 2, 2));
        assert_eq!(buf[(2, 1)].symbol(), "│");
        assert_eq!(buf[(1, 0)].symbol(), " ");
    }
}
