//! The run-scoped override of a failure alert (agent-orchestrator, Failure
//! options and relaunch): the engine and model the user picked for one task
//! until the run ends, and the detached picker that chooses them. Pure. Never
//! written to the configuration and never part of the menu draft.

use crate::core::ai::{
    Detection,
    config::{AiConfig, Assignment, Task},
    menu::AiMenu,
    picker::{Level, Outcome, Picker, Target},
};

/// Said when a row was chosen that needs a screen only the menu has (a custom
/// name, a download, an install, a copy).
pub const NOT_HERE: &str = "not available here: use the menu (Ctrl+Space m)";
/// Said when the row under the cursor cannot be chosen.
pub const NOT_CHOOSABLE: &str = "that row cannot be chosen";

/// What the user chose to change.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Change {
    Model,
    Provider,
}

/// What an Enter on the picker did.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Picked {
    /// The picker is still open (a level opened, or a note says why not).
    Open,
    /// A leaf was chosen; the default assignment means "use general".
    Chosen(Assignment),
}

/// The engine and model picked per task for the rest of one run.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct RunOverrides(Vec<(Task, Assignment)>);

impl RunOverrides {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The choice for `task` replaces an earlier one.
    pub fn set(&mut self, task: Task, assignment: Assignment) {
        self.0.retain(|(known, _)| *known != task);
        self.0.push((task, assignment));
    }

    pub fn get(&self, task: Task) -> Option<&Assignment> {
        self.0
            .iter()
            .find_map(|(known, assignment)| (*known == task).then_some(assignment))
    }

    /// `config` with the overrides on top: a copy, the menu is untouched.
    pub fn apply(&self, config: &AiConfig) -> AiConfig {
        let mut applied = config.clone();
        for (task, assignment) in &self.0 {
            applied.set(*task, assignment.clone());
        }
        applied
    }

    /// `engine>model` of the override for `task`, for the run state and the
    /// rows; `default` when the choice was to use the general assignment.
    pub fn label(&self, task: Task) -> Option<String> {
        let assignment = self.get(task)?;
        Some(match (&assignment.engine, &assignment.model) {
            (Some(engine), Some(model)) => format!("{engine}>{model}"),
            (Some(engine), None) => engine.clone(),
            (None, _) => "default".to_owned(),
        })
    }
}

/// The menu picker of one task, detached from the menu: its own copy of the
/// configuration, so a choice changes nothing but what it returns.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FailurePicker {
    menu: AiMenu,
    task: Task,
    note: Option<&'static str>,
}

impl FailurePicker {
    /// Opens the picker of `task`, seeded from `in_use`, the engine and
    /// model of the failed worker (an empty engine when it had none).
    pub fn open(
        task: Task,
        config: &AiConfig,
        in_use: (&str, Option<&str>),
        engines: Option<Detection>,
        change: Change,
    ) -> Self {
        let mut seeded = config.clone();
        let engine = Some(in_use.0.to_owned()).filter(|engine| !engine.is_empty());
        let model = in_use.1.filter(|_| engine.is_some()).map(str::to_owned);
        let has_engine = engine.is_some();
        seeded.set(task, Assignment { engine, model });
        let mut menu = AiMenu::new(seeded, engines);
        menu.open_picker(Target::Task(task));
        if change == Change::Model && has_engine {
            menu.drive(Picker::enter);
        }
        Self {
            menu,
            task,
            note: None,
        }
    }

    pub fn task(&self) -> Task {
        self.task
    }

    /// Why the last key did nothing, if it did nothing.
    pub fn note(&self) -> Option<&'static str> {
        self.note
    }

    pub fn step(&mut self, forward: bool) {
        self.note = None;
        self.menu.edit_with(|p, d, e| p.step(forward, d, e));
    }

    /// Chooses the row under the cursor. Only the engine and model levels
    /// exist here: anything that opens another screen is backed out of.
    pub fn enter(&mut self) -> Picked {
        self.note = None;
        if self.menu.cursor_row().is_some_and(|row| !row.enabled) {
            self.note = Some(NOT_CHOOSABLE);
            return Picked::Open;
        }
        let outcome = self.menu.drive(Picker::enter);
        if let Outcome::Commit(assignment) = outcome {
            return Picked::Chosen(assignment);
        }
        let mut backed = matches!(outcome, Outcome::Copy(_));
        for _ in 0..4 {
            let plain = self
                .menu
                .picker()
                .is_none_or(|p| matches!(p.level, Level::Engines | Level::Models { .. }));
            if plain {
                break;
            }
            self.menu.drive(Picker::back);
            backed = true;
        }
        self.note = backed.then_some(NOT_HERE);
        Picked::Open
    }

    /// `Engine for review` or `Model for review (claude)`.
    pub fn title(&self) -> String {
        let task = self.task.id();
        match self.menu.picker().map(|p| &p.level) {
            Some(Level::Models { engine } | Level::Custom { engine, .. }) => {
                format!("Model for {task} ({engine})")
            }
            _ => format!("Engine for {task}"),
        }
    }

    /// The title, the rows around the cursor (`[row]` under it, `●` on the
    /// current one) and the keys, as one paragraph.
    pub fn view(&self) -> String {
        let rows = self.menu.pick_rows();
        let mut title = self.title();
        let window = self.menu.picker().map(|p| {
            if let Some(counter) = p.counter(rows.len()) {
                title = format!("{title} {counter}");
            }
            (p.visible(rows.len()), p.cursor())
        });
        let (range, cursor) = window.unwrap_or((0..0, 0));
        let shown: Vec<String> = range
            .map(|index| {
                let row = &rows[index];
                let label = if index == cursor {
                    format!("[{}]", row.label)
                } else {
                    row.label.clone()
                };
                format!("{}{label}", if row.current { "\u{25cf}" } else { "" })
            })
            .collect();
        let note = self
            .note
            .map(|note| format!(" {note} \u{b7}"))
            .unwrap_or_default();
        format!(
            "{title}: {} \u{b7}{note} j/k move \u{b7} Enter choose \u{b7} Esc back",
            shown.join(" \u{b7} ")
        )
    }
}

#[cfg(test)]
mod tests;
