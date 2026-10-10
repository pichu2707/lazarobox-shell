//! Prefix key and its action tree, kept as data so rebinding is a one-line change.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::text::Line;

use super::layout::Direction;

/// A key plus the exact modifiers it must be pressed with. SHIFT is ignored
/// for character keys, because terminals report `B` with or without it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyChord {
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

impl KeyChord {
    pub fn matches(&self, key: &KeyEvent) -> bool {
        key.code == self.code && Self::significant(key.code, key.modifiers) == self.mods
    }

    /// Modifiers that count when matching: SHIFT is already encoded in the
    /// case of a `Char`, so it is dropped for those only.
    fn significant(code: KeyCode, mods: KeyModifiers) -> KeyModifiers {
        match code {
            KeyCode::Char(_) => mods - KeyModifiers::SHIFT,
            _ => mods,
        }
    }

    /// Short text for the key hint, e.g. `q`, `Space` or `Ctrl+Space`.
    pub fn label(&self) -> String {
        let key = match self.code {
            KeyCode::Char(' ') => "Space".to_string(),
            code => code.to_string(),
        };
        if self.mods.contains(KeyModifiers::CONTROL) {
            format!("Ctrl+{key}")
        } else {
            key
        }
    }
}

/// What a key pressed after the prefix does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrefixAction {
    /// Forward the prefix itself (NUL) to the child.
    SendPrefixLiteral,
    RequestQuit,
    EnterCopy,
    /// Reserved for the future command viewer; no effect yet.
    ShowCommands,
    Focus(Direction),
    SplitRight,
    SplitBelow,
    ClosePane,
    EnterResize,
    /// Opens the config menu. Not in the table until the `m` binding lands.
    OpenMenu,
    ToggleZoom,
    NewTab,
    CloseTab,
    NextTab,
    PrevTab,
    /// 1-based tab number.
    GotoTab(u8),
}

/// Ctrl+Space; crossterm reports the NUL byte as `Char(' ')` + CONTROL.
pub const PREFIX_KEY: KeyChord = KeyChord {
    code: KeyCode::Char(' '),
    mods: KeyModifiers::CONTROL,
};

/// A named set of bindings opened by a key; `label` names the pending mode.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Group {
    pub label: &'static str,
    pub bindings: &'static [Binding],
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Action(PrefixAction),
    Group(&'static Group),
}

/// One prefix binding. `description` is the human-readable text shared by the
/// key hint and a future command viewer, so the tree is the single source of
/// truth. Unhinted bindings stay out of the hint.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Binding {
    pub chord: KeyChord,
    pub description: &'static str,
    pub target: Target,
    pub hinted: bool,
}

const fn chord(c: char) -> KeyChord {
    KeyChord {
        code: KeyCode::Char(c),
        mods: KeyModifiers::NONE,
    }
}

const fn act(c: char, description: &'static str, action: PrefixAction) -> Binding {
    Binding {
        chord: chord(c),
        description,
        target: Target::Action(action),
        hinted: true,
    }
}

const fn group(c: char, description: &'static str, group: &'static Group) -> Binding {
    Binding {
        chord: chord(c),
        description,
        target: Target::Group(group),
        hinted: true,
    }
}

/// Same binding, left out of the key hint.
const fn unhinted(binding: Binding) -> Binding {
    Binding {
        hinted: false,
        ..binding
    }
}

const WINDOW: Group = Group {
    label: "WINDOW",
    bindings: &[
        act('v', "split right", PrefixAction::SplitRight),
        act('h', "split below", PrefixAction::SplitBelow),
        act('q', "close", PrefixAction::ClosePane),
        act('r', "resize", PrefixAction::EnterResize),
        act('z', "zoom", PrefixAction::ToggleZoom),
    ],
};

const TAB: Group = Group {
    label: "TAB",
    bindings: &[
        act('n', "new", PrefixAction::NewTab),
        act('c', "close", PrefixAction::CloseTab),
    ],
};

const GO: Group = Group {
    label: "GO",
    bindings: &[
        act('b', "next", PrefixAction::NextTab),
        act('B', "prev", PrefixAction::PrevTab),
    ],
};

const BUFFER: Group = Group {
    label: "BUFFER",
    bindings: &[
        act('1', "tab 1", PrefixAction::GotoTab(1)),
        act('2', "tab 2", PrefixAction::GotoTab(2)),
        act('3', "tab 3", PrefixAction::GotoTab(3)),
        act('4', "tab 4", PrefixAction::GotoTab(4)),
        act('5', "tab 5", PrefixAction::GotoTab(5)),
        act('6', "tab 6", PrefixAction::GotoTab(6)),
        act('7', "tab 7", PrefixAction::GotoTab(7)),
        act('8', "tab 8", PrefixAction::GotoTab(8)),
        act('9', "tab 9", PrefixAction::GotoTab(9)),
    ],
};

/// Hinted entries come first so the root hint reads in table order.
pub const PREFIX_TREE: &[Binding] = &[
    group('w', "window", &WINDOW),
    group('t', "tab", &TAB),
    group('g', "go", &GO),
    group('b', "buffer", &BUFFER),
    act('[', "copy", PrefixAction::EnterCopy),
    act('m', "menu", PrefixAction::OpenMenu),
    act('q', "quit", PrefixAction::RequestQuit),
    unhinted(act('h', "focus left", PrefixAction::Focus(Direction::Left))),
    unhinted(act('j', "focus down", PrefixAction::Focus(Direction::Down))),
    unhinted(act('k', "focus up", PrefixAction::Focus(Direction::Up))),
    unhinted(act(
        'l',
        "focus right",
        PrefixAction::Focus(Direction::Right),
    )),
    unhinted(act('?', "commands (reserved)", PrefixAction::ShowCommands)),
    Binding {
        chord: PREFIX_KEY,
        description: "send the prefix key to the program",
        target: Target::Action(PrefixAction::SendPrefixLiteral),
        hinted: false,
    },
];

/// Outcome of a key pressed while the prefix (or a group) is pending.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Run(PrefixAction),
    Enter(&'static Group),
    Cancel,
}

/// Resolve `key` against `table`; unknown keys and Esc cancel.
pub fn lookup(table: &'static [Binding], key: &KeyEvent) -> Step {
    table
        .iter()
        .find(|binding| binding.chord.matches(key))
        .map_or(Step::Cancel, |binding| match binding.target {
            Target::Action(action) => Step::Run(action),
            Target::Group(group) => Step::Enter(group),
        })
}

/// One-line key hint (`v split right · h split below · …`) from the hinted
/// bindings, in table order, clipped by whole entries to `max_cols` columns.
pub fn hint(bindings: &[Binding], max_cols: u16) -> String {
    const SEP: &str = " · ";
    const ELLIPSIS: &str = "…";
    let fits = |text: &str| Line::from(text).width() <= usize::from(max_cols);
    let entries: Vec<String> = bindings
        .iter()
        .filter(|binding| binding.hinted)
        .map(|binding| format!("{} {}", binding.chord.label(), binding.description))
        .collect();
    let full = entries.join(SEP);
    if fits(&full) {
        return full;
    }
    (1..entries.len())
        .rev()
        .map(|kept| format!("{}{SEP}{ELLIPSIS}", entries[..kept].join(SEP)))
        .find(|clipped| fits(clipped))
        .or_else(|| fits(ELLIPSIS).then(|| ELLIPSIS.to_string()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    fn plain(c: char) -> KeyEvent {
        key(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn event(chord: KeyChord) -> KeyEvent {
        key(chord.code, chord.mods)
    }

    /// Every table of the tree: the root first, then each group, depth first.
    fn tables() -> Vec<&'static [Binding]> {
        fn visit(table: &'static [Binding], out: &mut Vec<&'static [Binding]>) {
            out.push(table);
            for binding in table {
                if let Target::Group(group) = binding.target {
                    visit(group.bindings, out);
                }
            }
        }
        let mut out = Vec::new();
        visit(PREFIX_TREE, &mut out);
        out
    }

    fn actions() -> Vec<PrefixAction> {
        tables()
            .into_iter()
            .flatten()
            .filter_map(|b| match b.target {
                Target::Action(action) => Some(action),
                Target::Group(_) => None,
            })
            .collect()
    }

    fn group(table: &'static [Binding], c: char) -> &'static Group {
        match lookup(table, &plain(c)) {
            Step::Enter(group) => group,
            other => panic!("{c} should open a group, got {other:?}"),
        }
    }

    #[test]
    fn prefix_key_is_ctrl_space() {
        assert_eq!(PREFIX_KEY.code, KeyCode::Char(' '));
        assert_eq!(PREFIX_KEY.mods, KeyModifiers::CONTROL);
    }

    #[test]
    fn prefix_key_chord_matches_a_ctrl_space_event() {
        assert!(PREFIX_KEY.matches(&key(KeyCode::Char(' '), KeyModifiers::CONTROL)));
        assert!(!PREFIX_KEY.matches(&key(KeyCode::Char(' '), KeyModifiers::NONE)));
    }

    // --- Table integrity ---

    #[test]
    fn chords_are_unique_within_every_group() {
        for table in tables() {
            for (i, a) in table.iter().enumerate() {
                for b in &table[i + 1..] {
                    assert_ne!(a.chord, b.chord, "duplicate chord {}", a.chord.label());
                }
            }
        }
    }

    #[test]
    fn every_binding_and_group_has_text() {
        assert!(!PREFIX_TREE.is_empty());
        for table in tables() {
            for binding in table {
                assert!(
                    !binding.description.trim().is_empty(),
                    "{} has no description",
                    binding.chord.label()
                );
                assert!(
                    !binding.chord.label().trim().is_empty(),
                    "{:?} has no label",
                    binding.chord
                );
                if let Target::Group(group) = binding.target {
                    assert!(!group.label.trim().is_empty());
                    assert!(!group.bindings.is_empty());
                }
            }
        }
    }

    #[test]
    fn prefix_key_is_bound_at_the_root_only() {
        let root = PREFIX_TREE.iter().filter(|b| b.chord == PREFIX_KEY);
        assert_eq!(
            root.map(|b| b.target).collect::<Vec<_>>(),
            [Target::Action(PrefixAction::SendPrefixLiteral)]
        );
        for table in &tables()[1..] {
            assert!(table.iter().all(|b| b.chord != PREFIX_KEY));
        }
    }

    // Spec: modal-input `m` is in the table; Not from other modes.
    #[test]
    fn m_is_a_hinted_root_leaf_that_opens_the_menu() {
        let binding = PREFIX_TREE
            .iter()
            .find(|b| b.chord.matches(&plain('m')))
            .expect("m is bound at the root");
        assert_eq!(binding.target, Target::Action(PrefixAction::OpenMenu));
        assert_eq!(binding.description, "menu");
        assert!(binding.hinted);
    }

    #[test]
    fn m_inside_a_group_does_not_open_the_menu() {
        for table in &tables()[1..] {
            assert_ne!(
                lookup(table, &plain('m')),
                Step::Run(PrefixAction::OpenMenu)
            );
        }
    }

    #[test]
    fn question_mark_is_reserved_and_unhinted() {
        let binding = PREFIX_TREE
            .iter()
            .find(|b| b.chord.matches(&plain('?')))
            .expect("? is bound");
        assert_eq!(binding.target, Target::Action(PrefixAction::ShowCommands));
        assert!(!binding.hinted);
    }

    #[test]
    fn every_action_is_reachable_exactly_once() {
        let all = actions();
        for (i, a) in all.iter().enumerate() {
            assert!(!all[i + 1..].contains(a), "duplicate action {a:?}");
        }
        // literal + quit + copy + menu + `?` + 4 focus + 5 window + 2 tab + 2 go + 9 goto
        assert_eq!(all.len(), 27);
    }

    #[test]
    fn every_binding_is_reachable_through_lookup() {
        for table in tables() {
            for binding in table {
                let expected = match binding.target {
                    Target::Action(action) => Step::Run(action),
                    Target::Group(group) => Step::Enter(group),
                };
                assert_eq!(lookup(table, &event(binding.chord)), expected);
            }
        }
    }

    #[test]
    fn root_lists_hinted_entries_first_then_the_unhinted_ones() {
        let hinted: Vec<bool> = PREFIX_TREE.iter().map(|b| b.hinted).collect();
        let first_unhinted = hinted.iter().position(|h| !h).expect("has unhinted");
        assert!(hinted[..first_unhinted].iter().all(|h| *h));
        assert!(hinted[first_unhinted..].iter().all(|h| !*h));
        let labels: Vec<String> = PREFIX_TREE[..first_unhinted]
            .iter()
            .map(|b| b.chord.label())
            .collect();
        assert_eq!(labels, ["w", "t", "g", "b", "[", "m", "q"]);
    }

    #[test]
    fn groups_have_the_agreed_labels_and_bindings() {
        let expected = [
            ('w', "WINDOW", "v h q r z"),
            ('t', "TAB", "n c"),
            ('g', "GO", "b B"),
            ('b', "BUFFER", "1 2 3 4 5 6 7 8 9"),
        ];
        for (c, label, keys) in expected {
            let group = group(PREFIX_TREE, c);
            assert_eq!(group.label, label);
            let got: Vec<String> = group.bindings.iter().map(|b| b.chord.label()).collect();
            assert_eq!(got.join(" "), keys, "keys of {label}");
            assert!(group.bindings.iter().all(|b| b.hinted));
        }
    }

    #[test]
    fn the_prefix_key_cancels_inside_a_group() {
        let window = group(PREFIX_TREE, 'w');
        assert_eq!(lookup(window.bindings, &event(PREFIX_KEY)), Step::Cancel);
    }

    #[test]
    fn descriptions_name_what_each_binding_does() {
        let describe = |table: &'static [Binding], c: char| {
            table
                .iter()
                .find(|b| b.chord.matches(&plain(c)))
                .map(|b| b.description)
        };
        assert_eq!(describe(PREFIX_TREE, 'q'), Some("quit"));
        assert_eq!(describe(PREFIX_TREE, '['), Some("copy"));
        assert_eq!(describe(PREFIX_TREE, 'w'), Some("window"));
        let window = group(PREFIX_TREE, 'w').bindings;
        assert_eq!(describe(window, 'v'), Some("split right"));
        assert_eq!(describe(window, 'h'), Some("split below"));
        let go = group(PREFIX_TREE, 'g').bindings;
        assert_eq!(describe(go, 'b'), Some("next"));
        assert_eq!(describe(go, 'B'), Some("prev"));
        assert_eq!(
            describe(group(PREFIX_TREE, 'b').bindings, '3'),
            Some("tab 3")
        );
    }

    // --- lookup ---

    #[test]
    fn lookup_ctrl_space_sends_the_literal_prefix() {
        let ev = key(KeyCode::Char(' '), KeyModifiers::CONTROL);
        assert_eq!(
            lookup(PREFIX_TREE, &ev),
            Step::Run(PrefixAction::SendPrefixLiteral)
        );
    }

    #[test]
    fn lookup_q_requests_quit() {
        assert_eq!(
            lookup(PREFIX_TREE, &plain('q')),
            Step::Run(PrefixAction::RequestQuit)
        );
    }

    #[test]
    fn lookup_open_bracket_enters_copy() {
        assert_eq!(
            lookup(PREFIX_TREE, &plain('[')),
            Step::Run(PrefixAction::EnterCopy)
        );
    }

    #[test]
    fn lookup_focus_keys_and_group_keys() {
        assert_eq!(
            lookup(PREFIX_TREE, &plain('h')),
            Step::Run(PrefixAction::Focus(Direction::Left))
        );
        assert_eq!(
            lookup(PREFIX_TREE, &plain('l')),
            Step::Run(PrefixAction::Focus(Direction::Right))
        );
        // the same key means something else inside a group
        let window = group(PREFIX_TREE, 'w');
        assert_eq!(
            lookup(window.bindings, &plain('h')),
            Step::Run(PrefixAction::SplitBelow)
        );
        assert_eq!(
            lookup(window.bindings, &plain('v')),
            Step::Run(PrefixAction::SplitRight)
        );
    }

    #[test]
    fn lookup_unmapped_keys_and_esc_cancel() {
        let esc = key(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(lookup(PREFIX_TREE, &esc), Step::Cancel);
        assert_eq!(lookup(PREFIX_TREE, &plain('z')), Step::Cancel);
        let window = group(PREFIX_TREE, 'w').bindings;
        assert_eq!(lookup(window, &esc), Step::Cancel);
        assert_eq!(lookup(window, &plain('x')), Step::Cancel);
    }

    #[test]
    fn lookup_requires_exact_ctrl() {
        let ctrl_q = key(KeyCode::Char('q'), KeyModifiers::CONTROL);
        assert_eq!(lookup(PREFIX_TREE, &ctrl_q), Step::Cancel);
        let space = key(KeyCode::Char(' '), KeyModifiers::NONE);
        assert_eq!(lookup(PREFIX_TREE, &space), Step::Cancel);
        let ctrl_alt = KeyModifiers::CONTROL | KeyModifiers::ALT;
        assert_eq!(
            lookup(PREFIX_TREE, &key(KeyCode::Char(' '), ctrl_alt)),
            Step::Cancel
        );
    }

    #[test]
    fn shift_is_ignored_for_character_keys() {
        let go = group(PREFIX_TREE, 'g').bindings;
        let with = key(KeyCode::Char('B'), KeyModifiers::SHIFT);
        let without = key(KeyCode::Char('B'), KeyModifiers::NONE);
        assert_eq!(lookup(go, &with), Step::Run(PrefixAction::PrevTab));
        assert_eq!(lookup(go, &without), Step::Run(PrefixAction::PrevTab));
        // case still decides: lowercase `b` is the next tab
        assert_eq!(lookup(go, &plain('b')), Step::Run(PrefixAction::NextTab));
    }

    #[test]
    fn shift_still_counts_for_non_character_keys() {
        let enter = KeyChord {
            code: KeyCode::Enter,
            mods: KeyModifiers::NONE,
        };
        assert!(enter.matches(&key(KeyCode::Enter, KeyModifiers::NONE)));
        assert!(!enter.matches(&key(KeyCode::Enter, KeyModifiers::SHIFT)));
    }

    // --- label ---

    #[test]
    fn labels_name_the_key() {
        assert_eq!(event_chord('q', KeyModifiers::NONE).label(), "q");
        assert_eq!(event_chord('B', KeyModifiers::NONE).label(), "B");
        assert_eq!(event_chord(' ', KeyModifiers::NONE).label(), "Space");
        assert_eq!(PREFIX_KEY.label(), "Ctrl+Space");
        assert_eq!(event_chord('x', KeyModifiers::CONTROL).label(), "Ctrl+x");
        let esc = KeyChord {
            code: KeyCode::Esc,
            mods: KeyModifiers::NONE,
        };
        assert_eq!(esc.label(), "Esc");
    }

    fn event_chord(c: char, mods: KeyModifiers) -> KeyChord {
        KeyChord {
            code: KeyCode::Char(c),
            mods,
        }
    }

    // --- hint ---

    fn width(text: &str) -> u16 {
        Line::from(text).width() as u16
    }

    const FULL_ROOT: &str = "w window · t tab · g go · b buffer · [ copy · m menu · q quit";

    #[test]
    fn group_hints_list_every_binding_in_table_order() {
        let expected = [
            (
                'w',
                "v split right · h split below · q close · r resize · z zoom",
            ),
            ('t', "n new · c close"),
            ('g', "b next · B prev"),
            (
                'b',
                "1 tab 1 · 2 tab 2 · 3 tab 3 · 4 tab 4 · 5 tab 5 · 6 tab 6 · 7 tab 7 · 8 tab 8 · 9 tab 9",
            ),
        ];
        for (c, text) in expected {
            assert_eq!(hint(group(PREFIX_TREE, c).bindings, 200), text);
        }
    }

    #[test]
    fn root_hint_lists_only_the_hinted_entries() {
        assert_eq!(hint(PREFIX_TREE, 200), FULL_ROOT);
    }

    #[test]
    fn hint_follows_the_hinted_flag_not_the_position() {
        let table = [
            act('a', "alpha", PrefixAction::NewTab),
            unhinted(act('b', "beta", PrefixAction::CloseTab)),
            act('c', "gamma", PrefixAction::NextTab),
        ];
        assert_eq!(hint(&table, 200), "a alpha · c gamma");
        assert_eq!(hint(&table[1..2], 200), "");
        assert_eq!(hint(&[], 200), "");
    }

    #[test]
    fn hint_uses_the_chord_label() {
        let table = [Binding {
            chord: PREFIX_KEY,
            ..act('x', "send", PrefixAction::SendPrefixLiteral)
        }];
        assert_eq!(hint(&table, 200), "Ctrl+Space send");
    }

    #[test]
    fn hint_fits_exactly_at_its_own_width() {
        assert_eq!(width(FULL_ROOT), 61);
        assert_eq!(hint(PREFIX_TREE, 61), FULL_ROOT);
    }

    #[test]
    fn hint_is_clipped_by_whole_entries_with_an_ellipsis() {
        assert_eq!(
            hint(PREFIX_TREE, 60),
            "w window · t tab · g go · b buffer · [ copy · m menu · …"
        );
        // the clipped text is 56 columns, so 56 still fits and 55 drops one more
        assert_eq!(hint(PREFIX_TREE, 56), hint(PREFIX_TREE, 60));
        assert_eq!(
            hint(PREFIX_TREE, 55),
            "w window · t tab · g go · b buffer · [ copy · …"
        );
    }

    #[test]
    fn narrow_widths_degrade_to_an_ellipsis_then_nothing() {
        assert_eq!(hint(PREFIX_TREE, 12), "w window · …");
        assert_eq!(hint(PREFIX_TREE, 11), "…");
        assert_eq!(hint(PREFIX_TREE, 1), "…");
        assert_eq!(hint(PREFIX_TREE, 0), "");
    }

    #[test]
    fn hint_never_exceeds_the_width_nor_shows_a_partial_entry() {
        let entries: Vec<&str> = FULL_ROOT.split(" · ").collect();
        for max in 0..=60u16 {
            let text = hint(PREFIX_TREE, max);
            assert!(width(&text) <= max, "{text:?} is wider than {max}");
            if text == FULL_ROOT || text.is_empty() {
                continue;
            }
            let shown = text.strip_suffix('…').expect("clipped hint ends with …");
            let shown = shown.strip_suffix(" · ").unwrap_or(shown);
            let shown: Vec<&str> = shown.split(" · ").filter(|e| !e.is_empty()).collect();
            assert_eq!(shown, entries[..shown.len()], "partial entry at {max}");
        }
    }
}
