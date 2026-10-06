use std::io;

use ratatui::{
    Frame,
    crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    layout::{Constraint, Layout},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Padding, Paragraph},
};

use crate::app::AppMode;
use crate::ui::{components::statusline::StatusLine, theme::LazaroboxTheme};

/// Draws the theme preview: palette swatches in a panel plus a powerline status line.
pub fn render_theme_preview(frame: &mut Frame, theme: &LazaroboxTheme, mode: AppMode) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::new().bg(theme.bg_base)), area);

    let [body, status] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);

    let panel = Block::new()
        .title(" lazarobox-theme")
        .title_bottom(
            Line::from(" Tab: next mode \u{b7} q: quit").style(Style::new().fg(theme.text_muted)),
        )
        .padding(Padding::horizontal(1))
        .style(Style::new().bg(theme.bg_panel).fg(theme.primary_cyan));
    let rows: Vec<Line> = theme
        .palette()
        .into_iter()
        .map(|(name, hex, color)| {
            Line::from(vec![
                Span::styled("      ", Style::new().bg(color)),
                Span::styled(
                    format!("  {name:<15}{hex}"),
                    Style::new().fg(theme.text_muted),
                ),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(rows).block(panel), body);

    frame.render_widget(
        StatusLine::new(theme, mode, "~/lazarobox-shell", "ollama \u{b7} llama3.2"),
        status,
    );
}

/// Runs the interactive preview. Tab cycles modes, q/Esc quits.
/// `ratatui::run` restores the terminal on exit; its panic hook does so on panic.
pub fn run_preview() -> io::Result<()> {
    ratatui::run(|terminal| {
        let theme = LazaroboxTheme::default();
        let mut mode = AppMode::default();
        loop {
            terminal.draw(|frame| render_theme_preview(frame, &theme, mode))?;
            if let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                match key.code {
                    KeyCode::Tab => mode = mode.next(),
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    // Raw mode disables SIGINT, so Ctrl+C must be handled explicitly.
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

    const WIDTH: u16 = 50;
    const HEIGHT: u16 = 14;

    fn render(mode: AppMode) -> Buffer {
        let theme = LazaroboxTheme::default();
        let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).unwrap();
        terminal
            .draw(|frame| render_theme_preview(frame, &theme, mode))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn buffer_text(buffer: &Buffer) -> String {
        let area = buffer.area;
        (area.top()..area.bottom())
            .map(|y| {
                (area.left()..area.right())
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn assert_status_mode_block_bg(mode: AppMode, expected: ratatui::style::Color) {
        let buffer = render(mode);
        assert_eq!(buffer[(1, HEIGHT - 1)].bg, expected, "mode={mode:?}");
    }

    #[test]
    fn status_line_uses_cyan_in_normal_mode() {
        assert_status_mode_block_bg(AppMode::Normal, LazaroboxTheme::default().primary_cyan);
    }

    #[test]
    fn status_line_uses_purple_in_ai_chat_mode() {
        assert_status_mode_block_bg(AppMode::AiChat, LazaroboxTheme::default().ai_purple);
    }

    #[test]
    fn key_hint_is_kept_in_panel() {
        let text = buffer_text(&render(AppMode::Normal));
        assert!(text.contains("Tab: next mode"));
    }

    #[test]
    fn panel_has_no_border() {
        let text = buffer_text(&render(AppMode::Normal));
        for border in ['┌', '┐', '└', '┘', '│', '─'] {
            assert!(!text.contains(border), "found border char {border:?}");
        }
    }

    #[test]
    fn panel_spans_full_width_without_outer_margin() {
        let buffer = render(AppMode::Normal);
        let bg_panel = LazaroboxTheme::default().bg_panel;
        assert_eq!(buffer[(0, 0)].bg, bg_panel);
        assert_eq!(buffer[(WIDTH - 1, 0)].bg, bg_panel);
    }

    #[test]
    fn snapshot_normal_mode() {
        insta::assert_snapshot!(buffer_text(&render(AppMode::Normal)));
    }
}
