//! Draws the emulator screen into a ratatui buffer.

use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    widgets::Widget,
};

use crate::{
    core::pane::{CellView, Pane, TermColor},
    ui::theme::LazaroboxTheme,
};

/// The emulator screen as a widget. Cells outside the pane stay untouched and
/// pane cells outside the area are clipped.
pub struct TerminalView<'a> {
    pane: &'a Pane,
    theme: &'a LazaroboxTheme,
}

impl<'a> TerminalView<'a> {
    pub fn new(pane: &'a Pane, theme: &'a LazaroboxTheme) -> Self {
        Self { pane, theme }
    }

    fn style(&self, cell: &CellView) -> Style {
        let mut style = Style::new()
            .fg(map_color(cell.fg, Color::Reset))
            .bg(map_color(cell.bg, self.theme.bg_base));
        for (on, modifier) in [
            (cell.bold, Modifier::BOLD),
            (cell.italic, Modifier::ITALIC),
            (cell.underline, Modifier::UNDERLINED),
            (cell.inverse, Modifier::REVERSED),
        ] {
            if on {
                style = style.add_modifier(modifier);
            }
        }
        style
    }
}

/// Default colors resolve to `default`; the rest map one to one.
fn map_color(color: TermColor, default: Color) -> Color {
    match color {
        TermColor::Default => default,
        TermColor::Idx(i) => Color::Indexed(i),
        TermColor::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

impl Widget for TerminalView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for row in 0..area.height {
            for col in 0..area.width {
                let Some(cell) = self.pane.cell(row, col) else {
                    continue;
                };
                // The right half of a wide char is covered by its left half.
                if cell.wide_cont {
                    continue;
                }
                let symbol = if cell.text.is_empty() { " " } else { cell.text };
                buf[(area.x + col, area.y + row)]
                    .set_symbol(symbol)
                    .set_style(self.style(&cell));
            }
        }
    }
}

/// Where the real cursor goes, in buffer coordinates, or `None` when the child
/// hid it, the view is scrolled back, or it falls outside `area`.
pub fn cursor_position(pane: &Pane, area: Rect) -> Option<Position> {
    let (row, col) = pane.cursor()?;
    (col < area.width && row < area.height).then(|| Position::new(area.x + col, area.y + row))
}

#[cfg(test)]
mod tests {
    use ratatui::{
        buffer::Buffer,
        layout::{Position, Rect},
        style::{Color, Modifier},
        widgets::Widget,
    };

    use super::*;
    use crate::core::pane::{Pane, PaneSize};
    use crate::ui::theme::LazaroboxTheme;

    fn pane_with(rows: u16, cols: u16, bytes: &[u8]) -> Pane {
        let mut pane = Pane::new(PaneSize { rows, cols }, 100);
        pane.feed(bytes);
        pane
    }

    fn render(pane: &Pane, width: u16, height: u16) -> Buffer {
        let theme = LazaroboxTheme::default();
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        TerminalView::new(pane, &theme).render(area, &mut buf);
        buf
    }

    fn text(buf: &Buffer) -> String {
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    // Spec: snapshot of styled bytes in a 10x40 emulator.
    #[test]
    fn snapshot_styled_screen() {
        let pane = pane_with(
            10,
            40,
            b"plain\r\n\x1b[1;31mbold red\x1b[0m\r\n\x1b[38;2;1;2;3mrgb\x1b[0m \x1b[7minv\x1b[0m\r\n\x1b[4munder\x1b[0m \x1b[3mital\x1b[0m",
        );
        insta::assert_snapshot!(text(&render(&pane, 40, 10)));
    }

    #[test]
    fn maps_colors_and_attributes() {
        let pane = pane_with(
            4,
            20,
            b"\x1b[1;31mA\x1b[0m\x1b[38;2;1;2;3;48;5;200mB\x1b[0m\x1b[7mC\x1b[0m\x1b[4mD\x1b[0m\x1b[3mE",
        );
        let buf = render(&pane, 20, 4);
        assert_eq!(buf[(0, 0)].fg, Color::Indexed(1));
        assert!(buf[(0, 0)].modifier.contains(Modifier::BOLD));
        assert_eq!(buf[(1, 0)].fg, Color::Rgb(1, 2, 3));
        assert_eq!(buf[(1, 0)].bg, Color::Indexed(200));
        assert!(buf[(2, 0)].modifier.contains(Modifier::REVERSED));
        assert!(buf[(3, 0)].modifier.contains(Modifier::UNDERLINED));
        assert!(buf[(4, 0)].modifier.contains(Modifier::ITALIC));
        assert!(!buf[(0, 1)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn default_background_comes_from_the_theme() {
        let pane = pane_with(2, 10, b"x");
        let buf = render(&pane, 10, 2);
        let theme = LazaroboxTheme::default();
        assert_eq!(buf[(0, 0)].bg, theme.bg_base);
        assert_eq!(buf[(5, 1)].bg, theme.bg_base);
    }

    // Spec: wide characters take two columns and the continuation draws nothing.
    #[test]
    fn wide_char_is_drawn_once_and_the_next_cell_is_skipped() {
        let pane = pane_with(2, 10, "a世b".as_bytes());
        let buf = render(&pane, 10, 2);
        assert_eq!(buf[(0, 0)].symbol(), "a");
        assert_eq!(buf[(1, 0)].symbol(), "世");
        assert_eq!(buf[(3, 0)].symbol(), "b");
        assert_eq!(text(&buf).lines().next().unwrap().trim_end(), "a世 b");
    }

    #[test]
    fn a_pane_larger_than_the_area_is_clipped() {
        let pane = pane_with(5, 20, b"hello world");
        let buf = render(&pane, 5, 2);
        assert_eq!(text(&buf), "hello\n");
    }

    #[test]
    fn scrolled_back_view_shows_history() {
        let mut pane = pane_with(2, 10, b"one\r\ntwo\r\nthree");
        pane.set_scrollback(1);
        assert_eq!(text(&render(&pane, 10, 2)), "one\ntwo");
    }

    // Spec: the real cursor is placed unless hidden.
    #[test]
    fn cursor_is_placed_inside_the_area() {
        let pane = pane_with(5, 20, b"ab\r\ncd");
        let area = Rect::new(3, 2, 20, 5);
        assert_eq!(cursor_position(&pane, area), Some(Position::new(5, 3)));
    }

    #[test]
    fn hidden_cursor_has_no_position() {
        let pane = pane_with(5, 20, b"\x1b[?25l");
        assert_eq!(cursor_position(&pane, Rect::new(0, 0, 20, 5)), None);
    }

    #[test]
    fn cursor_outside_a_clipped_area_has_no_position() {
        let pane = pane_with(5, 20, b"\r\n\r\n\r\nxyz");
        assert_eq!(cursor_position(&pane, Rect::new(0, 0, 20, 2)), None);
    }

    #[test]
    fn scrolled_back_view_has_no_cursor() {
        let mut pane = pane_with(2, 10, b"one\r\ntwo\r\nthree");
        pane.set_scrollback(1);
        assert_eq!(cursor_position(&pane, Rect::new(0, 0, 10, 2)), None);
    }
}
