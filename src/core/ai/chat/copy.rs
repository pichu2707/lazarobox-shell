//! COPY on a chat pane: the pure key table. The transcript only scrolls by
//! lines (no cursor, selection or yank); the app applies the command.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::core::prefix::PREFIX_KEY;

/// What a key does in chat COPY.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChatCopyCommand {
    /// `j`: one line down.
    Down,
    /// `k`: one line up.
    Up,
    /// Ctrl+d: half a page down.
    HalfDown,
    /// Ctrl+u: half a page up.
    HalfUp,
    /// `gg`: the top of the transcript.
    Top,
    /// `G`: the bottom of the transcript.
    Bottom,
    /// Esc or `q`: back to TERMINAL, following.
    Exit,
    /// Ctrl+Space: the prefix, as in any pane.
    Prefix,
    /// Swallowed.
    Ignore,
}

/// The half-typed `gg`. The view position stays in `ChatState::scroll`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ChatCopy {
    pub pending_g: bool,
}

impl ChatCopy {
    /// Resolves one key. A held exit key (`repeat`) is ignored; every key but
    /// `g` discards a pending `g`.
    pub fn on_key(&mut self, key: &KeyEvent, repeat: bool) -> ChatCopyCommand {
        let was_pending = std::mem::take(&mut self.pending_g);
        if PREFIX_KEY.matches(key) {
            return ChatCopyCommand::Prefix;
        }
        let plain = key.modifiers.difference(KeyModifiers::SHIFT).is_empty();
        let ctrl = key.modifiers == KeyModifiers::CONTROL;
        match key.code {
            KeyCode::Char('g') if plain => {
                if was_pending {
                    ChatCopyCommand::Top
                } else {
                    self.pending_g = true;
                    ChatCopyCommand::Ignore
                }
            }
            KeyCode::Char('j') if plain => ChatCopyCommand::Down,
            KeyCode::Char('k') if plain => ChatCopyCommand::Up,
            KeyCode::Char('G') if plain => ChatCopyCommand::Bottom,
            KeyCode::Char('d') if ctrl => ChatCopyCommand::HalfDown,
            KeyCode::Char('u') if ctrl => ChatCopyCommand::HalfUp,
            KeyCode::Char('q') | KeyCode::Esc if plain && !repeat => ChatCopyCommand::Exit,
            _ => ChatCopyCommand::Ignore,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    fn ch(c: char) -> KeyEvent {
        let mods = if c.is_ascii_uppercase() {
            KeyModifiers::SHIFT
        } else {
            KeyModifiers::NONE
        };
        press(KeyCode::Char(c), mods)
    }

    fn ctrl(c: char) -> KeyEvent {
        press(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn run(keys: &[KeyEvent]) -> Vec<ChatCopyCommand> {
        let mut copy = ChatCopy::default();
        keys.iter().map(|k| copy.on_key(k, false)).collect()
    }

    #[test]
    fn the_scroll_keys_map_to_commands() {
        use ChatCopyCommand::*;
        let keys = [
            ch('j'),
            ch('k'),
            ctrl('d'),
            ctrl('u'),
            ch('G'),
            press(KeyCode::Esc, KeyModifiers::NONE),
            ch('q'),
            press(KeyCode::Char(' '), KeyModifiers::CONTROL),
        ];
        assert_eq!(
            run(&keys),
            [Down, Up, HalfDown, HalfUp, Bottom, Exit, Exit, Prefix]
        );
    }

    #[test]
    fn gg_goes_to_the_top() {
        use ChatCopyCommand::*;
        assert_eq!(run(&[ch('g'), ch('g')]), [Ignore, Top]);
        // The pair is consumed: a third `g` starts a new one.
        assert_eq!(run(&[ch('g'), ch('g'), ch('g')]), [Ignore, Top, Ignore]);
    }

    #[test]
    fn a_lone_g_is_discarded_by_any_other_key() {
        use ChatCopyCommand::*;
        assert_eq!(run(&[ch('g'), ch('j'), ch('g')]), [Ignore, Down, Ignore]);
        assert_eq!(run(&[ch('g'), ch('x'), ch('g')]), [Ignore, Ignore, Ignore]);
        let mut copy = ChatCopy::default();
        copy.on_key(&ch('g'), false);
        copy.on_key(&ctrl('d'), false);
        assert!(!copy.pending_g);
    }

    #[test]
    fn selection_and_editing_keys_are_swallowed() {
        let swallowed = [
            ch('v'),
            ch('V'),
            ch('y'),
            ch('i'),
            ch('h'),
            ch('l'),
            ch('0'),
            ch('$'),
            ch('a'),
            ch('x'),
            press(KeyCode::Enter, KeyModifiers::NONE),
            press(KeyCode::Backspace, KeyModifiers::NONE),
            press(KeyCode::Tab, KeyModifiers::NONE),
            ctrl('c'),
            press(KeyCode::Char('j'), KeyModifiers::ALT),
            press(KeyCode::Char('q'), KeyModifiers::CONTROL),
        ];
        for key in swallowed {
            let mut copy = ChatCopy::default();
            assert_eq!(copy.on_key(&key, false), ChatCopyCommand::Ignore, "{key:?}");
        }
    }

    #[test]
    fn a_held_scroll_key_acts_and_a_held_exit_key_does_not() {
        use ChatCopyCommand::*;
        let mut copy = ChatCopy::default();
        assert_eq!(copy.on_key(&ch('j'), true), Down);
        assert_eq!(copy.on_key(&ch('k'), true), Up);
        assert_eq!(copy.on_key(&ctrl('d'), true), HalfDown);
        assert_eq!(copy.on_key(&ctrl('u'), true), HalfUp);
        assert_eq!(
            copy.on_key(&press(KeyCode::Esc, KeyModifiers::NONE), true),
            Ignore
        );
        assert_eq!(copy.on_key(&ch('q'), true), Ignore);
    }
}
