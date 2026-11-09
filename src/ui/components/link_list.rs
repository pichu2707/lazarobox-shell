//! The popup of `Ctrl+Space u`: the links of the focused pane, each with its
//! key.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, Widget},
};

use crate::core::link_list::LinkList;
use crate::ui::theme::LazaroboxTheme;

const MAX_WIDTH: u16 = 76;
const HINT: &str = "key open · j/k move · Enter open · y copy · Esc close";

pub struct LinkListPopup<'a> {
    list: &'a LinkList,
    theme: &'a LazaroboxTheme,
}

impl<'a> LinkListPopup<'a> {
    pub fn new(list: &'a LinkList, theme: &'a LazaroboxTheme) -> Self {
        Self { list, theme }
    }
}

impl Widget for LinkListPopup<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let urls = self.list.urls();
        let wanted = u16::try_from(urls.len())
            .unwrap_or(u16::MAX)
            .saturating_add(3);
        let (width, height) = (area.width.min(MAX_WIDTH), area.height.min(wanted));
        if width < 3 || height < 3 {
            return;
        }
        let popup = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        let theme = self.theme;
        let base = Style::default().bg(theme.bg_base);
        Clear.render(popup, buf);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Links ")
            .style(base)
            .border_style(base.fg(theme.info_blue));
        let inner = block.inner(popup);
        block.render(popup, buf);
        let hint = u16::from(inner.height > 1);
        let body = usize::from(inner.height - hint);
        let first = (self.list.selected() + 1).saturating_sub(body);
        for (row, (index, url)) in urls.iter().enumerate().skip(first).take(body).enumerate() {
            let y = inner.y + u16::try_from(row).unwrap_or(0);
            let key = LinkList::key_of(index).unwrap_or(' ');
            let style = match index == self.list.selected() {
                true => base.add_modifier(Modifier::REVERSED),
                false => base,
            };
            let text = format!("{key}  {url}");
            buf.set_stringn(inner.x, y, &text, usize::from(inner.width), style);
        }
        if hint == 1 {
            let y = inner.y + inner.height - 1;
            let style = base.fg(theme.text_muted);
            buf.set_stringn(inner.x, y, HINT, usize::from(inner.width), style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(urls: &[&str], up: usize, (width, height): (u16, u16)) -> Vec<String> {
        let mut list = LinkList::new(urls.iter().map(|u| (*u).to_owned()).collect()).unwrap();
        let key = ratatui::crossterm::event::KeyEvent::new(
            ratatui::crossterm::event::KeyCode::Up,
            ratatui::crossterm::event::KeyModifiers::NONE,
        );
        for _ in 0..up {
            list.on_key(&key, false);
        }
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        let theme = LazaroboxTheme::default();
        LinkListPopup::new(&list, &theme).render(area, &mut buf);
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buf[(x, y)].symbol().to_owned())
                    .collect()
            })
            .collect()
    }

    fn has(rows: &[String], text: &str) -> bool {
        rows.iter().any(|row| row.contains(text))
    }

    #[test]
    fn each_link_is_drawn_with_its_key_at_80_columns() {
        let rows = drawn(&["https://a.io", "http://localhost:3000"], 0, (80, 24));
        assert!(has(&rows, "1  https://a.io"), "{rows:#?}");
        assert!(has(&rows, "2  http://localhost:3000"), "{rows:#?}");
        assert!(has(&rows, "Links"), "{rows:#?}");
        assert!(has(&rows, "y copy"), "{rows:#?}");
    }

    #[test]
    fn the_selected_row_stays_in_view_on_a_short_terminal() {
        let urls: Vec<String> = (0..20).map(|n| format!("https://a.io/{n}")).collect();
        let urls: Vec<&str> = urls.iter().map(String::as_str).collect();
        let rows = drawn(&urls, 0, (80, 6));
        assert!(has(&rows, "https://a.io/19"), "{rows:#?}");
        let rows = drawn(&urls, 19, (80, 6));
        assert!(has(&rows, "https://a.io/0"), "{rows:#?}");
    }

    #[test]
    fn a_long_link_is_cut_inside_the_popup() {
        let long = format!("https://a.io/{}", "x".repeat(300));
        let rows = drawn(&[long.as_str()], 0, (80, 24));
        assert!(has(&rows, "1  https://a.io/xxx"), "{rows:#?}");
    }

    #[test]
    fn a_tiny_terminal_draws_nothing_and_does_not_panic() {
        let rows = drawn(&["https://a.io"], 0, (2, 2));
        assert!(rows.iter().all(|row| row.trim().is_empty()), "{rows:#?}");
    }
}
