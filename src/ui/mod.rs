pub mod components;
pub mod preview;
pub mod theme;

use ratatui::Frame;

use crate::{app::App, core::layout::Rect};
use components::{
    statusline::StatusLine,
    terminal_view::{TerminalView, cursor_position},
};
use theme::LazaroboxTheme;

/// Draws the terminal pane over the statusline, with the child's cwd on the
/// left and the shell name on the right.
pub fn render(frame: &mut Frame, app: &App, theme: &LazaroboxTheme) {
    let screen = app.screen();
    let body = within(frame, screen.body);
    let status = within(frame, screen.status);

    let pane = app.focused_pane();
    frame.render_widget(TerminalView::new(pane, theme), body);
    let cwd = app.cwd_label();
    frame.render_widget(
        StatusLine::input(theme, app.input(), &cwd, app.shell_name()),
        status,
    );

    if let Some(position) = cursor_position(pane, body) {
        frame.set_cursor_position(position);
    }
}

/// `rect` as a ratatui rect, cut to what the frame can actually draw.
fn within(frame: &Frame, rect: Rect) -> ratatui::layout::Rect {
    ratatui::layout::Rect::new(rect.x, rect.y, rect.width, rect.height).intersection(frame.area())
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Position};

    use super::*;
    use crate::{
        app::{App, AppEvent, InputMode},
        core::{layout::PaneId, pty::PtyEvent},
    };

    const COLS: u16 = 60;
    const ROWS: u16 = 6;

    fn app() -> App {
        let mut app = App::new(COLS, ROWS);
        app.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"hi there".to_vec()),
        ));
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
        assert_eq!(app.input(), InputMode::Prefix);
        let (buf, _) = draw(&app);
        assert!(row(&buf, ROWS - 1).contains("PREFIX"));
    }

    #[test]
    fn statusline_shows_the_cwd_and_the_shell_name() {
        let mut app = app().with_env(
            Some("/bin/zsh"),
            Some("/home/ana".into()),
            "/home/ana".into(),
        );
        let (buf, _) = draw(&app);
        let line = row(&buf, ROWS - 1);
        assert!(line.contains("~"), "{line:?}");
        assert!(line.trim_end().ends_with("zsh"), "{line:?}");
        app.update(AppEvent::Cwd("/tmp".into()));
        let (buf, _) = draw(&app);
        assert!(row(&buf, ROWS - 1).contains("/tmp"));
    }

    #[test]
    fn real_cursor_follows_the_pane_cursor() {
        let (_, cursor) = draw(&app());
        assert_eq!(cursor, Position::new(8, 0));
    }
}
