pub mod components;
pub mod preview;
pub mod theme;

use ratatui::Frame;

use crate::{app::App, core::layout::Rect};
use components::{
    separators::SeparatorView,
    statusline::StatusLine,
    terminal_view::{TerminalView, cursor_position},
};
use theme::LazaroboxTheme;

impl From<Rect> for ratatui::layout::Rect {
    fn from(rect: Rect) -> Self {
        ratatui::layout::Rect::new(rect.x, rect.y, rect.width, rect.height)
    }
}

/// Draws every pane in its tile, the separators between them and the
/// statusline. Only the focused pane places the real cursor.
pub fn render(frame: &mut Frame, app: &App, theme: &LazaroboxTheme) {
    let screen = app.screen();
    let status = within(frame, screen.status);

    let tiling = app.tiling();
    let focused = app.focused();
    let mut focused_rect = Rect::default();
    for &(id, rect) in &tiling.panes {
        let area = within(frame, rect);
        let Some(pane) = app.pane(id) else { continue };
        if area.is_empty() {
            continue;
        }
        frame.render_widget(TerminalView::new(pane, theme), area);
        if id == focused {
            focused_rect = rect;
            if let Some(position) = cursor_position(pane, area) {
                frame.set_cursor_position(position);
            }
        }
    }
    frame.render_widget(
        SeparatorView::new(
            &tiling.separators,
            focused_rect,
            theme.input_accent(app.input()),
            theme.text_muted,
            theme.bg_base,
        ),
        within(frame, screen.body),
    );

    let path = app.status_path();
    frame.render_widget(
        StatusLine::input(theme, app.input(), &path, app.shell_name()),
        status,
    );
}

/// `rect` as a ratatui rect, cut to what the frame can actually draw.
fn within(frame: &Frame, rect: Rect) -> ratatui::layout::Rect {
    ratatui::layout::Rect::from(rect).intersection(frame.area())
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
        app.update(AppEvent::Cwd(PaneId::FIRST, "/tmp".into()));
        let (buf, _) = draw(&app);
        assert!(row(&buf, ROWS - 1).contains("/tmp"));
    }

    #[test]
    fn real_cursor_follows_the_pane_cursor() {
        let (_, cursor) = draw(&app());
        assert_eq!(cursor, Position::new(8, 0));
    }

    #[test]
    fn a_spawn_failure_notice_replaces_the_cwd_until_the_next_key() {
        let mut app = app().with_env(None, None, "/home/ana".into());
        let key = |c, mods| AppEvent::Key(KeyEvent::new(KeyCode::Char(c), mods));
        app.update(key(' ', KeyModifiers::CONTROL));
        app.update(key('w', KeyModifiers::NONE));
        app.update(key('v', KeyModifiers::NONE));
        app.update(AppEvent::SpawnFailed(PaneId::for_test(2), "boom".into()));
        let (buf, _) = draw(&app);
        let line = row(&buf, ROWS - 1);
        assert!(line.contains("spawn failed: boom"), "{line:?}");
        assert!(!line.contains("/home/ana"), "{line:?}");
        app.update(key('x', KeyModifiers::NONE));
        let (buf, _) = draw(&app);
        assert!(row(&buf, ROWS - 1).contains("/home/ana"));
    }

    fn press(app: &mut App, c: char, mods: KeyModifiers) {
        app.update(AppEvent::Key(KeyEvent::new(KeyCode::Char(c), mods)));
    }

    /// Two panes side by side (`w v`); the new right pane has the focus.
    fn split_app() -> App {
        let mut app = App::new(COLS, ROWS);
        app.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"left".to_vec()),
        ));
        press(&mut app, ' ', KeyModifiers::CONTROL);
        press(&mut app, 'w', KeyModifiers::NONE);
        press(&mut app, 'v', KeyModifiers::NONE);
        app.update(AppEvent::Pty(
            PaneId::for_test(2),
            PtyEvent::Output(b"right".to_vec()),
        ));
        app
    }

    fn cell_fg(buf: &Buffer, x: u16, y: u16) -> ratatui::style::Color {
        buf[(x, y)].style().fg.unwrap()
    }

    #[test]
    fn every_pane_is_drawn_in_its_rect_with_a_separator_between() {
        let app = split_app();
        let tiling = app.tiling();
        let (_, right) = tiling.panes[1];
        let sep = tiling.separators[0];
        let (buf, _) = draw(&app);
        let line = row(&buf, 0);
        assert!(line.starts_with("left"), "{line:?}");
        assert_eq!(buf[(sep.x, 0)].symbol(), "\u{2502}");
        let right_text: String = (right.x..right.x + 5)
            .map(|x| buf[(x, 0)].symbol())
            .collect();
        assert_eq!(right_text, "right");
        let left_half: String = line.chars().take(right.x.into()).collect();
        assert!(!left_half.contains("right"), "{line:?}");
        insta::assert_snapshot!(
            (0..ROWS - 1)
                .map(|y| row(&buf, y).trim_end().to_string())
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    #[test]
    fn only_the_focused_pane_places_the_cursor_at_its_rect() {
        let app = split_app();
        let (_, right) = app.tiling().panes[1];
        assert_eq!(app.focused(), PaneId::for_test(2));
        let (_, cursor) = draw(&app);
        assert_eq!(cursor, Position::new(right.x + 5, 0));
    }

    #[test]
    fn a_hidden_cursor_in_the_focused_pane_leaves_none() {
        let mut app = split_app();
        app.update(AppEvent::Pty(
            PaneId::for_test(2),
            PtyEvent::Output(b"\x1b[?25l".to_vec()),
        ));
        let (_, cursor) = draw(&app);
        // Nothing placed it, so it stays at the backend default.
        assert_eq!(cursor, Position::new(0, 0));
    }

    #[test]
    fn the_separator_accent_follows_the_mode_and_the_focus() {
        let theme = theme::LazaroboxTheme::default();
        let mut app = split_app();
        let sep = app.tiling().separators[0];
        let (buf, _) = draw(&app);
        assert_eq!(cell_fg(&buf, sep.x, 0), theme.input_accent(app.input()));
        press(&mut app, ' ', KeyModifiers::CONTROL);
        let (buf, _) = draw(&app);
        assert_eq!(cell_fg(&buf, sep.x, 0), theme.warning_orange);
        // Esc, then focus left (`h`): the line still touches the focused pane.
        app.update(AppEvent::Key(KeyEvent::new(
            KeyCode::Esc,
            KeyModifiers::NONE,
        )));
        let (buf, _) = draw(&app);
        assert_eq!(cell_fg(&buf, sep.x, 0), theme.success_green);
    }

    #[test]
    fn a_terminal_too_small_to_tile_does_not_panic() {
        for (cols, rows) in [(1, 2), (2, 2), (3, 3)] {
            let mut app = App::new(cols, rows);
            press(&mut app, ' ', KeyModifiers::CONTROL);
            press(&mut app, 'w', KeyModifiers::NONE);
            press(&mut app, 'v', KeyModifiers::NONE);
            let mut terminal = Terminal::new(TestBackend::new(cols, rows)).unwrap();
            terminal
                .draw(|frame| render(frame, &app, &theme::LazaroboxTheme::default()))
                .unwrap();
        }
    }
}
