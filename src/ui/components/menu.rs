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
        ai::{
            marks::{ToolGlyph, badge_text},
            picker::{Mark, NOT_IN_LIST},
        },
        config::Config,
        menu::{CAUTION_ROWS, Footer, MenuState, NOTE_ROWS, Row, popup_layout},
    },
    ui::theme::LazaroboxTheme,
};

/// Draws `MenuState::rows` in a bordered box, or a one-line fallback.
pub struct MenuPopup<'a> {
    menu: &'a MenuState,
    config: &'a Config,
    theme: &'a LazaroboxTheme,
    glyph: ToolGlyph,
}

impl<'a> MenuPopup<'a> {
    pub fn new(menu: &'a MenuState, config: &'a Config, theme: &'a LazaroboxTheme) -> Self {
        Self {
            menu,
            config,
            theme,
            glyph: ToolGlyph::default(),
        }
    }

    /// How the tool badge is drawn; decided once by the runtime.
    pub fn with_glyph(mut self, glyph: ToolGlyph) -> Self {
        self.glyph = glyph;
        self
    }
}

const FALLBACK: &str = "MENU · s save · Esc cancel";
/// The hint at the right of the tab bar: the key that switches tabs.
const TAB_HINT: &str = "⇥ Tab";
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
                tools,
            } => {
                // A long engine status gives way to a short label (an engine
                // name stays whole, with its badge and `●`). Any other
                // mark, or a status next to a label over half of the row, keeps
                // half of the row at least; below
                // CLIP_MARK_FROM only a status is clipped, other marks stay whole.
                let dot = if *current { " ●" } else { "" };
                let raw_mark = mark;
                let mark = mark_text(mark);
                let badge = tools.map_or("", |t| badge_text(t, self.glyph));
                let badge_cells = if badge.is_empty() {
                    0
                } else {
                    cells(badge) + 1
                };
                let mark_cap =
                    width.saturating_sub(cells(label) + 1 + badge_cells + cells(dot) + 1);
                let is_status = matches!(raw_mark, Mark::Status(_));
                let mark_cap = if is_status && cells(label) <= width / 2 {
                    mark_cap
                } else {
                    mark_cap.max(width / 2)
                };
                let clips = width >= CLIP_MARK_FROM || is_status;
                let mark = if clips && cells(mark) > mark_cap {
                    clip(mark, mark_cap)
                } else {
                    mark.to_owned()
                };
                let badge = if badge.is_empty() {
                    String::new()
                } else {
                    format!(" {badge}")
                };
                let mark = if mark.is_empty() {
                    String::new()
                } else {
                    format!(" {mark}")
                };
                let tail = format!("{mark}{badge}{dot}");
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

/// The tab bar of the menu as styled segments for `width` inner cells: each
/// label padded as ` label `, joined by `│`, the `active` one on the accent
/// colour and the rest muted. It picks the first step that fits, with BAR the
/// width of the whole bar:
///
/// 1. `width >= BAR + hint + 2`: the bar, then the `hint` right-aligned, ending
///    one column before the inner right edge (like row values);
/// 2. `width >= BAR`: the bar alone;
/// 3. otherwise only the active tab as ` ‹ label › `, clipped to `width`.
///
/// Without a `hint` (the keys are inert over a sub-level) step 1 is skipped.
pub fn tab_line(
    labels: &[&str],
    active: usize,
    width: usize,
    hint: Option<&str>,
    theme: &LazaroboxTheme,
) -> Vec<(String, Style)> {
    let muted = Style::default().fg(theme.text_muted);
    let accent = Style::default()
        .fg(theme.bg_base)
        .bg(theme.primary_cyan)
        .add_modifier(Modifier::BOLD);
    if width == 0 || labels.is_empty() {
        return Vec::new();
    }
    let mut bar = Vec::new();
    for (index, label) in labels.iter().enumerate() {
        if index > 0 {
            bar.push(("│".to_owned(), muted));
        }
        let style = if index == active { accent } else { muted };
        bar.push((format!(" {label} "), style));
    }
    let bar_width: usize = bar.iter().map(|(text, _)| cells(text)).sum();
    if bar_width > width {
        let label = labels.get(active).copied().unwrap_or(labels[0]);
        return vec![(clip(&format!(" ‹ {label} › "), width), accent)];
    }
    if let Some(hint) = hint
        && width >= bar_width + cells(hint) + 2
    {
        let gap = width - bar_width - cells(hint) - 1;
        bar.push((" ".repeat(gap), muted));
        bar.push((hint.to_owned(), muted));
        bar.push((" ".to_owned(), muted));
    }
    bar
}

impl Widget for MenuPopup<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let area = area.intersection(buf.area);
        if area.is_empty() {
            return;
        }
        let rows = self.menu.rows(self.config);
        let labels = self.menu.tab_labels();
        let tabbed = labels.len() > 1;
        let note = self.menu.update_note();
        let note_rows = note.as_ref().map_or(0, |lines| lines.len());
        // The same layout the click hit-test reads (`MenuState::on_click`).
        let Some(layout) = popup_layout(
            area.into(),
            rows.len(),
            tabbed,
            self.menu.has_ai(),
            note_rows,
        ) else {
            self.render_fallback(area, buf);
            return;
        };
        let (body, extra) = (layout.body, layout.extra);
        let popup = Rect::from(layout.popup);
        Clear.render(popup, buf);
        let accent = Style::default().fg(self.theme.info_blue);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(accent)
            .title(" Menu ")
            .title_style(accent)
            .style(Style::default().bg(self.theme.bg_panel));
        let inner = Rect::from(layout.inner);
        block.render(popup, buf);

        // The bar (tabbed), the body rows, the update note when all of it
        // fits above the footer, then (height permitting) a blank row, then
        // the footer.
        let note = if tabbed {
            note
        } else {
            note.filter(|lines| usize::from(inner.height) > body + lines.len())
        };
        let used = body + extra.min(note.as_ref().map_or(0, |lines| lines.len()));
        let blank = usize::from(inner.height) > used + 1;
        let width = usize::from(inner.width);
        if tabbed {
            let names: Vec<&str> = labels.iter().map(String::as_str).collect();
            let hint = (!self.menu.in_sub_level()).then_some(TAB_HINT);
            let segments = tab_line(&names, self.menu.active_tab(), width, hint, self.theme);
            let mut x = inner.x;
            for (text, style) in &segments {
                let room = usize::from(inner.right().saturating_sub(x));
                buf.set_stringn(x, inner.y, text, room, *style);
                x += cells(text) as u16;
            }
        }
        for (offset, row) in rows.iter().enumerate() {
            let (text, style) = self.line(row, inner.width);
            let y = layout.first_row + offset as u16;
            if y >= inner.bottom() {
                break;
            }
            buf.set_style(Rect::new(inner.x, y, inner.width, 1), style);
            buf.set_stringn(inner.x, y, clip(&text, width), width, style);
        }
        for (offset, line) in note.iter().flatten().enumerate() {
            let first = if tabbed { body - NOTE_ROWS } else { body };
            let y = inner.y + (first + offset) as u16;
            if y >= inner.bottom() {
                break;
            }
            let style = Style::default().fg(self.theme.warning_orange);
            buf.set_stringn(inner.x, y, clip(&format!(" {line}"), width), width, style);
        }
        if tabbed && let Some(line) = self.menu.caution() {
            let y = inner.y + (body - CAUTION_ROWS) as u16;
            if y < inner.bottom() {
                let style = Style::default().fg(self.theme.warning_orange);
                buf.set_stringn(inner.x, y, clip(&format!(" {line}"), width), width, style);
            }
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

    use crate::core::menu::{self, CHROME, MENU_BODY, MIN_WIDTH, fits_size};

    /// The popup box the layout centres in `area` (`menu::popup_area`).
    fn popup_area(area: Rect, want_w: u16, want_h: u16) -> Rect {
        menu::popup_area(area.into(), want_w, want_h).into()
    }

    /// Whether the full popup of `rows` body rows fits `area`.
    fn fits(area: Rect, rows: usize) -> bool {
        fits_size(area.width, area.height, rows)
    }

    /// The AI popup's height: its fixed body plus the chrome.
    const POPUP_H: u16 = MENU_BODY as u16 + CHROME;
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
        // Four body rows: the popup is 24 wide and rows + 3 high.
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
        insta::assert_snapshot!(text(&draw(51, 9, &fresh())));
    }

    #[test]
    fn snapshot_popup_is_centred_in_a_larger_area() {
        insta::assert_snapshot!(text(&draw(60, 11, &fresh())));
    }

    // Spec: the blank row is dropped first on a short height.
    #[test]
    fn snapshot_short_height_drops_the_blank_row() {
        insta::assert_snapshot!(text(&draw(51, 9, &fresh())));
    }

    #[test]
    fn selected_row_is_reversed_and_the_others_are_not() {
        let buf = draw(51, 9, &fresh());
        let reversed = |y| buf[(2, y)].modifier.contains(Modifier::REVERSED);
        assert!(!reversed(1));
        assert!(reversed(2));
        assert!(!reversed(3));
    }

    #[test]
    fn the_mouse_row_is_drawn_like_the_other_settings() {
        let buf = draw(51, 9, &fresh());
        let theme = LazaroboxTheme::default();
        assert_ne!(buf[(2, 4)].fg, theme.text_muted);
        assert_eq!(buf[(2, 4)].fg, buf[(2, 3)].fg);
    }

    // Spec: Footer: Error replaces hint.
    #[test]
    fn snapshot_error_replaces_the_hint_and_is_clipped() {
        let mut menu = fresh();
        menu.set_error("config.toml: statusline is not a table; fix it before saving".into());
        let buf = draw(51, 9, &menu);
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
    fn exactly_24_by_9_shows_the_full_popup() {
        let buf = draw(24, 9, &fresh());
        assert!(text(&buf).contains("Settings"));
        assert!(!text(&buf).contains("MENU"));
        let below = draw(23, 9, &fresh());
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
                    binary: None,
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
            ..Default::default()
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
                    binary: None,
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
                    version: (e.id == "claude").then(|| "2.1.302".to_owned()),
                    model_labels: Vec::new(),
                    diagnostics: Default::default(),
                })
                .collect(),
            ..Default::default()
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
        menu.goto(label);
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
    fn snapshot_models_level_of_llamacpp_without_llama_server() {
        let mut menu = picker_of("code", 0);
        let on = |menu: &MenuState, name: &str| {
            menu.rows(&Config::default())
                .iter()
                .any(|r| matches!(r, Row::Pick { label, selected: true, .. } if label == name))
        };
        for _ in 0..10 {
            if on(&menu, "llamacpp") {
                break;
            }
            press(&mut menu, &[KeyCode::Char('j')]);
        }
        assert!(on(&menu, "llamacpp"));
        press(&mut menu, &[KeyCode::Enter]);
        // Down to the Hugging Face row, so its mark is drawn selected.
        press(&mut menu, &[KeyCode::Char('j')]);
        insta::assert_snapshot!(text(&draw(MIN_WIDTH, MIN_HEIGHT, &menu)));
    }

    mod install;
    mod tool_badge;

    #[test]
    fn snapshot_level_one_lists_the_engines() {
        insta::assert_snapshot!(text(&draw(62, POPUP_H, &picker_of("code", 0))));
    }

    #[test]
    fn snapshot_ollama_scrolled_to_a_disabled_last_row() {
        let mut menu = picker_of("general", 0);
        press(&mut menu, &[KeyCode::Enter]);
        let top = draw(62, POPUP_H, &menu);
        // Up wraps to the `download from Ollama…` row, then the last
        // curated row, which is disabled.
        press(&mut menu, &[KeyCode::Char('k'), KeyCode::Char('k')]);
        let buf = draw(62, POPUP_H, &menu);
        let shown = text(&buf);
        assert!(shown.contains("18/19"), "{shown}");
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
        // Past the `download from Ollama…` row, onto the last curated one.
        press(
            &mut menu,
            &[KeyCode::Enter, KeyCode::Char('k'), KeyCode::Char('k')],
        );
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
            let l = lines(&draw(80, POPUP_H + 4, &menu));
            let border: Vec<usize> = (0..l.len())
                .filter(|y| l[*y].contains('┌') || l[*y].contains('└'))
                .collect();
            assert_eq!(border.len(), 2);
            assert_eq!(border[1] - border[0] + 1, usize::from(POPUP_H));
            assert_eq!(l[border[0]].trim().chars().count(), 62);
        }
    }

    // Spec (open-links S2): a click finds a row where it is drawn, because
    // the hit-test reads the same layout as the renderer.
    #[test]
    fn the_layout_puts_every_row_where_it_is_drawn() {
        let mut field = with_ai("llama3:8b").with_hf(Default::default(), None);
        goto(&mut field, "Hugging Face");
        press(&mut field, &[KeyCode::Enter]);
        let config = Config::default();
        for menu in [fresh(), mixed(), field] {
            for (w, h) in [
                (80, 24),
                (40, MIN_HEIGHT),
                (MIN_WIDTH, MIN_HEIGHT),
                (120, 40),
            ] {
                let screen = crate::core::layout::Rect {
                    x: 0,
                    y: 0,
                    width: w,
                    height: h,
                };
                let Some(layout) = menu.popup_layout(&config, screen) else {
                    assert!(text(&draw(w, h, &menu)).contains("MENU"), "{w}x{h}");
                    continue;
                };
                let buf = draw(w, h, &menu);
                let inner = layout.inner;
                for (index, row) in menu.rows(&config).iter().enumerate() {
                    let y = layout.first_row + index as u16;
                    if y >= inner.y + inner.height {
                        break;
                    }
                    let start = match row {
                        Row::Title(t) | Row::Text(t) => format!(" {t}"),
                        Row::Field(t) => format!(" {t}"),
                        Row::Item { label, .. } => format!(" {label}"),
                        Row::Pick { label, .. } => format!(" {label}"),
                    };
                    let drawn: String = (inner.x..inner.x + inner.width)
                        .map(|x| buf[(x, y)].symbol())
                        .collect();
                    // The first cells, before any clipping.
                    let head: String = start.chars().take(8).collect();
                    let head = head.trim_end();
                    assert!(
                        drawn.starts_with(head),
                        "{w}x{h} row {index}: {drawn:?} vs {head:?}"
                    );
                }
            }
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
        let mut menu = with_ai(long);
        tab_key(&mut menu);
        let shown = text(&draw(62, POPUP_H, &menu));
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
        assert!(
            task[(2, 1)].modifier.contains(Modifier::BOLD),
            "the active tab"
        );
        assert!(!task[(2, 2)].modifier.contains(Modifier::BOLD));
        let picker = draw(62, POPUP_H, &picker_of("code", 0));
        assert!(
            picker[(2, 2)].modifier.contains(Modifier::BOLD),
            "the title"
        );
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
                    binary: None,
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
            ..Default::default()
        };
        let row = |name: &str| {
            let saved = AiConfig::general(Some("ollama"), None);
            let mut menu = fresh().with_ai(saved, Some(detection(name)));
            goto(&mut menu, "general");
            press(&mut menu, &[KeyCode::Enter, KeyCode::Enter]);
            lines(&draw(62, POPUP_H, &menu))[3].clone()
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
                    binary: None,
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
                        ..Default::default()
                    },
                })
                .collect(),
            ..Default::default()
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
    fn snapshot_engine_row_refuses_codex_and_says_why() {
        let menu = codex_engines_level(codex_degraded(None, Some("Landlock ABI 2, needs 3+")));
        let shown = text(&draw(62, POPUP_H, &menu));
        assert!(shown.contains("refused · Landlock"), "{shown}");
        assert!(!shown.contains("available"), "{shown}");
        insta::assert_snapshot!(shown);
    }

    #[test]
    fn snapshot_engine_row_names_the_install_layout_after_the_version() {
        let mut detection = codex_degraded(None, None);
        for s in &mut detection.engines {
            if s.id == "codex" {
                s.version = Some("0.153.4".to_owned());
                s.diagnostics.install =
                    Some(crate::core::ai::codex_install::CodexInstall::Standalone);
            }
        }
        let shown = text(&draw(62, POPUP_H, &codex_engines_level(detection)));
        assert!(
            shown.contains("codex available · 0.153.4 · standalone"),
            "{shown}"
        );
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
        assert!(row.starts_with("│ codex refused · a-very-long"), "{row}");
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
            JailRefusal::Unsupported(JailOff::NoBackend),
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
        let mut whys: Vec<String> = reasons.iter().map(|r| r.short(home)).collect();
        whys.push(JailOff::NoBackend.short_required());
        for why in whys {
            let menu = codex_engines_level(codex_degraded(None, Some(&why)));
            let row = lines(&draw(62, POPUP_H, &menu))
                .into_iter()
                .find(|l| l.contains("codex"))
                .unwrap();
            assert!(row.contains(&format!("refused · {why} ●")), "{why}: {row}");
        }
    }

    /// Only claude is installed, at `version`.
    fn claude_at(version: &str) -> Detection {
        Detection {
            engines: CATALOG
                .iter()
                .map(|e| EngineState {
                    binary: None,
                    id: e.id,
                    status: if e.id == "claude" {
                        Status::Available
                    } else {
                        Status::NotInstalled
                    },
                    models: if e.id == "claude" {
                        vec!["default".to_owned()]
                    } else {
                        Vec::new()
                    },
                    version: (e.id == "claude").then(|| version.to_owned()),
                    model_labels: Vec::new(),
                    diagnostics: Default::default(),
                })
                .collect(),
            ..Default::default()
        }
    }

    fn claude_row(version: &str) -> String {
        let saved = AiConfig::general(Some("claude"), None);
        let mut menu = fresh().with_ai(saved, Some(claude_at(version)));
        goto(&mut menu, "general");
        press(&mut menu, &[KeyCode::Enter]);
        let buf = draw(62, POPUP_H, &menu);
        let row = lines(&buf)
            .into_iter()
            .find(|l| l.contains("claude"))
            .unwrap();
        assert_eq!(cells(&row), 62, "{row}");
        row
    }

    #[test]
    fn an_outdated_claude_row_says_update_needed_whole_at_62_columns() {
        for version in ["2.1.100 (Claude Code)", "1.0.0", "2.1.258 (Claude Code)"] {
            let row = claude_row(version);
            assert!(
                row.contains("claude available · update needed: 2.1.259+ ●"),
                "{version}: {row}"
            );
        }
    }

    #[test]
    fn a_current_claude_row_keeps_showing_its_version() {
        let row = claude_row("2.1.302 (Claude Code)");
        assert!(
            row.contains("claude available · 2.1.302 (Claude Code)"),
            "{row}"
        );
        assert!(!row.contains("update needed"), "{row}");
    }

    #[test]
    fn a_detected_only_engine_row_says_so_whole_at_62_columns() {
        let long = "1.19.0-a-very-long-build-string-that-would-not-fit-in-the-row";
        let det = Detection {
            engines: CATALOG
                .iter()
                .map(|e| EngineState {
                    binary: None,
                    id: e.id,
                    status: Status::Available,
                    models: vec!["m".to_owned()],
                    version: Some(long.to_owned()),
                    model_labels: Vec::new(),
                    diagnostics: Default::default(),
                })
                .collect(),
            ..Default::default()
        };
        let mut menu = fresh().with_ai(AiConfig::general(Some("claude"), None), Some(det));
        goto(&mut menu, "general");
        press(&mut menu, &[KeyCode::Enter]);
        let all = lines(&draw(62, POPUP_H + 6, &menu));
        let row = all.iter().find(|l| l.contains("opencode")).unwrap();
        assert!(row.contains("opencode detected · not in chat yet"), "{row}");
        assert_eq!(cells(row), 62, "{row}");
        let claude = all.iter().find(|l| l.contains("claude")).unwrap();
        assert!(!claude.contains("not in chat"), "{claude}");
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

    // Spec: Menu tab bar; Popup height is fixed.
    fn tab_key(menu: &mut MenuState) {
        press(menu, &[KeyCode::Tab]);
    }

    fn ai_tab() -> MenuState {
        let mut menu = mixed();
        tab_key(&mut menu);
        menu
    }

    fn custom_view() -> MenuState {
        let mut menu = picker_of("general", 1);
        press(
            &mut menu,
            &[KeyCode::Enter, KeyCode::Char('k'), KeyCode::Enter],
        );
        menu
    }

    fn models_view() -> MenuState {
        let mut menu = picker_of("general", 0);
        press(&mut menu, &[KeyCode::Enter]);
        menu
    }

    fn sub_levels() -> Vec<MenuState> {
        vec![picker_of("code", 0), models_view(), custom_view(), warned()]
    }

    #[test]
    fn the_bar_is_body_row_zero_with_the_hint_on_a_normal_terminal() {
        for menu in [mixed(), ai_tab()] {
            let l = lines(&draw(62, POPUP_H, &menu));
            assert!(l[1].starts_with("│ Settings │ AI "), "{}", l[1]);
            assert!(
                l[1].ends_with("⇥ Tab │"),
                "hint one column from the edge: {}",
                l[1]
            );
        }
    }

    #[test]
    fn the_bar_marks_the_active_tab_in_cyan_and_not_in_the_row_style() {
        let theme = LazaroboxTheme::default();
        let settings = draw(62, POPUP_H, &mixed());
        assert_eq!(settings[(2, 1)].bg, theme.primary_cyan);
        assert_eq!(settings[(2, 1)].fg, theme.bg_base);
        assert!(settings[(2, 1)].modifier.contains(Modifier::BOLD));
        assert!(!settings[(2, 1)].modifier.contains(Modifier::REVERSED));
        assert_ne!(settings[(13, 1)].bg, theme.primary_cyan, "AI is idle");
        let ai = draw(62, POPUP_H, &ai_tab());
        assert_ne!(ai[(2, 1)].bg, theme.primary_cyan);
        assert_eq!(ai[(13, 1)].bg, theme.primary_cyan, "AI is active");
    }

    #[test]
    fn the_hint_is_absent_over_every_sub_level_and_the_bar_stays() {
        for menu in sub_levels() {
            let l = lines(&draw(62, POPUP_H, &menu));
            assert!(l[1].starts_with("│ Settings │ AI "), "{}", l[1]);
            assert!(!l[1].contains('⇥'), "{}", l[1]);
        }
    }

    #[test]
    fn no_section_titles_are_drawn_when_tabbed() {
        let settings = lines(&draw(62, POPUP_H, &mixed()));
        assert!(
            settings[2].starts_with("│ Statusline position"),
            "{}",
            settings[2]
        );
        assert!(
            !settings[2..].iter().any(|l| l.starts_with("│ AI")),
            "{settings:?}"
        );
        let ai = lines(&draw(62, POPUP_H, &ai_tab()));
        assert!(ai[2].starts_with("│ Set all tasks"), "{}", ai[2]);
        assert!(ai[11].starts_with("│ Tools"), "{}", ai[11]);
    }

    // Spec (config-menu): the caution line of a family that can change files
    // or run code shows under the rows; the popup keeps its size.
    #[test]
    fn snapshot_exec_row_selected_shows_the_caution() {
        let mut menu = ai_tab();
        goto(&mut menu, "Exec");
        insta::assert_snapshot!(text(&draw(62, POPUP_H, &menu)));
    }

    #[test]
    fn the_caution_shows_only_on_the_risky_family_rows_and_keeps_the_size() {
        let caution = "Can change files or run code";
        let mut sizes = Vec::new();
        for (label, shown) in [
            ("Search", false),
            ("Write", true),
            ("Exec", true),
            ("Git", false),
            ("Git write", true),
            ("Tools", false),
        ] {
            let mut menu = ai_tab();
            goto(&mut menu, label);
            let l = lines(&draw(62, POPUP_H, &menu));
            assert_eq!(l.iter().any(|row| row.contains(caution)), shown, "{label}");
            sizes.push(l.len());
        }
        sizes.dedup();
        assert_eq!(sizes.len(), 1, "the popup does not change size");
    }

    #[test]
    fn titles_are_still_drawn_for_a_single_tab() {
        let shown = lines(&draw(51, 9, &fresh()));
        assert!(shown[1].starts_with("│ Settings"), "{}", shown[1]);
        assert!(!shown[1].contains("│ Settings │"), "no bar: {}", shown[1]);
    }

    #[test]
    fn the_popup_is_twenty_three_rows_on_both_tabs_and_in_every_sub_level() {
        assert_eq!(POPUP_H, 23);
        let mut views = vec![mixed(), ai_tab(), with_update()];
        views.extend(sub_levels());
        for menu in &views {
            let l = lines(&draw(80, 24, menu));
            let border: Vec<usize> = (0..l.len())
                .filter(|y| l[*y].contains('┌') || l[*y].contains('└'))
                .collect();
            assert_eq!(border.len(), 2);
            assert_eq!(border[1] - border[0] + 1, 23);
        }
    }

    #[test]
    fn the_update_note_sits_in_the_last_two_body_rows_of_settings_only() {
        let l = lines(&draw(62, POPUP_H, &with_update()));
        assert!(
            l[18].contains("lazarobox 0.16.0 is available."),
            "{}",
            l[18]
        );
        assert!(
            l[19].contains("Go to Updates and press Enter."),
            "{}",
            l[19]
        );
        assert_eq!(
            l[20].trim_matches(|c| c == '│' || c == ' '),
            "",
            "blank row"
        );
        assert!(l[21].contains("s save"), "footer: {}", l[21]);
        let mut menu = with_update();
        tab_key(&mut menu);
        let shown = text(&draw(62, POPUP_H, &menu));
        assert!(!shown.contains("Go to Updates and press Enter."), "{shown}");
        for sub in sub_levels() {
            let shown = text(&draw(62, POPUP_H, &sub.with_update(Some("0.16.0".into()))));
            assert!(!shown.contains("Go to Updates and press Enter."), "{shown}");
        }
    }

    #[test]
    fn the_fallback_explanation_fits_the_popup_without_clipping() {
        // At 80 columns the popup is 51 wide: nothing may be cut.
        let mut menu = with_update();
        menu.goto("Updates");
        menu.show_update_command(crate::core::update::Fallback::UntrustedCargo);
        let l = lines(&draw(80, POPUP_H, &menu));
        let body = |row: &str| row.trim_matches(|c| c == '\u{2502}' || c == ' ').to_owned();
        assert_eq!(body(&l[18]), "lazarobox cannot update itself here. Run:");
        assert_eq!(body(&l[19]), "cargo install lazarobox-shell --locked");
        assert_eq!(
            body(&l[21]),
            "Enter or y copies it \u{b7} Rust folder is too open"
        );
    }

    #[test]
    fn the_no_rust_explanation_fits_the_popup_without_clipping() {
        let mut menu = with_update();
        menu.goto("Updates");
        menu.show_update_command(crate::core::update::Fallback::NoCargo);
        let l = lines(&draw(80, POPUP_H, &menu));
        let body = |row: &str| row.trim_matches(|c| c == '\u{2502}' || c == ' ').to_owned();
        assert_eq!(body(&l[18]), "Install Rust first (rustup.rs), then run:");
        assert_eq!(body(&l[19]), "cargo install lazarobox-shell --locked");
        assert_eq!(
            body(&l[21]),
            "Enter or y copies it \u{b7} Rust is not installed"
        );
    }

    #[test]
    fn the_update_note_stays_inside_the_body_at_the_smallest_heights() {
        // The note is part of the fixed body: it never gives way, and the
        // blank row is the only thing the short height drops.
        for height in [POPUP_H, MIN_HEIGHT] {
            let shown = text(&draw(62, height, &with_update()));
            assert!(shown.contains("Go to Updates and press Enter."), "{shown}");
            assert!(shown.contains("lazarobox 0.16.0 is available."), "{shown}");
            assert!(shown.contains("s save"), "the footer: {shown}");
        }
    }

    #[test]
    fn narrow_and_short_terminals_fall_back_to_one_line() {
        assert!(text(&draw(22, 20, &mixed())).contains("MENU"));
        assert!(text(&draw(62, 14, &mixed())).contains("MENU"));
        assert!(text(&draw(24, MIN_HEIGHT - 1, &mixed())).contains("MENU"));
        assert!(!text(&draw(24, MIN_HEIGHT, &mixed())).contains("MENU"));
        assert!(text(&draw(24, MIN_HEIGHT, &mixed())).contains("Menu"));
    }

    #[test]
    fn at_24_columns_the_star_drops_the_hint_and_the_plain_label_keeps_it() {
        let starred = lines(&draw(24, MIN_HEIGHT, &with_update()));
        assert!(
            starred[1].starts_with("│ Settings* │ AI "),
            "{}",
            starred[1]
        );
        assert!(!starred[1].contains('⇥'), "{}", starred[1]);
        let plain = lines(&draw(24, MIN_HEIGHT, &mixed()));
        assert!(plain[1].contains("⇥ Tab"), "{}", plain[1]);
    }

    #[test]
    fn the_no_ai_popup_keeps_its_size() {
        let l = lines(&draw(60, 11, &fresh()));
        let border: Vec<usize> = (0..l.len())
            .filter(|y| l[*y].contains('┌') || l[*y].contains('└'))
            .collect();
        assert_eq!(border[1] - border[0] + 1, 10, "6 rows and the chrome");
    }

    #[test]
    fn tabbed_views_never_panic_at_any_size() {
        let mut views = vec![mixed(), ai_tab(), with_update()];
        let mut ai_update = with_update();
        tab_key(&mut ai_update);
        views.push(ai_update);
        views.extend(sub_levels());
        let mut error = mixed();
        error.set_error("config.toml: ai is not a table; fix it before saving".into());
        views.push(error);
        let mut error_ai = ai_tab();
        error_ai.set_error("save failed: disk full".into());
        views.push(error_ai);
        for view in &views {
            for w in 0..=70 {
                for h in 0..=20 {
                    draw(w, h, view);
                }
            }
        }
    }

    #[test]
    fn snapshot_ai_tab() {
        insta::assert_snapshot!(text(&draw(62, POPUP_H, &ai_tab())));
    }

    // Spec (config-menu, Hugging Face account row): Account row.
    #[test]
    fn snapshot_the_account_row_is_last_and_not_connected() {
        let mut menu = ai_tab();
        menu.goto("Hugging Face");
        let shown = text(&draw(62, POPUP_H, &menu));
        let at = |label: &str| {
            shown
                .lines()
                .position(|l| l.contains(label))
                .unwrap_or_else(|| panic!("no {label}"))
        };
        assert!(at("Approval") < at("Hugging Face"));
        let row = shown.lines().nth(at("Hugging Face")).unwrap();
        assert!(row.contains("not connected"), "{row}");
        insta::assert_snapshot!(shown);
    }

    // Spec (config-menu, Hugging Face account row): The field hides the token.
    #[test]
    fn snapshot_the_token_field_hides_the_token() {
        let mut menu = ai_tab();
        menu.goto("Hugging Face");
        press(&mut menu, &[KeyCode::Enter]);
        menu.on_paste("hf_abc");
        let shown = text(&draw(62, POPUP_H, &menu));
        assert!(shown.contains(" ••••••"), "{shown}");
        assert!(shown.contains("paste a Hugging Face token"), "{shown}");
        // The link to create a token fits on one line, so terminals that
        // detect URLs keep it clickable.
        assert!(
            shown.contains(" https://huggingface.co/settings/tokens/new?tokenType=read "),
            "{shown}"
        );
        // No run of cells holds the token, nor any part of it.
        assert!(!shown.contains("hf_"), "{shown}");
        insta::assert_snapshot!(shown);
    }

    // Spec (config-menu, Feedback): the form fits the narrow popup, with what
    // is attached shown.
    #[test]
    fn snapshot_the_feedback_form_in_the_narrow_popup() {
        let mut menu =
            MenuState::open(&Config::default()).with_feedback(crate::core::menu::feedback::Facts {
                version: "0.25.1".to_owned(),
                os: "linux".to_owned(),
                wsl: true,
                shell: Some("zsh".to_owned()),
                terminal: Some("WezTerm".to_owned()),
                engines: Some(vec!["ollama 0.12.3 available".to_owned()]),
            });
        menu.goto("Feedback");
        press(&mut menu, &[KeyCode::Enter, KeyCode::Tab]);
        menu.on_paste("Scroll jumps to the top");
        press(&mut menu, &[KeyCode::Tab]);
        menu.on_paste("When a long answer streams, the view jumps back to the first line.");
        let shown = text(&draw(51, 24, &menu));
        assert!(shown.contains("› Description"), "{shown}");
        insta::assert_snapshot!(shown);
    }

    // Spec (config-menu): `run` only changes the value of the row, never the
    // popup.
    #[test]
    fn the_run_value_shows_and_the_popup_keeps_its_height() {
        let ask = ai_tab();
        let mut run = ai_tab();
        run.goto("Approval");
        press(&mut run, &[KeyCode::Char('l'), KeyCode::Char('l')]);
        let (ask, run) = (
            lines(&draw(62, POPUP_H, &ask)),
            lines(&draw(62, POPUP_H, &run)),
        );
        assert_eq!(ask.len(), run.len());
        let row = |lines: &[String]| {
            lines
                .iter()
                .find(|l| l.contains("Approval"))
                .cloned()
                .unwrap_or_default()
        };
        assert!(row(&ask).contains("ask"), "{:?}", row(&ask));
        assert!(row(&run).contains("run"), "{:?}", row(&run));
        let others = |lines: &[String]| {
            lines
                .iter()
                // The footer reads "unsaved changes" once the draft differs.
                .filter(|l| {
                    !l.contains("Approval") && !l.contains("s to save") && !l.contains("s save")
                })
                .cloned()
                .collect::<Vec<_>>()
        };
        assert_eq!(others(&ask), others(&run), "no other row changes");
    }

    #[test]
    fn snapshot_settings_tab_with_the_update_note() {
        insta::assert_snapshot!(text(&draw(62, POPUP_H, &with_update())));
    }

    #[test]
    fn snapshot_24_columns_with_the_starred_label() {
        insta::assert_snapshot!(text(&draw(MIN_WIDTH, MIN_HEIGHT, &with_update())));
    }
}

#[cfg(test)]
mod tab_line_tests {
    use super::*;

    const HINT: &str = "⇥ Tab";

    fn theme() -> LazaroboxTheme {
        LazaroboxTheme::default()
    }

    fn line(
        labels: &[&str],
        active: usize,
        width: usize,
        hint: Option<&str>,
    ) -> Vec<(String, Style)> {
        tab_line(labels, active, width, hint, &theme())
    }

    fn joined(segments: &[(String, Style)]) -> String {
        segments.iter().map(|(text, _)| text.as_str()).collect()
    }

    const TWO: [&str; 2] = ["Settings", "AI"];
    const STAR: [&str; 2] = ["Settings*", "AI"];
    const THREE: [&str; 3] = ["Settings", "AI", "MCP"];
    const THREE_STAR: [&str; 3] = ["Settings*", "AI", "MCP"];

    #[test]
    fn two_tabs_with_room_draw_the_bar_and_the_right_aligned_hint() {
        for width in [22, 23, 30, 60] {
            let text = joined(&line(&TWO, 0, width, Some(HINT)));
            assert_eq!(cells(&text), width, "W {width}");
            assert!(text.starts_with(" Settings │ AI "), "{text:?}");
            assert!(
                text.ends_with("⇥ Tab "),
                "one column before the edge: {text:?}"
            );
        }
    }

    #[test]
    fn the_bar_is_15_cells_and_16_with_the_update_star() {
        assert_eq!(joined(&line(&TWO, 0, 21, Some(HINT))), " Settings │ AI ");
        assert_eq!(cells(&joined(&line(&TWO, 0, 21, Some(HINT)))), 15);
        assert_eq!(joined(&line(&STAR, 0, 22, Some(HINT))), " Settings* │ AI ");
        assert_eq!(cells(&joined(&line(&STAR, 0, 22, Some(HINT)))), 16);
    }

    #[test]
    fn the_hint_needs_seven_more_cells_than_the_bar() {
        // BAR 15: hint from W 22. BAR 16 (star): hint from W 23.
        assert!(!joined(&line(&TWO, 0, 21, Some(HINT))).contains('⇥'));
        assert!(joined(&line(&TWO, 0, 22, Some(HINT))).contains('⇥'));
        assert!(!joined(&line(&STAR, 0, 22, Some(HINT))).contains('⇥'));
        assert!(joined(&line(&STAR, 0, 23, Some(HINT))).contains('⇥'));
    }

    #[test]
    fn the_bar_alone_shows_from_its_own_width_up_to_the_hint_threshold() {
        for width in 15..22 {
            assert_eq!(
                joined(&line(&TWO, 1, width, Some(HINT))),
                " Settings │ AI ",
                "W {width}"
            );
        }
        for width in 16..23 {
            assert_eq!(
                joined(&line(&STAR, 1, width, Some(HINT))),
                " Settings* │ AI ",
                "W {width}"
            );
        }
    }

    #[test]
    fn below_the_bar_only_the_active_tab_shows_between_chevrons() {
        assert_eq!(joined(&line(&TWO, 1, 14, Some(HINT))), " ‹ AI › ");
        assert_eq!(joined(&line(&TWO, 0, 14, Some(HINT))), " ‹ Settings › ");
        assert_eq!(joined(&line(&TWO, 1, 8, None)), " ‹ AI › ");
    }

    #[test]
    fn the_active_only_form_is_clipped_to_the_width() {
        for width in 1..8 {
            let text = joined(&line(&TWO, 1, width, Some(HINT)));
            assert_eq!(cells(&text), width, "W {width}: {text:?}");
        }
        assert_eq!(joined(&line(&TWO, 1, 5, None)), " ‹ A…");
    }

    #[test]
    fn three_tabs_use_the_same_step_table() {
        // BAR 21 (22 with the star); the hint adds seven.
        assert_eq!(
            joined(&line(&THREE, 2, 21, Some(HINT))),
            " Settings │ AI │ MCP "
        );
        assert_eq!(cells(&joined(&line(&THREE, 2, 21, Some(HINT)))), 21);
        assert!(!joined(&line(&THREE, 0, 27, Some(HINT))).contains('⇥'));
        assert!(joined(&line(&THREE, 0, 28, Some(HINT))).contains('⇥'));
        assert_eq!(cells(&joined(&line(&THREE_STAR, 0, 22, Some(HINT)))), 22);
        assert!(!joined(&line(&THREE_STAR, 0, 28, Some(HINT))).contains('⇥'));
        assert!(joined(&line(&THREE_STAR, 0, 29, Some(HINT))).contains('⇥'));
        assert_eq!(joined(&line(&THREE, 1, 20, Some(HINT))), " ‹ AI › ");
    }

    #[test]
    fn a_zero_or_one_cell_width_never_panics() {
        for hint in [Some(HINT), None] {
            for width in 0..=1 {
                for labels in [&TWO[..], &THREE[..], &STAR[..]] {
                    for active in 0..labels.len() {
                        let text = joined(&line(labels, active, width, hint));
                        assert!(cells(&text) <= width, "{text:?}");
                    }
                }
            }
        }
        assert_eq!(joined(&line(&TWO, 0, 0, Some(HINT))), "");
    }

    #[test]
    fn the_active_segment_is_bold_dark_on_cyan_and_the_rest_is_muted() {
        let theme = theme();
        let segments = line(&TWO, 0, 30, Some(HINT));
        let style_of = |needle: &str| {
            segments
                .iter()
                .find(|(text, _)| text.contains(needle))
                .map(|(_, style)| *style)
                .unwrap_or_else(|| panic!("no segment with {needle}"))
        };
        let active = Style::default()
            .fg(theme.bg_base)
            .bg(theme.primary_cyan)
            .add_modifier(Modifier::BOLD);
        let muted = Style::default().fg(theme.text_muted);
        assert_eq!(style_of("Settings"), active);
        assert_eq!(style_of("AI"), muted);
        assert_eq!(style_of("│"), muted);
        assert_eq!(style_of("⇥"), muted);
        // The second tab active swaps the styles.
        let segments = line(&TWO, 1, 30, Some(HINT));
        let ai = segments.iter().find(|(t, _)| t.contains("AI")).unwrap().1;
        let settings = segments
            .iter()
            .find(|(t, _)| t.contains("Settings"))
            .unwrap()
            .1;
        assert_eq!(ai, active);
        assert_eq!(settings, muted);
    }

    #[test]
    fn the_active_only_form_keeps_the_active_style() {
        let theme = theme();
        let segments = line(&TWO, 1, 10, Some(HINT));
        assert!(
            segments
                .iter()
                .all(|(_, s)| s.bg == Some(theme.primary_cyan))
        );
    }

    #[test]
    fn the_active_style_differs_from_the_selected_row_style() {
        let theme = theme();
        let active = line(&TWO, 0, 30, None)[0].1;
        let selected = Style::default()
            .fg(theme.info_blue)
            .add_modifier(Modifier::REVERSED);
        assert_ne!(active, selected);
        assert!(!active.add_modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn without_a_hint_the_bar_is_alone_at_any_width() {
        for width in [15, 22, 30, 80] {
            let text = joined(&line(&TWO, 0, width, None));
            assert_eq!(text, " Settings │ AI ", "W {width}");
            assert!(!text.contains('⇥'));
        }
    }
}
