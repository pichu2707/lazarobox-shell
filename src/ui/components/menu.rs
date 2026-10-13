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
        menu::{AI_SECTION, Footer, MenuState, Row, SECTIONS},
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
/// task list and the picker. The task list is the tallest view: a title row
/// per section, then every settings row and every AI row (a picker is a title
/// and `VIEW` rows, which also fits). Derived from the descriptor tables, so
/// adding a row grows the popup instead of overflowing it.
const AI_BODY: usize = ai_body();

/// Rows of the task list: the section titles (those of `SECTIONS` and `AI`)
/// plus every item.
const fn ai_body() -> usize {
    let mut rows = SECTIONS.len() + 1 + AI_SECTION.items.len();
    let mut i = 0;
    while i < SECTIONS.len() {
        rows += SECTIONS[i].items.len();
        i += 1;
    }
    rows
}
const _: () = assert!(VIEW < AI_BODY, "a picker must fit the fixed body");
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

    /// The AI popup's height: its fixed body plus the chrome.
    const POPUP_H: u16 = AI_BODY as u16 + CHROME;
    /// The least height that still shows the full popup (no blank row).
    const MIN_HEIGHT: u16 = POPUP_H - 1;

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
    fn snapshot_ai_task_list_with_the_cursor_on_settings() {
        insta::assert_snapshot!(text(&draw(62, POPUP_H, &mixed())));
    }

    #[test]
    fn snapshot_level_one_lists_the_engines() {
        insta::assert_snapshot!(text(&draw(62, POPUP_H, &picker_of("code", 0))));
    }

    #[test]
    fn snapshot_ollama_scrolled_to_a_disabled_last_row() {
        let mut menu = picker_of("general", 0);
        press(&mut menu, &[KeyCode::Enter]);
        let top = draw(62, POPUP_H, &menu);
        press(&mut menu, &[KeyCode::Char('k')]);
        let buf = draw(62, POPUP_H, &menu);
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
        let buf = draw(62, POPUP_H, &menu);
        let theme = LazaroboxTheme::default();
        let y = (0..POPUP_H)
            .find(|y| buf[(4, *y)].modifier.contains(Modifier::REVERSED))
            .expect("the cursor row is reversed");
        assert!(lines(&buf)[usize::from(y)].contains("download required"));
        assert_eq!(buf[(4, y)].fg, theme.text_muted);
    }

    #[test]
    fn snapshot_cli_models_end_with_custom() {
        let mut menu = picker_of("general", 1);
        press(&mut menu, &[KeyCode::Enter, KeyCode::Char('k')]);
        insta::assert_snapshot!(text(&draw(62, POPUP_H, &menu)));
    }

    #[test]
    fn snapshot_custom_entry() {
        let mut menu = picker_of("general", 1);
        press(
            &mut menu,
            &[KeyCode::Enter, KeyCode::Char('k'), KeyCode::Enter],
        );
        press(&mut menu, &[KeyCode::Char('m'), KeyCode::Char('y')]);
        insta::assert_snapshot!(text(&draw(62, POPUP_H, &menu)));
    }

    #[test]
    fn snapshot_checking_cue_before_the_first_detection() {
        let saved = AiConfig::general(Some("ollama"), Some("llama3:8b"));
        let mut menu = fresh().with_ai(saved, None);
        goto(&mut menu, "code");
        insta::assert_snapshot!(text(&draw(62, POPUP_H, &menu)));
    }

    #[test]
    fn snapshot_refreshing_cue() {
        let mut menu = mixed();
        goto(&mut menu, "code");
        menu.set_checking(true);
        insta::assert_snapshot!(text(&draw(62, POPUP_H, &menu)));
    }

    // Spec (config-menu): Profile row render.
    #[test]
    fn snapshot_profile_row_selected_with_the_analyst_profile() {
        let mut saved = AiConfig::general(Some("ollama"), Some("model1:7b"));
        saved.set_profile(crate::core::ai::config::Profile::Analyst);
        let mut menu = fresh().with_ai(saved, Some(rich()));
        goto(&mut menu, "Profile");
        insta::assert_snapshot!(text(&draw(62, POPUP_H, &menu)));
    }

    // Spec (Context toggle): the row reads `on` by default and `off` when
    // the flag is false.
    #[test]
    fn snapshot_context_row_selected_and_off() {
        let mut saved = AiConfig::default();
        saved.set_context(false);
        let mut menu = fresh().with_ai(saved, None);
        goto(&mut menu, "Context");
        insta::assert_snapshot!(text(&draw(62, POPUP_H, &menu)));
    }

    #[test]
    fn the_context_row_reads_on_by_default() {
        let mut menu = fresh().with_ai(AiConfig::default(), None);
        goto(&mut menu, "Profile");
        let shown = text(&draw(62, POPUP_H, &menu));
        assert!(shown.contains("Context"), "{shown}");
        assert!(shown.contains("on"), "{shown}");
    }

    #[test]
    fn snapshot_profile_row_with_the_default_profile() {
        let mut menu = fresh().with_ai(AiConfig::default(), None);
        goto(&mut menu, "Profile");
        insta::assert_snapshot!(text(&draw(62, POPUP_H, &menu)));
    }

    // The body is fixed so the popup never jumps between views: every AI
    // view must fit it.
    #[test]
    fn every_ai_view_fits_the_fixed_body() {
        let views = [
            mixed(),
            picker_of("code", 0),
            picker_of("general", 1),
            fresh().with_ai(AiConfig::default(), None),
        ];
        for view in &views {
            let rows = view.rows(&Config::default()).len();
            assert!(rows <= AI_BODY, "{rows} rows > AI_BODY {AI_BODY}");
        }
        assert_eq!(AI_BODY, mixed().rows(&Config::default()).len());
    }

    #[test]
    fn the_ai_popup_is_62_wide_and_body_plus_chrome_high_in_both_views() {
        for menu in [mixed(), picker_of("code", 0)] {
            let l = lines(&draw(80, 20, &menu));
            let border: Vec<usize> = (0..l.len())
                .filter(|y| l[*y].contains('┌') || l[*y].contains('└'))
                .collect();
            assert_eq!(border.len(), 2);
            assert_eq!(border[1] - border[0] + 1, usize::from(POPUP_H));
            assert_eq!(l[border[0]].trim().chars().count(), 62);
        }
    }

    #[test]
    fn snapshot_smallest_full_popup_and_its_fallbacks() {
        let task = mixed();
        let picker = picker_of("code", 0);
        for menu in [&task, &picker] {
            let full = text(&draw(MIN_WIDTH, MIN_HEIGHT, menu));
            assert!(!full.contains("MENU"), "{full}");
            assert!(full.contains("Menu"), "{full}");
            for (w, h) in [(MIN_WIDTH - 1, MIN_HEIGHT), (MIN_WIDTH, MIN_HEIGHT - 1)] {
                let fall = text(&draw(w, h, menu));
                assert!(fall.contains("MENU"), "{w}x{h}");
                assert!(!fall.contains("Menu"), "{w}x{h}");
            }
        }
        insta::assert_snapshot!(text(&draw(MIN_WIDTH, MIN_HEIGHT, &task)));
        insta::assert_snapshot!(
            "picker_smallest_full",
            text(&draw(MIN_WIDTH, MIN_HEIGHT, &picker))
        );
    }

    #[test]
    fn every_view_draws_at_every_size_without_panic() {
        let mut views = vec![mixed(), picker_of("code", 0), picker_of("general", 1)];
        let mut models = picker_of("general", 0);
        press(&mut models, &[KeyCode::Enter, KeyCode::Char('k')]);
        views.push(models);
        let mut top = picker_of("general", 0);
        press(&mut top, &[KeyCode::Enter]);
        views.push(top);
        let mut cli = picker_of("general", 1);
        press(&mut cli, &[KeyCode::Enter]);
        views.push(cli);
        let mut custom = picker_of("general", 1);
        press(
            &mut custom,
            &[KeyCode::Enter, KeyCode::Char('k'), KeyCode::Enter],
        );
        views.push(custom);
        let mut error = mixed();
        error.set_error("config.toml: ai is not a table; fix it before saving".into());
        views.push(error);
        let mut checking = picker_of("code", 0);
        checking.set_checking(true);
        views.push(checking);
        let mut unchecked = fresh().with_ai(AiConfig::general(Some("ollama"), None), None);
        goto(&mut unchecked, "code");
        views.push(unchecked);
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
        let shown = text(&draw(62, POPUP_H, &with_ai(long)));
        assert!(shown.contains("· download required"), "{shown}");
        assert!(shown.contains("…"), "{shown}");
    }

    // The widget draws MenuState::footer(): `s` saves, Esc cancels.
    #[test]
    fn the_footer_is_the_menu_footer_not_the_old_hint() {
        let shown = text(&draw(62, POPUP_H, &mixed()));
        assert!(shown.contains("s save"), "{shown}");
        assert!(!shown.contains("Enter save"), "{shown}");
    }

    // The widget only styles the footer: Error is red, Hint is muted.
    fn footer_fg(buf: &Buffer, needle: &str) -> ratatui::style::Color {
        let y = lines(buf)
            .iter()
            .position(|l| l.contains(needle))
            .expect("footer is drawn") as u16;
        let x = (0..buf.area.width)
            .find(|x| buf[(*x, y)].symbol() == needle.chars().next().unwrap().to_string())
            .expect("first cell");
        buf[(x, y)].fg
    }

    #[test]
    fn the_error_footer_is_red_and_the_hint_is_muted() {
        let theme = LazaroboxTheme::default();
        assert_ne!(theme.error_red, theme.text_muted);
        let mut menu = with_ai("llama3:8b");
        let hint = draw(62, POPUP_H, &menu);
        assert_eq!(footer_fg(&hint, "j/k move"), theme.text_muted);
        menu.set_error("save failed: disk full".into());
        let error = draw(62, POPUP_H, &menu);
        assert_eq!(footer_fg(&error, "save failed"), theme.error_red);
    }

    // `fit` keeps the 20-cell mark whole once the head has a cell to spare.
    #[test]
    fn fit_keeps_the_mark_from_room_21_and_clips_the_whole_value_below() {
        let value = "abcdefghij · download required";
        assert_eq!(cells(value), 30);
        assert_eq!(fit(value, 30), value);
        assert_eq!(fit(value, 22), "a… · download required");
        assert_eq!(fit(value, 21), "… · download required");
        assert_eq!(fit(value, 20), "abcdefghij · downlo…");
        assert_eq!(cells(&fit(value, 20)), 20);
        assert_eq!(fit("plain value that is long", 8), "plain v…");
    }

    // Review W1: the cursor on a task row shows the task hint, no h/l.
    #[test]
    fn snapshot_task_row_hint_after_a_finished_detection() {
        let mut menu = mixed();
        goto(&mut menu, "code");
        let shown = text(&draw(62, POPUP_H, &menu));
        assert!(shown.contains("Enter pick · r refresh · s save"), "{shown}");
        assert!(!shown.contains("h/l"), "{shown}");
        insta::assert_snapshot!(shown);
    }

    // Review W2: Ollama level 2 at the top, the downloaded rows visible.
    #[test]
    fn snapshot_ollama_models_at_the_top_show_the_downloaded_rows() {
        let mut menu = picker_of("general", 0);
        press(&mut menu, &[KeyCode::Enter]);
        let shown = text(&draw(62, POPUP_H, &menu));
        assert!(shown.contains("model1:7b ✓"), "{shown}");
        insta::assert_snapshot!(shown);
    }

    // Review W3: the current dot on a plain row, bold titles, full-width bar.
    #[test]
    fn a_current_plain_row_carries_the_dot() {
        let mut menu = picker_of("code", 0);
        press(&mut menu, &[KeyCode::Enter]);
        let shown = lines(&draw(62, POPUP_H, &menu));
        assert!(shown.iter().any(|l| l.contains("│ opus ●")), "{shown:?}");
        assert!(!shown.iter().any(|l| l.contains("sonnet ●")), "{shown:?}");
    }

    #[test]
    fn titles_are_bold_and_the_selected_bar_spans_the_inner_width() {
        let task = draw(62, POPUP_H, &mixed());
        assert!(task[(2, 1)].modifier.contains(Modifier::BOLD));
        assert!(task[(2, 5)].modifier.contains(Modifier::BOLD));
        assert!(!task[(2, 2)].modifier.contains(Modifier::BOLD));
        let picker = draw(62, POPUP_H, &picker_of("code", 0));
        assert!(picker[(2, 1)].modifier.contains(Modifier::BOLD));
        for buf in [&task, &picker] {
            let bar: Vec<u16> = (0..POPUP_H)
                .filter(|y| buf[(2, *y)].modifier.contains(Modifier::REVERSED))
                .collect();
            assert_eq!(bar.len(), 1);
            for x in 1..=60 {
                assert!(
                    buf[(x, bar[0])].modifier.contains(Modifier::REVERSED),
                    "x={x}"
                );
            }
            assert!(!buf[(0, bar[0])].modifier.contains(Modifier::REVERSED));
            assert!(!buf[(61, bar[0])].modifier.contains(Modifier::REVERSED));
        }
    }

    // The label gives way only when it does not fit next to its mark.
    #[test]
    fn a_picker_label_is_clipped_only_past_its_room() {
        let fitting = "m".repeat(57);
        let long = "m".repeat(58);
        let detection = |name: &str| Detection {
            engines: CATALOG
                .iter()
                .map(|e| EngineState {
                    id: e.id,
                    status: Status::Available,
                    models: if e.id == "ollama" {
                        vec![name.to_owned()]
                    } else {
                        Vec::new()
                    },
                    version: None,
                })
                .collect(),
        };
        let row = |name: &str| {
            let saved = AiConfig::general(Some("ollama"), None);
            let mut menu = fresh().with_ai(saved, Some(detection(name)));
            goto(&mut menu, "general");
            press(&mut menu, &[KeyCode::Enter, KeyCode::Enter]);
            lines(&draw(62, POPUP_H, &menu))[2].clone()
        };
        assert_eq!(row(&fitting), format!("│ {fitting} ✓│"));
        let clipped = row(&long);
        assert!(clipped.contains("…"), "{clipped}");
        assert!(clipped.ends_with("✓│"), "{clipped}");
    }
}
