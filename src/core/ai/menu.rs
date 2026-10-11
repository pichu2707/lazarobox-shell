//! Pure model of the AI section of the config menu: a draft engine and
//! model chosen from what the last detection found. No IO.

use super::{CATALOG, Detection, Status, config::AiConfig};

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

    pub fn draft(&self) -> &AiConfig {
        &self.draft
    }

    pub fn changed(&self) -> bool {
        self.draft != self.original
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
        for id in [&self.original.engine, &self.draft.engine]
            .into_iter()
            .flatten()
        {
            if !choices.contains(&Some(id.clone())) {
                choices.push(Some(id.clone()));
            }
        }
        choices
    }

    /// The display value of an editable row.
    pub fn value(&self, field: AiField) -> String {
        match field {
            AiField::Engine => match &self.draft.engine {
                None => "none".to_owned(),
                Some(id) if self.is_available(id) => id.clone(),
                Some(id) if self.engines.is_none() => format!("{id} (checking)"),
                Some(id) => format!("{id} (not detected)"),
            },
            AiField::Model => String::new(),
        }
    }

    /// The display text of the Status row.
    pub fn status(&self) -> String {
        String::new()
    }

    /// Moves a row to its next or previous choice. `false` when there is
    /// nothing to move to.
    pub fn cycle(&mut self, field: AiField, forward: bool) -> bool {
        match field {
            AiField::Engine => {
                let choices = self.engine_choices();
                let at = choices.iter().position(|c| *c == self.draft.engine);
                let len = choices.len();
                if len < 2 {
                    return false;
                }
                let next = match (at, forward) {
                    (Some(i), true) => (i + 1) % len,
                    (Some(i), false) => (i + len - 1) % len,
                    (None, _) => 0,
                };
                self.draft.engine.clone_from(&choices[next]);
                true
            }
            AiField::Model => false,
        }
    }
}
