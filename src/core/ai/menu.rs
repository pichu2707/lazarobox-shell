//! Pure model of the AI section of the config menu: the draft assignment of
//! every task, shown against what the last detection found. No IO.

use std::path::{Path, PathBuf};

use super::{
    Detection, Status,
    config::{
        AiConfig, ApprovalMode, Assignment, ManagedServer, NotifyMode, Profile, Task, ToolToggle,
        cycled_idle,
    },
    hf::{Answer, Ask},
    picker::{InstallReply, Outcome, PickRow, Picker, Running, Target, Wizard, task_label},
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
    /// The Hugging Face download that runs, for llamacpp's entry row.
    download: Option<Running>,
}

impl AiMenu {
    pub fn new(original: AiConfig, engines: Option<Detection>) -> Self {
        Self {
            draft: original.clone(),
            original,
            engines,
            picker: None,
            warning: false,
            download: None,
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

    /// Steps the notify mode of the draft.
    pub fn cycle_notify(&mut self, forward: bool) {
        let next = self.draft.notify().cycled(forward);
        self.draft.set_notify(next);
    }

    /// Flips the notify sound flag of the draft.
    pub fn toggle_notify_sound(&mut self) {
        let next = !self.draft.notify_sound();
        self.draft.set_notify_sound(next);
    }

    /// The notify sound flag in the draft, when it differs from the open one.
    pub fn changed_notify_sound(&self) -> Option<bool> {
        (self.draft.notify_sound() != self.original.notify_sound())
            .then(|| self.draft.notify_sound())
    }

    /// The notify mode in the draft, when it differs from the open one.
    pub fn changed_notify(&self) -> Option<NotifyMode> {
        (self.draft.notify() != self.original.notify()).then(|| self.draft.notify())
    }

    /// Flips the agent tools flag of the draft.
    pub fn toggle_tools(&mut self) {
        let next = !self.draft.tools_enabled();
        self.draft.set_tools_enabled(next);
    }

    /// The model file of `assignment` when it is one lazarobox starts a
    /// stopped llama-server with: the managed one or a file found on the
    /// machine. `None` for anything a running server or a CLI serves.
    fn local_model(&self, assignment: &Assignment) -> Option<PathBuf> {
        if assignment.engine.as_deref() != Some("llamacpp") {
            return None;
        }
        let model = PathBuf::from(assignment.model.as_deref()?);
        let detection = self.engines.as_ref()?;
        // Managed, a server answering on the port (another lazarobox's, or
        // the user's) does not stop the model from getting one of its own.
        let managed = self.draft.managed().is_some();
        let startable = detection.engines.iter().any(|s| {
            s.id == "llamacpp"
                && (s.status == Status::NotRunning || (managed && s.status == Status::Available))
        });
        let known = self.draft.managed().is_some_and(|s| s.model == model)
            || detection.local_models.contains(&model);
        (startable && known).then_some(model)
    }

    /// Moves every task whose own model is one lazarobox starts to `model`:
    /// llama-server loads only one, so the rows say what answers.
    fn follow(&mut self, model: &Path) {
        let starts = |path: &Path| {
            self.draft.managed().is_some_and(|s| s.model == path)
                || self
                    .engines
                    .as_ref()
                    .is_some_and(|d| d.local_models.iter().any(|m| m == path))
        };
        let moved: Vec<Task> = Task::ALL
            .into_iter()
            .filter(|&task| {
                let own = self.draft.get(task);
                own.engine.as_deref() == Some("llamacpp")
                    && own
                        .model
                        .as_deref()
                        .is_some_and(|m| Path::new(m) != model && starts(Path::new(m)))
            })
            .collect();
        for task in moved {
            self.draft.set(
                task,
                Assignment {
                    engine: Some("llamacpp".to_owned()),
                    model: Some(model.to_string_lossy().into_owned()),
                },
            );
        }
    }

    /// The managed llama-server of the draft, when the menu turned it on or
    /// changed its model.
    pub fn changed_managed(&self) -> Option<&ManagedServer> {
        (self.draft.managed() != self.original.managed())
            .then(|| self.draft.managed())
            .flatten()
    }

    /// Flips one family switch of the draft.
    pub fn toggle_family(&mut self, toggle: ToolToggle) {
        let next = !self.draft.toggle(toggle);
        self.draft.set_toggle(toggle, next);
    }

    /// The family switches whose draft differs from the open one, in row
    /// order, with the draft value.
    pub fn changed_toggles(&self) -> impl Iterator<Item = (ToolToggle, bool)> + '_ {
        ToolToggle::ALL
            .into_iter()
            .filter(|t| self.draft.toggle(*t) != self.original.toggle(*t))
            .map(|t| (t, self.draft.toggle(t)))
    }

    /// The tools flag in the draft, when it differs from the open one.
    pub fn changed_tools(&self) -> Option<bool> {
        (self.draft.tools_enabled() != self.original.tools_enabled())
            .then(|| self.draft.tools_enabled())
    }

    /// Cycles the approval mode of the draft.
    pub fn cycle_approval(&mut self, forward: bool) {
        let next = self.draft.approval().cycled(forward);
        self.draft.set_approval(next);
    }

    /// Steps the idle time of the managed llama-server in the draft.
    pub fn cycle_idle(&mut self, forward: bool) {
        let minutes = self.draft.managed_options().idle_minutes;
        self.draft.set_idle_minutes(cycled_idle(minutes, forward));
    }

    /// The idle minutes in the draft, when they differ from the open ones.
    pub fn changed_idle(&self) -> Option<u32> {
        let minutes = self.draft.managed_options().idle_minutes;
        (minutes != self.original.managed_options().idle_minutes).then_some(minutes)
    }

    /// The approval mode in the draft, when it differs from the open one.
    pub fn changed_approval(&self) -> Option<ApprovalMode> {
        (self.draft.approval() != self.original.approval()).then(|| self.draft.approval())
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
        let mut picker = Picker::open(target, &self.draft, self.engines.as_ref());
        picker.set_download(self.download.clone());
        self.picker = Some(picker);
    }

    /// The download that runs (`None` once it ended): an open picker shows it
    /// at once.
    pub fn set_download(&mut self, download: Option<Running>) {
        if let Some(picker) = &mut self.picker {
            picker.set_download(download.clone());
        }
        self.download = download;
    }

    /// A reply of the Hub, for the open picker if it still waits for it;
    /// the message of a failed request it waited for.
    pub fn hub_reply(&mut self, ask: &Ask, answer: Result<Answer, String>) -> Option<String> {
        self.picker.as_mut()?.hub_reply(ask, answer)
    }

    pub fn picker(&self) -> Option<&Picker> {
        self.picker.as_ref()
    }

    /// The result of install request `id`, for the open wizard.
    pub fn install_reply(&mut self, id: u64, answer: Result<InstallReply, String>) {
        if let Some(picker) = &mut self.picker {
            picker.install_reply(id, answer);
        }
    }

    /// The bytes on disk of the install download of request `id`.
    pub fn install_progress(&mut self, id: u64, done: u64) {
        if let Some(picker) = &mut self.picker {
            picker.install_progress(id, done);
        }
    }

    /// Whether an install download runs for the open wizard.
    pub fn install_downloading(&self) -> bool {
        self.picker
            .as_ref()
            .and_then(|p| p.install())
            .is_some_and(Wizard::downloading)
    }

    /// The wizard screen was drawn: its prompts now accept `y`/`n`.
    pub fn install_arm(&mut self) {
        if let Some(picker) = &mut self.picker {
            picker.install_arm();
        }
    }

    /// Opens the install wizard over the open picker. Tests only: the entry
    /// row that opens it comes later.
    #[cfg(test)]
    pub fn open_install(&mut self, variants: Vec<crate::core::ai::install::Variant>) {
        if let Some(picker) = &mut self.picker {
            picker.open_install(variants);
        }
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

    /// Whether the detected engine `id` is installed but not answering.
    pub fn is_not_running(&self, id: &str) -> bool {
        self.engines.as_ref().is_some_and(|d| {
            d.engines
                .iter()
                .any(|s| s.id == id && s.status == Status::NotRunning)
        })
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
                let local = self.local_model(assignment);
                target.apply(&mut self.draft, assignment.clone());
                if let Some(model) = local {
                    self.follow(&model);
                    self.draft.manage(model);
                } else if self.changed_managed().is_some() && !self.draft.uses_managed() {
                    // A local model picked earlier that no task uses any more.
                    self.draft.keep_servers_of(&self.original);
                }
                self.picker = None;
            }
            Outcome::Close => self.picker = None,
            Outcome::Stay
            | Outcome::Refresh
            | Outcome::Copy(_)
            | Outcome::Ask(_)
            | Outcome::Download(_)
            | Outcome::CancelDownload
            | Outcome::Install(_) => {}
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
