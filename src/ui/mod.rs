pub mod components;
pub mod preview;
pub mod theme;

use ratatui::Frame;

use crate::{
    app::{App, InputMode, PaneView},
    core::{ai::chat::streams::pick_stream, layout::Rect},
};
use components::{
    chat::ChatView,
    menu::MenuPopup,
    scroll_badge::ScrollBadge,
    separators::SeparatorView,
    statusline::{StatusLine, thinking::ThinkingView},
    tab_bar::TabBar,
    terminal_view::{TerminalView, cell_position, cursor_position},
};
use theme::LazaroboxTheme;

impl From<Rect> for ratatui::layout::Rect {
    fn from(rect: Rect) -> Self {
        ratatui::layout::Rect::new(rect.x, rect.y, rect.width, rect.height)
    }
}

/// Draws the tab bar (with several tabs), every pane in its tile, the
/// separators between them, the statusline and, in MENU, the popup. Only the focused pane places the real cursor.
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
        // The separators accent the focused pane, whatever its kind.
        if id == focused {
            focused_rect = rect;
        }
        let Some(view) = app.pane_view(id) else {
            continue;
        };
        if area.is_empty() {
            continue;
        }
        let pane = match view {
            PaneView::Pty(pane) => pane,
            PaneView::Chat(chat) => {
                let guidance = app.chat_guidance(id);
                let approval = app.approval_prompt(id);
                let chat_view = ChatView::new(chat, theme)
                    .guidance(guidance.as_deref())
                    .approval(approval.as_ref());
                // The composer owns the real cursor of a focused chat; MENU
                // hides it like it hides a shell's.
                // Chat COPY scrolls the transcript and edits nothing: no cursor.
                let cursor = (id == focused
                    && app.menu().is_none()
                    && !matches!(app.input(), InputMode::ChatCopy(_)))
                .then(|| chat_view.cursor(area))
                .flatten();
                frame.render_widget(chat_view, area);
                if let Some(offset) = app.scrolled_by(id) {
                    frame.render_widget(ScrollBadge::new(offset, theme), area);
                }
                if let Some(position) = cursor {
                    frame.set_cursor_position(position);
                }
                continue;
            }
        };
        let highlight = if id == focused {
            app.copy_highlight()
        } else {
            None
        };
        frame.render_widget(TerminalView::new(pane, theme).highlight(highlight), area);
        if let Some(offset) = app.scrolled_by(id) {
            frame.render_widget(ScrollBadge::new(offset, theme), area);
        }
        if id == focused {
            // MENU hides the real cursor: it would blink under the popup.
            // In COPY the real cursor is the COPY cursor, even if the child hid its own.
            let position = match app.input() {
                InputMode::Copy(_) => app
                    .copy_cursor()
                    .and_then(|(row, col)| cell_position(area, row, col)),
                _ => cursor_position(pane, area),
            };
            if app.menu().is_none()
                && let Some(position) = position
            {
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

    let streams = app.streams();
    let thinking = pick_stream(&streams, focused)
        .map(|(stream, others)| ThinkingView::new(stream, others, app.anim_frame(), app.now()));
    let right = app.status_right();
    let line = |path| {
        StatusLine::input(theme, app.input(), path, &right)
            .chat(app.focused_is_chat())
            .memory(app.status_memory())
            .update(app.status_update())
            .thinking(thinking.clone())
            .zoomed(app.zoomed())
            .copy_position(app.scroll_position())
            .marker(app.copy_selection_kind())
    };
    let path = app.status_path(line("").path_budget(status.width));
    frame.render_widget(line(&path), status);

    // The menu popup goes over everything else.
    if let Some(menu) = app.menu() {
        let popup = MenuPopup::new(menu, app.config(), theme);
        frame.render_widget(popup, frame.area());
    }
}

/// `rect` as a ratatui rect, cut to what the frame can actually draw.
fn within(frame: &Frame, rect: Rect) -> ratatui::layout::Rect {
    ratatui::layout::Rect::from(rect).intersection(frame.area())
}

#[cfg(test)]
mod chat_status_tests;
#[cfg(test)]
mod scroll_tests;
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

    // ai-chat C7: a chat pane draws its transcript and composer next to a
    // shell that still draws.
    #[test]
    fn a_chat_pane_draws_its_transcript_and_composer() {
        let mut app = app();
        app.open_chat(None);
        let id = PaneId::for_test(2);
        let (_, chat) = app.tiling().panes[1];
        app.chat_send(id, "question");
        app.update(AppEvent::Chat(
            id,
            1,
            crate::core::ai::chat::engine::StreamEvent::Delta("answer".into()),
        ));
        app.chat_type(id, "draft");
        let (buf, _) = draw(&app);
        let at = |y: u16| -> String {
            row(&buf, y)
                .chars()
                .skip(usize::from(chat.x))
                .collect::<String>()
                .trim_end()
                .to_owned()
        };
        assert!(row(&buf, 0).starts_with("hi there"));
        assert_eq!(at(0), "› question");
        assert_eq!(at(2), "answer");
        assert_eq!(at(chat.height - 1), "draft");
    }

    // 8.0: the composer owns the real cursor of a focused chat.
    #[test]
    fn a_focused_chat_places_the_cursor_in_its_composer() {
        let mut app = app();
        app.open_chat(None);
        let id = PaneId::for_test(2);
        let (_, chat) = app.tiling().panes[1];
        assert_eq!(app.focused(), id);
        assert_eq!(
            draw(&app).1,
            Position::new(chat.x, chat.y + chat.height - 1),
            "empty composer"
        );
        app.chat_type(id, "draft");
        assert_eq!(
            draw(&app).1,
            Position::new(chat.x + 5, chat.y + chat.height - 1)
        );
    }

    #[test]
    fn an_unfocused_chat_leaves_the_cursor_to_the_focused_shell() {
        let mut app = app();
        app.open_chat(None);
        app.chat_type(PaneId::for_test(2), "draft");
        press(&mut app, ' ', KeyModifiers::CONTROL);
        press(&mut app, 'h', KeyModifiers::NONE);
        assert_eq!(app.focused(), PaneId::FIRST);
        assert_eq!(draw(&app).1, Position::new(8, 0));
    }

    #[test]
    fn menu_hides_the_cursor_of_a_chat() {
        let mut app = app();
        app.open_chat(None);
        press(&mut app, ' ', KeyModifiers::CONTROL);
        press(&mut app, 'm', KeyModifiers::NONE);
        assert!(app.menu().is_some());
        assert_eq!(draw(&app).1, Position::new(0, 0));
    }

    // The separator next to a focused chat gets the accent, like any pane.
    #[test]
    fn the_separator_next_to_a_focused_chat_gets_the_accent() {
        let mut app = app();
        app.open_chat(None);
        let (_, chat) = app.tiling().panes[1];
        let (buf, _) = draw(&app);
        let theme = theme::LazaroboxTheme::default();
        let cell = &buf[(chat.x - 1, 0)];
        assert_eq!(cell.fg, theme.input_accent(app.input()));
        assert_ne!(cell.fg, theme.text_muted);
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

    // copy-mode-scrollback: Block follows the COPY cursor.
    #[test]
    fn the_real_cursor_follows_the_copy_cursor_in_copy() {
        let mut app = app();
        assert_eq!(
            draw(&app).1,
            Position::new(8, 0),
            "child cursor in TERMINAL"
        );
        press(&mut app, ' ', KeyModifiers::CONTROL);
        keys(&mut app, "[jjhhhh");
        assert_eq!(app.copy_cursor(), Some((2, 4)));
        assert_eq!(draw(&app).1, Position::new(4, 2));
    }

    #[test]
    fn the_copy_cursor_is_shown_even_when_the_child_hid_its_cursor() {
        let mut app = app();
        app.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"\x1b[?25l".to_vec()),
        ));
        press(&mut app, ' ', KeyModifiers::CONTROL);
        keys(&mut app, "[");
        assert_eq!(draw(&app).1, Position::new(0, ROWS - 2));
        press(&mut app, 'q', KeyModifiers::NONE);
        assert_eq!(
            draw(&app).1,
            Position::new(0, 0),
            "hidden again in TERMINAL"
        );
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
            line.contains(
                "w window · t tab · g go · b buffer · [ copy · m menu · c chat · n note · q quit"
            ),
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
        // The COPY cursor starts on the bottom of 9 body rows: 8 moves reach the top,
        // 2 more scroll.
        keys(&mut app, "[kkkkkkkkkk");
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

    fn draw_at(app: &App, cols: u16, rows: u16) -> (Buffer, Position) {
        let mut terminal = Terminal::new(TestBackend::new(cols, rows)).unwrap();
        terminal
            .draw(|frame| render(frame, app, &theme::LazaroboxTheme::default()))
            .unwrap();
        let cursor = terminal.get_cursor_position().unwrap();
        (terminal.backend().buffer().clone(), cursor)
    }

    fn menu_app(cols: u16, rows: u16) -> App {
        let mut app = App::new(cols, rows);
        app.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"hi there".to_vec()),
        ));
        press(&mut app, ' ', KeyModifiers::CONTROL);
        press(&mut app, 'm', KeyModifiers::NONE);
        assert_eq!(app.input(), InputMode::Menu);
        app
    }

    // Spec: tabs: Bar forced with one tab (render); the popup is drawn last.
    #[test]
    fn menu_popup_is_drawn_over_the_panes_after_the_bars() {
        // 24 rows: the popup with its AI section is 22 rows high, centred.
        let app = menu_app(64, 24);
        let (buf, _) = draw_at(&app, 64, 24);
        assert!(row(&buf, 0).contains(" 1 "), "{:?}", row(&buf, 0));
        assert!(row(&buf, 1).contains(" Menu "), "{:?}", row(&buf, 1));
        assert!(row(&buf, 23).contains("MENU"), "{:?}", row(&buf, 23));
        insta::assert_snapshot!(
            (0..24)
                .map(|y| row(&buf, y).trim_end().to_string())
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    #[test]
    fn a_known_update_marks_the_statusline_and_the_menu_note_fits_a_real_frame() {
        let mut app = App::new(80, 25).with_update(Some("0.16.0".into()));
        let line = status(&app);
        assert!(line.contains("update 0.16.0"), "{line:?}");
        insta::assert_snapshot!("update_statusline", line.trim_end());

        press(&mut app, ' ', KeyModifiers::CONTROL);
        press(&mut app, 'm', KeyModifiers::NONE);
        let (buf, _) = draw_at(&app, 80, 25);
        let frame = (0..25)
            .map(|y| row(&buf, y).trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(frame.contains("lazarobox 0.16.0 is available."), "{frame}");
        assert!(frame.contains("Go to Updates and press Enter."), "{frame}");
        insta::assert_snapshot!("update_menu_frame", frame);
    }

    #[test]
    fn without_an_update_the_statusline_has_no_mark() {
        let app = App::new(80, 24);
        assert!(!status(&app).contains("update"));
    }

    #[test]
    fn real_cursor_is_not_placed_while_the_menu_is_open() {
        let mut app = menu_app(60, 12);
        let (_, cursor) = draw_at(&app, 60, 12);
        // Nothing placed it, so it stays at the backend default.
        assert_eq!(cursor, Position::new(0, 0));
        // Characterization: closing the menu gives the cursor back (the
        // forced bar is gone, so the pane starts at row 0 again).
        app.update(AppEvent::Key(KeyEvent::new(
            KeyCode::Esc,
            KeyModifiers::NONE,
        )));
        let (_, cursor) = draw_at(&app, 60, 12);
        assert_eq!(cursor, Position::new(8, 0));
    }

    // Spec: Tiny terminals: the minimal message, and Esc still works.
    #[test]
    fn a_tiny_terminal_shows_the_fallback_and_esc_still_reverts() {
        let mut app = menu_app(20, 5);
        let (buf, _) = draw_at(&app, 20, 5);
        assert_eq!(row(&buf, 2), "MENU · s save · Esc…");
        app.update(AppEvent::Key(KeyEvent::new(
            KeyCode::Esc,
            KeyModifiers::NONE,
        )));
        assert_eq!(app.input(), InputMode::Terminal);
        assert!(app.menu().is_none());
        let (buf, _) = draw_at(&app, 20, 5);
        assert!(!row(&buf, 2).contains("MENU"));
    }

    fn reversed_cells(buf: &Buffer, y: u16) -> Vec<u16> {
        (0..COLS)
            .filter(|&x| buf[(x, y)].modifier == ratatui::style::Modifier::REVERSED)
            .collect()
    }

    // statusline: Marker with indicator, end to end through the app.
    #[test]
    fn the_statusline_shows_the_selection_marker_after_the_position() {
        let mut app = App::new(100, 10);
        for i in 0..60 {
            let out = format!("line{i}\r\n").into_bytes();
            app.update(AppEvent::Pty(PaneId::FIRST, PtyEvent::Output(out)));
        }
        press(&mut app, ' ', KeyModifiers::CONTROL);
        keys(&mut app, "[kkkkkkkkkk");
        keys(&mut app, "v");
        let (offset, total) = app.copy_position().expect("history");
        let line = status(&app);
        assert!(
            line.contains(&format!("COPY \u{2191}{offset}/{total} VISUAL ")),
            "{line:?}"
        );
        keys(&mut app, "v");
        let line = status(&app);
        assert!(!line.contains("VISUAL"), "{line:?}");
        keys(&mut app, "V");
        assert!(status(&app).contains("V-LINE"), "{:?}", status(&app));
    }

    // statusline: Line marker without history (alternate screen).
    #[test]
    fn the_alternate_screen_shows_the_line_marker_without_an_indicator() {
        let mut app = App::new(100, 10);
        app.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"\x1b[?1049h".to_vec()),
        ));
        press(&mut app, ' ', KeyModifiers::CONTROL);
        keys(&mut app, "[V");
        let line = status(&app);
        assert!(line.contains("COPY V-LINE "), "{line:?}");
        assert!(!line.contains('\u{2191}'), "{line:?}");
    }

    // copy-selection: Character highlight, through the whole render.
    #[test]
    fn the_focused_pane_paints_the_selection_and_the_other_does_not() {
        let mut app = split_app();
        press(&mut app, ' ', KeyModifiers::CONTROL);
        keys(&mut app, "[0vll");
        let (buf, _) = draw(&app);
        let right = app.tiling().panes[1].1;
        assert_eq!(
            reversed_cells(&buf, 0),
            vec![right.x, right.x + 1, right.x + 2]
        );
        assert_eq!(app.focused(), PaneId::for_test(2));
    }

    // copy-selection: the highlight follows a scrolled viewport.
    #[test]
    fn the_highlight_lands_on_the_selected_rows_of_a_scrolled_viewport() {
        let mut app = App::new(COLS, ROWS);
        for i in 0..60 {
            let out = format!("line{i}\r\n").into_bytes();
            app.update(AppEvent::Pty(PaneId::FIRST, PtyEvent::Output(out)));
        }
        press(&mut app, ' ', KeyModifiers::CONTROL);
        keys(&mut app, "[kkkkkkkkkkkkvj");
        let (buf, _) = draw(&app);
        let rows: Vec<(u16, String)> = (0..ROWS)
            .filter(|&y| !reversed_cells(&buf, y).is_empty())
            .map(|y| (y, row(&buf, y).trim_end().to_string()))
            // In COPY the scrolled pane (offset 8 here) draws its `[+8]` badge
            // over the end of row 0; it is an overlay, not pane text, so the
            // test compares the pane's own text only.
            .map(|(y, text)| (y, text.trim_end_matches("[+8]").trim_end().to_string()))
            .collect();
        assert_eq!(
            rows,
            vec![(0, "line48".to_string()), (1, "line49".to_string())]
        );
    }

    #[test]
    fn leaving_copy_removes_the_highlight() {
        let mut app = split_app();
        press(&mut app, ' ', KeyModifiers::CONTROL);
        keys(&mut app, "[0vll");
        keys(&mut app, "q");
        let (buf, _) = draw(&app);
        assert_eq!(reversed_cells(&buf, 0), Vec::<u16>::new());
    }
}
