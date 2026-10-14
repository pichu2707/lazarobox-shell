//! The conversation list, the first view of a chat pane (chat-history).
//! Pure: the keys act on its state and the app does the rest.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{store::ConvRow, tags::Tags, wrap::text_width};
use crate::core::ai::sanitize::{clean, is_format};

/// The prompt shown while a delete waits for its answer.
pub const DELETE_PROMPT: &str = "Delete conversation? (y/n)";
/// What the user sees of a refused note delete.
const NOTES_REFUSED: &str = "engram refused the delete (check ENGRAM_HTTP_TOKEN)";
/// System line when a conversation is open in another pane.
pub const OPEN_ELSEWHERE: &str = "already open in another pane";
/// System line when the delete failed.
pub const DELETE_FAILED: &str = "could not delete conversation";
/// System line when the list could not be read.
pub const LIST_FAILED: &str = "could not read conversations";
/// System line when the tags could not be stored.
pub const TAGS_FAILED: &str = "could not save tags";
/// System line when the conversation could not be read.
pub const RESUME_FAILED: &str = "could not open conversation";

/// What a key did to the list, for the app to act on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ListOutcome {
    /// Handled here; nothing else to do.
    Handled,
    /// Start a new conversation: the list closes.
    New,
    /// Open the conversation with this database id.
    Resume(i64),
    /// The delete was confirmed for this database id. The prompt had no
    /// choice about notes.
    Delete(i64),
    /// `y` in the three-way prompt: delete the conversation, keep its notes
    /// in engram.
    DeleteKeepNotes(i64),
    /// `a` in the three-way prompt: delete the conversation and then its
    /// notes in engram.
    DeleteWithNotes(i64),
    /// Edit the tags of the conversation with this database id.
    EditTags(i64),
}

/// The list of conversations of a chat pane.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ListView {
    /// The project tag proposed for the pane.
    project: Option<String>,
    /// Showing every project (`a`).
    all: bool,
    /// Every conversation of the store, newest first; `None` until the store
    /// answers.
    rows: Option<Vec<ConvRow>>,
    cursor: usize,
    /// The conversation a delete prompt is about, and how many notes the
    /// prompt offered to delete with it (0: the plain prompt).
    confirming: Option<(i64, u32)>,
    /// Whether the app could delete notes in engram now: memory is on and a
    /// token is present. Set by the app before each key.
    notes_deletable: bool,
    /// A line for the user, until the next key.
    line: Option<String>,
}

impl ListView {
    pub fn new(project: Option<String>) -> Self {
        Self {
            project,
            all: false,
            rows: None,
            cursor: 0,
            confirming: None,
            notes_deletable: false,
            line: None,
        }
    }

    /// Tells the list whether notes can be deleted in engram (memory on and a
    /// token present), so the delete prompt of a conversation with notes can
    /// offer it. The app sets it before each key; it is read when `d` is
    /// pressed.
    pub fn set_notes_deletable(&mut self, deletable: bool) {
        self.notes_deletable = deletable;
    }

    /// The store's answer: every conversation, newest first.
    pub fn set_rows(&mut self, rows: Vec<ConvRow>) {
        self.rows = Some(rows);
        self.clamp();
    }

    /// The project tag the list filters by, once the store thread resolved it.
    /// Back to the project's rows.
    pub fn set_project(&mut self, project: Option<String>) {
        self.project = project;
        self.clamp();
    }

    pub fn loaded(&self) -> bool {
        self.rows.is_some()
    }

    pub fn project(&self) -> Option<&str> {
        self.project.as_deref()
    }

    /// Whether every project shows: toggled, or there is no project to filter
    /// by.
    pub fn showing_all(&self) -> bool {
        self.all || self.project.is_none()
    }

    /// The rows on screen: the project's, or all of them.
    pub fn visible(&self) -> Vec<&ConvRow> {
        let all = self.showing_all();
        self.rows
            .iter()
            .flatten()
            .filter(|row| all || row.tag.as_deref() == self.project.as_deref())
            .collect()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The row under the cursor.
    pub fn selected(&self) -> Option<&ConvRow> {
        self.visible().get(self.cursor).copied()
    }

    pub fn title(&self) -> String {
        match (&self.project, self.showing_all()) {
            (Some(project), false) => format!("Conversations · {project}"),
            _ => "Conversations · all projects".to_owned(),
        }
    }

    /// What the list says instead of rows, when it has none to show.
    pub fn empty_text(&self) -> Option<String> {
        let Some(rows) = &self.rows else {
            return Some("Loading conversations…".to_owned());
        };
        if !self.visible().is_empty() {
            return None;
        }
        Some(match (&self.project, rows.is_empty()) {
            (Some(project), false) if !self.showing_all() => {
                format!("No conversations for {project}. Press n to start one.")
            }
            _ => "No conversations yet. Press n to start one.".to_owned(),
        })
    }

    /// Whether a delete waits for its answer.
    pub fn confirming(&self) -> bool {
        self.confirming.is_some()
    }

    /// The delete prompt, while it is shown.
    pub fn prompt(&self) -> Option<String> {
        self.confirming.map(|(_, offered)| match offered {
            0 => DELETE_PROMPT.to_owned(),
            1 => "Delete conversation? y · n · a also delete its 1 note".to_owned(),
            n => format!("Delete conversation? y · n · a also delete its {n} notes"),
        })
    }

    pub fn line(&self) -> Option<&str> {
        self.line.as_deref()
    }

    /// Shows `line` until the next key.
    pub fn set_line(&mut self, line: &str) {
        self.line = Some(line.to_owned());
    }

    /// Drops a conversation from the list (it was deleted).
    pub fn remove(&mut self, id: i64) {
        if let Some(rows) = &mut self.rows {
            rows.retain(|row| row.id != id);
        }
        self.confirming = None;
        self.clamp();
    }

    /// The tags of a row, or `None` when the list has no such row.
    pub fn tags_of(&self, id: i64) -> Option<Tags> {
        let row = self.rows.iter().flatten().find(|row| row.id == id)?;
        Some(Tags {
            project: row.tag.clone(),
            extras: row.extras.clone(),
        })
    }

    /// The store saved new tags for a conversation: its row takes them and the
    /// new last activity, and the filter applies at once.
    pub fn set_tags(&mut self, id: i64, tags: &Tags, updated_at: i64) {
        let selected = self.selected().map(|row| row.id);
        if let Some(rows) = &mut self.rows {
            if let Some(row) = rows.iter_mut().find(|row| row.id == id) {
                row.tag = tags.project.clone();
                row.extras = tags.extras.clone();
                row.updated_at = updated_at;
            }
            // Newest activity first, the higher id first on a tie, as the
            // store lists them.
            rows.sort_by_key(|row| std::cmp::Reverse((row.updated_at, row.id)));
        }
        // The cursor follows its row when the order changes.
        if let Some(at) = selected.and_then(|sel| self.visible().iter().position(|r| r.id == sel)) {
            self.cursor = at;
        }
        self.clamp();
    }

    fn clamp(&mut self) {
        let last = self.visible().len().saturating_sub(1);
        self.cursor = self.cursor.min(last);
    }

    /// One key. Everything is consumed: nothing here reaches a composer or a
    /// PTY, and Ctrl+C never quits. `repeat` marks a held key, which moves the
    /// cursor but never decides anything.
    pub fn on_key(&mut self, key: &KeyEvent, repeat: bool) -> ListOutcome {
        if let Some((id, offered)) = self.confirming {
            if repeat {
                return ListOutcome::Handled;
            }
            self.confirming = None;
            if key.modifiers != KeyModifiers::NONE {
                return ListOutcome::Handled;
            }
            return match (key.code, offered) {
                (KeyCode::Char('y'), 0) => ListOutcome::Delete(id),
                (KeyCode::Char('y'), _) => ListOutcome::DeleteKeepNotes(id),
                (KeyCode::Char('a'), 1..) => ListOutcome::DeleteWithNotes(id),
                _ => ListOutcome::Handled,
            };
        }
        self.line = None;
        if key.modifiers != KeyModifiers::NONE {
            return ListOutcome::Handled;
        }
        let last = self.visible().len().saturating_sub(1);
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.cursor = (self.cursor + 1).min(last),
            KeyCode::Char('k') | KeyCode::Up => self.cursor = self.cursor.saturating_sub(1),
            _ if repeat => {}
            KeyCode::Enter => {
                if let Some(row) = self.selected() {
                    return ListOutcome::Resume(row.id);
                }
            }
            KeyCode::Char('n') | KeyCode::Esc => return ListOutcome::New,
            KeyCode::Char('d') => {
                let deletable = self.notes_deletable;
                self.confirming = self
                    .selected()
                    .map(|row| (row.id, if deletable { row.notes } else { 0 }));
            }
            KeyCode::Char('t') => {
                if let Some(row) = self.selected() {
                    return ListOutcome::EditTags(row.id);
                }
            }
            KeyCode::Char('a') => {
                self.all = !self.all;
                self.cursor = 0;
            }
            _ => {}
        }
        ListOutcome::Handled
    }
}

/// Why the notes of a deleted conversation stay in engram.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NotesStay {
    /// Memory is off.
    MemoryOff,
    /// `ENGRAM_HTTP_TOKEN` is not set (or empty).
    NoToken,
    /// The prompt was the plain one because the list had no note for the
    /// conversation; a note was saved after the list was read.
    SavedLater,
}

/// The system line after a delete that leaves `notes` notes in engram.
pub fn notes_stay_line(notes: usize, why: NotesStay) -> String {
    let why = match why {
        NotesStay::MemoryOff => "memory is off",
        NotesStay::NoToken => "set ENGRAM_HTTP_TOKEN to delete them",
        NotesStay::SavedLater if notes == 1 => "it was saved after the list was read",
        NotesStay::SavedLater => "they were saved after the list was read",
    };
    match notes {
        1 => format!("its 1 note stays in engram ({why})"),
        n => format!("its {n} notes stay in engram ({why})"),
    }
}

/// The system line after the notes of a deleted conversation were deleted one
/// by one: `ok` went, `failed` did not, and `refused` says engram answered
/// 401 or 403 to one of them.
pub fn notes_result_line(ok: usize, failed: usize, refused: bool) -> String {
    let total = ok + failed;
    if failed == 0 {
        return match ok {
            1 => "deleted 1 note from engram".to_owned(),
            n => format!("deleted {n} notes from engram"),
        };
    }
    if ok == 0 {
        let reason = if refused {
            NOTES_REFUSED
        } else {
            "engram did not delete them"
        };
        return format!("could not delete notes: {reason}");
    }
    let line = format!("deleted {ok} of {total} notes from engram");
    if refused {
        format!("{line}; {NOTES_REFUSED}")
    } else {
        line
    }
}

/// The `YYYY-MM-DD` (UTC) of a time in seconds since the Unix epoch.
pub fn date_of(secs: i64) -> String {
    // Civil date from a day count (days since 1970-01-01), proleptic Gregorian.
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// The first message on one line: control and format characters are dropped,
/// every run of whitespace (newlines, tabs) becomes one space.
fn one_line(text: &str) -> String {
    let kept: String = clean(text)
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|c| !c.is_control() && !is_format(*c))
        .collect();
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A row as one line of at most `width` columns: `YYYY-MM-DD · profile ·
/// first message`, cut with `…` when it does not fit.
pub fn row_text(row: &ConvRow, width: usize) -> String {
    let head = format!("{} · {} ·", date_of(row.updated_at), one_line(&row.profile));
    let first = one_line(&row.first);
    let full = if first.is_empty() {
        head
    } else {
        format!("{head} {first}")
    };
    fit(&full, width)
}

/// `text` cut to `width` display columns, the last one an `…` when it was cut.
fn fit(text: &str, width: usize) -> String {
    if text_width(text) <= width {
        return text.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = text_width(c.encode_utf8(&mut [0; 4]));
        if used + w + 1 > width {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests;
