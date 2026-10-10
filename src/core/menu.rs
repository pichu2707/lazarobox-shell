//! Config menu: a static descriptor table plus the pure state machine that
//! drives it. The popup only renders `MenuState::rows`, so a new setting is a
//! table entry and never a widget change.

use ratatui::crossterm::event::{KeyEvent, KeyEventKind, KeyModifiers};

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

    pub fn on_key(&mut self, _key: &KeyEvent, _repeat: bool, _config: &mut Config) -> MenuCommand {
        let _ = (KeyEventKind::Press, KeyModifiers::NONE, BarPosition::Top);
        MenuCommand::None
    }

    /// The keys that differ from the config captured at open.
    pub fn edits(&self, _live: &Config) -> Vec<ConfigEdit> {
        Vec::new()
    }

    pub fn set_error(&mut self, _message: String) {}

    pub fn error(&self) -> Option<&str> {
        None
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
}
