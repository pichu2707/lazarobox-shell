//! Pure model of the AI section of the config menu: the draft assignment of
//! every task, shown against what the last detection found. No IO.

use super::{
    Detection,
    config::{AiConfig, Task},
    picker::task_label,
};

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

    /// The value of a task row, with its detection marks, or of `Set all
    /// tasks` (`None`): the shared assignment or `(mixed)`.
    pub fn value(&self, task: Option<Task>) -> String {
        match task {
            Some(task) => task_label(&self.draft, task, self.engines.as_ref()),
            None => self.draft.aggregate(),
        }
    }
}
