//! The conversation list, the first view of a chat pane (chat-history).
//! Pure: the keys act on its state and the app does the rest.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{store::ConvRow, wrap::text_width};
use crate::core::ai::sanitize::{clean, is_format};

/// The prompt shown while a delete waits for its answer.
pub const DELETE_PROMPT: &str = "Delete conversation? (y/n)";
/// System line when a conversation is open in another pane.
pub const OPEN_ELSEWHERE: &str = "already open in another pane";
/// System line when the delete failed.
pub const DELETE_FAILED: &str = "could not delete conversation";
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
    /// The delete was confirmed for this database id.
    Delete(i64),
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
    /// The conversation a delete prompt is about.
    confirming: Option<i64>,
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
            line: None,
        }
    }

    /// The store's answer: every conversation, newest first.
    pub fn set_rows(&mut self, rows: Vec<ConvRow>) {
        self.rows = Some(rows);
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
        self.confirming.map(|_| DELETE_PROMPT.to_owned())
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

    fn clamp(&mut self) {
        let last = self.visible().len().saturating_sub(1);
        self.cursor = self.cursor.min(last);
    }

    /// One key. Everything is consumed: nothing here reaches a composer or a
    /// PTY, and Ctrl+C never quits. `repeat` marks a held key, which moves the
    /// cursor but never decides anything.
    pub fn on_key(&mut self, key: &KeyEvent, repeat: bool) -> ListOutcome {
        if let Some(id) = self.confirming {
            if repeat {
                return ListOutcome::Handled;
            }
            self.confirming = None;
            let yes = key.code == KeyCode::Char('y') && key.modifiers == KeyModifiers::NONE;
            return if yes {
                ListOutcome::Delete(id)
            } else {
                ListOutcome::Handled
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
            KeyCode::Char('d') => self.confirming = self.selected().map(|row| row.id),
            KeyCode::Char('a') if self.project.is_some() => {
                self.all = !self.all;
                self.cursor = 0;
            }
            _ => {}
        }
        ListOutcome::Handled
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
