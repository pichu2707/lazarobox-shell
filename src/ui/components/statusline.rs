use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph, Widget},
};

use crate::app::AppMode;
use crate::ui::theme::LazaroboxTheme;

/// Nerd Font powerline glyph "upper left triangle" (U+E0BC).
/// Placed right after a left-hand block: `fg` = that block's bg, `bg` = the
/// next segment's bg. The filled upper-left half continues the block and the
/// edge slants like "/".
const SLANT_RIGHT: &str = "\u{E0BC}";

/// Nerd Font powerline glyph "lower right triangle" (U+E0BA).
/// Placed right before a right-hand block: `fg` = that block's bg, `bg` = the
/// surrounding bg. The filled lower-right half starts the block with a "/" edge.
const SLANT_LEFT: &str = "\u{E0BA}";

/// Powerline-style status line: `[ mode ][ path ]` on the left, `[ model ]`
/// on the right, with a flexible `bg_base` fill in between.
pub struct StatusLine<'a> {
    theme: &'a LazaroboxTheme,
    mode: AppMode,
    path: &'a str,
    model: &'a str,
}

impl<'a> StatusLine<'a> {
    pub fn new(theme: &'a LazaroboxTheme, mode: AppMode, path: &'a str, model: &'a str) -> Self {
        Self {
            theme,
            mode,
            path,
            model,
        }
    }

    fn left_line(&self) -> Line<'a> {
        let t = self.theme;
        let mode_style = t.mode_style(&self.mode);
        let mode_bg = mode_style.bg.unwrap_or(t.primary_cyan);
        Line::from(vec![
            Span::styled(
                format!(" \u{25D0} {} ", self.mode.label()),
                mode_style.add_modifier(Modifier::BOLD),
            ),
            Span::styled(SLANT_RIGHT, Style::new().fg(mode_bg).bg(t.bg_panel)),
            Span::styled(
                format!(" {} ", self.path),
                Style::new().fg(t.text_muted).bg(t.bg_panel),
            ),
            Span::styled(SLANT_RIGHT, Style::new().fg(t.bg_panel).bg(t.bg_base)),
        ])
    }

    fn right_line(&self) -> Line<'a> {
        let t = self.theme;
        Line::from(vec![
            Span::styled(SLANT_LEFT, Style::new().fg(t.bg_panel).bg(t.bg_base)),
            Span::styled(
                format!(" {} ", self.model),
                Style::new().fg(t.primary_cyan).bg(t.bg_panel),
            ),
        ])
    }
}

impl Widget for StatusLine<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Block::new()
            .style(Style::new().bg(self.theme.bg_base))
            .render(area, buf);

        let right = self.right_line();
        let right_width = u16::try_from(right.width()).unwrap_or(u16::MAX);
        let [left_area, right_area] =
            Layout::horizontal([Constraint::Min(0), Constraint::Length(right_width)]).areas(area);

        Paragraph::new(self.left_line()).render(left_area, buf);
        Paragraph::new(right).render(right_area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend, style::Color};

    const WIDTH: u16 = 60;
    const PATH: &str = "~/lazarobox-shell";
    const MODEL: &str = "ollama \u{b7} llama3.2";

    fn render_with(theme: &LazaroboxTheme, mode: AppMode) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(WIDTH, 1)).unwrap();
        terminal
            .draw(|frame| {
                frame.render_widget(StatusLine::new(theme, mode, PATH, MODEL), frame.area())
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn render(mode: AppMode) -> Buffer {
        render_with(&LazaroboxTheme::default(), mode)
    }

    fn line_text(buf: &Buffer) -> String {
        (0..WIDTH).map(|x| buf[(x, 0)].symbol()).collect()
    }

    #[test]
    fn mode_block_follows_mode_accent_and_is_bold() {
        let t = LazaroboxTheme::default();
        let cases = [
            (AppMode::Normal, t.primary_cyan),
            (AppMode::AiChat, t.ai_purple),
            (AppMode::Metrics, t.success_green),
            (AppMode::Settings, t.warning_orange),
        ];
        for (mode, accent) in cases {
            let buf = render(mode);
            let cell = &buf[(1, 0)];
            assert_eq!(cell.bg, accent, "bg for {mode:?}");
            assert_eq!(cell.fg, t.bg_base, "fg for {mode:?}");
            assert!(cell.modifier.contains(Modifier::BOLD), "bold for {mode:?}");
        }
    }

    #[test]
    fn mode_label_is_rendered() {
        assert!(line_text(&render(AppMode::Normal)).contains("NORMAL"));
        assert!(line_text(&render(AppMode::AiChat)).contains("AI CHAT"));
    }

    #[test]
    fn path_segment_uses_panel_bg_and_muted_fg() {
        let t = LazaroboxTheme::default();
        let text = line_text(&render(AppMode::Normal));
        let idx = text.chars().position(|c| c == '~').unwrap() as u16;
        let buf = render(AppMode::Normal);
        assert_eq!(buf[(idx, 0)].bg, t.bg_panel);
        assert_eq!(buf[(idx, 0)].fg, t.text_muted);
    }

    #[test]
    fn model_text_ends_at_right_edge() {
        let buf = render(AppMode::Normal);
        let text = line_text(&buf);
        assert!(text.ends_with(&format!("{MODEL} ")));
        let last = &buf[(WIDTH - 1, 0)];
        assert_eq!(last.bg, LazaroboxTheme::default().bg_panel);
        assert_eq!(last.fg, LazaroboxTheme::default().primary_cyan);
    }

    #[test]
    fn middle_fill_uses_bg_base() {
        let t = LazaroboxTheme::default();
        let buf = render(AppMode::Normal);
        // Cells strictly between the left cluster and the right cluster.
        let left_w = Line::from(" \u{25D0} NORMAL ").width() + 1 + (PATH.len() + 2) + 1;
        let right_w = 1 + MODEL.chars().count() + 2;
        for x in left_w..(WIDTH as usize - right_w) {
            assert_eq!(buf[(x as u16, 0)].bg, t.bg_base, "x={x}");
        }
    }

    #[test]
    fn colors_come_from_the_theme() {
        let theme = LazaroboxTheme {
            primary_cyan: Color::Rgb(1, 2, 3),
            bg_base: Color::Rgb(4, 5, 6),
            bg_panel: Color::Rgb(7, 8, 9),
            text_muted: Color::Rgb(10, 11, 12),
            ..LazaroboxTheme::default()
        };
        let buf = render_with(&theme, AppMode::Normal);
        assert_eq!(buf[(1, 0)].bg, Color::Rgb(1, 2, 3));
        assert_eq!(buf[(1, 0)].fg, Color::Rgb(4, 5, 6));
        assert_eq!(buf[(WIDTH - 1, 0)].fg, Color::Rgb(1, 2, 3));
        assert_eq!(buf[(WIDTH - 1, 0)].bg, Color::Rgb(7, 8, 9));
        assert_eq!(buf[(WIDTH / 2, 0)].bg, Color::Rgb(4, 5, 6));
    }

    #[test]
    fn snapshot_normal_mode() {
        insta::assert_snapshot!(line_text(&render(AppMode::Normal)));
    }
}
