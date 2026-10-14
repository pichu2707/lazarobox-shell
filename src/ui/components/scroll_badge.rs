//! The `[+N]` badge of a scrolled pane: a drawing overlay on the top-right
//! corner of the content, never part of the pane's text.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    widgets::Widget,
};

use crate::ui::theme::LazaroboxTheme;

/// `[+N]` over the top-right corner of `area`. Nothing is drawn at offset 0
/// or when the badge is wider than the area.
pub struct ScrollBadge<'a> {
    offset: usize,
    theme: &'a LazaroboxTheme,
}

impl<'a> ScrollBadge<'a> {
    pub fn new(offset: usize, theme: &'a LazaroboxTheme) -> Self {
        Self { offset, theme }
    }

    /// The badge text for `offset`.
    pub fn text(offset: usize) -> String {
        format!("[+{offset}]")
    }
}

impl Widget for ScrollBadge<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.offset == 0 || area.is_empty() {
            return;
        }
        let text = Self::text(self.offset);
        let Ok(width) = u16::try_from(text.len()) else {
            return;
        };
        if width > area.width {
            return;
        }
        let style = Style::new()
            .fg(self.theme.bg_base)
            .bg(self.theme.warning_orange)
            .add_modifier(Modifier::BOLD);
        buf.set_string(area.right() - width, area.y, text, style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draw(offset: usize, width: u16, height: u16) -> Buffer {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        for y in 0..height {
            for x in 0..width {
                buf[(x, y)].set_symbol("x");
            }
        }
        ScrollBadge::new(offset, &LazaroboxTheme::default()).render(area, &mut buf);
        buf
    }

    fn row(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
    }

    #[test]
    fn the_badge_is_right_aligned_on_the_first_row() {
        let buf = draw(12, 20, 3);
        assert_eq!(row(&buf, 0), "xxxxxxxxxxxxxxx[+12]");
        assert_eq!(row(&buf, 1), "x".repeat(20), "other rows are untouched");
        assert_eq!(row(&buf, 2), "x".repeat(20));
    }

    #[test]
    fn the_badge_follows_the_digits_of_the_offset() {
        assert_eq!(row(&draw(7, 10, 1), 0), "xxxxxx[+7]");
        assert_eq!(row(&draw(12345, 10, 1), 0), "xx[+12345]");
    }

    #[test]
    fn a_badge_that_fills_the_pane_exactly_is_drawn() {
        assert_eq!(row(&draw(7, 5, 1), 0), "x[+7]");
        assert_eq!(row(&draw(7, 4, 1), 0), "[+7]");
    }

    #[test]
    fn offset_zero_draws_nothing() {
        assert_eq!(row(&draw(0, 20, 2), 0), "x".repeat(20));
    }

    #[test]
    fn a_pane_too_narrow_gets_no_badge() {
        assert_eq!(row(&draw(7, 3, 2), 0), "xxx");
        assert_eq!(row(&draw(12, 4, 2), 0), "xxxx");
        assert_eq!(row(&draw(12, 5, 2), 0), "[+12]", "exactly as wide: drawn");
        for width in 0..=4 {
            let _ = draw(9, width, 1);
        }
    }

    #[test]
    fn an_empty_area_does_not_panic() {
        let _ = draw(5, 0, 0);
        let _ = draw(5, 10, 0);
    }

    #[test]
    fn the_badge_is_drawn_inside_an_offset_area() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 3));
        let area = Rect::new(10, 1, 12, 2);
        ScrollBadge::new(3, &LazaroboxTheme::default()).render(area, &mut buf);
        let drawn: String = (18..22).map(|x| buf[(x, 1)].symbol()).collect();
        assert_eq!(drawn, "[+3]");
        assert_eq!(buf[(17, 1)].symbol(), " ");
        assert_eq!(buf[(22, 1)].symbol(), " ");
    }
}
