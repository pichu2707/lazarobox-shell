//! Config menu: a static descriptor table plus the pure state machine that
//! drives it. The popup only renders `MenuState::rows`, so a new setting is a
//! table entry and never a widget change.

use std::borrow::Cow;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::{
    ai::{
        Detection,
        config::{AiConfig, Task},
        menu::AiMenu,
        picker::{Level, Mark, Outcome, PickKey, PickRow, Picker, Target},
    },
    config::{BarPosition, Config, ConfigEdit, MEMORY_TABLE},
};

/// A setting that cycles through an ordered list of values. The value string
/// is both the display text and the TOML value.
pub struct Choice {
    pub table: &'static str,
    pub key: &'static str,
    pub values: &'static [&'static str],
    pub get: fn(&Config) -> usize,
    pub set: fn(&mut Config, usize),
}

pub enum ItemKind {
    Choice(Choice),
    /// Drawn but never selectable.
    Disabled,
    /// An AI assignment row; `None` is `Set all tasks`.
    Assign(Option<Task>),
    /// The memory opt-in row: a static `off`/`on` item over the AI draft.
    /// Turning it on without the acknowledgement opens the privacy warning.
    Memory(MemoryRow),
}

/// The descriptor of the `Memory` row; the value is the `enabled` key.
pub struct MemoryRow {
    pub table: &'static str,
    pub key: &'static str,
    pub values: &'static [&'static str],
}

pub struct Item {
    pub label: &'static str,
    pub kind: ItemKind,
}

pub struct Section {
    pub title: &'static str,
    pub items: &'static [Item],
}

fn position_index(position: BarPosition) -> usize {
    match position {
        BarPosition::Top => 0,
        BarPosition::Bottom => 1,
    }
}

fn position_from(index: usize) -> BarPosition {
    if index == 0 {
        BarPosition::Top
    } else {
        BarPosition::Bottom
    }
}

pub static SECTIONS: &[Section] = &[Section {
    title: "Settings",
    items: &[
        Item {
            label: "Statusline position",
            kind: ItemKind::Choice(Choice {
                table: "statusline",
                key: "position",
                values: &["top", "bottom"],
                get: |c| position_index(c.bars.statusline),
                set: |c, i| c.bars.statusline = position_from(i),
            }),
        },
        Item {
            label: "Tab bar position",
            kind: ItemKind::Choice(Choice {
                table: "tabbar",
                key: "position",
                values: &["top", "bottom"],
                get: |c| position_index(c.bars.tabbar),
                set: |c, i| c.bars.tabbar = position_from(i),
            }),
        },
        Item {
            label: "Mouse: off (coming soon)",
            kind: ItemKind::Disabled,
        },
    ],
}];

/// The AI section, attached only by [`MenuState::with_ai`]. The labels of
/// the task rows are the task ids; their key path is `Task::table`. The
/// static rows follow the tasks: `Memory` now, and `Profile` (ai-chat) is to
/// be slotted in BEFORE `Memory`, so the order is `Profile` then `Memory`.
pub static AI_SECTION: Section = Section {
    title: "AI",
    items: &[
        Item {
            label: "Set all tasks",
            kind: ItemKind::Assign(None),
        },
        Item {
            label: "general",
            kind: ItemKind::Assign(Some(Task::General)),
        },
        Item {
            label: "code",
            kind: ItemKind::Assign(Some(Task::Code)),
        },
        Item {
            label: "chat",
            kind: ItemKind::Assign(Some(Task::Chat)),
        },
        Item {
            label: "review",
            kind: ItemKind::Assign(Some(Task::Review)),
        },
        Item {
            label: "summarize",
            kind: ItemKind::Assign(Some(Task::Summarize)),
        },
        Item {
            label: "Memory",
            kind: ItemKind::Memory(MemoryRow {
                table: MEMORY_TABLE,
                key: "enabled",
                values: &["off", "on"],
            }),
        },
    ],
};

/// The selectable settings, in table order.
fn choices() -> impl Iterator<Item = (&'static Item, &'static Choice)> {
    SECTIONS
        .iter()
        .flat_map(|section| section.items.iter())
        .filter_map(|item| match &item.kind {
            ItemKind::Choice(choice) => Some((item, choice)),
            ItemKind::Disabled | ItemKind::Assign(_) | ItemKind::Memory(_) => None,
        })
}

/// Rows around the popup body: two borders, the blank row and the footer.
pub const CHROME: u16 = 4;
/// With the AI section the body is fixed: the popup never jumps between the
/// task list, the picker (a title and `VIEW` rows) and the privacy warning.
/// The task list is the tallest: two titles, three settings, the six task
/// rows and `Memory` (`Profile` adds one more row when ai-chat lands it); a
/// test keeps this equal to the tallest AI view.
pub const AI_BODY: usize = 12;
/// Smallest width that still shows the full popup.
pub const MIN_WIDTH: u16 = 24;

/// Whether a `width` x `height` screen is big enough for the full popup of
/// `rows` body rows (the blank row is the first thing dropped, so the height
/// may lose one). Below it the popup is a one-line fallback.
pub fn fits_size(width: u16, height: u16, rows: usize) -> bool {
    width >= MIN_WIDTH && usize::from(height) >= rows + usize::from(CHROME) - 1
}

/// What a key did, for the app to act on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuCommand {
    None,
    /// The live config changed: relayout.
    Changed,
    Save,
    Cancel,
    /// Run the engine detection again.
    Refresh,
}

const HINT_SETTINGS: &str = "j/k move · h/l change · s save · Esc cancel";
const HINT_TASK: &str = "j/k move · Enter pick · r refresh · s save · Esc cancel";
const HINT_PICKER: &str = "j/k move · Enter choose · r refresh · Esc back · s save";
const HINT_WARNING: &str = "y accept (then s to save) · any other key declines";
/// The privacy warning: the three facts, each line short enough for the AI
/// popup width.
const WARNING_TEXT: [&str; 7] = [
    "Saved notes leave lazarobox's private store (0600).",
    "They become readable by every local process and",
    "every agent that uses engram, and by engram sync",
    "if it is on. lazarobox cannot restrict that.",
    "Notes stay in engram until you delete them, and",
    "secret redaction is best effort.",
    "Once saved, this note is not shown again.",
];
const WARNING_PROMPT: &str = "Turn memory on? (y/n)";
const HINT_CUSTOM: &str = "type a model · Enter ok · Ctrl+U clear · Esc back";

/// Why a picker row cannot be chosen: its mark, or the text of an
/// explanation row. A saved model of an engine that is gone has no mark.
fn disabled_reason(row: &PickRow) -> &str {
    match row.mark {
        Mark::DownloadRequired => "download required",
        Mark::Checking => "checking…",
        _ if row.key == PickKey::Info => &row.label,
        _ => "engine not detected",
    }
}

/// The footer of the popup: the widget only styles it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Footer<'a> {
    Error(&'a str),
    Hint(Cow<'static, str>),
}

/// One line of the popup body.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Row {
    Title(Cow<'static, str>),
    Item {
        label: &'static str,
        value: Option<Cow<'static, str>>,
        selected: bool,
        enabled: bool,
    },
    /// A picker row: the cursor row is `selected`, the current assignment of
    /// the target is `current`, a row that cannot be chosen is not `enabled`.
    Pick {
        label: String,
        mark: Mark,
        selected: bool,
        enabled: bool,
        current: bool,
    },
    /// The custom model field and its buffer.
    Field(String),
    /// A plain line of text (the privacy warning).
    Text(Cow<'static, str>),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MenuState {
    selected: Option<usize>,
    original: Config,
    error: Option<String>,
    ai: Option<AiMenu>,
    /// A detection run is in flight.
    checking: bool,
    /// The terminal size at the last key, `None` before the app gave one.
    term: Option<(u16, u16)>,
}

impl MenuState {
    pub fn open(original: &Config) -> Self {
        Self {
            selected: (choices().count() > 0).then_some(0),
            original: *original,
            error: None,
            ai: None,
            checking: false,
            term: None,
        }
    }

    /// The terminal size, given by the app before each key so the menu knows
    /// whether the popup (or only its one-line fallback) is on screen.
    pub fn set_term(&mut self, cols: u16, rows: u16) {
        self.term = Some((cols, rows));
    }

    /// Whether the privacy warning is readable in full: the popup fits the
    /// terminal. An unknown size counts as fitting.
    pub fn fits_warning(&self) -> bool {
        let body = AI_BODY.max(Self::warning_rows().len());
        self.term
            .is_none_or(|(cols, rows)| fits_size(cols, rows, body))
    }

    /// Adds the AI section, editing a draft of `ai` against `engines`.
    pub fn with_ai(mut self, ai: AiConfig, engines: Option<Detection>) -> Self {
        self.ai = Some(AiMenu::new(ai, engines));
        self
    }

    /// Marks a detection run as in flight (the `refreshing…` cue).
    pub fn set_checking(&mut self, on: bool) {
        self.checking = on;
    }

    /// Replaces the detection in place; selection and draft stay.
    pub fn set_engines(&mut self, engines: Option<Detection>) {
        self.checking = false;
        if let Some(ai) = &mut self.ai {
            ai.set_engines(engines);
        }
    }

    /// The AI choice being edited (unset without an AI section).
    pub fn ai_draft(&self) -> AiConfig {
        self.ai
            .as_ref()
            .map(|ai| ai.draft().clone())
            .unwrap_or_default()
    }

    /// Whether the menu has an AI section.
    pub fn has_ai(&self) -> bool {
        self.ai.is_some()
    }

    /// Whether the selection is on an AI task row.
    pub fn on_ai_row(&self) -> bool {
        self.ai.is_some()
            && self
                .selected
                .is_some_and(|index| index >= choices().count())
    }

    /// Whether the selection is on the `Memory` row.
    pub fn on_memory_row(&self) -> bool {
        let Some(first) = self.selected.and_then(|i| i.checked_sub(choices().count())) else {
            return false;
        };
        self.ai.is_some()
            && AI_SECTION
                .items
                .get(first)
                .is_some_and(|item| matches!(item.kind, ItemKind::Memory(_)))
    }

    /// Whether the privacy warning is shown over the menu.
    pub fn in_warning(&self) -> bool {
        self.ai.as_ref().is_some_and(AiMenu::warning)
    }

    /// Whether the selection is on a task row (an assignment, not `Memory`).
    fn on_task_row(&self) -> bool {
        self.on_ai_row() && !self.on_memory_row()
    }

    /// The privacy warning that replaces the rows while it is open.
    fn warning_rows() -> Vec<Row> {
        let mut rows = vec![Row::Title(Cow::Borrowed("Memory: save notes to engram"))];
        rows.extend(WARNING_TEXT.iter().map(|l| Row::Text(Cow::Borrowed(*l))));
        rows.push(Row::Text(Cow::Borrowed("")));
        rows.push(Row::Title(Cow::Borrowed(WARNING_PROMPT)));
        rows
    }

    /// Keys while the warning is open: only an unmodified lowercase `y`
    /// accepts, everything else declines, and Esc never leaves the menu.
    /// When the popup does not fit the facts are hidden behind the one-line
    /// fallback, so nothing accepts then (informed consent). Repeat events
    /// are ignored.
    fn on_warning_key(&mut self, key: &KeyEvent, repeat: bool) -> MenuCommand {
        if repeat {
            return MenuCommand::None;
        }
        let accept =
            self.fits_warning() && key.code == KeyCode::Char('y') && key.modifiers.is_empty();
        if let Some(ai) = &mut self.ai {
            ai.answer_warning(accept);
        }
        MenuCommand::None
    }

    /// Selectable rows: the settings, then the AI rows.
    fn selectable(&self) -> usize {
        choices().count()
            + if self.ai.is_some() {
                AI_SECTION.items.len()
            } else {
                0
            }
    }

    /// The picker view: a title and the window of rows around the cursor.
    fn picker_rows(&self) -> Option<Vec<Row>> {
        let ai = self.ai.as_ref()?;
        let picker = ai.picker()?;
        let all = ai.pick_rows();
        let row = match picker.target {
            Target::All => "Set all tasks",
            Target::Task(task) => task.id(),
        };
        let mut title = match &picker.level {
            Level::Engines => format!("Engine for {row}"),
            Level::Models { engine } | Level::Custom { engine, .. } => {
                format!("Model for {row} ({engine})")
            }
        };
        if let Some(counter) = picker.counter(all.len()) {
            title = format!("{title} {counter}");
        }
        let mut rows = vec![Row::Title(Cow::Owned(title))];
        if let Some(text) = picker.text() {
            rows.push(Row::Field(text.to_owned()));
            return Some(rows);
        }
        for index in picker.visible(all.len()) {
            let pick = &all[index];
            rows.push(Row::Pick {
                label: pick.label.clone(),
                mark: pick.mark.clone(),
                selected: index == picker.cursor(),
                enabled: pick.enabled,
                current: pick.current,
            });
        }
        Some(rows)
    }

    /// The target of the selected AI row.
    fn selected_target(&self) -> Option<Target> {
        match self.selected?.checked_sub(choices().count())? {
            0 => Some(Target::All),
            n => Task::ALL.get(n - 1).map(|task| Target::Task(*task)),
        }
    }

    pub fn rows(&self, config: &Config) -> Vec<Row> {
        if self.in_warning() {
            return Self::warning_rows();
        }
        if let Some(rows) = self.picker_rows() {
            return rows;
        }
        let mut rows = Vec::new();
        let mut index = 0;
        let ai_section = self.ai.as_ref().map(|_| &AI_SECTION);
        for section in SECTIONS.iter().chain(ai_section) {
            rows.push(Row::Title(Cow::Borrowed(section.title)));
            for item in section.items {
                rows.push(match &item.kind {
                    ItemKind::Assign(task) => {
                        let row = Row::Item {
                            label: item.label,
                            value: self.ai.as_ref().map(|ai| Cow::Owned(ai.value(*task))),
                            selected: self.selected == Some(index),
                            enabled: true,
                        };
                        index += 1;
                        row
                    }
                    ItemKind::Memory(memory) => {
                        let on = self
                            .ai
                            .as_ref()
                            .is_some_and(|ai| ai.draft().memory.active());
                        let row = Row::Item {
                            label: item.label,
                            value: Some(Cow::Borrowed(memory.values[usize::from(on)])),
                            selected: self.selected == Some(index),
                            enabled: true,
                        };
                        index += 1;
                        row
                    }
                    ItemKind::Choice(choice) => {
                        let row = Row::Item {
                            label: item.label,
                            value: Some(Cow::Borrowed(choice.values[(choice.get)(config)])),
                            selected: self.selected == Some(index),
                            enabled: true,
                        };
                        index += 1;
                        row
                    }
                    ItemKind::Disabled => Row::Item {
                        label: item.label,
                        value: None,
                        selected: false,
                        enabled: false,
                    },
                });
            }
        }
        rows
    }

    /// Interprets a key. Release is ignored; a repeat moves and cycles but
    /// never saves or cancels; a pending error is cleared by the next press.
    pub fn on_key(&mut self, key: &KeyEvent, repeat: bool, config: &mut Config) -> MenuCommand {
        if key.kind == KeyEventKind::Release {
            return MenuCommand::None;
        }
        if !repeat {
            self.error = None;
        }
        if self.in_warning() {
            return self.on_warning_key(key, repeat);
        }
        if self.ai.as_ref().is_some_and(|ai| ai.picker().is_some()) {
            return self.on_picker_key(key, repeat);
        }
        if !key.modifiers.difference(KeyModifiers::SHIFT).is_empty() {
            return MenuCommand::None;
        }
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.step_selection(true),
            KeyCode::Char('k') | KeyCode::Up => self.step_selection(false),
            KeyCode::Char('l' | ' ') | KeyCode::Right => self.cycle(true, config),
            KeyCode::Char('h') | KeyCode::Left => self.cycle(false, config),
            KeyCode::Char('s') if !repeat => MenuCommand::Save,
            KeyCode::Char('r') if !repeat && self.on_task_row() => MenuCommand::Refresh,
            KeyCode::Enter if !repeat && self.on_task_row() => self.open_picker(),
            KeyCode::Enter if repeat && self.on_ai_row() => MenuCommand::None,
            KeyCode::Enter => self.cycle(true, config),
            KeyCode::Esc if !repeat => MenuCommand::Cancel,
            _ => MenuCommand::None,
        }
    }

    fn open_picker(&mut self) -> MenuCommand {
        if let (Some(target), Some(ai)) = (self.selected_target(), &mut self.ai) {
            ai.open_picker(target);
        }
        MenuCommand::None
    }

    /// Keys while the picker is open: nothing reaches the task list, and Esc
    /// never cancels the menu.
    fn on_picker_key(&mut self, key: &KeyEvent, repeat: bool) -> MenuCommand {
        let Some(ai) = &mut self.ai else {
            return MenuCommand::None;
        };
        let plain = key.modifiers.difference(KeyModifiers::SHIFT).is_empty();
        // The custom field is checked before the modifier guard: Ctrl+U
        // clears it, and every letter, shortcuts included, is typed.
        if ai.picker().is_some_and(|p| p.text().is_some()) {
            match key.code {
                KeyCode::Char('u') if key.modifiers == KeyModifiers::CONTROL => {
                    ai.edit(Picker::clear_text);
                }
                _ if !plain => {}
                KeyCode::Char(c) => ai.edit(|p| p.type_char(c)),
                KeyCode::Backspace => ai.edit(Picker::backspace),
                KeyCode::Enter if !repeat => {
                    ai.drive(Picker::enter);
                }
                KeyCode::Esc if !repeat => {
                    ai.drive(Picker::back);
                }
                _ => {}
            }
            return MenuCommand::None;
        }
        if !plain {
            return MenuCommand::None;
        }
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => ai.edit_with(|p, d, e| p.step(true, d, e)),
            KeyCode::Char('k') | KeyCode::Up => ai.edit_with(|p, d, e| p.step(false, d, e)),
            KeyCode::Enter if !repeat => {
                ai.drive(Picker::enter);
            }
            KeyCode::Esc if !repeat => {
                ai.drive(Picker::back);
            }
            KeyCode::Char('s') if !repeat => return MenuCommand::Save,
            KeyCode::Char('r')
                if !repeat && ai.drive(|p, _, _| p.refresh()) == Outcome::Refresh =>
            {
                return MenuCommand::Refresh;
            }
            _ => {}
        }
        MenuCommand::None
    }

    fn step_selection(&mut self, forward: bool) -> MenuCommand {
        let count = self.selectable();
        debug_assert!(
            count > 0 || self.selected.is_none(),
            "selection without rows"
        );
        self.selected = self.selected.map(|index| {
            if forward {
                (index + 1) % count
            } else {
                (index + count - 1) % count
            }
        });
        MenuCommand::None
    }

    fn cycle(&mut self, forward: bool, config: &mut Config) -> MenuCommand {
        let Some(index) = self.selected else {
            return MenuCommand::None;
        };
        if self.on_memory_row() {
            if let Some(ai) = &mut self.ai {
                ai.toggle_memory();
            }
            return MenuCommand::None;
        }
        let Some((_, choice)) = choices().nth(index) else {
            return MenuCommand::None;
        };
        let len = choice.values.len();
        debug_assert!(len > 0, "a choice needs values to cycle");
        let current = (choice.get)(config);
        let next = if forward {
            (current + 1) % len
        } else {
            (current + len - 1) % len
        };
        (choice.set)(config, next);
        MenuCommand::Changed
    }

    /// The keys that differ from the config captured at open.
    pub fn edits(&self, live: &Config) -> Vec<ConfigEdit> {
        let settings = choices()
            .filter(|(_, c)| (c.get)(live) != (c.get)(&self.original))
            .map(|(_, c)| ConfigEdit {
                table: c.table,
                key: c.key,
                value: Cow::Borrowed(c.values[(c.get)(live)]),
            });
        let tasks = self
            .ai
            .iter()
            .flat_map(AiMenu::changed_tasks)
            .flat_map(|(task, a)| {
                let value = |v: &Option<String>| Cow::Owned(v.clone().unwrap_or_default());
                [
                    ConfigEdit {
                        table: task.table(),
                        key: "engine",
                        value: value(&a.engine),
                    },
                    ConfigEdit {
                        table: task.table(),
                        key: "model",
                        value: value(&a.model),
                    },
                ]
            });
        let memory = self.ai.iter().flat_map(|ai| {
            ai.changed_memory()
                .into_iter()
                .map(|(key, on)| ConfigEdit::memory_flag(key, on))
        });
        settings.chain(tasks).chain(memory).collect()
    }

    /// A paste: goes to the custom field when it is focused, else ignored.
    pub fn on_paste(&mut self, text: &str) {
        if let Some(ai) = &mut self.ai {
            ai.edit(|p| p.paste(text));
        }
    }

    /// The config captured when the menu opened.
    pub fn original(&self) -> Config {
        self.original
    }

    /// The footer line: a pending error, else the hint of the current view.
    pub fn footer(&self) -> Footer<'_> {
        if let Some(error) = self.error() {
            return Footer::Error(error);
        }
        if self.in_warning() {
            return Footer::Hint(Cow::Borrowed(HINT_WARNING));
        }
        let ai = self.ai.as_ref();
        let picker = ai.and_then(AiMenu::picker);
        if picker.is_some_and(|p| p.text().is_some()) {
            return Footer::Hint(Cow::Borrowed(HINT_CUSTOM));
        }
        if let Some(row) = ai.and_then(AiMenu::cursor_row).filter(|r| !r.enabled) {
            let reason = disabled_reason(&row);
            return Footer::Hint(Cow::Owned(format!("{reason} · not selectable")));
        }
        let in_ai_view = picker.is_some() || self.on_task_row();
        let cue = ai.and_then(|ai| match (ai.has_result(), self.checking) {
            (false, _) => Some("checking engines…"),
            (true, true) => Some("refreshing…"),
            (true, false) => None,
        });
        Footer::Hint(match cue {
            Some(cue) if in_ai_view => {
                let leave = if picker.is_some() { "back" } else { "cancel" };
                Cow::Owned(format!("{cue} · s save · Esc {leave}"))
            }
            _ if picker.is_some() => Cow::Borrowed(HINT_PICKER),
            _ if in_ai_view => Cow::Borrowed(HINT_TASK),
            _ => Cow::Borrowed(HINT_SETTINGS),
        })
    }

    pub fn set_error(&mut self, message: String) {
        self.error = Some(message);
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> impl Iterator<Item = &'static Item> {
        SECTIONS.iter().flat_map(|s| s.items.iter())
    }

    #[test]
    fn descriptor_table_is_well_formed() {
        assert!(!SECTIONS.is_empty());
        let mut paths = Vec::new();
        for section in SECTIONS {
            assert!(!section.title.is_empty());
        }
        for item in items() {
            assert!(!item.label.is_empty());
            if let ItemKind::Choice(c) = &item.kind {
                assert!(!c.table.is_empty() && !c.key.is_empty());
                assert!(c.values.len() >= 2);
                paths.push((c.table, c.key));
            }
        }
        let mut unique = paths.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), paths.len(), "duplicate key path");
    }

    fn row(label: &'static str, value: Option<&'static str>, selected: bool) -> Row {
        Row::Item {
            label,
            value: value.map(Cow::Borrowed),
            selected,
            enabled: value.is_some(),
        }
    }

    #[test]
    fn rows_list_title_then_settings_then_disabled_mouse() {
        let menu = MenuState::open(&Config::default());
        assert_eq!(
            menu.rows(&Config::default()),
            vec![
                Row::Title("Settings".into()),
                row("Statusline position", Some("bottom"), true),
                row("Tab bar position", Some("top"), false),
                row("Mouse: off (coming soon)", None, false),
            ]
        );
    }

    #[test]
    fn rows_show_the_live_values() {
        let mut live = Config::default();
        live.bars.statusline = BarPosition::Top;
        let rows = MenuState::open(&Config::default()).rows(&live);
        assert_eq!(rows[1], row("Statusline position", Some("top"), true));
    }

    #[test]
    fn open_selects_the_first_row_with_no_error() {
        let menu = MenuState::open(&Config::default());
        assert_eq!(menu.selected, Some(0));
        assert_eq!(menu.error(), None);
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ch(c: char) -> KeyEvent {
        key(KeyCode::Char(c))
    }

    fn selected(menu: &MenuState) -> Option<usize> {
        menu.selected
    }

    #[test]
    fn move_keys_wrap_in_both_directions() {
        for (down, up) in [(ch('j'), ch('k')), (key(KeyCode::Down), key(KeyCode::Up))] {
            let mut menu = MenuState::open(&Config::default());
            let mut config = Config::default();
            assert_eq!(menu.on_key(&down, false, &mut config), MenuCommand::None);
            assert_eq!(selected(&menu), Some(1));
            menu.on_key(&down, false, &mut config);
            assert_eq!(selected(&menu), Some(0), "wraps past the last row");
            menu.on_key(&up, false, &mut config);
            assert_eq!(selected(&menu), Some(1), "wraps before the first row");
            menu.on_key(&up, false, &mut config);
            assert_eq!(selected(&menu), Some(0));
        }
    }

    #[test]
    fn repeat_moves_like_a_press() {
        let mut menu = MenuState::open(&Config::default());
        menu.on_key(&ch('j'), true, &mut Config::default());
        assert_eq!(selected(&menu), Some(1));
    }

    #[test]
    fn cycle_keys_change_the_selected_value() {
        let forward = [ch('l'), key(KeyCode::Right), ch(' ')];
        let backward = [ch('h'), key(KeyCode::Left)];
        for k in forward.iter().chain(&backward) {
            let mut menu = MenuState::open(&Config::default());
            let mut config = Config::default();
            assert_eq!(menu.on_key(k, false, &mut config), MenuCommand::Changed);
            assert_eq!(config.bars.statusline, BarPosition::Top, "{k:?}");
            menu.on_key(k, true, &mut config);
            assert_eq!(config.bars.statusline, BarPosition::Bottom, "wraps");
        }
    }

    #[test]
    fn cycling_acts_on_the_selected_row() {
        let mut menu = MenuState::open(&Config::default());
        let mut config = Config::default();
        menu.on_key(&ch('j'), false, &mut config);
        menu.on_key(&ch('h'), false, &mut config);
        assert_eq!(config.bars.tabbar, BarPosition::Bottom);
        assert_eq!(config.bars.statusline, BarPosition::Bottom);
    }

    #[test]
    fn s_saves_and_esc_cancels_on_press_only() {
        let mut menu = MenuState::open(&Config::default());
        let mut config = Config::default();
        assert_eq!(menu.on_key(&ch('s'), false, &mut config), MenuCommand::Save);
        assert_eq!(
            menu.on_key(&key(KeyCode::Esc), false, &mut config),
            MenuCommand::Cancel
        );
        assert_eq!(menu.on_key(&ch('s'), true, &mut config), MenuCommand::None);
        assert_eq!(
            menu.on_key(&key(KeyCode::Esc), true, &mut config),
            MenuCommand::None
        );
    }

    #[test]
    fn release_ctrl_space_and_unknown_keys_are_swallowed() {
        let mut menu = MenuState::open(&Config::default());
        let mut config = Config::default();
        let mut release = ch('l');
        release.kind = KeyEventKind::Release;
        let ctrl_space = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL);
        for k in [release, ctrl_space, ch('x'), key(KeyCode::Tab)] {
            assert_eq!(
                menu.on_key(&k, false, &mut config),
                MenuCommand::None,
                "{k:?}"
            );
        }
        assert_eq!(config, Config::default());
    }

    #[test]
    fn edits_list_only_the_changed_keys() {
        let menu = MenuState::open(&Config::default());
        assert!(menu.edits(&Config::default()).is_empty());
        let mut live = Config::default();
        live.bars.statusline = BarPosition::Top;
        let edit = |table, value| ConfigEdit {
            table,
            key: "position",
            value: Cow::Borrowed(value),
        };
        assert_eq!(menu.edits(&live), vec![edit("statusline", "top")]);
        live.bars.tabbar = BarPosition::Bottom;
        assert_eq!(
            menu.edits(&live),
            vec![edit("statusline", "top"), edit("tabbar", "bottom")]
        );
    }

    #[test]
    fn the_error_clears_on_the_next_press_which_then_acts() {
        let mut menu = MenuState::open(&Config::default());
        let mut config = Config::default();
        menu.set_error("boom".to_owned());
        assert_eq!(menu.error(), Some("boom"));
        menu.on_key(&ch('j'), true, &mut config);
        let mut release = ch('j');
        release.kind = KeyEventKind::Release;
        menu.on_key(&release, false, &mut config);
        assert_eq!(menu.error(), Some("boom"), "repeat and release keep it");
        assert_eq!(
            menu.on_key(&ch('l'), false, &mut config),
            MenuCommand::Changed
        );
        assert_eq!(menu.error(), None);
    }

    #[test]
    fn an_unlisted_key_clears_the_error_with_no_other_effect() {
        let mut menu = MenuState::open(&Config::default());
        let mut config = Config::default();
        menu.set_error("boom".to_owned());
        assert_eq!(menu.on_key(&ch('x'), false, &mut config), MenuCommand::None);
        assert_eq!(menu.error(), None);
        assert_eq!(selected(&menu), Some(0));
        assert_eq!(config, Config::default());
    }

    // Characterization: pins behaviour that already held, so they pass at once.
    #[test]
    fn shift_is_ignored_for_listed_keys_and_uppercase_is_swallowed() {
        let mut menu = MenuState::open(&Config::default());
        let mut config = Config::default();
        let shift_space = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::SHIFT);
        assert_eq!(
            menu.on_key(&shift_space, false, &mut config),
            MenuCommand::Changed
        );
        assert_eq!(config.bars.statusline, BarPosition::Top);
        let shift_enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT);
        assert_eq!(
            menu.on_key(&shift_enter, false, &mut config),
            MenuCommand::Changed
        );
        assert_eq!(config.bars.statusline, BarPosition::Bottom, "cycled back");
        let upper_j = KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT);
        assert_eq!(menu.on_key(&upper_j, false, &mut config), MenuCommand::None);
        assert_eq!(selected(&menu), Some(0), "J is not j");
    }

    // Characterization: the descriptors already round-trip.
    #[test]
    fn every_choice_get_returns_what_set_stored() {
        for (item, choice) in choices() {
            for i in 0..choice.values.len() {
                let mut config = Config::default();
                (choice.set)(&mut config, i);
                assert_eq!((choice.get)(&config), i, "{}", item.label);
            }
        }
    }
}

#[cfg(test)]
mod ai_tests;

#[cfg(test)]
mod picker_tests;

#[cfg(test)]
mod disabled_tests;

#[cfg(test)]
mod custom_flow_tests;

#[cfg(test)]
mod reanchor_tests;

#[cfg(test)]
mod save_tests;

#[cfg(test)]
mod footer_tests;

#[cfg(test)]
mod guard_tests;

#[cfg(test)]
mod edits_tests;

#[cfg(test)]
mod memory_tests;
