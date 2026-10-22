//! Pure derivation of the rows shown by the AI picker and of the marks on
//! task rows. No state and no IO: the same draft and detection always give
//! the same rows.
//!
//! The model catalogs behind these rows are informational and possibly
//! stale (see `catalog`); nothing here downloads a model.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::catalog::{cli_models, curated, same_model};
use super::chat::adapters::adapter_for;
use super::config::{AiConfig, Assignment, Task};
use super::parse::sanitize;
use super::sanitize::is_format;
use super::{CATALOG, Detection, Engine, Kind, Status, start_command, status_text};

/// What a picker is assigning to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    /// `Set all tasks`: the choice is written to all five tasks.
    All,
    Task(Task),
}

impl Target {
    /// Write a committed choice into the draft: one task, or all five for
    /// `Set all tasks`. The default assignment clears.
    pub fn apply(self, draft: &mut AiConfig, assignment: Assignment) {
        match self {
            Self::Task(task) => draft.set(task, assignment),
            Self::All => {
                for task in Task::ALL {
                    draft.set(task, assignment.clone());
                }
            }
        }
    }
}

/// Identity of a row, stable across detection results.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PickKey {
    /// The unassign entry.
    Clear,
    Engine(String),
    Model(String),
    /// The `custom…` entry of a CLI engine.
    Custom,
    /// A non-selectable explanation row (`no models detected`,
    /// `engine not detected`).
    Info,
    /// Row `row` of a wrapped note that can be copied but never chosen. Every
    /// row of the note carries its whole, sanitized text; `row` keeps the
    /// keys distinct, so a cursor on one row stays there after a refresh.
    Note {
        row: usize,
        text: String,
    },
}

/// The mark drawn after a row label.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Mark {
    Plain,
    /// Detected on the server (`✓`).
    Downloaded,
    /// Known but not downloaded (`download required`).
    DownloadRequired,
    /// No detection result yet (`checking…`).
    Checking,
    /// A CLI model the account's own list does not contain. A warning only:
    /// the list may be stale, so the row stays selectable.
    NotInList,
    /// An engine status text, such as `available · 2 models`.
    Status(String),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PickRow {
    pub key: PickKey,
    pub label: String,
    pub mark: Mark,
    pub enabled: bool,
    /// Whether this row is the current assignment of the target (`●`).
    pub current: bool,
}

/// The assignment shared by every task a target writes to (as resolved, like
/// the aggregate row); `None` when a `Set all tasks` draft is mixed.
fn current_assignment(target: Target, draft: &AiConfig) -> Option<Assignment> {
    match target {
        Target::Task(task) => Some(draft.get(task).clone()),
        Target::All => {
            let first = draft.resolve(Task::ALL[0]).0;
            Task::ALL
                .iter()
                .all(|t| draft.resolve(*t).0 == first)
                .then_some(first)
        }
    }
}

/// A detection that found nothing at all is a failed one: it says nothing
/// about any engine, so it counts as no result.
fn known(detection: Option<&Detection>) -> Option<&Detection> {
    detection.filter(|d| !d.engines.is_empty())
}

fn is_available(detection: Option<&Detection>, id: &str) -> bool {
    has_status(detection, id, Status::Available)
}

fn has_status(detection: Option<&Detection>, id: &str, status: Status) -> bool {
    detection.is_some_and(|d| d.engines.iter().any(|s| s.id == id && s.status == status))
}

/// A server engine whose binary is on PATH but that did not answer on its
/// port: listed, so that choosing it says how to start it.
fn is_stopped_server(detection: Option<&Detection>, engine: &Engine) -> bool {
    matches!(engine.kind, Kind::Server { .. })
        && has_status(detection, engine.id, Status::NotRunning)
}

/// The Engines level of a picker: the unassign entry, the available
/// engines in catalog order (an engine chat has no adapter for is listed
/// but not selectable, since chat would refuse it), the installed servers
/// that are not running, then every other engine assigned anywhere in the
/// draft that is not available (selectable, so it can be kept or reopened).
pub fn engine_rows(
    target: Target,
    draft: &AiConfig,
    detection: Option<&Detection>,
) -> Vec<PickRow> {
    let detection = known(detection);
    let current = current_assignment(target, draft);
    let current_engine = current.as_ref().and_then(|a| a.engine.as_deref());
    let clear_label = match target {
        Target::Task(task) if task != Task::General => "(default · use general)",
        _ => "none",
    };
    let mut rows = vec![PickRow {
        key: PickKey::Clear,
        label: clear_label.to_owned(),
        mark: Mark::Plain,
        enabled: true,
        current: current.as_ref().is_some_and(|a| a.engine.is_none()),
    }];
    let mut push = |id: &str, mark: Mark, enabled: bool| {
        rows.push(PickRow {
            key: PickKey::Engine(id.to_owned()),
            label: id.to_owned(),
            mark,
            enabled,
            current: current_engine == Some(id),
        });
    };
    for engine in CATALOG {
        if is_available(detection, engine.id) {
            push(
                engine.id,
                Mark::Status(status_text(detection, engine.id)),
                adapter_for(engine.id).is_some(),
            );
        }
    }
    let mut listed: Vec<&str> = Vec::new();
    for engine in CATALOG {
        if is_stopped_server(detection, engine) {
            listed.push(engine.id);
            push(
                engine.id,
                Mark::Status(status_text(detection, engine.id)),
                adapter_for(engine.id).is_some(),
            );
        }
    }
    for task in Task::ALL {
        let Some(id) = draft.get(task).engine.as_deref() else {
            continue;
        };
        if is_available(detection, id) || listed.contains(&id) {
            continue;
        }
        listed.push(id);
        let mark = match detection {
            None => Mark::Checking,
            Some(_) => Mark::Status("(not detected)".to_owned()),
        };
        push(id, mark, true);
    }
    rows
}

/// The `(label, value)` models the account lists for a CLI engine; empty
/// when the engine is not detected or has no listing.
fn account_models<'a>(engine: &str, detection: Option<&'a Detection>) -> &'a [(String, String)] {
    detection
        .and_then(|d| {
            d.engines
                .iter()
                .find(|s| s.id == engine && s.status == Status::Available)
        })
        .map_or(&[], |s| s.model_labels.as_slice())
}

/// The Models level of a picker for `engine`.
pub fn model_rows(
    engine: &str,
    target: Target,
    draft: &AiConfig,
    detection: Option<&Detection>,
) -> Vec<PickRow> {
    let current = current_assignment(target, draft)
        .filter(|a| a.engine.as_deref() == Some(engine))
        .and_then(|a| a.model);
    let is_current = |name: &str| {
        current
            .as_deref()
            .is_some_and(|c| same_model(engine, c, name))
    };
    // `label` is display only; the key and the match carry the saved value.
    let labeled = |label: &str, name: &str, mark: Mark, enabled: bool| PickRow {
        key: PickKey::Model(name.to_owned()),
        label: label.to_owned(),
        mark,
        enabled,
        current: is_current(name),
    };
    let row = |name: &str, mark: Mark, enabled: bool| labeled(name, name, mark, enabled);
    let info = |label: &str, mark: Mark| PickRow {
        key: PickKey::Info,
        label: label.to_owned(),
        mark,
        enabled: false,
        current: false,
    };
    // Every model assigned to this engine anywhere in the draft, once.
    let mut assigned: Vec<&str> = Vec::new();
    for task in Task::ALL {
        let slot = draft.get(task);
        let (Some(e), Some(m)) = (slot.engine.as_deref(), slot.model.as_deref()) else {
            continue;
        };
        if e == engine && !assigned.iter().any(|a| same_model(engine, a, m)) {
            assigned.push(m);
        }
    }
    let is_cli = CATALOG
        .iter()
        .any(|e| e.id == engine && matches!(e.kind, Kind::Cli));

    if is_cli {
        // The static entries, then the models the account lists (codex).
        let mut catalog: Vec<(String, String)> = cli_models(engine)
            .iter()
            .map(|(label, value)| ((*label).to_owned(), (*value).to_owned()))
            .collect();
        let account = account_models(engine, detection);
        for (label, value) in account {
            if !catalog.iter().any(|(_, v)| v == value) {
                catalog.push((label.clone(), value.clone()));
            }
        }
        let mut rows: Vec<PickRow> = catalog
            .iter()
            .map(|(label, value)| labeled(label, value, Mark::Plain, true))
            .collect();
        // Only a non-empty account list can say a model is missing from it.
        let missing_mark = if account.is_empty() {
            Mark::Plain
        } else {
            Mark::NotInList
        };
        rows.extend(
            assigned
                .iter()
                .filter(|m| !catalog.iter().any(|(_, value)| value == *m))
                .map(|m| row(m, missing_mark.clone(), true)),
        );
        // Why the account's list is missing, as a line that cannot be picked.
        let why = detection.and_then(|d| {
            d.engines
                .iter()
                .find(|s| s.id == engine && s.status == Status::Available)
                .and_then(|s| s.diagnostics.models.as_deref())
        });
        if let Some(why) = why {
            let full = format!("model list unavailable: {}", sanitize(why));
            rows.extend(note_rows(&full));
        }
        rows.push(PickRow {
            key: PickKey::Custom,
            label: "custom…".to_owned(),
            mark: Mark::Plain,
            enabled: true,
            current: false,
        });
        return rows;
    }

    let Some(detection) = detection else {
        let mut names: Vec<&str> = assigned;
        for m in curated(engine) {
            if !names.iter().any(|n| same_model(engine, n, m)) {
                names.push(m);
            }
        }
        if names.is_empty() {
            return vec![info("checking…", Mark::Plain)];
        }
        return names
            .iter()
            .map(|m| row(m, Mark::Checking, false))
            .collect();
    };

    let state = detection
        .engines
        .iter()
        .find(|s| s.id == engine && s.status == Status::Available);
    let Some(state) = state else {
        let mut rows: Vec<PickRow> = assigned
            .iter()
            .map(|m| row(m, Mark::Plain, false))
            .collect();
        let start = CATALOG
            .iter()
            .find(|e| e.id == engine)
            .filter(|e| is_stopped_server(Some(detection), e))
            .and_then(|_| {
                let port = draft.servers().port(engine)?;
                let command = start_command(engine, port)?;
                Some(format!(
                    "{engine} not running on 127.0.0.1:{port} · start it, then press r: {command}"
                ))
            });
        match start {
            Some(full) => rows.extend(note_rows(&full)),
            None => rows.push(info("engine not detected", Mark::Plain)),
        }
        return rows;
    };

    let mut listed: Vec<&str> = Vec::new();
    let mut rows = Vec::new();
    for m in &state.models {
        if !listed.iter().any(|l| same_model(engine, l, m)) {
            listed.push(m);
            rows.push(row(m, Mark::Downloaded, true));
        }
    }
    let missing = assigned.iter().chain(curated(engine).iter());
    for m in missing {
        if !listed.iter().any(|l| same_model(engine, l, m)) {
            listed.push(m);
            rows.push(row(m, Mark::DownloadRequired, false));
        }
    }
    if rows.is_empty() {
        rows.push(info("no models detected", Mark::Plain));
    }
    rows
}

/// The rows of a note: `text` wrapped, each row carrying the whole text to
/// copy. Never selectable.
fn note_rows(text: &str) -> impl Iterator<Item = PickRow> + '_ {
    wrap_note(text)
        .into_iter()
        .enumerate()
        .map(move |(row, line)| PickRow {
            key: PickKey::Note {
                row,
                text: text.to_owned(),
            },
            label: line,
            mark: Mark::Plain,
            enabled: false,
            current: false,
        })
}

/// Cells a wrapped note row may use: the AI popup is 62 wide, 60 inside, and
/// a row has a leading space.
pub const NOTE_WIDTH: usize = 58;

/// Most rows a note takes; a longer text is clipped on the last one.
pub const NOTE_ROWS: usize = 4;

/// `text` wrapped on spaces to [`NOTE_WIDTH`], a word longer than a row
/// broken, and at most [`NOTE_ROWS`] rows, the last one ending in `…` when
/// text was left out. The full text stays available to copy.
pub fn wrap_note(text: &str) -> Vec<String> {
    let width = |s: &str| UnicodeWidthStr::width(s);
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        // A word wider than a row is cut at the last character that fits.
        let mut rest = word;
        while width(rest) > NOTE_WIDTH {
            let cut = fit_chars(rest, NOTE_WIDTH);
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            lines.push(rest[..cut].to_owned());
            rest = &rest[cut..];
        }
        if !line.is_empty() && width(&line) + 1 + width(rest) > NOTE_WIDTH {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(rest);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    if lines.len() > NOTE_ROWS {
        lines.truncate(NOTE_ROWS);
        let last = &lines[NOTE_ROWS - 1];
        let cut = fit_chars(last, NOTE_WIDTH - 1);
        lines[NOTE_ROWS - 1] = format!("{}…", last[..cut].trim_end());
    }
    lines
}

/// Byte length of the longest prefix of `s` within `room` display cells, at
/// least one character so a wide one cannot stall the caller.
fn fit_chars(s: &str, room: usize) -> usize {
    let mut used = 0;
    for (at, c) in s.char_indices() {
        let w = UnicodeWidthChar::width(c).unwrap_or(0);
        if used + w > room && at > 0 {
            return at;
        }
        used += w;
    }
    s.len()
}

/// The warning for a CLI model the account's own list does not contain.
pub const NOT_IN_LIST: &str = "not in your account's list";

/// The label of a task row with its detection marks. An assigned engine
/// that is not available gets ` (not detected)` and no model mark; a model
/// of an available server that is not detected gets ` · download required`.
/// CLI assignments never get a model mark, and nothing is marked before the
/// first detection result.
pub fn task_label(config: &AiConfig, task: Task, detection: Option<&Detection>) -> String {
    let detection = known(detection);
    let plain = config.label(task);
    let (
        Some(detection),
        Assignment {
            engine: Some(id),
            model,
        },
    ) = (detection, config.get(task))
    else {
        return plain;
    };
    let Some(state) = detection
        .engines
        .iter()
        .find(|s| s.id == id && s.status == Status::Available)
    else {
        return match model {
            Some(model) => format!("{id} (not detected) › {model}"),
            None => format!("{id} (not detected)"),
        };
    };
    let is_server = CATALOG
        .iter()
        .any(|e| e.id == id && matches!(e.kind, Kind::Server { .. }));
    let account = state.model_labels.as_slice();
    match model {
        Some(model)
            if !account.is_empty()
                && model != "default"
                && !account.iter().any(|(_, value)| value == model) =>
        {
            format!("{plain} · {NOT_IN_LIST}")
        }
        Some(model) if is_server && !state.models.iter().any(|m| same_model(id, m, model)) => {
            format!("{plain} · download required")
        }
        _ => plain,
    }
}

/// Why an available CLI engine runs without the jail, sanitized.
fn jail_reason(detection: Option<&Detection>, id: &str) -> Option<String> {
    detection?
        .engines
        .iter()
        .find(|s| s.id == id && s.status == Status::Available)
        .and_then(|s| s.diagnostics.jail.as_deref())
        .map(sanitize)
        .filter(|why| !why.is_empty())
}

/// Rows visible at once in the picker body.
pub const VIEW: usize = 10;

/// Longest custom model name, in characters.
pub const MAX_NAME: usize = 128;

/// A custom model name once trimmed and capped; `None` when blank or when
/// it holds a control or format (bidi, zero-width) character.
pub fn validate_custom(name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().any(|c| c.is_control() || is_format(c)) {
        return None;
    }
    Some(name.chars().take(MAX_NAME).collect())
}

/// Whether a character may be typed into the custom buffer.
fn accepts(c: char) -> bool {
    !c.is_control() && !is_format(c) && !c.is_whitespace()
}

/// Which list the picker is showing.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Level {
    Engines,
    Models { engine: String },
    Custom { engine: String, text: String },
}

/// What a picker action asks of the caller.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// Nothing for the caller to do (the picker may have changed).
    Stay,
    /// A leaf was chosen: apply it to the target and close the picker. The
    /// default assignment means "clear".
    Commit(Assignment),
    /// Leave the picker without choosing.
    Close,
    /// Run a new detection.
    Refresh,
    /// Put this text on the clipboard. The picker and the draft stay as they
    /// are.
    Copy(String),
}

/// The picker state: where it is and where the cursor is. Rows are never
/// stored; every call derives them from the draft and the detection.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Picker {
    pub target: Target,
    pub level: Level,
    cursor: usize,
    offset: usize,
}

impl Picker {
    pub fn open(target: Target, draft: &AiConfig, detection: Option<&Detection>) -> Self {
        let mut picker = Self {
            target,
            level: Level::Engines,
            cursor: 0,
            offset: 0,
        };
        picker.cursor = picker.entry_cursor(draft, detection);
        picker
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    /// The rows of the current level (the Custom level sits on top of its
    /// engine's model rows).
    pub fn rows(&self, draft: &AiConfig, detection: Option<&Detection>) -> Vec<PickRow> {
        match &self.level {
            Level::Engines => engine_rows(self.target, draft, detection),
            Level::Models { engine } | Level::Custom { engine, .. } => {
                model_rows(engine, self.target, draft, detection)
            }
        }
    }

    /// Where the cursor starts on the current level: on the current engine,
    /// or on the current model when selectable, else on the first
    /// selectable row, else on the first row.
    fn entry_cursor(&self, draft: &AiConfig, detection: Option<&Detection>) -> usize {
        let rows = self.rows(draft, detection);
        match self.level {
            Level::Engines => rows
                .iter()
                .position(|r| r.current && matches!(r.key, PickKey::Engine(_)))
                .unwrap_or(0),
            _ => rows
                .iter()
                .position(|r| r.current && r.enabled)
                .or_else(|| rows.iter().position(|r| r.enabled))
                .unwrap_or(0),
        }
    }

    /// Move the cursor one row, wrapping over every row (disabled included).
    pub fn step(&mut self, forward: bool, draft: &AiConfig, detection: Option<&Detection>) {
        let len = self.rows(draft, detection).len();
        if len == 0 {
            return;
        }
        self.cursor = if forward {
            (self.cursor + 1) % len
        } else {
            (self.cursor + len - 1) % len
        };
        self.follow();
    }

    /// Choose the row under the cursor.
    pub fn enter(&mut self, draft: &AiConfig, detection: Option<&Detection>) -> Outcome {
        let rows = self.rows(draft, detection);
        if let Some(PickKey::Note { text, .. }) = rows.get(self.cursor).map(|r| &r.key)
            && !matches!(self.level, Level::Custom { .. })
        {
            return Outcome::Copy(text.clone());
        }
        match self.level.clone() {
            Level::Engines => {
                let Some(row) = rows.get(self.cursor).filter(|r| r.enabled) else {
                    return Outcome::Stay;
                };
                match &row.key {
                    PickKey::Clear => Outcome::Commit(Assignment::default()),
                    PickKey::Engine(id) => {
                        self.level = Level::Models { engine: id.clone() };
                        self.offset = 0;
                        self.cursor = self.entry_cursor(draft, detection);
                        self.follow();
                        Outcome::Stay
                    }
                    _ => Outcome::Stay,
                }
            }
            Level::Models { engine } => {
                let Some(row) = rows.get(self.cursor).filter(|r| r.enabled) else {
                    return Outcome::Stay;
                };
                match &row.key {
                    PickKey::Model(name) => Outcome::Commit(Assignment {
                        engine: Some(engine),
                        model: Some(name.clone()),
                    }),
                    PickKey::Custom => {
                        self.level = Level::Custom {
                            engine,
                            text: String::new(),
                        };
                        Outcome::Stay
                    }
                    _ => Outcome::Stay,
                }
            }
            Level::Custom { engine, text } => match validate_custom(&text) {
                Some(model) => Outcome::Commit(Assignment {
                    engine: Some(engine),
                    model: Some(model),
                }),
                None => Outcome::Stay,
            },
        }
    }

    /// `y`: copy what the row under the cursor explains. A note row gives its
    /// whole text; an engine row gives its no-jail reason, if it has one.
    /// Anything else, and the custom field, copies nothing.
    pub fn copy(&self, draft: &AiConfig, detection: Option<&Detection>) -> Outcome {
        if matches!(self.level, Level::Custom { .. }) {
            return Outcome::Stay;
        }
        let rows = self.rows(draft, detection);
        match rows.get(self.cursor).map(|r| &r.key) {
            Some(PickKey::Note { text, .. }) => Outcome::Copy(text.clone()),
            Some(PickKey::Engine(id)) if matches!(self.level, Level::Engines) => {
                jail_reason(detection, id).map_or(Outcome::Stay, Outcome::Copy)
            }
            _ => Outcome::Stay,
        }
    }

    /// Go back one level; `Close` from the first level.
    pub fn back(&mut self, draft: &AiConfig, detection: Option<&Detection>) -> Outcome {
        match self.level.clone() {
            Level::Engines => Outcome::Close,
            Level::Models { engine } => {
                self.level = Level::Engines;
                let rows = self.rows(draft, detection);
                self.cursor = rows
                    .iter()
                    .position(|r| r.key == PickKey::Engine(engine.clone()))
                    .unwrap_or(0);
                self.offset = 0;
                self.follow();
                Outcome::Stay
            }
            Level::Custom { engine, .. } => {
                self.level = Level::Models { engine };
                let rows = self.rows(draft, detection);
                self.cursor = rows
                    .iter()
                    .position(|r| r.key == PickKey::Custom)
                    .unwrap_or(0);
                self.offset = 0;
                self.follow();
                Outcome::Stay
            }
        }
    }

    /// The custom buffer, when the field is focused.
    pub fn text(&self) -> Option<&str> {
        match &self.level {
            Level::Custom { text, .. } => Some(text),
            _ => None,
        }
    }

    /// Type one character into the custom buffer.
    pub fn type_char(&mut self, c: char) {
        if let Level::Custom { text, .. } = &mut self.level
            && accepts(c)
            && text.chars().count() < MAX_NAME
        {
            text.push(c);
        }
    }

    /// Delete the last character of the custom buffer.
    pub fn backspace(&mut self) {
        if let Level::Custom { text, .. } = &mut self.level {
            text.pop();
        }
    }

    /// Ctrl+U: empty the custom buffer.
    pub fn clear_text(&mut self) {
        if let Level::Custom { text, .. } = &mut self.level {
            text.clear();
        }
    }

    /// Paste into the custom buffer; ignored outside the field.
    pub fn paste(&mut self, pasted: &str) {
        let first_line = pasted.split(['\n', '\r']).next().unwrap_or("");
        for c in first_line.chars() {
            self.type_char(c);
        }
    }

    /// The identity of the row under the cursor.
    pub fn key(&self, draft: &AiConfig, detection: Option<&Detection>) -> Option<PickKey> {
        self.rows(draft, detection)
            .get(self.cursor)
            .map(|r| r.key.clone())
    }

    /// After the rows changed (a detection arrived), put the cursor back on
    /// the row with the `previous` key, or clamp it when that row is gone.
    pub fn reanchor(
        &mut self,
        previous: Option<PickKey>,
        draft: &AiConfig,
        detection: Option<&Detection>,
    ) {
        let rows = self.rows(draft, detection);
        let found = previous.and_then(|key| rows.iter().position(|r| r.key == key));
        self.cursor = found
            .unwrap_or(self.cursor)
            .min(rows.len().saturating_sub(1));
        self.offset = self.offset.min(rows.len().saturating_sub(VIEW));
        self.follow();
    }

    /// The indexes of the rows inside the window.
    pub fn visible(&self, total: usize) -> std::ops::Range<usize> {
        let start = self.offset.min(total.saturating_sub(VIEW));
        start..(start + VIEW).min(total)
    }

    /// `n/total`, only when the list is longer than the window.
    pub fn counter(&self, total: usize) -> Option<String> {
        (total > VIEW).then(|| format!("{}/{total}", self.cursor + 1))
    }

    /// Keep the cursor inside the window: a cursor above it becomes the
    /// first row, one below it the last row.
    fn follow(&mut self) {
        if self.cursor < self.offset {
            self.offset = self.cursor;
        } else if self.cursor >= self.offset + VIEW {
            self.offset = self.cursor + 1 - VIEW;
        }
    }

    /// `r`: ask for a new detection, except where `r` is typed.
    pub fn refresh(&self) -> Outcome {
        match self.level {
            Level::Custom { .. } => Outcome::Stay,
            _ => Outcome::Refresh,
        }
    }
}

#[cfg(test)]
mod assign_tests;
#[cfg(test)]
mod custom_tests;
#[cfg(test)]
mod machine_tests;
#[cfg(test)]
mod note_tests;
#[cfg(test)]
mod rekey_tests;
#[cfg(test)]
mod viewport_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ai::EngineState;

    fn state(id: &'static str, status: Status, models: &[&str]) -> EngineState {
        EngineState {
            id,
            status,
            models: models.iter().map(|m| m.to_string()).collect(),
            version: None,
            model_labels: Vec::new(),
            diagnostics: Default::default(),
        }
    }

    fn detection(states: Vec<EngineState>) -> Detection {
        Detection { engines: states }
    }

    fn assign(config: &mut AiConfig, task: Task, engine: &str, model: Option<&str>) {
        config.set(
            task,
            Assignment {
                engine: Some(engine.to_owned()),
                model: model.map(str::to_owned),
            },
        );
    }

    fn row(key: PickKey, label: &str, mark: Mark, enabled: bool, current: bool) -> PickRow {
        PickRow {
            key,
            label: label.to_owned(),
            mark,
            enabled,
            current,
        }
    }

    fn engine(id: &str, status: &str, current: bool) -> PickRow {
        row(
            PickKey::Engine(id.to_owned()),
            id,
            Mark::Status(status.to_owned()),
            true,
            current,
        )
    }

    fn compact(rows: &[PickRow]) -> Vec<(String, Mark, bool)> {
        rows.iter()
            .map(|r| (r.label.clone(), r.mark.clone(), r.enabled))
            .collect()
    }

    fn model(label: &str, mark: Mark, enabled: bool, current: bool) -> PickRow {
        row(
            PickKey::Model(label.to_owned()),
            label,
            mark,
            enabled,
            current,
        )
    }

    fn missing(label: &str) -> PickRow {
        model(label, Mark::DownloadRequired, false, false)
    }

    fn curated_rows(skip: &[&str]) -> Vec<PickRow> {
        crate::core::ai::catalog::OLLAMA_CURATED
            .iter()
            .filter(|m| !skip.contains(m))
            .map(|m| missing(m))
            .collect()
    }

    fn ollama(models: &[&str]) -> Detection {
        detection(vec![state("ollama", Status::Available, models)])
    }

    fn info(label: &str) -> PickRow {
        row(PickKey::Info, label, Mark::Plain, false, false)
    }

    // --- task_label ------------------------------------------------------

    #[test]
    fn task_rows_carry_download_and_detection_marks() {
        let mut draft = AiConfig::default();
        assign(&mut draft, Task::Code, "ollama", Some("old:7b"));
        assign(&mut draft, Task::Chat, "ollama", Some("llama3:8b"));
        assign(&mut draft, Task::Review, "codex", Some("default"));
        let d = detection(vec![
            state("ollama", Status::Available, &["llama3:8b"]),
            state("codex", Status::NotInstalled, &[]),
        ]);
        let label = |task| task_label(&draft, task, Some(&d));
        assert_eq!(label(Task::Code), "ollama › old:7b · download required");
        assert_eq!(label(Task::Chat), "ollama › llama3:8b");
        assert_eq!(label(Task::Review), "codex (not detected) › default");
        assert_eq!(label(Task::Summarize), "(default)");
        assert_eq!(label(Task::General), "(not set)");
    }

    #[test]
    fn task_rows_have_no_marks_before_the_first_detection() {
        let mut draft = AiConfig::default();
        assign(&mut draft, Task::Code, "ollama", Some("llama3:8b"));
        assert_eq!(task_label(&draft, Task::Code, None), "ollama › llama3:8b");
    }

    #[test]
    fn task_row_marks_follow_the_tag_less_rule_and_skip_clis_and_engine_only() {
        let mut draft = AiConfig::default();
        assign(&mut draft, Task::Code, "ollama", Some("llama3"));
        assign(&mut draft, Task::Chat, "claude", Some("not-in-any-catalog"));
        assign(&mut draft, Task::Review, "ollama", None);
        assign(&mut draft, Task::Summarize, "vllm", None);
        let d = detection(vec![
            state("ollama", Status::Available, &["llama3:latest"]),
            state("claude", Status::Available, &[]),
        ]);
        let label = |task| task_label(&draft, task, Some(&d));
        assert_eq!(label(Task::Code), "ollama › llama3");
        assert_eq!(label(Task::Chat), "claude › not-in-any-catalog");
        assert_eq!(label(Task::Review), "ollama");
        assert_eq!(label(Task::Summarize), "vllm (not detected)");
    }

    // --- codex account list ----------------------------------------------

    const NOT_IN_LIST: &str = "not in your account's list";

    fn codex_with(models: &[(&str, &str)]) -> Detection {
        let mut codex = state("codex", Status::Available, &[]);
        codex.model_labels = models
            .iter()
            .map(|(l, v)| (l.to_string(), v.to_string()))
            .collect();
        detection(vec![codex])
    }

    fn pick(label: &str, value: &str, mark: Mark, current: bool) -> PickRow {
        row(PickKey::Model(value.to_owned()), label, mark, true, current)
    }

    fn custom_row() -> PickRow {
        row(PickKey::Custom, "custom…", Mark::Plain, true, false)
    }

    #[test]
    fn codex_rows_are_default_then_the_account_models_then_custom() {
        let d = codex_with(&[("GPT-6.1-Sol", "gpt-6.1-sol"), ("GPT-6-Luna", "gpt-6-luna")]);
        assert_eq!(
            model_rows(
                "codex",
                Target::Task(Task::Code),
                &AiConfig::default(),
                Some(&d)
            ),
            [
                pick("default (CLI picks)", "default", Mark::Plain, false),
                pick("GPT-6.1-Sol", "gpt-6.1-sol", Mark::Plain, false),
                pick("GPT-6-Luna", "gpt-6-luna", Mark::Plain, false),
                custom_row(),
            ]
        );
    }

    fn codex_failed(why: &str) -> Detection {
        let mut codex = state("codex", Status::Available, &["default"]);
        codex.diagnostics.models = Some(why.to_owned());
        detection(vec![codex])
    }

    #[test]
    fn a_failed_codex_listing_adds_a_note_row_that_cannot_be_picked() {
        let d = codex_failed("codex exited 1: not logged in");
        assert_eq!(
            model_rows(
                "codex",
                Target::Task(Task::Code),
                &AiConfig::default(),
                Some(&d)
            ),
            [
                pick("default (CLI picks)", "default", Mark::Plain, false),
                row(
                    PickKey::Note {
                        row: 0,
                        text: "model list unavailable: codex exited 1: not logged in".to_owned()
                    },
                    "model list unavailable: codex exited 1: not logged in",
                    Mark::Plain,
                    false,
                    false
                ),
                custom_row(),
            ]
        );
    }

    #[test]
    fn a_good_listing_has_no_note_row() {
        let d = codex_with(&[("GPT-6.1-Sol", "gpt-6.1-sol")]);
        let rows = model_rows("codex", Target::All, &AiConfig::default(), Some(&d));
        assert!(rows.iter().all(|r| !matches!(r.key, PickKey::Note { .. })));
    }

    #[test]
    fn a_listing_note_is_one_clean_line() {
        let d = codex_failed("a\u{1b}[31mb\nc\u{202e}d");
        let rows = model_rows("codex", Target::All, &AiConfig::default(), Some(&d));
        let note = rows
            .iter()
            .find(|r| matches!(r.key, PickKey::Note { .. }))
            .unwrap();
        assert_eq!(note.label, "model list unavailable: abcd");
    }

    #[test]
    fn a_listed_codex_model_is_current_by_its_slug() {
        let d = codex_with(&[("GPT-6.1-Sol", "gpt-6.1-sol")]);
        let mut draft = AiConfig::default();
        assign(&mut draft, Task::Code, "codex", Some("gpt-6.1-sol"));
        assert_eq!(
            model_rows("codex", Target::Task(Task::Code), &draft, Some(&d)),
            [
                pick("default (CLI picks)", "default", Mark::Plain, false),
                pick("GPT-6.1-Sol", "gpt-6.1-sol", Mark::Plain, true),
                custom_row(),
            ]
        );
    }

    #[test]
    fn an_assigned_codex_model_missing_from_the_list_is_warned_but_selectable() {
        let d = codex_with(&[("GPT-6.1-Sol", "gpt-6.1-sol")]);
        let mut draft = AiConfig::default();
        assign(&mut draft, Task::Code, "codex", Some("gpt-5.4"));
        assert_eq!(
            model_rows("codex", Target::Task(Task::Code), &draft, Some(&d)),
            [
                pick("default (CLI picks)", "default", Mark::Plain, false),
                pick("GPT-6.1-Sol", "gpt-6.1-sol", Mark::Plain, false),
                pick("gpt-5.4", "gpt-5.4", Mark::NotInList, true),
                custom_row(),
            ]
        );
    }

    #[test]
    fn no_warning_when_the_codex_list_is_empty_or_the_model_is_default() {
        let mut draft = AiConfig::default();
        assign(&mut draft, Task::Code, "codex", Some("gpt-5.4"));
        assign(&mut draft, Task::Chat, "codex", Some("default"));
        for d in [codex_with(&[]), detection(vec![])] {
            let rows = model_rows("codex", Target::Task(Task::Code), &draft, Some(&d));
            assert!(rows.iter().all(|r| r.mark == Mark::Plain), "{rows:?}");
        }
        let mut only_default = AiConfig::default();
        assign(&mut only_default, Task::Chat, "codex", Some("default"));
        let d = codex_with(&[("GPT-6.1-Sol", "gpt-6.1-sol")]);
        let rows = model_rows("codex", Target::Task(Task::Chat), &only_default, Some(&d));
        assert!(!rows.iter().any(|r| r.mark == Mark::NotInList));
    }

    #[test]
    fn other_cli_engines_never_get_the_account_warning() {
        let mut draft = AiConfig::default();
        assign(&mut draft, Task::Code, "claude", Some("whatever"));
        let d = detection(vec![state("claude", Status::Available, &[])]);
        let rows = model_rows("claude", Target::Task(Task::Code), &draft, Some(&d));
        assert!(rows.iter().all(|r| r.mark == Mark::Plain));
    }

    #[test]
    fn the_task_label_warns_for_a_codex_model_outside_the_account_list() {
        let d = codex_with(&[("GPT-6.1-Sol", "gpt-6.1-sol")]);
        let mut draft = AiConfig::default();
        assign(&mut draft, Task::Code, "codex", Some("gpt-5.4"));
        assign(&mut draft, Task::Chat, "codex", Some("gpt-6.1-sol"));
        assign(&mut draft, Task::Review, "codex", Some("default"));
        assert_eq!(
            task_label(&draft, Task::Code, Some(&d)),
            format!("codex › gpt-5.4 · {NOT_IN_LIST}")
        );
        assert_eq!(
            task_label(&draft, Task::Chat, Some(&d)),
            "codex › gpt-6.1-sol"
        );
        assert_eq!(
            task_label(&draft, Task::Review, Some(&d)),
            "codex › default"
        );
        let none = codex_with(&[]);
        assert_eq!(
            task_label(&draft, Task::Code, Some(&none)),
            "codex › gpt-5.4"
        );
    }

    // --- model_rows ------------------------------------------------------

    #[test]
    fn detected_models_come_first_then_the_curated_rest() {
        let draft = AiConfig::default();
        let d = ollama(&["llama3:8b", "mistral:7b"]);
        let mut expected = vec![
            model("llama3:8b", Mark::Downloaded, true, false),
            model("mistral:7b", Mark::Downloaded, true, false),
        ];
        expected.extend(curated_rows(&["mistral:7b"]));
        assert_eq!(
            model_rows("ollama", Target::Task(Task::Code), &draft, Some(&d)),
            expected
        );
    }

    #[test]
    fn a_tag_less_assignment_matches_the_detected_latest() {
        let mut draft = AiConfig::default();
        assign(&mut draft, Task::Code, "ollama", Some("llama3"));
        let d = ollama(&["llama3:latest"]);
        let mut expected = vec![model("llama3:latest", Mark::Downloaded, true, true)];
        expected.extend(curated_rows(&[]));
        assert_eq!(
            model_rows("ollama", Target::Task(Task::Code), &draft, Some(&d)),
            expected
        );
    }

    #[test]
    fn an_assigned_model_that_is_not_detected_sits_between_detected_and_curated() {
        let mut draft = AiConfig::default();
        assign(&mut draft, Task::Code, "ollama", Some("old:7b"));
        assign(&mut draft, Task::Chat, "ollama", Some("mistral:7b"));
        assign(&mut draft, Task::Review, "claude", Some("opus"));
        let d = ollama(&["llama3:8b"]);
        let mut expected = vec![
            model("llama3:8b", Mark::Downloaded, true, false),
            model("old:7b", Mark::DownloadRequired, false, true),
            missing("mistral:7b"),
        ];
        expected.extend(curated_rows(&["mistral:7b"]));
        assert_eq!(
            model_rows("ollama", Target::Task(Task::Code), &draft, Some(&d)),
            expected
        );
    }

    #[test]
    fn a_server_without_models_or_catalog_shows_one_empty_row() {
        let draft = AiConfig::default();
        let d = detection(vec![state("lmstudio", Status::Available, &[])]);
        assert_eq!(
            model_rows("lmstudio", Target::Task(Task::Code), &draft, Some(&d)),
            [info("no models detected")]
        );
    }

    #[test]
    fn servers_other_than_ollama_have_no_curated_rows_and_no_custom() {
        let draft = AiConfig::default();
        let d = detection(vec![state("vllm", Status::Available, &["m"])]);
        assert_eq!(
            model_rows("vllm", Target::Task(Task::Code), &draft, Some(&d)),
            [model("m", Mark::Downloaded, true, false)]
        );
    }

    #[test]
    fn an_unavailable_server_keeps_assigned_models_disabled_without_curated_rows() {
        let mut draft = AiConfig::default();
        assign(&mut draft, Task::Code, "ollama", Some("llama3:8b"));
        assign(&mut draft, Task::Chat, "ollama", Some("qwen3:8b"));
        let d = detection(vec![state("ollama", Status::NotInstalled, &[])]);
        assert_eq!(
            model_rows("ollama", Target::Task(Task::Code), &draft, Some(&d)),
            [
                model("llama3:8b", Mark::Plain, false, true),
                model("qwen3:8b", Mark::Plain, false, false),
                info("engine not detected"),
            ]
        );
    }

    #[test]
    fn an_engine_missing_from_the_detection_is_not_detected() {
        let draft = AiConfig::default();
        assert_eq!(
            model_rows(
                "vllm",
                Target::Task(Task::Code),
                &draft,
                Some(&Detection::default())
            ),
            [info("engine not detected")]
        );
    }

    #[test]
    fn before_the_first_detection_server_models_are_disabled_and_checking() {
        let mut draft = AiConfig::default();
        assign(&mut draft, Task::Code, "ollama", Some("llama3:8b"));
        let rows = model_rows("ollama", Target::Task(Task::Code), &draft, None);
        let mut expected = vec![model("llama3:8b", Mark::Checking, false, true)];
        expected.extend(
            crate::core::ai::catalog::OLLAMA_CURATED
                .iter()
                .map(|m| model(m, Mark::Checking, false, false)),
        );
        assert_eq!(rows, expected);
        assert_eq!(
            model_rows("lmstudio", Target::Task(Task::Code), &draft, None),
            [info("checking…")]
        );
    }

    #[test]
    fn cli_models_are_the_catalog_then_custom() {
        let draft = AiConfig::default();
        let d = detection(vec![
            state("claude", Status::Available, &[]),
            state("codex", Status::Available, &[]),
            state("gemini", Status::Available, &[]),
        ]);
        let custom = row(PickKey::Custom, "custom…", Mark::Plain, true, false);
        let labeled = |label: &str, value: &str| {
            row(
                PickKey::Model(value.to_owned()),
                label,
                Mark::Plain,
                true,
                false,
            )
        };
        assert_eq!(
            model_rows("claude", Target::Task(Task::Code), &draft, Some(&d)),
            [
                labeled("default (CLI picks)", "default"),
                labeled("Opus 5.5", "opus"),
                labeled("Sonnet 5.5", "sonnet"),
                labeled("Haiku 5.5", "haiku"),
                labeled("Fable 5.1", "fable"),
                custom.clone(),
            ]
        );
        assert_eq!(
            model_rows("gemini", Target::Task(Task::Code), &draft, Some(&d)),
            [
                labeled("default (auto)", "default"),
                labeled("Flash 3.8", "gemini-3.8-flash"),
                labeled("Flash 3.5", "gemini-3.5-flash"),
                labeled("Flash Lite 3.5", "gemini-3.5-flash-lite"),
                custom.clone(),
            ]
        );
        assert_eq!(
            model_rows("codex", Target::Task(Task::Code), &draft, Some(&d)),
            [labeled("default (CLI picks)", "default"), custom]
        );
    }

    // The saved value stays the CLI value: a saved alias or id loads on its
    // labeled row, and committing a labeled row yields the value.
    #[test]
    fn a_saved_value_is_current_on_its_labeled_row() {
        let d = detection(vec![
            state("claude", Status::Available, &[]),
            state("gemini", Status::Available, &[]),
        ]);
        let current = |engine: &str, saved: &str| {
            let mut draft = AiConfig::default();
            assign(&mut draft, Task::Code, engine, Some(saved));
            model_rows(engine, Target::Task(Task::Code), &draft, Some(&d))
                .into_iter()
                .filter(|r| r.current)
                .map(|r| (r.label, r.key))
                .collect::<Vec<_>>()
        };
        let key = |v: &str| PickKey::Model(v.to_owned());
        assert_eq!(
            current("claude", "opus"),
            [("Opus 5.5".into(), key("opus"))]
        );
        assert_eq!(
            current("claude", "default"),
            [("default (CLI picks)".into(), key("default"))]
        );
        assert_eq!(
            current("gemini", "gemini-3.5-flash"),
            [("Flash 3.5".into(), key("gemini-3.5-flash"))]
        );
    }

    // A saved value outside the catalog (a retired id, a full model id) is not
    // an error: it is listed under its own value, before `custom…`.
    #[test]
    fn a_saved_value_outside_the_catalog_is_a_custom_row() {
        let d = detection(vec![
            state("claude", Status::Available, &[]),
            state("gemini", Status::Available, &[]),
        ]);
        for (engine, saved) in [
            ("gemini", "gemini-2.5-flash"),
            ("claude", "claude-opus-4-1"),
        ] {
            let mut draft = AiConfig::default();
            assign(&mut draft, Task::Code, engine, Some(saved));
            let rows = model_rows(engine, Target::Task(Task::Code), &draft, Some(&d));
            let n = rows.len();
            assert_eq!(
                rows[n - 2],
                model(saved, Mark::Plain, true, true),
                "{engine}"
            );
            assert_eq!(rows[n - 1].key, PickKey::Custom, "{engine}");
            assert_eq!(rows.iter().filter(|r| r.current).count(), 1, "{engine}");
        }
    }

    #[test]
    fn an_assigned_custom_cli_model_is_listed_before_custom() {
        let mut draft = AiConfig::default();
        assign(&mut draft, Task::Code, "codex", Some("gem-exp"));
        assign(&mut draft, Task::Chat, "codex", Some("default"));
        assign(&mut draft, Task::Review, "codex", Some("gem-exp"));
        let d = detection(vec![state("codex", Status::Available, &[])]);
        assert_eq!(
            model_rows("codex", Target::Task(Task::Code), &draft, Some(&d)),
            [
                pick("default (CLI picks)", "default", Mark::Plain, false),
                model("gem-exp", Mark::Plain, true, true),
                row(PickKey::Custom, "custom…", Mark::Plain, true, false),
            ]
        );
    }

    #[test]
    fn set_all_marks_the_model_only_when_the_assignment_is_shared() {
        let mut draft = AiConfig::default();
        for task in Task::ALL {
            assign(&mut draft, task, "claude", Some("opus"));
        }
        let d = detection(vec![state("claude", Status::Available, &[])]);
        let current = |draft: &AiConfig| {
            model_rows("claude", Target::All, draft, Some(&d))
                .iter()
                .map(|r| r.current)
                .collect::<Vec<_>>()
        };
        assert_eq!(current(&draft), [false, true, false, false, false, false]);
        assign(&mut draft, Task::Chat, "claude", Some("sonnet"));
        assert_eq!(current(&draft), [false; 6]);
    }

    // --- engine_rows -----------------------------------------------------

    #[test]
    fn engine_list_for_a_task_with_an_undetected_assignment() {
        let mut draft = AiConfig::default();
        assign(&mut draft, Task::Code, "codex", Some("default"));
        let d = detection(vec![
            state("ollama", Status::Available, &["a", "b"]),
            state("lmstudio", Status::NotRunning, &[]),
            state("claude", Status::Available, &[]),
            state("codex", Status::NotInstalled, &[]),
        ]);
        assert_eq!(
            engine_rows(Target::Task(Task::Code), &draft, Some(&d)),
            [
                row(
                    PickKey::Clear,
                    "(default · use general)",
                    Mark::Plain,
                    true,
                    false
                ),
                engine("ollama", "available · 2 models", false),
                engine("claude", "available · version unknown", false),
                engine("lmstudio", "not running", false),
                engine("codex", "(not detected)", true),
            ]
        );
    }

    #[test]
    fn installed_servers_that_are_not_running_are_listed_after_the_available_engines() {
        let draft = AiConfig::default();
        let d = detection(vec![
            state("ollama", Status::NotRunning, &[]),
            state("lmstudio", Status::NotInstalled, &[]),
            state("llamacpp", Status::NotRunning, &[]),
            state("claude", Status::Available, &[]),
            state("codex", Status::NotRunning, &[]),
        ]);
        assert_eq!(
            compact(&engine_rows(Target::Task(Task::Chat), &draft, Some(&d))[1..]),
            [
                (
                    "claude".to_owned(),
                    Mark::Status("available · version unknown".to_owned()),
                    true
                ),
                (
                    "ollama".to_owned(),
                    Mark::Status("not running".to_owned()),
                    true
                ),
                (
                    "llamacpp".to_owned(),
                    Mark::Status("not running".to_owned()),
                    true
                ),
            ]
        );
    }

    #[test]
    fn a_server_that_is_not_running_says_where_it_is_looked_for_and_how_to_start_it() {
        let ports = crate::core::ai::ServerPorts::default().with("llamacpp", 39080);
        let mut draft = AiConfig::default().with_servers(ports);
        assign(&mut draft, Task::Chat, "llamacpp", Some("qwen"));
        let d = detection(vec![state("llamacpp", Status::NotRunning, &[])]);
        let text = "llamacpp not running on 127.0.0.1:39080 · start it, then press r: \
                    llama-server --port 39080 -m <model.gguf>";
        let rows = model_rows("llamacpp", Target::Task(Task::Chat), &draft, Some(&d));
        let mut want = vec![model("qwen", Mark::Plain, false, true)];
        want.extend(
            wrap_note(text)
                .into_iter()
                .enumerate()
                .map(|(row, line)| PickRow {
                    key: PickKey::Note {
                        row,
                        text: text.to_owned(),
                    },
                    label: line,
                    mark: Mark::Plain,
                    enabled: false,
                    current: false,
                }),
        );
        assert_eq!(rows, want);
        // A server that is not installed keeps the plain line.
        let d = detection(vec![state("llamacpp", Status::NotInstalled, &[])]);
        let rows = model_rows("llamacpp", Target::Task(Task::Chat), &draft, Some(&d));
        assert_eq!(rows.last().unwrap().key, PickKey::Info);
        assert_eq!(rows.last().unwrap().label, "engine not detected");
    }

    #[test]
    fn a_detected_engine_without_a_chat_adapter_is_listed_but_not_selectable() {
        let draft = AiConfig::default();
        let d = detection(vec![
            state("llamacpp", Status::Available, &["m"]),
            state("opencode", Status::Available, &[]),
        ]);
        let rows = engine_rows(Target::Task(Task::Code), &draft, Some(&d));
        assert_eq!(
            compact(&rows[1..]),
            [
                (
                    "llamacpp".to_owned(),
                    Mark::Status("available · 1 model".to_owned()),
                    true
                ),
                (
                    "opencode".to_owned(),
                    Mark::Status("detected · not in chat yet".to_owned()),
                    false
                ),
            ]
        );
    }

    #[test]
    fn the_unassign_entry_is_labelled_per_target() {
        let draft = AiConfig::default();
        let d = detection(Vec::new());
        let label = |target| engine_rows(target, &draft, Some(&d))[0].clone();
        assert_eq!(
            label(Target::Task(Task::Chat)),
            row(
                PickKey::Clear,
                "(default · use general)",
                Mark::Plain,
                true,
                true
            )
        );
        assert_eq!(
            label(Target::Task(Task::General)),
            row(PickKey::Clear, "none", Mark::Plain, true, true)
        );
        assert_eq!(
            label(Target::All),
            row(PickKey::Clear, "none", Mark::Plain, true, true)
        );
    }

    #[test]
    fn assigned_engines_of_any_task_are_listed_once_after_the_available_ones() {
        let mut draft = AiConfig::default();
        assign(&mut draft, Task::Chat, "vllm", None);
        assign(&mut draft, Task::Review, "vllm", Some("m"));
        assign(&mut draft, Task::Summarize, "claude", Some("opus"));
        let d = detection(vec![
            state("claude", Status::Available, &[]),
            state("vllm", Status::NotRunning, &[]),
        ]);
        assert_eq!(
            engine_rows(Target::Task(Task::Code), &draft, Some(&d)),
            [
                row(
                    PickKey::Clear,
                    "(default · use general)",
                    Mark::Plain,
                    true,
                    true
                ),
                engine("claude", "available · version unknown", false),
                engine("vllm", "not running", false),
            ]
        );
    }

    #[test]
    fn the_current_engine_of_the_target_is_marked() {
        let mut draft = AiConfig::default();
        assign(&mut draft, Task::Code, "claude", Some("opus"));
        let d = detection(vec![state("claude", Status::Available, &[])]);
        let rows = engine_rows(Target::Task(Task::Code), &draft, Some(&d));
        assert_eq!(
            rows.iter().map(|r| r.current).collect::<Vec<_>>(),
            [false, true]
        );
    }

    #[test]
    fn set_all_marks_an_engine_only_when_all_five_share_it() {
        let d = detection(vec![state("claude", Status::Available, &[])]);
        let mut draft = AiConfig::default();
        for task in Task::ALL {
            assign(&mut draft, task, "claude", Some("opus"));
        }
        let shared = engine_rows(Target::All, &draft, Some(&d));
        assert_eq!(
            shared.iter().map(|r| r.current).collect::<Vec<_>>(),
            [false, true]
        );
        assign(&mut draft, Task::Chat, "claude", Some("sonnet"));
        let mixed = engine_rows(Target::All, &draft, Some(&d));
        assert_eq!(
            mixed.iter().map(|r| r.current).collect::<Vec<_>>(),
            [false, false]
        );
    }

    // The aggregate row resolves inheritance, so the bullet does too.
    #[test]
    fn set_all_marks_what_the_aggregate_row_shows() {
        let d = detection(vec![state("claude", Status::Available, &[])]);
        let mut draft = AiConfig::default();
        assign(&mut draft, Task::General, "claude", Some("opus"));
        let currents = |rows: Vec<PickRow>| rows.iter().map(|r| r.current).collect::<Vec<_>>();
        assert_eq!(draft.aggregate(), "claude › opus");
        assert_eq!(
            currents(engine_rows(Target::All, &draft, Some(&d))),
            [false, true]
        );
        assert_eq!(
            currents(model_rows("claude", Target::All, &draft, Some(&d))),
            [false, true, false, false, false, false]
        );
    }

    #[test]
    fn before_the_first_detection_only_assigned_engines_show_as_checking() {
        let mut draft = AiConfig::default();
        assign(&mut draft, Task::Code, "codex", None);
        assert_eq!(
            compact(&engine_rows(Target::Task(Task::Code), &draft, None)),
            [
                ("(default · use general)".to_owned(), Mark::Plain, true),
                ("codex".to_owned(), Mark::Checking, true),
            ]
        );
    }

    // A failed detection is an empty one: it says nothing, like no result.
    #[test]
    fn an_empty_detection_marks_nothing_as_not_detected() {
        let mut draft = AiConfig::default();
        assign(&mut draft, Task::General, "mystery", Some("m"));
        let d = Detection::default();
        assert_eq!(
            compact(&engine_rows(Target::Task(Task::General), &draft, Some(&d))),
            [
                ("none".to_owned(), Mark::Plain, true),
                ("mystery".to_owned(), Mark::Checking, true),
            ]
        );
        assert_eq!(task_label(&draft, Task::General, Some(&d)), "mystery › m");
    }
}
