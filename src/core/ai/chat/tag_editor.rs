//! The tag editor of a chat (chat-history: Edit tags): one line of
//! comma-separated tags, the first the project tag. Pure: the keys act on its
//! state and the app does the rest.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{composer::Composer, tags::Tags};

/// The most characters of the line: the project tag and 8 extras of 64
/// characters with their separators fit.
pub const LINE_CAP: usize = 640;

/// What the editor edits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TagTarget {
    /// The conversation of the pane, stored or not yet.
    Pane,
    /// The stored conversation with this database id, from the list.
    Row(i64),
}

/// What a key did to the editor, for the app to act on.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TagOutcome {
    /// Consumed; nothing for the app to do but redraw.
    Handled,
    /// Enter: these are the tags to keep.
    Save(Tags),
    /// Esc: the user dropped the editor.
    Discard,
}

/// The tag editor.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TagEditor {
    target: TagTarget,
    line: Composer,
}

impl TagEditor {
    /// An editor on `tags`, the cursor at the end of the line.
    pub fn new(target: TagTarget, tags: &Tags) -> Self {
        let mut line = Composer::with_cap(LINE_CAP);
        line.paste(&tags.line());
        Self { target, line }
    }

    pub fn target(&self) -> TagTarget {
        self.target
    }

    /// The line as typed.
    pub fn text(&self) -> String {
        self.line.text()
    }

    pub fn composer(&self) -> &Composer {
        &self.line
    }

    /// The tags the line holds now, as a save would keep them.
    pub fn preview(&self) -> Tags {
        Tags::parse(&self.line.text())
    }

    /// The part of the line to draw in `width` columns, and the column of the
    /// cursor in it. The line scrolls sideways to keep the cursor in view,
    /// with a column left for it at the end.
    pub fn window(&self, width: usize) -> (String, usize) {
        self.line.window(width)
    }

    /// One key. Everything is consumed: nothing here reaches the composer
    /// underneath or a PTY, and Ctrl+C never quits. `repeat` marks a held key,
    /// which edits but never decides.
    pub fn on_key(&mut self, key: &KeyEvent, repeat: bool) -> TagOutcome {
        let (code, mods) = (key.code, key.modifiers);
        let plain = mods - KeyModifiers::SHIFT == KeyModifiers::NONE;
        match code {
            KeyCode::Esc if mods == KeyModifiers::NONE => {
                return if repeat {
                    TagOutcome::Handled
                } else {
                    TagOutcome::Discard
                };
            }
            KeyCode::Enter if mods == KeyModifiers::NONE => {
                return if repeat {
                    TagOutcome::Handled
                } else {
                    TagOutcome::Save(self.preview())
                };
            }
            KeyCode::Char('u') if mods == KeyModifiers::CONTROL => self.line.clear(),
            KeyCode::Char(ch) if plain => self.line.insert(ch),
            KeyCode::Backspace if plain => self.line.backspace(),
            KeyCode::Delete if plain => self.line.delete(),
            KeyCode::Left if plain => self.line.left(),
            KeyCode::Right if plain => self.line.right(),
            KeyCode::Home if plain => self.line.home(),
            KeyCode::End if plain => self.line.end(),
            _ => {}
        }
        TagOutcome::Handled
    }

    /// A paste: the line is one line, so a line break or a tab becomes a
    /// space.
    pub fn paste(&mut self, text: &str) {
        let one_line = text.replace(['\r', '\n', '\t'], " ");
        self.line.paste(&one_line);
    }
}
