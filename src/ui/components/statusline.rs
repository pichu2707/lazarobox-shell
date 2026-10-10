use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph, Widget},
};

use crate::app::{AppMode, InputMode};
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

/// Powerline-style status line: `[ mode ][ path ]` on the left, `[ right ]`
/// on the right, with a flexible `bg_base` fill in between.
pub struct StatusLine<'a> {
    theme: &'a LazaroboxTheme,
    label: &'a str,
    accent: Color,
    path: &'a str,
    right: &'a str,
    zoomed: bool,
    copy_position: Option<(usize, usize)>,
}

impl<'a> StatusLine<'a> {
    pub fn new(theme: &'a LazaroboxTheme, mode: AppMode, path: &'a str, model: &'a str) -> Self {
        Self {
            theme,
            label: mode.label(),
            accent: theme.accent(mode),
            path,
            right: model,
            zoomed: false,
            copy_position: None,
        }
    }

    /// Status line for an input mode (TERMINAL, PREFIX, COPY, quit prompt).
    pub fn input(
        theme: &'a LazaroboxTheme,
        mode: InputMode,
        path: &'a str,
        right: &'a str,
    ) -> Self {
        Self {
            theme,
            label: mode.label(),
            accent: theme.input_accent(mode),
            path,
            right,
            zoomed: false,
            copy_position: None,
        }
    }

    /// Marks the active tab as zoomed (`[Z]` right after the mode label).
    pub fn zoomed(mut self, zoomed: bool) -> Self {
        self.zoomed = zoomed;
        self
    }

    /// COPY viewport position `(rows above the live bottom, history rows)`,
    /// shown as `↑offset/total` right after the mode label.
    pub fn copy_position(mut self, position: Option<(usize, usize)>) -> Self {
        self.copy_position = position;
        self
    }

    /// Columns left for the path text in a statusline `width` columns wide:
    /// what the mode block, the right segment (when it fits) and the path
    /// padding and slant leave over.
    pub fn path_budget(&self, width: u16) -> u16 {
        // Padding on both sides of the path plus the slant that closes it.
        const PATH_DECORATION: usize = 3;
        let left = self.mode_block_width() + PATH_DECORATION;
        let right = self.right_line().width();
        let right = if right + self.mode_block_width() <= usize::from(width) {
            right
        } else {
            0
        };
        u16::try_from(usize::from(width).saturating_sub(left + right)).unwrap_or(u16::MAX)
    }

    fn mode_block_text(&self) -> String {
        let zoom = if self.zoomed { " [Z]" } else { "" };
        let position = self
            .copy_position
            .map(|(offset, total)| format!(" \u{2191}{offset}/{total}"))
            .unwrap_or_default();
        format!(" \u{25D0} {}{position}{zoom} ", self.label)
    }

    /// Columns taken by the mode block and the slant that closes it.
    fn mode_block_width(&self) -> usize {
        Line::from(self.mode_block_text()).width() + 1
    }

    fn left_line(&self) -> Line<'a> {
        let t = self.theme;
        let mode_style = Style::new().bg(self.accent).fg(t.bg_base);
        Line::from(vec![
            Span::styled(
                self.mode_block_text(),
                mode_style.add_modifier(Modifier::BOLD),
            ),
            Span::styled(SLANT_RIGHT, Style::new().fg(self.accent).bg(t.bg_panel)),
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
                format!(" {} ", self.right),
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
        // The mode block has priority: the right segment is dropped when it
        // would not fit next to it.
        let right_width = if right.width() + self.mode_block_width() <= usize::from(area.width) {
            u16::try_from(right.width()).unwrap_or(u16::MAX)
        } else {
            0
        };
        let [left_area, right_area] =
            Layout::horizontal([Constraint::Min(0), Constraint::Length(right_width)]).areas(area);

        Paragraph::new(self.left_line()).render(left_area, buf);
        Paragraph::new(right).render(right_area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Confirm;
    use crate::core::{
        copy::CopyState,
        prefix::{self, PREFIX_TREE, Step},
    };
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn group(c: char) -> InputMode {
        let key = KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        match prefix::lookup(PREFIX_TREE, &key) {
            Step::Enter(group) => InputMode::Group(group),
            other => panic!("{c} is not a group: {other:?}"),
        }
    }
    use ratatui::{Terminal, backend::TestBackend};

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
        let left_w = Line::from(" \u{25D0} NORMAL ").width() + 1 + (PATH.chars().count() + 2) + 1;
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

    const SHELL: &str = "zsh";

    fn input_modes() -> [InputMode; 8] {
        [
            InputMode::Terminal,
            InputMode::Prefix,
            InputMode::Copy(CopyState::default()),
            InputMode::Confirm(Confirm::Quit),
            group('w'),
            group('g'),
            InputMode::Resize,
            group('b'),
        ]
    }

    fn render_input_at(mode: InputMode, width: u16) -> Buffer {
        let theme = LazaroboxTheme::default();
        let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
        terminal
            .draw(|frame| {
                frame.render_widget(StatusLine::input(&theme, mode, PATH, SHELL), frame.area())
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn text_of(buf: &Buffer) -> String {
        (0..buf.area.width).map(|x| buf[(x, 0)].symbol()).collect()
    }

    #[test]
    fn input_label_is_rendered_per_mode() {
        let expected = [
            (InputMode::Terminal, "TERMINAL"),
            (InputMode::Prefix, "PREFIX"),
            (InputMode::Copy(CopyState::default()), "COPY"),
            (InputMode::Confirm(Confirm::Quit), "Quit? (y/n)"),
            (group('w'), "WINDOW"),
            (group('t'), "TAB"),
            (group('g'), "GO"),
            (group('b'), "BUFFER"),
            (InputMode::Resize, "RESIZE"),
        ];
        for (mode, label) in expected {
            let text = text_of(&render_input_at(mode, WIDTH));
            assert!(text.contains(label), "{label:?} in {text:?}");
        }
    }

    #[test]
    fn input_block_uses_the_input_mode_color_and_bold() {
        let t = LazaroboxTheme::default();
        for mode in input_modes() {
            let buf = render_input_at(mode, WIDTH);
            let cell = &buf[(1, 0)];
            assert_eq!(cell.bg, t.input_accent(mode), "bg for {mode:?}");
            assert_eq!(cell.fg, t.bg_base, "fg for {mode:?}");
            assert!(cell.modifier.contains(Modifier::BOLD), "bold for {mode:?}");
        }
    }

    #[test]
    fn input_block_slant_continues_the_mode_color() {
        let t = LazaroboxTheme::default();
        for mode in input_modes() {
            let buf = render_input_at(mode, WIDTH);
            let slant_x = (0..WIDTH)
                .find(|&x| buf[(x, 0)].symbol() == SLANT_RIGHT)
                .unwrap();
            assert_eq!(buf[(slant_x, 0)].fg, t.input_accent(mode), "{mode:?}");
        }
    }

    #[test]
    fn input_shows_path_and_right_segment() {
        let buf = render_input_at(InputMode::Terminal, WIDTH);
        let text = text_of(&buf);
        assert!(text.contains(PATH));
        assert!(text.ends_with(&format!("{SHELL} ")));
    }

    #[test]
    fn narrow_width_keeps_the_mode_block_and_drops_the_right_segment() {
        // " ◐ TERMINAL " (12) + slant (1) leaves no room for path or shell.
        let buf = render_input_at(InputMode::Terminal, 14);
        let text = text_of(&buf);
        assert!(text.contains("TERMINAL"), "{text:?}");
        assert!(!text.contains(SHELL), "{text:?}");
    }

    #[test]
    fn degenerate_widths_do_not_panic() {
        for width in [0, 1, 2, 5] {
            for mode in input_modes() {
                render_input_at(mode, width);
            }
        }
    }

    #[test]
    fn snapshot_input_terminal() {
        insta::assert_snapshot!(text_of(&render_input_at(InputMode::Terminal, WIDTH)));
    }

    #[test]
    fn snapshot_input_prefix() {
        insta::assert_snapshot!(text_of(&render_input_at(InputMode::Prefix, WIDTH)));
    }

    #[test]
    fn snapshot_input_copy() {
        let mode = InputMode::Copy(CopyState::default());
        insta::assert_snapshot!(text_of(&render_input_at(mode, WIDTH)));
    }

    #[test]
    fn snapshot_input_confirm_quit() {
        insta::assert_snapshot!(text_of(&render_input_at(
            InputMode::Confirm(Confirm::Quit),
            WIDTH
        )));
    }

    #[test]
    fn snapshot_input_group_window() {
        insta::assert_snapshot!(text_of(&render_input_at(group('w'), WIDTH)));
    }

    #[test]
    fn snapshot_input_group_tab() {
        insta::assert_snapshot!(text_of(&render_input_at(group('t'), WIDTH)));
    }

    #[test]
    fn snapshot_input_group_go() {
        insta::assert_snapshot!(text_of(&render_input_at(group('g'), WIDTH)));
    }

    #[test]
    fn snapshot_input_group_buffer() {
        insta::assert_snapshot!(text_of(&render_input_at(group('b'), WIDTH)));
    }

    #[test]
    fn snapshot_input_resize() {
        insta::assert_snapshot!(text_of(&render_input_at(InputMode::Resize, WIDTH)));
    }

    #[test]
    fn snapshot_input_narrow() {
        insta::assert_snapshot!(text_of(&render_input_at(InputMode::Terminal, 14)));
    }

    fn render_zoomed(mode: InputMode, zoomed: bool) -> Buffer {
        let theme = LazaroboxTheme::default();
        let mut terminal = Terminal::new(TestBackend::new(WIDTH, 1)).unwrap();
        terminal
            .draw(|frame| {
                let line = StatusLine::input(&theme, mode, PATH, SHELL).zoomed(zoomed);
                frame.render_widget(line, frame.area())
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    // Spec: Zoomed; the indicator sits in the mode block, before the path.
    #[test]
    fn the_zoom_indicator_follows_the_mode_label() {
        let t = LazaroboxTheme::default();
        for mode in input_modes() {
            let buf = render_zoomed(mode, true);
            let text = text_of(&buf);
            let label = text.find(mode.label()).unwrap();
            let zoom = text
                .find("[Z]")
                .unwrap_or_else(|| panic!("no [Z] in {text:?}"));
            assert!(label < zoom && zoom < text.find(PATH).unwrap(), "{text:?}");
            let x = text[..zoom].chars().count() as u16;
            assert_eq!(buf[(x, 0)].bg, t.input_accent(mode), "{mode:?}");
        }
    }

    // Spec: Unzoomed.
    #[test]
    fn the_zoom_indicator_is_absent_when_not_zoomed() {
        for mode in input_modes() {
            let text = text_of(&render_zoomed(mode, false));
            assert!(!text.contains("[Z]"), "{text:?}");
        }
    }

    #[test]
    fn the_path_budget_accounts_for_the_zoom_indicator() {
        let theme = LazaroboxTheme::default();
        let plain = StatusLine::input(&theme, InputMode::Terminal, "", SHELL);
        let zoomed = StatusLine::input(&theme, InputMode::Terminal, "", SHELL).zoomed(true);
        assert_eq!(zoomed.path_budget(WIDTH) + 4, plain.path_budget(WIDTH));
    }

    fn render_copy(position: Option<(usize, usize)>, width: u16) -> Buffer {
        let theme = LazaroboxTheme::default();
        let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
        terminal
            .draw(|frame| {
                let mode = InputMode::Copy(CopyState::default());
                let line = StatusLine::input(&theme, mode, PATH, SHELL).copy_position(position);
                frame.render_widget(line, frame.area())
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    // Spec: Position indicator; sits in the mode block, before the path.
    #[test]
    fn the_copy_position_follows_the_mode_label_in_the_mode_block() {
        let t = LazaroboxTheme::default();
        let buf = render_copy(Some((12, 340)), WIDTH);
        let text = text_of(&buf);
        let at = text
            .find("COPY \u{2191}12/340")
            .unwrap_or_else(|| panic!("{text:?}"));
        assert!(at < text.find(PATH).unwrap(), "{text:?}");
        let x = text[..at].chars().count() as u16 + 8;
        assert_eq!(
            buf[(x, 0)].bg,
            t.input_accent(InputMode::Copy(CopyState::default()))
        );
        insta::assert_snapshot!(text);
    }

    // Spec: Bottom of the history shows an explicit zero.
    #[test]
    fn the_copy_position_shows_an_explicit_zero_offset() {
        let text = text_of(&render_copy(Some((0, 340)), WIDTH));
        assert!(text.contains("COPY \u{2191}0/340"), "{text:?}");
    }

    // Spec: No history shows the bare label.
    #[test]
    fn no_copy_position_leaves_the_bare_label() {
        let text = text_of(&render_copy(None, WIDTH));
        assert!(text.contains("COPY "), "{text:?}");
        assert!(!text.contains('\u{2191}'), "{text:?}");
    }

    #[test]
    fn the_path_budget_accounts_for_the_copy_position() {
        let theme = LazaroboxTheme::default();
        let mode = InputMode::Copy(CopyState::default());
        let plain = StatusLine::input(&theme, mode, "", SHELL);
        let placed = StatusLine::input(&theme, mode, "", SHELL).copy_position(Some((12, 340)));
        // " \u{2191}12/340" is 8 columns wide.
        assert_eq!(placed.path_budget(WIDTH) + 8, plain.path_budget(WIDTH));
    }

    // Spec: Narrow width never panics.
    #[test]
    fn a_narrow_copy_statusline_never_panics() {
        for width in 0..40 {
            for position in [
                None,
                Some((0, 0)),
                Some((12, 340)),
                Some((usize::MAX, usize::MAX)),
            ] {
                render_copy(position, width);
            }
        }
    }
}
