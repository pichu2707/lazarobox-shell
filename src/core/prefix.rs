//! Prefix key and its action table, kept as data so rebinding is a one-line change.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// A key plus the exact modifiers it must be pressed with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyChord {
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

impl KeyChord {
    pub fn matches(&self, key: &KeyEvent) -> bool {
        key.code == self.code && key.modifiers == self.mods
    }
}

/// What a key pressed after the prefix does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrefixAction {
    /// Forward the prefix itself (NUL) to the child.
    SendPrefixLiteral,
    RequestQuit,
    EnterCopy,
}

/// Ctrl+Space; crossterm reports the NUL byte as `Char(' ')` + CONTROL.
pub const PREFIX_KEY: KeyChord = KeyChord {
    code: KeyCode::Char(' '),
    mods: KeyModifiers::CONTROL,
};

/// One prefix binding. `description` is the human-readable text for a
/// future which-key / help viewer, so this table is the single source of truth.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PrefixBinding {
    pub chord: KeyChord,
    pub action: PrefixAction,
    pub description: &'static str,
}

pub const PREFIX_BINDINGS: &[PrefixBinding] = &[
    PrefixBinding {
        chord: PREFIX_KEY,
        action: PrefixAction::SendPrefixLiteral,
        description: "Send the prefix key to the program",
    },
    PrefixBinding {
        chord: KeyChord {
            code: KeyCode::Char('q'),
            mods: KeyModifiers::NONE,
        },
        action: PrefixAction::RequestQuit,
        description: "Quit",
    },
    PrefixBinding {
        chord: KeyChord {
            code: KeyCode::Char('['),
            mods: KeyModifiers::NONE,
        },
        action: PrefixAction::EnterCopy,
        description: "Copy mode (scrollback)",
    },
];

/// Action bound to `key` after the prefix; `None` (including Esc) cancels.
pub fn lookup(key: &KeyEvent) -> Option<PrefixAction> {
    PREFIX_BINDINGS
        .iter()
        .find(|binding| binding.chord.matches(key))
        .map(|binding| binding.action)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
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

    #[test]
    fn lookup_ctrl_space_sends_the_literal_prefix() {
        let ev = key(KeyCode::Char(' '), KeyModifiers::CONTROL);
        assert_eq!(lookup(&ev), Some(PrefixAction::SendPrefixLiteral));
    }

    #[test]
    fn lookup_q_requests_quit() {
        let ev = key(KeyCode::Char('q'), KeyModifiers::NONE);
        assert_eq!(lookup(&ev), Some(PrefixAction::RequestQuit));
    }

    #[test]
    fn lookup_open_bracket_enters_copy() {
        let ev = key(KeyCode::Char('['), KeyModifiers::NONE);
        assert_eq!(lookup(&ev), Some(PrefixAction::EnterCopy));
    }

    #[test]
    fn lookup_unmapped_keys_and_esc_cancel() {
        assert_eq!(lookup(&key(KeyCode::Esc, KeyModifiers::NONE)), None);
        assert_eq!(lookup(&key(KeyCode::Char('z'), KeyModifiers::NONE)), None);
    }

    #[test]
    fn lookup_requires_exact_modifiers() {
        assert_eq!(
            lookup(&key(KeyCode::Char('q'), KeyModifiers::CONTROL)),
            None
        );
        assert_eq!(lookup(&key(KeyCode::Char(' '), KeyModifiers::NONE)), None);
    }

    #[test]
    fn every_binding_is_reachable_through_lookup() {
        for binding in PREFIX_BINDINGS {
            let ev = key(binding.chord.code, binding.chord.mods);
            assert_eq!(lookup(&ev), Some(binding.action));
        }
    }

    #[test]
    fn every_binding_has_a_non_empty_description() {
        assert!(!PREFIX_BINDINGS.is_empty());
        for binding in PREFIX_BINDINGS {
            assert!(
                !binding.description.trim().is_empty(),
                "{:?} has no description",
                binding.action
            );
        }
    }

    #[test]
    fn descriptions_name_what_each_action_does() {
        let describe = |c: char, m| {
            PREFIX_BINDINGS
                .iter()
                .find(|b| b.chord.matches(&key(KeyCode::Char(c), m)))
                .map(|b| b.description)
        };
        assert_eq!(describe('q', KeyModifiers::NONE), Some("Quit"));
        assert_eq!(
            describe('[', KeyModifiers::NONE),
            Some("Copy mode (scrollback)")
        );
        assert_eq!(
            describe(' ', KeyModifiers::CONTROL),
            Some("Send the prefix key to the program")
        );
    }

    #[test]
    fn binding_keys_and_actions_are_unique() {
        for (i, a) in PREFIX_BINDINGS.iter().enumerate() {
            for b in &PREFIX_BINDINGS[i + 1..] {
                assert_ne!(a.chord, b.chord, "duplicate key for {:?}", a.action);
                assert_ne!(a.action, b.action, "duplicate action {:?}", a.action);
            }
        }
    }
}
