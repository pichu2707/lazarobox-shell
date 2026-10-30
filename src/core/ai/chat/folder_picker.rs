//! The folder picker of a chat (chat-folder, design D10). Pure: the runtime
//! reads the disk and the store; this module orders what they found and
//! drives the keys.

use std::path::{Path, PathBuf};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{
    composer::Composer,
    folder::{Probed, Refusal, Verdict, expand, typed},
};
use crate::core::ai::sanitize::is_format;

/// The most characters of the typed path.
const FIELD_CAP: usize = 1024;

/// The most rows the picker lists.
pub const ROWS_MAX: usize = 12;

/// Where a candidate came from, in the order the rows are listed.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Source {
    /// The folder the chat used last.
    LastUsed,
    /// The git root of a pane of the tab.
    PaneRoot,
    /// A folder an earlier conversation was kept under.
    Recent,
    /// The cwd the tab's last shell had.
    LastShell,
}

/// One row of the picker: a resolved, concrete folder.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Row {
    pub path: PathBuf,
    pub source: Source,
}

/// Orders probed candidates: by source, then as found; the same real path
/// once; only folders a chat may work in (`Concrete`); at most [`ROWS_MAX`].
pub fn merge(found: Vec<(Source, Result<Probed, Refusal>)>) -> Vec<Row> {
    let mut found: Vec<_> = found
        .into_iter()
        .filter_map(|(source, result)| match result {
            Ok(Probed {
                path,
                verdict: Verdict::Concrete,
            }) => Some(Row { path, source }),
            _ => None,
        })
        .collect();
    // Stable: candidates of one source keep the order they were found in.
    found.sort_by_key(|row| row.source);
    let mut rows: Vec<Row> = Vec::new();
    for row in found {
        if rows.len() < ROWS_MAX && rows.iter().all(|seen| seen.path != row.path) {
            rows.push(row);
        }
    }
    rows
}

/// How many recent folders to ask the store for: more than [`ROWS_MAX`], since
/// `merge` drops what a chat may not use before it caps the rows.
pub const RECENT_ASK: usize = 40;

/// The directory names a completion found.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Completion {
    pub names: Vec<String>,
    /// More matched than were listed: the names are not all of them, so they
    /// say nothing about a common prefix.
    pub truncated: bool,
    /// An earlier request was still on the disk, so nothing was listed.
    pub busy: bool,
}

/// What a Tab on `text` lists: the folder named up to the last `/` (the
/// base when there is none) and the name typed after it.
pub fn completion_target(
    text: &str,
    home: &Path,
    base: Option<&Path>,
) -> Option<(PathBuf, String)> {
    let (dir_part, prefix) = text.split_at(text.rfind('/').map_or(0, |at| at + 1));
    let dir = if dir_part.is_empty() {
        base.map(Path::to_path_buf)
    } else {
        expand(dir_part, home, base)
    };
    dir.map(|dir| (dir, prefix.to_owned()))
}

/// `text` completed with what `found` lists: the longest prefix the names
/// share, and a `/` when only one folder matches. `None` when nothing
/// matches, or when the list is cut and says nothing about a common prefix.
pub fn completed(text: &str, found: &Completion) -> Option<String> {
    // A name the screen would not draw as typed has no place in the field.
    let names: Vec<&String> = found
        .names
        .iter()
        .filter(|name| !name.chars().any(|c| c.is_control() || is_format(c)))
        .collect();
    let first = names.first().filter(|_| !found.truncated)?;
    let common = names.iter().skip(1).fold(first.as_str(), |common, name| {
        let same = common
            .chars()
            .zip(name.chars())
            .take_while(|(a, b)| a == b)
            .map(|(a, _)| a.len_utf8())
            .sum();
        &common[..same]
    });
    let dir_part = &text[..text.rfind('/').map_or(0, |at| at + 1)];
    let slash = if names.len() == 1 { "/" } else { "" };
    Some(format!("{dir_part}{common}{slash}"))
}

/// What the picker is waiting for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    /// The candidates are still being gathered.
    Gathering,
    Ready,
    /// A chosen folder is being probed; only Esc acts.
    Accepting,
}

/// What a key did to the picker, for the app to act on.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PickerOutcome {
    /// Consumed; nothing for the app to do but redraw.
    Handled,
    /// Enter: probe this absolute path, then answer with `on_probed`.
    Probe(PathBuf),
    /// Tab: list the directories of `dir` that start with `prefix`, then
    /// answer with `on_completed(text, names)`.
    Complete {
        text: String,
        dir: PathBuf,
        prefix: String,
    },
    /// Esc: the user dropped the picker.
    Cancel,
}

/// The picker state: candidate rows, one selected, and a typed path.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FolderPicker {
    /// Names this picker; answers for another (a closed one on the same
    /// pane) are dropped.
    id: u64,
    home: PathBuf,
    /// What a relative typed path reads against.
    base: Option<PathBuf>,
    rows: Vec<Row>,
    selected: usize,
    field: Composer,
    error: Option<&'static str>,
    state: State,
    /// A completion was asked for and has not answered.
    completing: bool,
}

impl FolderPicker {
    pub fn new(id: u64, home: PathBuf, base: Option<PathBuf>) -> Self {
        Self {
            id,
            home,
            base,
            rows: Vec::new(),
            selected: 0,
            field: Composer::with_cap(FIELD_CAP),
            error: None,
            state: State::Gathering,
            completing: false,
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// The selected row; the first one while rows exist.
    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn field(&self) -> &Composer {
        &self.field
    }

    /// Why the last choice was refused, until the field changes.
    pub fn error(&self) -> Option<&'static str> {
        self.error
    }

    pub fn gathering(&self) -> bool {
        self.state == State::Gathering
    }

    /// The gathered rows arrived: row 0 is pre-selected.
    pub fn set_rows(&mut self, id: u64, rows: Vec<Row>) {
        if id != self.id {
            return;
        }
        self.rows = rows;
        self.selected = 0;
        if self.state == State::Gathering {
            self.state = State::Ready;
        }
    }

    /// One key. Everything is consumed. `repeat` marks a held key, which
    /// edits but never decides.
    pub fn on_key(&mut self, key: &KeyEvent, repeat: bool) -> PickerOutcome {
        let (code, mods) = (key.code, key.modifiers);
        let plain = mods - KeyModifiers::SHIFT == KeyModifiers::NONE;
        if code == KeyCode::Esc && mods == KeyModifiers::NONE {
            return PickerOutcome::Cancel;
        }
        if self.state == State::Accepting {
            return PickerOutcome::Handled;
        }
        match code {
            KeyCode::Enter if mods == KeyModifiers::NONE && !repeat => return self.choose(),
            KeyCode::Tab if mods == KeyModifiers::NONE && !repeat && !self.completing => {
                return self.complete();
            }
            KeyCode::Up if plain => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down if plain => {
                self.selected = (self.selected + 1).min(self.rows.len().saturating_sub(1));
            }
            KeyCode::Char('u') if mods == KeyModifiers::CONTROL => self.edit(Composer::clear),
            KeyCode::Char(ch) if plain => self.edit(|f| f.insert(ch)),
            KeyCode::Backspace if plain => self.edit(Composer::backspace),
            KeyCode::Delete if plain => self.edit(Composer::delete),
            KeyCode::Left if plain => self.field.left(),
            KeyCode::Right if plain => self.field.right(),
            KeyCode::Home if plain => self.field.home(),
            KeyCode::End if plain => self.field.end(),
            _ => {}
        }
        PickerOutcome::Handled
    }

    fn edit(&mut self, change: impl FnOnce(&mut Composer)) {
        change(&mut self.field);
        self.error = None;
    }

    /// Enter: the typed path wins over the selected row.
    fn choose(&mut self) -> PickerOutcome {
        let text = self.field.text();
        let text = text.trim();
        let path = if text.is_empty() {
            match self.rows.get(self.selected) {
                Some(row) => row.path.clone(),
                None => return PickerOutcome::Handled,
            }
        } else {
            match expand(text, &self.home, self.base.as_deref()) {
                Some(path) => path,
                None => {
                    self.error = Some(Refusal::NotFound.text());
                    return PickerOutcome::Handled;
                }
            }
        };
        self.error = None;
        self.state = State::Accepting;
        PickerOutcome::Probe(path)
    }

    /// Tab: the part of the text up to the last `/` names the folder to list.
    fn complete(&mut self) -> PickerOutcome {
        let text = self.field.text().trim().to_owned();
        match completion_target(&text, &self.home, self.base.as_deref()) {
            Some((dir, prefix)) => {
                self.completing = true;
                PickerOutcome::Complete { dir, prefix, text }
            }
            None => {
                self.error = Some(Refusal::NotFound.text());
                PickerOutcome::Handled
            }
        }
    }

    /// A paste goes to the field, as one line.
    pub fn paste(&mut self, text: &str) {
        if self.state != State::Accepting {
            self.edit(|f| f.paste(&text.replace(['\r', '\n', '\t'], " ")));
        }
    }

    /// The probe of the chosen folder answered. A folder the chat may use
    /// comes back to be applied; a refused one keeps the text and the reason.
    pub fn on_probed(&mut self, id: u64, result: Result<Probed, Refusal>) -> Option<PathBuf> {
        if id != self.id || self.state != State::Accepting {
            return None;
        }
        self.state = State::Ready;
        match typed(result.as_ref().map(|probed| probed.verdict).map_err(|e| *e)) {
            Ok(_) => result.ok().map(|probed| probed.path),
            Err(refusal) => {
                self.error = Some(refusal.text());
                None
            }
        }
    }

    /// The completion asked for `text` answered with `names`. A reply for
    /// text the field no longer holds is dropped (`false`).
    pub fn on_completed(&mut self, id: u64, text: &str, found: &Completion) -> bool {
        if id != self.id {
            return false;
        }
        self.completing = false;
        if self.state != State::Ready || self.field.text().trim() != text {
            return false;
        }
        if let Some(done) = completed(text, found) {
            self.edit(|f| {
                f.clear();
                f.paste(&done);
            });
        }
        true
    }

    /// Shows `reason` under the rows, until the field changes.
    pub fn note(&mut self, reason: &'static str) {
        self.error = Some(reason);
    }
}
