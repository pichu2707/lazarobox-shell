//! Pure model of the AI section of the config menu: a draft engine and
//! model chosen from what the last detection found. No IO.

use super::{
    CATALOG, Detection, Status,
    config::{AiConfig, Assignment, Task},
    status_text,
};

/// The general assignment: until the task rows land, the menu edits only it.
fn general(config: &AiConfig) -> &Assignment {
    config.get(Task::General)
}

/// The two editable AI rows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AiField {
    Engine,
    Model,
}

impl AiField {
    /// The `[table] key` this row writes.
    pub fn key_path(self) -> (&'static str, &'static str) {
        match self {
            Self::Engine => ("ai", "engine"),
            Self::Model => ("ai", "model"),
        }
    }
}

/// The AI draft, edited in the menu and written only on save.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AiMenu {
    original: AiConfig,
    draft: AiConfig,
    engines: Option<Detection>,
}

impl AiMenu {
    pub fn new(original: AiConfig, engines: Option<Detection>) -> Self {
        Self {
            draft: original.clone(),
            original,
            engines,
        }
    }

    fn set_draft(&mut self, edit: impl FnOnce(&mut Assignment)) {
        let mut assignment = general(&self.draft).clone();
        edit(&mut assignment);
        self.draft.set(Task::General, assignment);
    }

    pub fn draft(&self) -> &AiConfig {
        &self.draft
    }

    pub fn changed(&self) -> bool {
        self.draft != self.original
    }

    /// The last detection result, `None` before the first.
    pub fn engines(&self) -> Option<&Detection> {
        self.engines.as_ref()
    }

    /// The value of a task row, or of `Set all tasks` (`None`): the shared
    /// assignment or `(mixed)`.
    pub fn task_value(&self, task: Option<Task>) -> String {
        match task {
            Some(task) => self.draft.label(task),
            None => self.draft.aggregate(),
        }
    }

    /// Replaces the detection. The draft is never touched.
    pub fn set_engines(&mut self, engines: Option<Detection>) {
        self.engines = engines;
    }

    fn is_available(&self, id: &str) -> bool {
        self.engines.as_ref().is_some_and(|d| {
            d.engines
                .iter()
                .any(|s| s.id == id && s.status == Status::Available)
        })
    }

    /// `none`, the available engines in catalog order, then the saved and
    /// draft ids when they are not already offered.
    fn engine_choices(&self) -> Vec<Option<String>> {
        let mut choices = vec![None];
        choices.extend(
            CATALOG
                .iter()
                .filter(|e| self.is_available(e.id))
                .map(|e| Some(e.id.to_owned())),
        );
        for id in [
            &general(&self.original).engine,
            &general(&self.draft).engine,
        ]
        .into_iter()
        .flatten()
        {
            if !choices.contains(&Some(id.clone())) {
                choices.push(Some(id.clone()));
            }
        }
        choices
    }

    /// The text written to `config.toml` for a row; unset is `""`.
    pub fn saved_value(&self, field: AiField) -> String {
        match field {
            AiField::Engine => general(&self.draft).engine.clone(),
            AiField::Model => general(&self.draft).model.clone(),
        }
        .unwrap_or_default()
    }

    /// What the draft engine offers, when it is available, without repeats
    /// (the first of equal names wins).
    fn engine_models(&self) -> Vec<String> {
        let id = general(&self.draft).engine.as_deref();
        let listed = self
            .engines
            .iter()
            .flat_map(|d| &d.engines)
            .find(|s| Some(s.id) == id && s.status == Status::Available)
            .map_or(&[][..], |s| &s.models[..]);
        let mut models: Vec<String> = Vec::with_capacity(listed.len());
        for model in listed {
            if !models.contains(model) {
                models.push(model.clone());
            }
        }
        models
    }

    /// The draft engine's models, then the saved model when the saved engine
    /// is the draft and no longer lists it, so it stays reachable (like the
    /// engine) for as long as the menu is open.
    fn model_choices(&self) -> Vec<String> {
        let mut choices = self.engine_models();
        if general(&self.draft).engine == general(&self.original).engine
            && let Some(model) = &general(&self.original).model
            && !choices.contains(model)
        {
            choices.push(model.clone());
        }
        choices
    }

    /// The display value of an editable row.
    pub fn value(&self, field: AiField) -> String {
        match field {
            AiField::Engine => match &general(&self.draft).engine {
                None => "none".to_owned(),
                Some(id) if self.is_available(id) => id.clone(),
                Some(id) if self.engines.is_none() => format!("{id} (checking)"),
                Some(id) => format!("{id} (not detected)"),
            },
            AiField::Model => match &general(&self.draft).model {
                None => "none".to_owned(),
                Some(model) if self.engine_models().contains(model) => model.clone(),
                Some(model) => format!("{model} · not found"),
            },
        }
    }

    /// The display text of the Status row.
    pub fn status(&self) -> String {
        match &general(&self.draft).engine {
            None => "-".to_owned(),
            Some(id) => status_text(self.engines.as_ref(), id),
        }
    }

    /// Moves a row to its next or previous choice; nothing to move to is a
    /// no-op.
    pub fn cycle(&mut self, field: AiField, forward: bool) {
        match field {
            AiField::Engine => {
                let choices = self.engine_choices();
                let at = choices
                    .iter()
                    .position(|c| *c == general(&self.draft).engine);
                let len = choices.len();
                if len < 2 {
                    return;
                }
                let next = match (at, forward) {
                    (Some(i), true) => (i + 1) % len,
                    (Some(i), false) => (i + len - 1) % len,
                    (None, _) => 0,
                };
                self.set_draft(|g| g.engine.clone_from(&choices[next]));
                let first = self.engine_models().first().cloned();
                self.set_draft(|g| g.model = first);
            }
            AiField::Model => {
                let choices = self.model_choices();
                let len = choices.len();
                let at = choices
                    .iter()
                    .position(|m| Some(m) == general(&self.draft).model.as_ref());
                let next = match (at, forward) {
                    (Some(i), true) => (i + 1) % len.max(1),
                    (Some(i), false) => (i + len - 1) % len.max(1),
                    (None, true) => 0,
                    (None, false) => len.saturating_sub(1),
                };
                if let Some(model) = choices.get(next) {
                    let model = model.clone();
                    self.set_draft(|g| g.model = Some(model));
                }
            }
        }
    }
}
