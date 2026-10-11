//! Pure model of the AI section of the config menu: the draft assignment of
//! every task, shown against what the last detection found. No IO.

use super::{
    Detection,
    config::{AiConfig, Task},
    picker::{Outcome, PickRow, Picker, Target, task_label},
};

/// The AI draft, edited in the menu and written only on save.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AiMenu {
    original: AiConfig,
    draft: AiConfig,
    engines: Option<Detection>,
    picker: Option<Picker>,
}

impl AiMenu {
    pub fn new(original: AiConfig, engines: Option<Detection>) -> Self {
        Self {
            draft: original.clone(),
            original,
            engines,
            picker: None,
        }
    }

    pub fn draft(&self) -> &AiConfig {
        &self.draft
    }

    pub fn changed(&self) -> bool {
        self.draft != self.original
    }

    /// Replaces the detection. The draft is never touched.
    /// An open picker keeps its level and its row.
    pub fn set_engines(&mut self, engines: Option<Detection>) {
        let row = self
            .picker
            .as_ref()
            .and_then(|p| p.key(&self.draft, self.engines.as_ref()));
        self.engines = engines;
        if let Some(picker) = &mut self.picker {
            picker.reanchor(row, &self.draft, self.engines.as_ref());
        }
    }

    pub fn open_picker(&mut self, target: Target) {
        self.picker = Some(Picker::open(target, &self.draft, self.engines.as_ref()));
    }

    pub fn picker(&self) -> Option<&Picker> {
        self.picker.as_ref()
    }

    /// The rows of the open picker; none when it is closed.
    pub fn pick_rows(&self) -> Vec<PickRow> {
        self.picker
            .as_ref()
            .map(|p| p.rows(&self.draft, self.engines.as_ref()))
            .unwrap_or_default()
    }

    /// Changes the open picker without choosing anything.
    pub fn edit(&mut self, change: impl FnOnce(&mut Picker)) {
        self.edit_with(|picker, _, _| change(picker));
    }

    /// Like [`Self::edit`], with the draft and the detection at hand.
    pub fn edit_with(&mut self, change: impl FnOnce(&mut Picker, &AiConfig, Option<&Detection>)) {
        self.drive(|picker, draft, engines| {
            change(picker, draft, engines);
            Outcome::Stay
        });
    }

    /// Runs one picker action. A choice goes into the draft and closes the
    /// picker, as does leaving the first level.
    pub fn drive(
        &mut self,
        act: impl FnOnce(&mut Picker, &AiConfig, Option<&Detection>) -> Outcome,
    ) -> Outcome {
        let Some(picker) = &mut self.picker else {
            return Outcome::Stay;
        };
        let target = picker.target;
        let outcome = act(picker, &self.draft, self.engines.as_ref());
        match &outcome {
            Outcome::Commit(assignment) => {
                target.apply(&mut self.draft, assignment.clone());
                self.picker = None;
            }
            Outcome::Close => self.picker = None,
            Outcome::Stay | Outcome::Refresh => {}
        }
        outcome
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
