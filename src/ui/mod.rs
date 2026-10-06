pub mod components;
pub mod preview;
pub mod theme;

use ratatui::{
    Frame,
    layout::{Constraint, Layout},
};

use crate::app::App;
use components::{
    statusline::StatusLine,
    terminal_view::{TerminalView, cursor_position},
};
use theme::LazaroboxTheme;

/// Draws the terminal pane over the statusline. The cwd and shell segments
/// arrive with the cwd polling, so both are empty for now.
pub fn render(frame: &mut Frame, app: &App, theme: &LazaroboxTheme) {
    let [body, status] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());

    frame.render_widget(TerminalView::new(&app.pane, theme), body);
    frame.render_widget(StatusLine::input(theme, app.input, "", ""), status);

    if let Some(position) = cursor_position(&app.pane, body) {
        frame.set_cursor_position(position);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Position};

    use super::*;
    use crate::{
        app::{App, AppEvent, InputMode},
        core::{pane::PaneSize, pty::PtyEvent},
    };

    const COLS: u16 = 60;
    const ROWS: u16 = 6;

    fn app() -> App {
        let mut app = App::new(PaneSize {
            rows: ROWS - 1,
            cols: COLS,
        });
        app.update(AppEvent::Pty(PtyEvent::Output(b"hi there".to_vec())));
        app
    }

    fn draw(app: &App) -> (Buffer, Position) {
        let mut terminal = Terminal::new(TestBackend::new(COLS, ROWS)).unwrap();
        terminal
            .draw(|frame| render(frame, app, &theme::LazaroboxTheme::default()))
            .unwrap();
        let cursor = terminal.get_cursor_position().unwrap();
        (terminal.backend().buffer().clone(), cursor)
    }

    fn row(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
    }

    #[test]
    fn pane_fills_the_top_and_statusline_the_bottom_row() {
        let (buf, _) = draw(&app());
        assert!(row(&buf, 0).starts_with("hi there"));
        assert!(row(&buf, ROWS - 1).contains("TERMINAL"));
        assert!(!row(&buf, ROWS - 2).contains("TERMINAL"));
    }

    #[test]
    fn statusline_follows_the_input_mode() {
        let mut app = app();
        app.update(AppEvent::Key(KeyEvent::new(
            KeyCode::Char(' '),
            KeyModifiers::CONTROL,
        )));
        assert_eq!(app.input, InputMode::Prefix);
        let (buf, _) = draw(&app);
        assert!(row(&buf, ROWS - 1).contains("PREFIX"));
    }

    #[test]
    fn real_cursor_follows_the_pane_cursor() {
        let (_, cursor) = draw(&app());
        assert_eq!(cursor, Position::new(8, 0));
    }
}
