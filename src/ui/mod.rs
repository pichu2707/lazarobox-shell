pub mod components;
pub mod preview;
pub mod theme;

use ratatui::Frame;

use crate::{app::App, core::layout::Rect};
use components::{
    separators::SeparatorView,
    statusline::StatusLine,
    tab_bar::TabBar,
    terminal_view::{TerminalView, cursor_position},
};
use theme::LazaroboxTheme;

impl From<Rect> for ratatui::layout::Rect {
    fn from(rect: Rect) -> Self {
        ratatui::layout::Rect::new(rect.x, rect.y, rect.width, rect.height)
    }
}

/// Draws the tab bar (with several tabs), every pane in its tile, the
/// separators between them and the statusline. Only the focused pane places the real cursor.
pub fn render(frame: &mut Frame, app: &App, theme: &LazaroboxTheme) {
    let screen = app.screen();
    let status = within(frame, screen.status);
    if let Some(bar) = screen.tab_bar {
        let labels = app.tab_labels();
        frame.render_widget(
            TabBar::new(&labels, app.active_tab(), theme),
            within(frame, bar),
        );
    }

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

    let line = |path| {
        StatusLine::input(theme, app.input(), path, app.shell_name())
            .zoomed(app.zoomed())
            .copy_position(app.copy_position())
    };
    let path = app.status_path(line("").path_budget(status.width));
    frame.render_widget(line(&path), status);
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
        core::{
            config::{BarPosition, BarPositions, Config},
            layout::PaneId,
            pty::PtyEvent,
        },
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

    fn draw_sized(app: &App, cols: u16, rows: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(cols, rows)).unwrap();
        terminal
            .draw(|frame| render(frame, app, &theme::LazaroboxTheme::default()))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    /// The statusline row of `app`, drawn at the size the app was built for.
    fn status(app: &App) -> String {
        let at = app.screen().status;
        row(&draw_sized(app, at.width, at.y + 1), at.y)
    }

    fn keys(app: &mut App, keys: &str) {
        for c in keys.chars() {
            press(app, c, KeyModifiers::NONE);
        }
    }

    // Spec: Root hint in PREFIX; Group hint; Hint gone after the group resolves.
    #[test]
    fn the_statusline_shows_the_root_and_group_hints_and_drops_them_on_resolve() {
        let mut app = App::new(100, 3).with_env(None, None, "/work".into());
        press(&mut app, ' ', KeyModifiers::CONTROL);
        let line = status(&app);
        assert!(line.contains("PREFIX"), "{line:?}");
        assert!(
            line.contains("w window · t tab · g go · b buffer · [ copy · m menu · q quit"),
            "{line:?}"
        );
        insta::assert_snapshot!("root_hint", line.trim_end());
        keys(&mut app, "w");
        let line = status(&app);
        assert!(line.contains("WINDOW"), "{line:?}");
        insta::assert_snapshot!("group_hint", line.trim_end());
        app.update(AppEvent::Key(KeyEvent::new(
            KeyCode::Esc,
            KeyModifiers::NONE,
        )));
        let line = status(&app);
        assert!(
            line.contains("TERMINAL") && line.contains("/work"),
            "{line:?}"
        );
        assert!(!line.contains("split right"), "{line:?}");
    }

    // Spec: statusline MENU label; the path segment never shows the root hint.
    #[test]
    fn menu_mode_shows_its_label_and_the_cwd_never_the_hint() {
        let mut app = App::new(100, 3).with_env(None, None, "/work".into());
        press(&mut app, ' ', KeyModifiers::CONTROL);
        keys(&mut app, "m");
        let line = status(&app);
        assert!(line.contains("MENU") && line.contains("/work"), "{line:?}");
        assert!(
            !line.contains("window") && !line.contains("quit"),
            "{line:?}"
        );
    }

    // Spec: Root hint clipped and cleared; Narrow width.
    #[test]
    fn a_narrow_statusline_clips_the_hint_by_whole_entries_and_never_panics() {
        let mut app = App::new(60, 3).with_env(None, None, "/work".into());
        press(&mut app, ' ', KeyModifiers::CONTROL);
        let line = status(&app);
        assert!(line.contains("PREFIX"), "{line:?}");
        assert!(line.contains('…') && !line.contains("quit"), "{line:?}");
        insta::assert_snapshot!("clipped_hint", line.trim_end());
        for cols in 1..=40 {
            let mut app = App::new(cols, 3);
            press(&mut app, ' ', KeyModifiers::CONTROL);
            keys(&mut app, "w");
            let _ = status(&app);
        }
    }

    // Spec: Notice shown beats the hint.
    #[test]
    fn a_notice_is_drawn_instead_of_the_hint() {
        let mut app = App::new(100, 3);
        press(&mut app, ' ', KeyModifiers::CONTROL);
        keys(&mut app, "wv");
        app.update(AppEvent::SpawnFailed(PaneId::for_test(2), "boom".into()));
        let line = status(&app);
        assert!(line.contains("spawn failed: boom"), "{line:?}");
        assert!(!line.contains("window"), "{line:?}");
    }

    // Spec: Close pane prompt.
    #[test]
    fn the_close_prompt_is_in_the_mode_block_and_the_cwd_stays() {
        let mut app = split_app().with_env(None, None, "/work".into());
        press(&mut app, ' ', KeyModifiers::CONTROL);
        keys(&mut app, "wq");
        let line = status(&app);
        assert!(line.contains("Close pane? (y/n)"), "{line:?}");
        assert!(!line.contains("split right"), "{line:?}");
    }

    // Spec: Focus switches cwd.
    #[test]
    fn focus_switches_the_cwd_shown() {
        let mut app = split_app();
        app.update(AppEvent::Cwd(PaneId::FIRST, "/left".into()));
        app.update(AppEvent::Cwd(PaneId::for_test(2), "/right".into()));
        assert!(status(&app).contains("/right"));
        press(&mut app, ' ', KeyModifiers::CONTROL);
        keys(&mut app, "h");
        assert!(status(&app).contains("/left"));
    }

    fn zoom_toggled(app: &mut App) {
        press(app, ' ', KeyModifiers::CONTROL);
        keys(app, "wz");
    }

    // Spec: Zoomed / Unzoomed, end to end through the app.
    #[test]
    fn the_statusline_shows_z_only_while_zoomed() {
        let mut app = split_app();
        assert!(!status(&app).contains("[Z]"));
        zoom_toggled(&mut app);
        assert!(status(&app).contains("[Z]"), "{:?}", status(&app));
        zoom_toggled(&mut app);
        assert!(!status(&app).contains("[Z]"));
    }

    // Spec: Position indicator, end to end through the app.
    #[test]
    fn the_statusline_shows_the_copy_position_while_scrolling() {
        let mut app = App::new(100, 10);
        for i in 0..60 {
            let out = format!("line{i}\r\n").into_bytes();
            app.update(AppEvent::Pty(PaneId::FIRST, PtyEvent::Output(out)));
        }
        assert!(!status(&app).contains('\u{2191}'));
        press(&mut app, ' ', KeyModifiers::CONTROL);
        keys(&mut app, "[kk");
        let (offset, total) = app.copy_position().expect("history");
        assert_eq!(offset, 2);
        let line = status(&app);
        assert!(
            line.contains(&format!("COPY \u{2191}2/{total}")),
            "{line:?}"
        );
    }

    // Spec: a zoomed pane fills the body and draws no separator.
    #[test]
    fn a_zoomed_pane_fills_the_body_without_separators() {
        let mut app = split_app();
        zoom_toggled(&mut app);
        let (buf, _) = draw(&app);
        assert!(row(&buf, 0).starts_with("right"), "{:?}", row(&buf, 0));
        for y in 0..ROWS - 1 {
            assert!(!row(&buf, y).contains('\u{2502}'), "row {y}");
        }
    }

    fn separator_fg_after(keys_: &str) -> ratatui::style::Color {
        let mut app = split_app();
        press(&mut app, ' ', KeyModifiers::CONTROL);
        keys(&mut app, keys_);
        let sep = app.tiling().separators[0];
        let (buf, _) = draw(&app);
        cell_fg(&buf, sep.x, 0)
    }

    // Spec: Accent follows the input mode (RESIZE, COPY, confirmation).
    #[test]
    fn the_separator_accent_follows_resize_copy_and_confirmation() {
        let theme = theme::LazaroboxTheme::default();
        assert_eq!(separator_fg_after("wr"), theme.ai_purple);
        assert_eq!(separator_fg_after("["), theme.primary_cyan);
        assert_eq!(separator_fg_after("wq"), theme.error_red);
    }

    // Spec: Two panes clipped. A long line in the left pane must not spill over
    // the separator or the right pane.
    #[test]
    fn a_long_line_in_the_left_pane_leaves_the_separator_and_right_pane_intact() {
        let mut app = split_app();
        app.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"\r\n".iter().chain(&[b'x'; 200]).copied().collect()),
        ));
        let sep = app.tiling().separators[0];
        let (_, right) = app.tiling().panes[1];
        let (buf, _) = draw(&app);
        for y in 0..sep.len {
            assert_eq!(buf[(sep.x, y)].symbol(), "\u{2502}", "separator row {y}");
        }
        let right_text: String = (right.x..right.x + 5)
            .map(|x| buf[(x, 0)].symbol())
            .collect();
        assert_eq!(right_text, "right");
        assert!((right.x + 5..right.x + right.width).all(|x| buf[(x, 0)].symbol() == " "));
        assert!(row(&buf, 1).starts_with(&"x".repeat(usize::from(sep.x))));
    }

    fn prefixed(app: &mut App, keys: &str) {
        press(app, ' ', KeyModifiers::CONTROL);
        for c in keys.chars() {
            press(app, c, KeyModifiers::NONE);
        }
    }

    // Spec: Hidden with one tab.
    #[test]
    fn no_tab_bar_is_drawn_with_one_tab() {
        let (buf, _) = draw(&app());
        assert!(row(&buf, 0).starts_with("hi there"));
    }

    // Spec: Shown with two tabs; the body starts below the bar.
    #[test]
    fn the_tab_bar_is_drawn_on_the_top_row_with_two_tabs() {
        let mut app = app();
        prefixed(&mut app, "tn");
        app.update(AppEvent::Cwd(PaneId::FIRST, "/home/u/proj".into()));
        app.update(AppEvent::Cwd(PaneId::for_test(2), "/tmp".into()));
        app.update(AppEvent::Pty(
            PaneId::for_test(2),
            PtyEvent::Output(b"second".to_vec()),
        ));
        let (buf, _) = draw(&app);
        insta::assert_snapshot!(
            (0..ROWS)
                .map(|y| row(&buf, y))
                .collect::<Vec<_>>()
                .join("\n")
        );
        let theme = theme::LazaroboxTheme::default();
        assert_eq!(buf[(9, 0)].bg, theme.primary_cyan);
        assert_ne!(buf[(1, 0)].bg, theme.primary_cyan);
    }

    // Spec: Bar disappears on close.
    #[test]
    fn the_bar_disappears_when_one_tab_is_left() {
        let mut app = app();
        prefixed(&mut app, "tn");
        prefixed(&mut app, "tc");
        press(&mut app, 'y', KeyModifiers::NONE);
        let (buf, _) = draw(&app);
        assert!(row(&buf, 0).starts_with("hi there"), "{:?}", row(&buf, 0));
    }

    // Spec: Narrow width.
    #[test]
    fn many_tabs_keep_the_active_one_visible_and_never_panic() {
        let mut app = App::new(20, 6);
        for _ in 0..8 {
            prefixed(&mut app, "tn");
        }
        let mut terminal = Terminal::new(TestBackend::new(20, 6)).unwrap();
        terminal
            .draw(|frame| render(frame, &app, &theme::LazaroboxTheme::default()))
            .unwrap();
        let line = row(terminal.backend().buffer(), 0);
        assert!(line.contains('9'), "{line:?}");
        assert!(!line.contains('1'), "{line:?}");
    }

    fn configured(statusline: BarPosition, tabbar: BarPosition, tabs: usize) -> App {
        let config = Config {
            bars: BarPositions { statusline, tabbar },
            ..Config::default()
        };
        let mut app = app().with_config(&config);
        for _ in 1..tabs {
            for event in [
                KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL),
                KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE),
                KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
            ] {
                app.update(AppEvent::Key(event));
            }
        }
        app
    }

    fn rows(buf: &Buffer) -> Vec<String> {
        (0..ROWS).map(|y| row(buf, y)).collect()
    }

    // Spec: Bar positions.
    #[test]
    fn a_statusline_on_top_leaves_the_pane_below_it() {
        let app = configured(BarPosition::Top, BarPosition::Top, 1);
        let (buf, cursor) = draw(&app);
        let rows = rows(&buf);
        assert!(rows[0].contains("TERMINAL"), "{rows:?}");
        assert!(rows[1].starts_with("hi there"), "{rows:?}");
        assert!(!rows[ROWS as usize - 1].contains("TERMINAL"), "{rows:?}");
        // The cursor follows the body's new origin.
        assert_eq!(cursor, Position::new(8, 1));
    }

    #[test]
    fn both_bars_at_the_bottom_end_with_the_tab_bar() {
        let app = configured(BarPosition::Bottom, BarPosition::Bottom, 2);
        let (buf, cursor) = draw(&app);
        let rows = rows(&buf);
        let last = ROWS as usize - 1;
        assert!(
            rows[last].contains('1') && rows[last].contains('2'),
            "{rows:?}"
        );
        assert!(!rows[last].contains("TERMINAL"), "{rows:?}");
        assert!(rows[last - 1].contains("TERMINAL"), "{rows:?}");
        assert!(rows[0].trim().is_empty(), "{rows:?}");
        assert_eq!(cursor, Position::new(0, 0));
    }

    #[test]
    fn both_bars_on_top_stack_the_tab_bar_over_the_statusline() {
        let app = configured(BarPosition::Top, BarPosition::Top, 2);
        let (buf, cursor) = draw(&app);
        let rows = rows(&buf);
        assert!(rows[0].contains('1') && rows[0].contains('2'), "{rows:?}");
        assert!(rows[1].contains("TERMINAL"), "{rows:?}");
        assert_eq!(cursor, Position::new(0, 2));
    }

    #[test]
    fn the_statusline_on_top_and_the_tab_bar_at_the_bottom() {
        let app = configured(BarPosition::Top, BarPosition::Bottom, 2);
        let (buf, cursor) = draw(&app);
        let rows = rows(&buf);
        assert!(rows[0].contains("TERMINAL"), "{rows:?}");
        assert!(rows[ROWS as usize - 1].contains('2'), "{rows:?}");
        assert_eq!(cursor, Position::new(0, 1));
    }

    // Spec: Startup notice.
    #[test]
    fn the_startup_notice_shows_in_the_statusline_wherever_it_is() {
        let app = configured(BarPosition::Top, BarPosition::Top, 1)
            .with_notice(Some("config: bad value".into()));
        let (buf, _) = draw(&app);
        assert!(row(&buf, 0).contains("config: bad value"));
    }
}
