//! Pure derivation of the rows shown by the AI picker and of the marks on
//! task rows. No state and no IO: the same draft and detection always give
//! the same rows.
//!
//! The model catalogs behind these rows are informational and possibly
//! stale (see `catalog`); nothing here downloads a model.

use super::catalog::{cli_models, curated, same_model};
use super::config::{AiConfig, Assignment, Task};
use super::parse::is_format;
use super::{CATALOG, Detection, Kind, Status, status_text};

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

/// The assignment shared by every task a target writes to; `None` when a
/// `Set all tasks` draft is mixed.
fn current_assignment(target: Target, draft: &AiConfig) -> Option<Assignment> {
    match target {
        Target::Task(task) => Some(draft.get(task).clone()),
        Target::All => {
            let first = draft.get(Task::ALL[0]);
            Task::ALL
                .iter()
                .all(|t| draft.get(*t) == first)
                .then(|| first.clone())
        }
    }
}

fn is_available(detection: Option<&Detection>, id: &str) -> bool {
    detection.is_some_and(|d| {
        d.engines
            .iter()
            .any(|s| s.id == id && s.status == Status::Available)
    })
}

/// The Engines level of a picker: the unassign entry, the available
/// engines in catalog order, then every engine assigned anywhere in the
/// draft that is not available (selectable, so it can be kept or reopened).
pub fn engine_rows(
    target: Target,
    draft: &AiConfig,
    detection: Option<&Detection>,
) -> Vec<PickRow> {
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
    let mut push = |id: &str, mark: Mark| {
        rows.push(PickRow {
            key: PickKey::Engine(id.to_owned()),
            label: id.to_owned(),
            mark,
            enabled: true,
            current: current_engine == Some(id),
        });
    };
    for engine in CATALOG {
        if is_available(detection, engine.id) {
            push(engine.id, Mark::Status(status_text(detection, engine.id)));
        }
    }
    let mut listed: Vec<&str> = Vec::new();
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
        push(id, mark);
    }
    rows
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
    let row = |name: &str, mark: Mark, enabled: bool| PickRow {
        key: PickKey::Model(name.to_owned()),
        label: name.to_owned(),
        mark,
        enabled,
        current: is_current(name),
    };
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
        let catalog = cli_models(engine);
        let mut rows: Vec<PickRow> = catalog.iter().map(|m| row(m, Mark::Plain, true)).collect();
        rows.extend(
            assigned
                .iter()
                .filter(|m| !catalog.contains(m))
                .map(|m| row(m, Mark::Plain, true)),
        );
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
            return vec![info("no models detected", Mark::Checking)];
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
        rows.push(info("engine not detected", Mark::Plain));
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

/// The label of a task row with its detection marks. An assigned engine
/// that is not available gets ` (not detected)` and no model mark; a model
/// of an available server that is not detected gets ` · download required`.
/// CLI assignments never get a model mark, and nothing is marked before the
/// first detection result.
pub fn task_label(config: &AiConfig, task: Task, detection: Option<&Detection>) -> String {
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
    match model {
        Some(model) if is_server && !state.models.iter().any(|m| same_model(id, m, model)) => {
            format!("{plain} · download required")
        }
        _ => plain,
    }
}

/// Rows visible at once in the picker body.
pub const VIEW: usize = 10;

/// Longest custom model name, in characters.
pub const MAX_NAME: usize = 128;

/// A custom model name once trimmed and capped; `None` when blank or when
/// it holds a control character.
pub fn validate_custom(name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().any(char::is_control) {
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
        let d = detection(vec![state("ollama", Status::NotRunning, &[])]);
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
            [row(
                PickKey::Info,
                "no models detected",
                Mark::Checking,
                false,
                false
            )]
        );
    }

    #[test]
    fn cli_models_are_the_catalog_then_custom() {
        let draft = AiConfig::default();
        let d = detection(vec![
            state("claude", Status::Available, &[]),
            state("codex", Status::Available, &[]),
        ]);
        let custom = row(PickKey::Custom, "custom…", Mark::Plain, true, false);
        assert_eq!(
            model_rows("claude", Target::Task(Task::Code), &draft, Some(&d)),
            [
                model("default", Mark::Plain, true, false),
                model("opus", Mark::Plain, true, false),
                model("sonnet", Mark::Plain, true, false),
                model("fable", Mark::Plain, true, false),
                custom.clone(),
            ]
        );
        assert_eq!(
            model_rows("codex", Target::Task(Task::Code), &draft, Some(&d)),
            [model("default", Mark::Plain, true, false), custom]
        );
    }

    #[test]
    fn an_assigned_custom_cli_model_is_listed_before_custom() {
        let mut draft = AiConfig::default();
        assign(&mut draft, Task::Code, "gemini", Some("gem-exp"));
        assign(&mut draft, Task::Chat, "gemini", Some("default"));
        assign(&mut draft, Task::Review, "gemini", Some("gem-exp"));
        let d = detection(vec![state("gemini", Status::Available, &[])]);
        assert_eq!(
            model_rows("gemini", Target::Task(Task::Code), &draft, Some(&d)),
            [
                model("default", Mark::Plain, true, false),
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
        assert_eq!(current(&draft), [false, true, false, false, false]);
        assign(&mut draft, Task::Chat, "claude", Some("sonnet"));
        assert_eq!(current(&draft), [false; 5]);
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
                engine("claude", "available", false),
                engine("codex", "(not detected)", true),
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
                engine("claude", "available", false),
                engine("vllm", "(not detected)", false),
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

    #[test]
    fn an_empty_detection_lists_assigned_engines_as_not_detected() {
        let mut draft = AiConfig::default();
        assign(&mut draft, Task::General, "mystery", None);
        let d = Detection::default();
        assert_eq!(
            compact(&engine_rows(Target::Task(Task::General), &draft, Some(&d))),
            [
                ("none".to_owned(), Mark::Plain, true),
                (
                    "mystery".to_owned(),
                    Mark::Status("(not detected)".to_owned()),
                    true
                ),
            ]
        );
    }
}
