//! Pure derivation of the rows shown by the AI picker and of the marks on
//! task rows. No state and no IO: the same draft and detection always give
//! the same rows.
//!
//! The model catalogs behind these rows are informational and possibly
//! stale (see `catalog`); nothing here downloads a model.

use super::config::{AiConfig, Assignment, Task};
use super::{CATALOG, Detection, Status, status_text};

/// What a picker is assigning to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    /// `Set all tasks`: the choice is written to all five tasks.
    All,
    Task(Task),
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
    _engine: &str,
    _target: Target,
    _draft: &AiConfig,
    _detection: Option<&Detection>,
) -> Vec<PickRow> {
    Vec::new()
}

/// The label of a task row with its detection marks.
pub fn task_label(_config: &AiConfig, _task: Task, _detection: Option<&Detection>) -> String {
    String::new()
}

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
