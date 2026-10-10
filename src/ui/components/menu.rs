//! The config-menu popup, drawn over the panes while MENU is open.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, Widget},
};

use super::tab_bar::{cells, clip};
use crate::{
    core::{
        config::Config,
        menu::{MenuState, Row},
    },
    ui::theme::LazaroboxTheme,
};

/// Widest popup wanted.
pub const WANT_WIDTH: u16 = 51;
/// Rows wanted: border, title, two settings, Mouse, blank, footer, border.
pub const WANT_HEIGHT: u16 = 8;
/// Smallest area that still shows the full popup.
pub const MIN_WIDTH: u16 = 24;
pub const MIN_HEIGHT: u16 = 7;

/// `want_w` x `want_h` centred in `area`, clamped to the area.
pub fn popup_area(area: Rect, want_w: u16, want_h: u16) -> Rect {
    let width = want_w.min(area.width);
    let height = want_h.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

/// Whether `area` is big enough for the full popup.
pub fn fits(area: Rect) -> bool {
    area.width >= MIN_WIDTH && area.height >= MIN_HEIGHT
}

/// Draws `MenuState::rows` in a bordered box, or a one-line fallback.
pub struct MenuPopup<'a> {
    menu: &'a MenuState,
    config: &'a Config,
    theme: &'a LazaroboxTheme,
}

impl<'a> MenuPopup<'a> {
    pub fn new(menu: &'a MenuState, config: &'a Config, theme: &'a LazaroboxTheme) -> Self {
        Self {
            menu,
            config,
            theme,
        }
    }
}

const HINT: &str = "j/k move · h/l change · Enter save · Esc cancel";
const FALLBACK: &str = "MENU · Esc cancel · Enter save";

impl MenuPopup<'_> {
    /// Below the size threshold: one line on the middle row.
    fn render_fallback(&self, area: Rect, buf: &mut Buffer) {
        let line = Rect::new(area.x, area.y + area.height / 2, area.width, 1);
        Clear.render(line, buf);
        let style = Style::default()
            .fg(self.theme.info_blue)
            .add_modifier(Modifier::REVERSED);
        buf.set_stringn(
            line.x,
            line.y,
            clip(FALLBACK, usize::from(line.width)),
            usize::from(line.width),
            style,
        );
    }

    fn line(&self, row: &Row, width: u16) -> (String, Style) {
        let plain = Style::default();
        match *row {
            Row::Title(title) => (format!(" {title}"), plain.add_modifier(Modifier::BOLD)),
            Row::Item {
                label,
                value,
                selected,
                enabled,
            } => {
                let value = value.unwrap_or("");
                let used = cells(label) + cells(value) + 2;
                let gap = usize::from(width).saturating_sub(used).max(1);
                let text = format!(" {label}{}{value} ", " ".repeat(gap));
                let style = if !enabled {
                    plain.fg(self.theme.text_muted)
                } else if selected {
                    plain
                        .fg(self.theme.info_blue)
                        .add_modifier(Modifier::REVERSED)
                } else {
                    plain
                };
                (text, style)
            }
        }
    }
}

impl Widget for MenuPopup<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let area = area.intersection(buf.area);
        if area.is_empty() {
            return;
        }
        if !fits(area) {
            self.render_fallback(area, buf);
            return;
        }
        let popup = popup_area(area, WANT_WIDTH, WANT_HEIGHT);
        Clear.render(popup, buf);
        let accent = Style::default().fg(self.theme.info_blue);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(accent)
            .title(" Menu ")
            .title_style(accent)
            .style(Style::default().bg(self.theme.bg_panel));
        let inner = block.inner(popup);
        block.render(popup, buf);

        let rows = self.menu.rows(self.config);
        // Body rows, then (height permitting) a blank row, then the footer.
        let blank = usize::from(inner.height) > rows.len() + 1;
        let width = usize::from(inner.width);
        for (offset, row) in rows.iter().enumerate() {
            let (text, style) = self.line(row, inner.width);
            let y = inner.y + offset as u16;
            buf.set_style(Rect::new(inner.x, y, inner.width, 1), style);
            buf.set_stringn(inner.x, y, clip(&text, width), width, style);
        }
        let footer_y = inner.y + rows.len() as u16 + u16::from(blank);
        if footer_y < inner.bottom() {
            let (text, style) = match self.menu.error() {
                Some(error) => (error, Style::default().fg(self.theme.error_red)),
                None => (HINT, Style::default().fg(self.theme.text_muted)),
            };
            let text = clip(&format!(" {text}"), width);
            buf.set_stringn(inner.x, footer_y, text, width, style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(w: u16, h: u16) -> Rect {
        Rect::new(0, 0, w, h)
    }

    // Spec: Tiny terminals (geometry).
    #[test]
    fn popup_is_centred_and_capped_at_51_by_8() {
        assert_eq!(popup_area(r(80, 24), 51, 8), Rect::new(14, 8, 51, 8));
    }

    #[test]
    fn popup_is_clamped_to_a_small_area() {
        assert_eq!(popup_area(r(30, 7), 51, 8), Rect::new(0, 0, 30, 7));
        assert_eq!(popup_area(r(51, 8), 51, 8), Rect::new(0, 0, 51, 8));
    }

    #[test]
    fn odd_sizes_round_the_offset_down_and_respect_the_origin() {
        assert_eq!(popup_area(r(52, 9), 51, 8), Rect::new(0, 0, 51, 8));
        assert_eq!(
            popup_area(Rect::new(3, 2, 60, 11), 51, 8),
            Rect::new(7, 3, 51, 8)
        );
    }

    #[test]
    fn zero_area_gives_an_empty_popup() {
        assert!(popup_area(r(0, 0), 51, 8).is_empty());
        assert!(popup_area(r(0, 10), 51, 8).is_empty());
        assert!(popup_area(r(10, 0), 51, 8).is_empty());
    }

    #[test]
    fn the_full_popup_needs_exactly_24_by_7() {
        assert!(fits(r(24, 7)));
        assert!(!fits(r(23, 7)));
        assert!(!fits(r(24, 6)));
        assert!(fits(r(200, 50)));
        assert!(!fits(r(0, 0)));
    }

    use crate::core::config::Config;

    fn draw(w: u16, h: u16, menu: &MenuState) -> Buffer {
        let theme = LazaroboxTheme::default();
        let area = r(w, h);
        let mut buf = Buffer::empty(area);
        MenuPopup::new(menu, &Config::default(), &theme).render(area, &mut buf);
        buf
    }

    fn text(buf: &Buffer) -> String {
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn fresh() -> MenuState {
        MenuState::open(&Config::default())
    }

    // Spec: Settings rows; Footer hint.
    #[test]
    fn snapshot_settings_popup_at_the_wanted_size() {
        insta::assert_snapshot!(text(&draw(51, 8, &fresh())));
    }

    #[test]
    fn snapshot_popup_is_centred_in_a_larger_area() {
        insta::assert_snapshot!(text(&draw(60, 11, &fresh())));
    }

    // Spec: the blank row is dropped first on a short height.
    #[test]
    fn snapshot_short_height_drops_the_blank_row() {
        insta::assert_snapshot!(text(&draw(51, 7, &fresh())));
    }

    #[test]
    fn selected_row_is_reversed_and_the_others_are_not() {
        let buf = draw(51, 8, &fresh());
        let reversed = |y| buf[(2, y)].modifier.contains(Modifier::REVERSED);
        assert!(!reversed(1));
        assert!(reversed(2));
        assert!(!reversed(3));
    }

    #[test]
    fn disabled_mouse_row_is_muted() {
        let buf = draw(51, 8, &fresh());
        let theme = LazaroboxTheme::default();
        assert_eq!(buf[(2, 4)].fg, theme.text_muted);
        assert_ne!(buf[(2, 3)].fg, theme.text_muted);
    }

    // Spec: Footer: Error replaces hint.
    #[test]
    fn snapshot_error_replaces_the_hint_and_is_clipped() {
        let mut menu = fresh();
        menu.set_error("config.toml: statusline is not a table; fix it before saving".into());
        let buf = draw(51, 8, &menu);
        assert!(!text(&buf).contains("j/k move"));
        insta::assert_snapshot!(text(&buf));
    }

    // Spec: Tiny terminals: one-line fallback.
    #[test]
    fn below_the_threshold_only_the_fallback_line_is_drawn() {
        let buf = draw(30, 5, &fresh());
        let lines: Vec<String> = text(&buf).lines().map(str::to_string).collect();
        assert_eq!(lines[2].trim_end(), "MENU · Esc cancel · Enter save");
        for (y, line) in lines.iter().enumerate() {
            if y != 2 {
                assert_eq!(line.trim(), "", "row {y}");
            }
        }
    }

    #[test]
    fn fallback_is_clipped_with_an_ellipsis() {
        let buf = draw(23, 6, &fresh());
        let lines: Vec<String> = text(&buf).lines().map(str::to_string).collect();
        assert_eq!(lines[3], "MENU · Esc cancel · En…");
    }

    #[test]
    fn exactly_24_by_7_shows_the_full_popup() {
        let buf = draw(24, 7, &fresh());
        assert!(text(&buf).contains("Settings"));
        assert!(!text(&buf).contains("MENU"));
        let below = draw(23, 7, &fresh());
        assert!(!text(&below).contains("Settings"));
        assert!(text(&below).contains("MENU"));
    }

    // Spec: No panic at any size.
    #[test]
    fn no_panic_at_any_size() {
        let mut menu = fresh();
        for w in 0..60 {
            for h in 0..12 {
                draw(w, h, &menu);
            }
        }
        menu.set_error("save failed: no config path (HOME is not set)".into());
        for w in 0..60 {
            for h in 0..12 {
                draw(w, h, &menu);
            }
        }
    }

    #[test]
    fn nothing_is_drawn_at_zero_size() {
        assert_eq!(draw(0, 0, &fresh()).content.len(), 0);
        assert_eq!(text(&draw(0, 9, &fresh())), "\n".repeat(8));
        assert_eq!(text(&draw(9, 0, &fresh())), "");
    }
}
