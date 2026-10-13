//! The note editor of a chat: the view the save-note key opens over the last
//! answer. Pure: the keys act on its state and the app does the rest.
//!
//! The text that reaches engram has been through the redaction pass twice:
//! once when the editor opens on the answer, once more on save, where a
//! change keeps the editor open so the user sees what was removed.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::composer::Composer;
use crate::core::ai::memory::{kind::MemKind, port::Note, redact::redact};

/// The most characters of a note's content the editor holds.
pub const CONTENT_CAP: usize = 16_000;
/// The most characters of a note's title.
pub const TITLE_CAP: usize = 120;

/// Notice for the save key with no answer to save.
pub const NOTHING_TO_SAVE: &str = "Nothing to save as a note";
/// Notice for the save key with memory off.
pub const MEMORY_OFF: &str = "Memory is off; enable it in the menu (Ctrl+Space m)";
/// Notice while the first answer about the project is awaited.
pub const MEMORY_CHECKING: &str = "Checking memory…";
/// Hint when the answer was longer than the content field.
pub const CUT_NOTICE: &str = "Cut to 16000 characters";
/// Notice when the second redaction pass changed the text.
pub const REDACTED_AGAIN: &str = "Redacted again; review and save";
/// Notice for a save with no title.
pub const TITLE_REQUIRED: &str = "Title is required";
/// Notice for a save with no content.
pub const CONTENT_REQUIRED: &str = "Content is required";
/// System line when a saved note could not be linked to its conversation.
pub const NOT_LINKED: &str = "note saved but not linked";

/// `Memory is not ready: <reason>`.
pub fn not_ready(reason: &str) -> String {
    format!("Memory is not ready: {reason}")
}

/// The field the keys act on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    Kind,
    Title,
    Content,
}

/// What a key did to the editor, for the app to act on.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum NoteOutcome {
    /// Consumed; nothing for the app to do but redraw.
    Handled,
    /// A valid, stable note to send. The editor now waits for the answer.
    Save(Note),
    /// The user dropped the editor.
    Discard,
}

/// The note editor.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NoteEditor {
    kind: MemKind,
    title: Composer,
    content: Composer,
    field: Field,
    /// What the opening did: secrets redacted, source cut.
    hint: Option<String>,
    /// The last refusal or failure; cleared by typing.
    notice: Option<String>,
    saving: bool,
}

impl NoteEditor {
    /// An editor on `answer`, already redacted and cut to the content cap.
    pub fn seeded(answer: &str) -> Self {
        let redacted = redact(answer);
        let cut = redacted.text.chars().count() > CONTENT_CAP;
        let mut parts = Vec::new();
        match redacted.hits {
            0 => {}
            1 => parts.push("1 secret redacted".to_owned()),
            n => parts.push(format!("{n} secrets redacted")),
        }
        if cut {
            parts.push(CUT_NOTICE.to_owned());
        }
        Self {
            kind: MemKind::Decision,
            title: Composer::with_cap(TITLE_CAP),
            content: filled(CONTENT_CAP, &redacted.text),
            field: Field::Title,
            hint: (!parts.is_empty()).then(|| parts.join(" · ")),
            notice: None,
            saving: false,
        }
    }

    pub fn kind(&self) -> MemKind {
        self.kind
    }

    pub fn title(&self) -> String {
        self.title.text()
    }

    pub fn content(&self) -> String {
        self.content.text()
    }

    pub fn title_composer(&self) -> &Composer {
        &self.title
    }

    pub fn content_composer(&self) -> &Composer {
        &self.content
    }

    pub fn field(&self) -> Field {
        self.field
    }

    /// What the opening did, like `2 secrets redacted`.
    pub fn hint(&self) -> Option<String> {
        self.hint.clone()
    }

    /// The last refusal or failure.
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// A save was sent and its answer has not come.
    pub fn is_saving(&self) -> bool {
        self.saving
    }

    /// The save did not go through: the text stays, the reason shows.
    pub fn save_failed(&mut self, reason: &str) {
        self.saving = false;
        self.notice = Some(reason.to_owned());
    }

    /// One key. Everything is consumed: nothing here reaches the composer
    /// underneath or a PTY, and Ctrl+C never quits. `repeat` marks a held key.
    pub fn on_key(&mut self, key: &KeyEvent, repeat: bool) -> NoteOutcome {
        let (code, mods) = (key.code, key.modifiers);
        if code == KeyCode::Esc && mods == KeyModifiers::NONE {
            return if repeat {
                NoteOutcome::Handled
            } else {
                NoteOutcome::Discard
            };
        }
        if self.saving {
            return NoteOutcome::Handled;
        }
        let plain = mods - KeyModifiers::SHIFT == KeyModifiers::NONE;
        match code {
            KeyCode::Char('s') if mods == KeyModifiers::CONTROL => {
                return if repeat {
                    NoteOutcome::Handled
                } else {
                    self.begin_save()
                };
            }
            KeyCode::Enter if mods == KeyModifiers::NONE => match self.field {
                Field::Title if !repeat => return self.begin_save(),
                Field::Content => self.edit(|c| c.newline()),
                Field::Title | Field::Kind => {}
            },
            KeyCode::Tab if plain => self.move_field(true),
            KeyCode::BackTab => self.move_field(false),
            KeyCode::Left if plain && self.field == Field::Kind => self.cycle_kind(false),
            KeyCode::Right if plain && self.field == Field::Kind => self.cycle_kind(true),
            KeyCode::Char(ch) if plain => self.edit(|c| c.insert(ch)),
            KeyCode::Backspace if plain => self.edit(|c| c.backspace()),
            KeyCode::Delete if plain => self.edit(|c| c.delete()),
            KeyCode::Left if plain => self.edit(|c| c.left()),
            KeyCode::Right if plain => self.edit(|c| c.right()),
            KeyCode::Home if plain => self.edit(|c| c.home()),
            KeyCode::End if plain => self.edit(|c| c.end()),
            KeyCode::Up if plain && self.field == Field::Content => self.edit(|c| c.up()),
            KeyCode::Down if plain && self.field == Field::Content => self.edit(|c| c.down()),
            _ => {}
        }
        NoteOutcome::Handled
    }

    /// A paste into the focused text field. A title is one line: a newline
    /// becomes a space. Nothing is pasted into the kind, or while saving.
    pub fn paste(&mut self, text: &str) {
        if self.saving {
            return;
        }
        match self.field {
            Field::Title => {
                let one_line = text.replace(['\r', '\n'], " ");
                self.edit(|c| c.paste(&one_line));
            }
            Field::Content => self.edit(|c| c.paste(text)),
            Field::Kind => {}
        }
    }

    fn edit(&mut self, change: impl FnOnce(&mut Composer)) {
        let target = match self.field {
            Field::Title => &mut self.title,
            Field::Content => &mut self.content,
            Field::Kind => return,
        };
        change(target);
        self.notice = None;
    }

    fn move_field(&mut self, forward: bool) {
        self.field = match (self.field, forward) {
            (Field::Kind, true) | (Field::Content, false) => Field::Title,
            (Field::Title, true) | (Field::Kind, false) => Field::Content,
            (Field::Content, true) | (Field::Title, false) => Field::Kind,
        };
    }

    fn cycle_kind(&mut self, forward: bool) {
        let all = MemKind::ALL;
        let at = all.iter().position(|&k| k == self.kind).unwrap_or(0);
        self.kind = all[(at + if forward { 1 } else { all.len() - 1 }) % all.len()];
    }

    /// The checks of a save: both fields filled, then the second redaction
    /// pass. Text the pass changes replaces the fields and is not sent.
    fn begin_save(&mut self) -> NoteOutcome {
        let (title, content) = (self.title.text(), self.content.text());
        if title.trim().is_empty() {
            self.field = Field::Title;
            self.notice = Some(TITLE_REQUIRED.to_owned());
            return NoteOutcome::Handled;
        }
        if content.trim().is_empty() {
            self.field = Field::Content;
            self.notice = Some(CONTENT_REQUIRED.to_owned());
            return NoteOutcome::Handled;
        }
        let (new_title, new_content) = (redact(&title), redact(&content));
        if new_title.text != title || new_content.text != content {
            self.title = filled(TITLE_CAP, &new_title.text);
            self.content = filled(CONTENT_CAP, &new_content.text);
            self.notice = Some(REDACTED_AGAIN.to_owned());
            return NoteOutcome::Handled;
        }
        self.notice = None;
        self.saving = true;
        NoteOutcome::Save(Note {
            kind: self.kind,
            title,
            content,
        })
    }
}

/// A composer of `cap` characters holding `text`, the cursor at its end.
fn filled(cap: usize, text: &str) -> Composer {
    let mut composer = Composer::with_cap(cap);
    composer.paste(text);
    composer
}
