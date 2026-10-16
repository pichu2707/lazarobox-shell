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
        ai::picker::{Mark, NOT_IN_LIST},
        config::Config,
        menu::{AI_BODY, CHROME, Footer, MenuState, Row, fits_size},
    },
    ui::theme::LazaroboxTheme,
};

/// Widest popup wanted without an AI section.
pub const WANT_WIDTH: u16 = 51;
/// With the AI section the `r refresh` hint needs eleven more columns.
pub const WANT_WIDTH_AI: u16 = 62;

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
    fits_size(area.width, area.height, rows)
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
/// The one line shown instead of the privacy warning on a tiny terminal. It
/// offers no choice: the menu declines every key then (informed consent).
const FALLBACK_WARNING: &str = "Enlarge the terminal to read the privacy note";

impl MenuPopup<'_> {
    /// Below the size threshold: one line on the middle row.
    fn render_fallback(&self, area: Rect, buf: &mut Buffer) {
        let fallback = if self.menu.in_warning() {
            FALLBACK_WARNING
        } else {
            FALLBACK
        };
        let line = Rect::new(area.x, area.y + area.height / 2, area.width, 1);
        Clear.render(line, buf);
        let style = Style::default()
            .fg(self.theme.info_blue)
            .add_modifier(Modifier::REVERSED);
        buf.set_stringn(
            line.x,
            line.y,
            clip(fallback, usize::from(line.width)),
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
                // A long mark (an engine status with a reason) gives way to
                // the label, but never below half of the row, and the `●`
                // of the current row stays. A very narrow popup keeps the
                // old rule: the label gives way and the mark stays whole.
                let dot = if *current { " ●" } else { "" };
                let mark = mark_text(mark);
                let mark_cap = width
                    .saturating_sub(cells(label) + 1 + cells(dot) + 1)
                    .max(width / 2);
                let mark = if width >= CLIP_MARK_FROM && cells(mark) > mark_cap {
                    clip(mark, mark_cap)
                } else {
                    mark.to_owned()
                };
                let tail = match (mark.as_str(), current) {
                    ("", false) => String::new(),
                    ("", true) => dot.to_owned(),
                    (mark, _) => format!(" {mark}{dot}"),
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
            Row::Text(text) => (format!(" {text}"), plain),
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
        Mark::NotInList => NOT_IN_LIST,
        Mark::Status(text) => text,
    }
}

/// The row width from which a long picker mark is clipped to keep its label.
const CLIP_MARK_FROM: usize = 40;

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
        // The update note is extra: it never makes the popup fall back.
        let note = self.menu.update_note();
        let note_rows = note.as_ref().map_or(0, |lines| lines.len());
        let want_height = (body + note_rows) as u16 + CHROME;
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

        // Body rows, the update note when all of it fits above the footer,
        // then (height permitting) a blank row, then the footer.
        let note = note.filter(|lines| usize::from(inner.height) > body + lines.len());
        let used = body + note.as_ref().map_or(0, |lines| lines.len());
        let blank = usize::from(inner.height) > used + 1;
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
        for (offset, line) in note.iter().flatten().enumerate() {
            let y = inner.y + (body + offset) as u16;
            let style = Style::default().fg(self.theme.warning_orange);
            buf.set_stringn(inner.x, y, clip(&format!(" {line}"), width), width, style);
        }
        let footer_y = inner.y + used as u16 + u16::from(blank);
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

    use crate::core::menu::MIN_WIDTH;

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
    fn the_mouse_row_is_drawn_like_the_other_settings() {
        let buf = draw(51, 8, &fresh());
        let theme = LazaroboxTheme::default();
        assert_ne!(buf[(2, 4)].fg, theme.text_muted);
        assert_eq!(buf[(2, 4)].fg, buf[(2, 3)].fg);
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
                    model_labels: Vec::new(),
                    diagnostics: Default::default(),
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
                    model_labels: Vec::new(),
                    diagnostics: Default::default(),
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

    fn with_update() -> MenuState {
        mixed().with_update(Some("0.16.0".into()))
    }

    #[test]
    fn snapshot_update_note_under_the_task_list() {
        insta::assert_snapshot!(text(&draw(62, POPUP_H + 2, &with_update())));
    }

    #[test]
    fn the_update_note_gives_way_when_the_height_is_tight() {
        // The same height as without a note: the note is not drawn and the
        // popup is the one it always was.
        assert_eq!(
            text(&draw(62, POPUP_H, &with_update())),
            text(&draw(62, POPUP_H, &mixed()))
        );
        assert_eq!(
            text(&draw(62, MIN_HEIGHT, &with_update())),
            text(&draw(62, MIN_HEIGHT, &mixed()))
        );
    }

    #[test]
    fn the_update_note_is_not_drawn_over_a_picker() {
        let mut menu = picker_of("code", 0);
        menu.set_update(Some("0.16.0".into()));
        assert!(!text(&draw(62, POPUP_H + 2, &menu)).contains("update"));
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

    #[test]
    fn the_ai_popup_is_62_wide_and_body_plus_chrome_high_in_both_views() {
        for menu in [mixed(), picker_of("code", 0), warned()] {
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

    // Spec (config-menu, engram-memory): the Memory row and its warning.
    fn warned() -> MenuState {
        let mut menu = fresh().with_ai(AiConfig::default(), Some(machine()));
        goto(&mut menu, "Memory");
        press(&mut menu, &[KeyCode::Char('l')]);
        assert!(menu.in_warning());
        menu
    }

    #[test]
    fn snapshot_memory_row_selected_shows_the_settings_hint() {
        let mut menu = mixed();
        goto(&mut menu, "Memory");
        let shown = text(&draw(62, POPUP_H, &menu));
        assert!(shown.contains("Memory"), "{shown}");
        assert!(
            shown.contains("j/k move · h/l change · s save · Esc cancel"),
            "{shown}"
        );
        assert!(!shown.contains("r refresh"), "{shown}");
        insta::assert_snapshot!(shown);
    }

    #[test]
    fn snapshot_privacy_warning_on_a_normal_terminal() {
        let shown = text(&draw(62, POPUP_H, &warned()));
        for fact in [
            "private store (0600)",
            "every local process",
            "every agent that uses engram",
            "engram sync",
            "cannot restrict",
            "Turn memory on? (y/n)",
        ] {
            assert!(shown.contains(fact), "{fact}: {shown}");
        }
        assert!(!shown.contains('…'), "no clipping: {shown}");
        insta::assert_snapshot!(shown);
    }

    #[test]
    fn the_warning_on_a_tiny_terminal_asks_to_enlarge_and_offers_no_choice() {
        let buf = draw(50, 5, &warned());
        let l = lines(&buf);
        assert_eq!(
            l[2].trim_end(),
            "Enlarge the terminal to read the privacy note"
        );
        assert!(!text(&buf).contains("(y/n)"), "no prompt without the facts");
        insta::assert_snapshot!(text(&buf));
        // Narrower: clipped, still one line.
        let narrow = lines(&draw(30, 5, &warned()));
        assert!(narrow[2].starts_with("Enlarge the terminal"), "{narrow:?}");
    }

    // Review: AI_BODY is derived, not trusted. Every AI view fits it, so
    // adding a row cannot silently resize the popup.
    #[test]
    fn every_ai_view_fits_the_fixed_body() {
        let mut views = vec![
            mixed(),
            warned(),
            fresh().with_ai(AiConfig::default(), None),
        ];
        for task in ["general", "code", "chat", "review", "summarize"] {
            views.push(picker_of(task, 0));
            let mut models = picker_of(task, 0);
            press(&mut models, &[KeyCode::Enter]);
            views.push(models);
        }
        views.push(picker_of("Set all tasks", 0));
        for menu in &views {
            let len = menu.rows(&Config::default()).len();
            assert!(len <= AI_BODY, "{len} rows > {AI_BODY}");
        }
        let tallest = views.iter().map(|m| m.rows(&Config::default()).len()).max();
        assert_eq!(tallest, Some(AI_BODY), "AI_BODY is as small as it can be");
    }

    #[test]
    fn the_warning_never_panics_at_any_size() {
        let menu = warned();
        for w in 0..=70 {
            for h in 0..=20 {
                draw(w, h, &menu);
            }
        }
    }

    // Review W3: the current dot on a plain row, bold titles, full-width bar.
    #[test]
    fn a_current_plain_row_carries_the_dot() {
        let mut menu = picker_of("code", 0);
        press(&mut menu, &[KeyCode::Enter]);
        let shown = lines(&draw(62, POPUP_H, &menu));
        assert!(
            shown.iter().any(|l| l.contains("│ Opus 5.5 ●")),
            "{shown:?}"
        );
        assert!(
            !shown.iter().any(|l| l.contains("Sonnet 5.5 ●")),
            "{shown:?}"
        );
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
                    model_labels: Vec::new(),
                    diagnostics: Default::default(),
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

    /// A machine where only codex is installed and its model list failed.
    fn codex_without_list(why: &str) -> Detection {
        codex_degraded(Some(why), None)
    }

    /// Only codex is installed; `models` and `jail` are the reasons it is
    /// degraded.
    fn codex_degraded(models: Option<&str>, jail: Option<&str>) -> Detection {
        Detection {
            engines: CATALOG
                .iter()
                .map(|e| EngineState {
                    id: e.id,
                    status: if e.id == "codex" {
                        Status::Available
                    } else {
                        Status::NotInstalled
                    },
                    models: if e.id == "codex" {
                        vec!["default".to_owned()]
                    } else {
                        Vec::new()
                    },
                    version: (e.id == "codex").then(|| "codex-cli 0.130.0".to_owned()),
                    model_labels: Vec::new(),
                    diagnostics: crate::core::ai::Diagnostics {
                        models: models.filter(|_| e.id == "codex").map(str::to_owned),
                        jail: jail.filter(|_| e.id == "codex").map(str::to_owned),
                    },
                })
                .collect(),
        }
    }

    fn codex_models_level(detection: Detection) -> MenuState {
        let saved = AiConfig::general(Some("codex"), None);
        let mut menu = fresh().with_ai(saved, Some(detection));
        goto(&mut menu, "general");
        press(&mut menu, &[KeyCode::Enter, KeyCode::Enter]);
        menu
    }

    #[test]
    fn snapshot_codex_models_say_why_the_list_is_missing() {
        let menu = codex_models_level(codex_without_list("codex exited 1: not logged in"));
        let shown = text(&draw(62, POPUP_H, &menu));
        assert!(
            shown.contains("model list unavailable: codex exited 1: not logged in"),
            "{shown}"
        );
        insta::assert_snapshot!(shown);
    }

    fn codex_engines_level(detection: Detection) -> MenuState {
        let saved = AiConfig::general(Some("codex"), None);
        let mut menu = fresh().with_ai(saved, Some(detection));
        goto(&mut menu, "general");
        press(&mut menu, &[KeyCode::Enter]);
        menu
    }

    #[test]
    fn snapshot_engine_row_says_why_codex_has_no_jail() {
        let menu = codex_engines_level(codex_degraded(None, Some("Landlock ABI 2, needs 3+")));
        let shown = text(&draw(62, POPUP_H, &menu));
        assert!(shown.contains("no jail: Landlock"), "{shown}");
        insta::assert_snapshot!(shown);
    }

    #[test]
    fn a_long_engine_status_is_clipped_but_keeps_its_label() {
        let why = "a-very-long-folder-name-that-cannot-fit-at-all-in-the-row too broad";
        let menu = codex_engines_level(codex_degraded(None, Some(why)));
        let buf = draw(62, POPUP_H, &menu);
        let row = lines(&buf)
            .into_iter()
            .find(|l| l.contains("codex"))
            .unwrap();
        assert!(
            row.starts_with("│ codex available · no jail: a-very-long"),
            "{row}"
        );
        assert!(row.contains('…') && row.ends_with("●│"), "{row}");
        assert_eq!(cells(&row), 62);
    }

    #[test]
    fn every_short_jail_reason_survives_whole_at_62_columns() {
        use crate::core::ai::jail::{JailOff, JailRefusal, Untrusted};
        let home = Some(std::path::Path::new("/home/u"));
        let long = std::path::PathBuf::from("/very/long/folder/structure/that/goes/on/lib");
        let reasons = [
            JailRefusal::Unsupported(JailOff::NoLandlock(12)),
            JailRefusal::Unsupported(JailOff::NoLandlock(0)),
            JailRefusal::Unsupported(JailOff::NotLinux),
            JailRefusal::NoHome,
            JailRefusal::HomeMissing("/h".into()),
            JailRefusal::NoInterpreter("node".into()),
            JailRefusal::InterpreterMissing(long.clone()),
            JailRefusal::UntrustedInterpreter {
                path: "/n".into(),
                why: Untrusted::Scratch,
            },
            JailRefusal::BadLoader(long.clone()),
            JailRefusal::RootTooBroad(long.clone()),
            JailRefusal::Unreadable(long.clone()),
            JailRefusal::NotCanonical(long),
        ];
        for refusal in reasons {
            let why = refusal.short(home);
            let menu = codex_engines_level(codex_degraded(None, Some(&why)));
            let row = lines(&draw(62, POPUP_H, &menu))
                .into_iter()
                .find(|l| l.contains("codex"))
                .unwrap();
            assert!(row.contains(&format!("no jail: {why} ●")), "{why}: {row}");
        }
    }

    const LONG_WHY: &str = "jail: no node found to run the CLI script; install it next to the CLI or on PATH (looked in /usr/local/bin)";

    #[test]
    fn snapshot_a_long_listing_reason_wraps_across_rows_and_the_cursor_can_copy_it() {
        let mut menu = codex_models_level(codex_without_list(LONG_WHY));
        let shown = text(&draw(62, POPUP_H, &menu));
        assert!(shown.contains("/usr/local/bin)"), "{shown}");
        assert!(shown.contains("no node found to run the CLI"), "{shown}");
        insta::assert_snapshot!("long_reason_wrapped", shown);
        // On the note the footer says how to copy it.
        press(&mut menu, &[KeyCode::Down, KeyCode::Down]);
        let on_note = text(&draw(62, POPUP_H, &menu));
        assert!(
            on_note.contains("Enter or y copies the full reason"),
            "{on_note}"
        );
        insta::assert_snapshot!("long_reason_cursor_on_note", on_note);
    }

    #[test]
    fn a_huge_listing_reason_is_bounded_to_four_rows_with_the_last_clipped() {
        let why = format!("codex exited 1: {}", "word ".repeat(100));
        let menu = codex_models_level(codex_without_list(&why));
        let buf = draw(62, POPUP_H, &menu);
        let rows = lines(&buf);
        let first = rows
            .iter()
            .position(|l| l.contains("model list unavailable"))
            .unwrap();
        assert!(rows[first + 3].contains('…'), "{rows:#?}");
        assert!(!rows[first + 4].contains("word"), "{rows:#?}");
        for l in &rows[first..first + 4] {
            assert_eq!(cells(l), 62);
        }
    }

    #[test]
    fn a_narrow_popup_still_clips_the_note_rows_as_a_last_resort() {
        let menu = codex_models_level(codex_without_list(LONG_WHY));
        for w in [30, 40, 50] {
            let buf = draw(w, POPUP_H, &menu);
            for l in lines(&buf) {
                assert_eq!(cells(&l), usize::from(w), "{l}");
            }
        }
    }
}
