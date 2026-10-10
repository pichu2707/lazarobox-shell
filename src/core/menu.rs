//! Config menu: a static descriptor table plus the pure state machine that
//! drives it. The popup only renders `MenuState::rows`, so a new setting is a
//! table entry and never a widget change.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::config::{BarPosition, Config, ConfigEdit};

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

/// The selectable settings, in table order.
fn choices() -> impl Iterator<Item = (&'static Item, &'static Choice)> {
    SECTIONS
        .iter()
        .flat_map(|section| section.items.iter())
        .filter_map(|item| match &item.kind {
            ItemKind::Choice(choice) => Some((item, choice)),
            ItemKind::Disabled => None,
        })
}

/// What a key did, for the app to act on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuCommand {
    None,
    /// The live config changed: relayout.
    Changed,
    Save,
    Cancel,
}

/// One line of the popup body.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Row {
    Title(&'static str),
    Item {
        label: &'static str,
        value: Option<&'static str>,
        selected: bool,
        enabled: bool,
    },
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MenuState {
    selected: Option<usize>,
    original: Config,
    error: Option<String>,
}

impl MenuState {
    pub fn open(original: &Config) -> Self {
        Self {
            selected: (choices().count() > 0).then_some(0),
            original: *original,
            error: None,
        }
    }

    pub fn rows(&self, config: &Config) -> Vec<Row> {
        let mut rows = Vec::new();
        let mut index = 0;
        for section in SECTIONS {
            rows.push(Row::Title(section.title));
            for item in section.items {
                rows.push(match &item.kind {
                    ItemKind::Choice(choice) => {
                        let row = Row::Item {
                            label: item.label,
                            value: Some(choice.values[(choice.get)(config)]),
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
        if !key.modifiers.difference(KeyModifiers::SHIFT).is_empty() {
            return MenuCommand::None;
        }
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.step_selection(true),
            KeyCode::Char('k') | KeyCode::Up => self.step_selection(false),
            KeyCode::Char('l' | ' ') | KeyCode::Right => self.cycle(true, config),
            KeyCode::Char('h') | KeyCode::Left => self.cycle(false, config),
            KeyCode::Enter if !repeat => MenuCommand::Save,
            KeyCode::Esc if !repeat => MenuCommand::Cancel,
            _ => MenuCommand::None,
        }
    }

    fn step_selection(&mut self, forward: bool) -> MenuCommand {
        let count = choices().count();
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

    fn cycle(&self, forward: bool, config: &mut Config) -> MenuCommand {
        let Some((_, choice)) = self.selected.and_then(|index| choices().nth(index)) else {
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
        choices()
            .filter(|(_, c)| (c.get)(live) != (c.get)(&self.original))
            .map(|(_, c)| ConfigEdit {
                table: c.table,
                key: c.key,
                value: c.values[(c.get)(live)],
            })
            .collect()
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
            value,
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
                Row::Title("Settings"),
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
    fn enter_saves_and_esc_cancels_on_press_only() {
        let mut menu = MenuState::open(&Config::default());
        let mut config = Config::default();
        assert_eq!(
            menu.on_key(&key(KeyCode::Enter), false, &mut config),
            MenuCommand::Save
        );
        assert_eq!(
            menu.on_key(&key(KeyCode::Esc), false, &mut config),
            MenuCommand::Cancel
        );
        assert_eq!(
            menu.on_key(&key(KeyCode::Enter), true, &mut config),
            MenuCommand::None
        );
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
            value,
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
            MenuCommand::Save
        );
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
