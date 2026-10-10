//! The one-row tab bar: `" N cwd-basename "` per tab, active tab on the accent.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::Span,
    widgets::Widget,
};

use crate::ui::theme::LazaroboxTheme;

/// Draws `labels` side by side, one padded cell block per tab.
///
/// Overflow: a label wider than the bar is clipped with `…`; when the tabs do
/// not fit, tabs are dropped from the left until the active one is visible
/// and the rest is cut on the right. Never panics.
pub struct TabBar<'a> {
    labels: &'a [String],
    active: usize,
    theme: &'a LazaroboxTheme,
}

/// Terminal cells `text` takes.
pub(super) fn cells(text: &str) -> usize {
    Span::raw(text).width()
}

impl<'a> TabBar<'a> {
    pub fn new(labels: &'a [String], active: usize, theme: &'a LazaroboxTheme) -> Self {
        Self {
            labels,
            active,
            theme,
        }
    }
}

impl Widget for TabBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let area = area.intersection(buf.area);
        if area.is_empty() {
            return;
        }
        let width = usize::from(area.width);
        let idle = Style::default()
            .fg(self.theme.text_muted)
            .bg(self.theme.bg_panel);
        let active = Style::default()
            .fg(self.theme.bg_base)
            .bg(self.theme.primary_cyan)
            .add_modifier(Modifier::BOLD);
        buf.set_style(area, idle);

        let blocks: Vec<String> = self
            .labels
            .iter()
            .map(|label| clip(&format!(" {label} "), width))
            .collect();
        // Drop tabs from the left only until the active one fits.
        let last = self.active.min(blocks.len().saturating_sub(1));
        let mut first = 0;
        while first < last && blocks[first..=last].iter().map(|b| cells(b)).sum::<usize>() > width {
            first += 1;
        }
        let mut x = area.x;
        for (index, block) in blocks.iter().enumerate().skip(first) {
            let left = area.right().saturating_sub(x);
            if left == 0 {
                break;
            }
            let style = if index == self.active { active } else { idle };
            buf.set_stringn(x, area.y, block, usize::from(left), style);
            x = x.saturating_add(u16::try_from(cells(block)).unwrap_or(u16::MAX));
        }
    }
}

/// `text` cut to `width` cells, ending in `…` when it had to be cut.
pub(super) fn clip(text: &str, width: usize) -> String {
    if cells(text) <= width {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = cells(c.encode_utf8(&mut [0; 4]));
        if used + w + 1 > width {
            break;
        }
        out.push(c);
        used += w;
    }
    if width > 0 {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use ratatui::{layout::Position, style::Color};

    use super::*;

    fn labels(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn render(items: &[&str], active: usize, width: u16) -> Buffer {
        let theme = LazaroboxTheme::default();
        let labels = labels(items);
        let area = Rect::new(0, 0, width, 1);
        let mut buf = Buffer::empty(area);
        TabBar::new(&labels, active, &theme).render(area, &mut buf);
        buf
    }

    fn text(buf: &Buffer) -> String {
        (0..buf.area.width).map(|x| buf[(x, 0)].symbol()).collect()
    }

    fn bg(buf: &Buffer, x: u16) -> Color {
        buf[Position::new(x, 0)].bg
    }

    // Spec: Shown with two tabs.
    #[test]
    fn snapshot_two_tabs_second_active() {
        insta::assert_snapshot!(text(&render(&["1 proj", "2 tmp"], 1, 30)));
    }

    #[test]
    fn the_active_tab_is_on_the_accent_and_the_others_are_not() {
        let theme = LazaroboxTheme::default();
        let buf = render(&["1 proj", "2 tmp"], 1, 30);
        // " 1 proj " is 8 cells, " 2 tmp " starts at 8.
        assert_eq!(bg(&buf, 0), theme.bg_panel);
        assert_eq!(bg(&buf, 7), theme.bg_panel);
        assert_eq!(bg(&buf, 8), theme.primary_cyan);
        assert_eq!(bg(&buf, 14), theme.primary_cyan);
        assert_eq!(bg(&buf, 15), theme.bg_panel);
        assert!(buf[Position::new(9, 0)].modifier.contains(Modifier::BOLD));
    }

    // Spec: Narrow width — the active tab stays visible, earlier tabs drop.
    #[test]
    fn snapshot_narrow_keeps_the_active_tab() {
        insta::assert_snapshot!(text(&render(
            &["1 alpha", "2 beta", "3 gamma", "4 delta"],
            3,
            18
        )));
    }

    #[test]
    fn tabs_before_the_active_one_drop_only_as_needed() {
        // Widths 5 + 5 + 5; width 10 fits the last two.
        let buf = render(&["1 a", "2 b", "3 c"], 2, 10);
        assert_eq!(text(&buf), " 2 b  3 c ");
        let buf = render(&["1 a", "2 b", "3 c"], 1, 10);
        assert_eq!(text(&buf), " 1 a  2 b ");
    }

    #[test]
    fn tabs_after_the_active_one_are_cut_on_the_right() {
        let buf = render(&["1 a", "2 b", "3 c"], 0, 8);
        assert_eq!(text(&buf), " 1 a  2 ");
    }

    #[test]
    fn an_active_label_wider_than_the_bar_is_clipped_with_an_ellipsis() {
        let buf = render(&["1 a", "2 very-long-name"], 1, 10);
        assert_eq!(text(&buf), " 2 very-l…");
        assert_eq!(bg(&buf, 0), LazaroboxTheme::default().primary_cyan);
    }

    #[test]
    fn wide_characters_are_measured_in_cells() {
        let buf = render(&["1 日本語"], 0, 6);
        assert_eq!(buf[(3, 0)].symbol(), "日");
        assert_eq!(buf[(5, 0)].symbol(), "…");
    }

    // Spec: Narrow width — no panic at any width, with any active index.
    #[test]
    fn never_panics_at_any_width() {
        let items = ["1 alpha", "2", "3 日本語", "4 /"];
        for width in 0..=40 {
            for active in 0..items.len() + 2 {
                let _ = render(&items, active, width);
            }
        }
        let _ = render(&[], 0, 10);
    }

    #[test]
    fn the_bar_background_fills_the_unused_width() {
        let buf = render(&["1 a", "2 b"], 0, 20);
        assert_eq!(bg(&buf, 19), LazaroboxTheme::default().bg_panel);
    }
}
