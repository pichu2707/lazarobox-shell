//! Pure model of the AI section of the config menu: the draft assignment of
//! every task, shown against what the last detection found. No IO.

use super::{
    Detection,
    config::{AiConfig, Assignment, Profile, Task},
    picker::{Outcome, PickRow, Picker, Target, task_label},
};

/// The AI draft, edited in the menu and written only on save.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AiMenu {
    original: AiConfig,
    draft: AiConfig,
    engines: Option<Detection>,
    picker: Option<Picker>,
    /// The privacy warning is shown over the menu.
    warning: bool,
}

impl AiMenu {
    pub fn new(original: AiConfig, engines: Option<Detection>) -> Self {
        Self {
            draft: original.clone(),
            original,
            engines,
            picker: None,
            warning: false,
        }
    }

    pub fn draft(&self) -> &AiConfig {
        &self.draft
    }

    pub fn changed(&self) -> bool {
        self.draft != self.original
    }

    /// The tasks whose draft differs from the saved one, in `Task::ALL`
    /// order, with their draft assignment.
    pub fn changed_tasks(&self) -> impl Iterator<Item = (Task, &Assignment)> {
        Task::ALL
            .into_iter()
            .filter(|t| self.draft.get(*t) != self.original.get(*t))
            .map(|t| (t, self.draft.get(t)))
    }

    /// Flips the memory draft. Turning it on without the acknowledgement
    /// opens the privacy warning instead: the draft changes only when the
    /// user accepts. Nothing is written or requested here.
    pub fn toggle_memory(&mut self) {
        let memory = &mut self.draft.memory;
        if memory.active() {
            memory.enabled = false;
        } else if memory.acknowledged {
            memory.enabled = true;
        } else {
            self.warning = true;
        }
    }

    /// Whether the privacy warning is open.
    pub fn warning(&self) -> bool {
        self.warning
    }

    /// Closes the warning. Accepting sets both drafts (memory on, note
    /// acknowledged); declining leaves them as they were.
    pub fn answer_warning(&mut self, accept: bool) {
        self.warning = false;
        if accept {
            self.draft.memory.enabled = true;
            self.draft.memory.acknowledged = true;
        }
    }

    /// The memory keys whose draft differs from the saved one, as
    /// `(key, value)` in file order. The port is owned but never edited.
    pub fn changed_memory(&self) -> Vec<(&'static str, bool)> {
        let (old, new) = (self.original.memory, self.draft.memory);
        [
            ("enabled", old.enabled, new.enabled),
            ("acknowledged", old.acknowledged, new.acknowledged),
        ]
        .into_iter()
        .filter(|(_, old, new)| old != new)
        .map(|(key, _, new)| (key, new))
        .collect()
    }

    /// Moves the draft profile to the next (or previous) one, wrapping.
    pub fn cycle_profile(&mut self, forward: bool) {
        let next = self.draft.profile().cycled(forward);
        self.draft.set_profile(next);
    }

    /// The profile in the draft, and whether it differs from the open one.
    pub fn changed_profile(&self) -> Option<Profile> {
        (self.draft.profile() != self.original.profile()).then(|| self.draft.profile())
    }

    /// Flips the context flag of the draft.
    pub fn toggle_context(&mut self) {
        let next = !self.draft.context();
        self.draft.set_context(next);
    }

    /// The context flag in the draft, when it differs from the open one.
    pub fn changed_context(&self) -> Option<bool> {
        (self.draft.context() != self.original.context()).then(|| self.draft.context())
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

    /// Whether a detection result has arrived. An empty one found nothing
    /// at all, so it counts as none, as in the picker rows.
    pub fn has_result(&self) -> bool {
        self.engines.as_ref().is_some_and(|d| !d.engines.is_empty())
    }

    /// The row under the cursor of the open picker.
    pub fn cursor_row(&self) -> Option<PickRow> {
        let picker = self.picker.as_ref()?;
        self.pick_rows().into_iter().nth(picker.cursor())
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
