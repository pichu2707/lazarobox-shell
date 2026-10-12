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
        ai::picker::{Mark, VIEW},
        config::Config,
        menu::{Footer, MenuState, Row},
    },
    ui::theme::LazaroboxTheme,
};

/// Widest popup wanted without an AI section.
pub const WANT_WIDTH: u16 = 51;
/// With the AI section the `r refresh` hint needs eleven more columns.
pub const WANT_WIDTH_AI: u16 = 62;
/// Rows around the body: two borders, the blank row and the footer.
const CHROME: u16 = 4;
/// With the AI section the body is fixed: the popup never jumps between the
/// task list and the picker (a title and `VIEW` rows).
const AI_BODY: usize = 1 + VIEW;
/// Smallest width that still shows the full popup.
pub const MIN_WIDTH: u16 = 24;

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

/// Whether `area` is big enough for the full popup of `rows` body rows (the
/// blank row is the first thing dropped, so the height may lose one).
pub fn fits(area: Rect, rows: usize) -> bool {
    area.width >= MIN_WIDTH && usize::from(area.height) >= rows + usize::from(CHROME) - 1
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

const FALLBACK: &str = "MENU · s save · Esc cancel";

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
        let width = usize::from(width);
        match row {
            Row::Title(title) => (format!(" {title}"), plain.add_modifier(Modifier::BOLD)),
            &Row::Item {
                label,
                ref value,
                selected,
                enabled,
            } => {
                let value = value.as_deref().unwrap_or("");
                // The label and the single spaces around the row stay whole;
                // the value gives way, but never its trailing mark.
                let room = width.saturating_sub(cells(label) + 4);
                let value = fit(value, room);
                let gap = width
                    .saturating_sub(cells(label) + cells(&value) + 2)
                    .max(1);
                let text = format!(" {label}{}{value} ", " ".repeat(gap));
                (text, self.row_style(selected, enabled))
            }
            Row::Pick {
                label,
                mark,
                selected,
                enabled,
                current,
            } => {
                let tail = match (mark_text(mark), current) {
                    ("", false) => String::new(),
                    ("", true) => " ●".to_owned(),
                    (mark, false) => format!(" {mark}"),
                    (mark, true) => format!(" {mark} ●"),
                };
                let room = width.saturating_sub(cells(&tail) + 1);
                let label = if cells(label) > room {
                    clip(label, room)
                } else {
                    label.clone()
                };
                (
                    format!(" {label}{tail}"),
                    self.row_style(*selected, *enabled),
                )
            }
            Row::Field(text) => (format!(" {text}"), plain),
        }
    }

    /// Selected rows are reversed; a disabled row is muted, and stays muted
    /// when the cursor lands on it.
    fn row_style(&self, selected: bool, enabled: bool) -> Style {
        let base = if enabled {
            Style::default().fg(self.theme.info_blue)
        } else {
            Style::default().fg(self.theme.text_muted)
        };
        match (selected, enabled) {
            (true, _) => base.add_modifier(Modifier::REVERSED),
            (false, true) => Style::default(),
            (false, false) => base,
        }
    }
}

/// The text a picker mark adds after the label.
fn mark_text(mark: &Mark) -> &str {
    match mark {
        Mark::Plain => "",
        Mark::Downloaded => "✓",
        Mark::DownloadRequired => "download required",
        Mark::Checking => "checking…",
        Mark::Status(text) => text,
    }
}

/// Suffix of a task-row value that must survive clipping.
const REQUIRED_MARK: &str = " · download required";

/// `value` within `room` cells: a long model name is clipped, its
/// ` · download required` mark is kept whole.
fn fit(value: &str, room: usize) -> String {
    if cells(value) <= room {
        return value.to_owned();
    }
    match value.strip_suffix(REQUIRED_MARK) {
        Some(head) if room > cells(REQUIRED_MARK) => {
            format!("{}{REQUIRED_MARK}", clip(head, room - cells(REQUIRED_MARK)))
        }
        _ => clip(value, room),
    }
}

impl Widget for MenuPopup<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let area = area.intersection(buf.area);
        if area.is_empty() {
            return;
        }
        let rows = self.menu.rows(self.config);
        let body = if self.menu.has_ai() {
            AI_BODY.max(rows.len())
        } else {
            rows.len()
        };
        if !fits(area, body) {
            self.render_fallback(area, buf);
            return;
        }
        let want_width = if self.menu.has_ai() {
            WANT_WIDTH_AI
        } else {
            WANT_WIDTH
        };
        let want_height = body as u16 + CHROME;
        let popup = popup_area(area, want_width, want_height);
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

        // Body rows, then (height permitting) a blank row, then the footer.
        let blank = usize::from(inner.height) > body + 1;
        let width = usize::from(inner.width);
        for (offset, row) in rows.iter().enumerate() {
            let (text, style) = self.line(row, inner.width);
            let y = inner.y + offset as u16;
            if y >= inner.bottom() {
                break;
            }
            buf.set_style(Rect::new(inner.x, y, inner.width, 1), style);
            buf.set_stringn(inner.x, y, clip(&text, width), width, style);
        }
        let footer_y = inner.y + body as u16 + u16::from(blank);
        if footer_y < inner.bottom() {
            let (text, style) = match self.menu.footer() {
                Footer::Error(error) => {
                    (error.to_owned(), Style::default().fg(self.theme.error_red))
                }
                Footer::Hint(hint) => (
                    hint.into_owned(),
                    Style::default().fg(self.theme.text_muted),
                ),
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
        assert!(fits(r(24, 7), 4));
        assert!(!fits(r(23, 7), 4));
        assert!(!fits(r(24, 6), 4));
        assert!(fits(r(200, 50), 4));
        assert!(!fits(r(0, 0), 4));
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
        assert_eq!(lines[2].trim_end(), "MENU · s save · Esc cancel");
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
        assert_eq!(lines[3], "MENU · s save · Esc ca…");
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

    // Spec: AI section.
    use crate::core::ai::{CATALOG, Detection, EngineState, Status, config::AiConfig};

    fn machine() -> Detection {
        Detection {
            engines: CATALOG
                .iter()
                .map(|e| EngineState {
                    id: e.id,
                    status: if e.id == "ollama" {
                        Status::Available
                    } else {
                        Status::NotInstalled
                    },
                    models: if e.id == "ollama" {
                        vec!["llama3:8b".to_owned(), "qwen2:7b".to_owned()]
                    } else {
                        Vec::new()
                    },
                    version: None,
                })
                .collect(),
        }
    }

    fn with_ai(model: &str) -> MenuState {
        let saved = AiConfig::general(Some("ollama"), Some(model));
        fresh().with_ai(saved, Some(machine()))
    }

    fn lines(buf: &Buffer) -> Vec<String> {
        text(buf).lines().map(str::to_owned).collect()
    }

    #[test]
    fn the_popup_height_follows_the_row_count() {
        assert!(fits(r(24, 11), 8));
        assert!(!fits(r(24, 10), 8));
        assert!(!fits(r(23, 11), 8));
        assert!(fits(r(24, 7), 4));
        assert!(!fits(r(24, 6), 4));
    }

    #[test]
    fn ai_popups_never_panic_at_any_size() {
        let mut menu = with_ai("llama3:8b");
        for pass in 0..2 {
            for w in 0..=40 {
                for h in 0..=20 {
                    draw(w, h, &menu);
                }
            }
            if pass == 0 {
                menu.set_error("config.toml: ai is not a table; fix it before saving".into());
            }
        }
    }

    #[test]
    fn below_the_ai_height_the_fallback_is_drawn() {
        let buf = draw(62, 10, &with_ai("llama3:8b"));
        assert_eq!(lines(&buf)[5].trim_end(), "MENU · s save · Esc cancel");
    }

    // Spec: Picker viewport; Tiny terminals; Footer; Detection cues.
    use crate::core::ai::config::{Assignment, Task};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn rich() -> Detection {
        let ollama: Vec<String> = (1..=8).map(|n| format!("model{n}:7b")).collect();
        Detection {
            engines: CATALOG
                .iter()
                .map(|e| EngineState {
                    id: e.id,
                    status: match e.id {
                        "ollama" | "claude" => Status::Available,
                        "lmstudio" => Status::NotRunning,
                        _ => Status::NotInstalled,
                    },
                    models: if e.id == "ollama" {
                        ollama.clone()
                    } else {
                        Vec::new()
                    },
                    version: (e.id == "claude").then(|| "2.1.0".to_owned()),
                })
                .collect(),
        }
    }

    fn mixed() -> MenuState {
        let mut saved = AiConfig::general(Some("ollama"), Some("model1:7b"));
        saved.set(
            Task::Code,
            Assignment {
                engine: Some("claude".to_owned()),
                model: Some("opus".to_owned()),
            },
        );
        fresh().with_ai(saved, Some(rich()))
    }

    fn press(menu: &mut MenuState, codes: &[KeyCode]) {
        let mut config = Config::default();
        for code in codes {
            menu.on_key(
                &KeyEvent::new(*code, KeyModifiers::NONE),
                false,
                &mut config,
            );
        }
    }

    fn goto(menu: &mut MenuState, label: &str) {
        for _ in 0..20 {
            let hit = menu.rows(&Config::default()).iter().any(
                |row| matches!(row, Row::Item { label: l, selected: true, .. } if *l == label),
            );
            if hit {
                return;
            }
            press(menu, &[KeyCode::Char('j')]);
        }
        panic!("no row {label}");
    }

    fn picker_of(task: &str, downs: usize) -> MenuState {
        let mut menu = mixed();
        goto(&mut menu, task);
        press(&mut menu, &[KeyCode::Enter]);
        press(&mut menu, &vec![KeyCode::Char('j'); downs]);
        menu
    }

    #[test]
    fn snapshot_ai_task_list_mixed_default_and_not_set() {
        insta::assert_snapshot!(text(&draw(62, 15, &mixed())));
    }

    #[test]
    fn snapshot_level_one_lists_the_engines() {
        insta::assert_snapshot!(text(&draw(62, 15, &picker_of("code", 0))));
    }

    #[test]
    fn snapshot_ollama_scrolled_to_a_disabled_last_row() {
        let mut menu = picker_of("general", 0);
        press(&mut menu, &[KeyCode::Enter]);
        let top = draw(62, 15, &menu);
        press(&mut menu, &[KeyCode::Char('k')]);
        let buf = draw(62, 15, &menu);
        let shown = text(&buf);
        assert!(shown.contains("18/18"), "{shown}");
        assert!(
            shown.contains("download required · not selectable"),
            "{shown}"
        );
        let rows = |b: &Buffer| lines(b).iter().filter(|l| l.contains('│')).count();
        assert_eq!(rows(&buf), rows(&top));
        insta::assert_snapshot!(shown);
    }

    #[test]
    fn the_cursor_on_a_disabled_row_is_muted_and_reversed() {
        let mut menu = picker_of("general", 0);
        press(&mut menu, &[KeyCode::Enter, KeyCode::Char('k')]);
        let buf = draw(62, 15, &menu);
        let theme = LazaroboxTheme::default();
        let y = (0..15)
            .find(|y| buf[(4, *y)].modifier.contains(Modifier::REVERSED))
            .expect("the cursor row is reversed");
        assert!(lines(&buf)[usize::from(y)].contains("download required"));
        assert_eq!(buf[(4, y)].fg, theme.text_muted);
    }

    #[test]
    fn snapshot_cli_models_end_with_custom() {
        let mut menu = picker_of("general", 1);
        press(&mut menu, &[KeyCode::Enter, KeyCode::Char('k')]);
        insta::assert_snapshot!(text(&draw(62, 15, &menu)));
    }

    #[test]
    fn snapshot_custom_entry() {
        let mut menu = picker_of("general", 1);
        press(
            &mut menu,
            &[KeyCode::Enter, KeyCode::Char('k'), KeyCode::Enter],
        );
        press(&mut menu, &[KeyCode::Char('m'), KeyCode::Char('y')]);
        insta::assert_snapshot!(text(&draw(62, 15, &menu)));
    }

    #[test]
    fn snapshot_checking_cue_before_the_first_detection() {
        let saved = AiConfig::general(Some("ollama"), Some("llama3:8b"));
        let mut menu = fresh().with_ai(saved, None);
        goto(&mut menu, "code");
        insta::assert_snapshot!(text(&draw(62, 15, &menu)));
    }

    #[test]
    fn snapshot_refreshing_cue() {
        let mut menu = mixed();
        goto(&mut menu, "code");
        menu.set_checking(true);
        insta::assert_snapshot!(text(&draw(62, 15, &menu)));
    }

    #[test]
    fn the_ai_popup_is_62_by_15_in_both_views() {
        for menu in [mixed(), picker_of("code", 0)] {
            let l = lines(&draw(80, 20, &menu));
            let border: Vec<usize> = (0..l.len())
                .filter(|y| l[*y].contains('┌') || l[*y].contains('└'))
                .collect();
            assert_eq!(border.len(), 2);
            assert_eq!(border[1] - border[0] + 1, 15);
            assert_eq!(l[border[0]].trim().chars().count(), 62);
        }
    }

    #[test]
    fn snapshot_smallest_full_popup_and_its_fallbacks() {
        let task = mixed();
        let picker = picker_of("code", 0);
        for menu in [&task, &picker] {
            let full = text(&draw(24, 14, menu));
            assert!(!full.contains("MENU"), "{full}");
            assert!(full.contains("Menu"), "{full}");
            for (w, h) in [(23, 14), (24, 13)] {
                let fall = text(&draw(w, h, menu));
                assert!(fall.contains("MENU"), "{w}x{h}");
                assert!(!fall.contains("Menu"), "{w}x{h}");
            }
        }
        insta::assert_snapshot!(text(&draw(24, 14, &task)));
        insta::assert_snapshot!("picker_24x14", text(&draw(24, 14, &picker)));
    }

    #[test]
    fn every_view_draws_at_every_size_without_panic() {
        let mut views = vec![mixed(), picker_of("code", 0), picker_of("general", 1)];
        let mut models = picker_of("general", 0);
        press(&mut models, &[KeyCode::Enter, KeyCode::Char('k')]);
        views.push(models);
        let mut custom = picker_of("general", 1);
        press(
            &mut custom,
            &[KeyCode::Enter, KeyCode::Char('k'), KeyCode::Enter],
        );
        views.push(custom);
        for view in &views {
            for w in 0..=70 {
                for h in 0..=20 {
                    draw(w, h, view);
                }
            }
        }
    }

    // Review W2: a long model name is clipped, the mark stays whole.
    #[test]
    fn a_long_model_name_is_clipped_and_keeps_the_download_mark() {
        let long = "qwen2.5-coder:32b-instruct-q4_K_M";
        let shown = text(&draw(62, 15, &with_ai(long)));
        assert!(shown.contains("· download required"), "{shown}");
        assert!(shown.contains("…"), "{shown}");
    }

    // The widget draws MenuState::footer(): `s` saves, Esc cancels.
    #[test]
    fn the_footer_is_the_menu_footer_not_the_old_hint() {
        let shown = text(&draw(62, 15, &mixed()));
        assert!(shown.contains("s save"), "{shown}");
        assert!(!shown.contains("Enter save"), "{shown}");
    }
}
