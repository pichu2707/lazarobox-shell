//! Deleting a downloaded model from its row on the model level (config-menu,
//! Delete a model). `d` opens a screen that names the model and its size;
//! only an unmodified `y`, once the screen was drawn, asks the runtime.
//! The checks of D3 run on `d` and again on `y`, since a chat turn or a
//! download may have started in between. Pure: the request leaves as
//! [`Outcome::Delete`] and its reply comes back through
//! [`Picker::delete_reply`].
//!
//! The screen knows only a [`DeleteTarget`], so another kind of model joins
//! by adding one. A llama.cpp file is measured first: `d` asks the runtime
//! for its size ([`Outcome::Measure`]), and `y` waits for it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::super::catalog::same_model;
use super::super::config::{AiConfig, Task};
use super::super::{Detection, Status, hf};
use super::hub::{info, shown};
use super::{Level, MANAGED_MARK, Mark, Outcome, PickKey, PickRow, Picker, wrap_note};

/// The footer of the confirmation: the screen's keys.
pub const HINT_DELETE: &str = "y delete · any other key back";

/// The footer while the delete runs.
pub const HINT_DELETING: &str = "deleting…";

/// The footer while the runtime measures a llama.cpp file: `y` waits.
pub const HINT_MEASURING: &str = "measuring… · any other key back";

/// The line of a file in the Hugging Face cache, which other tools share.
pub const SHARED_CACHE: &str =
    "in the shared Hugging Face cache: other tools that use it will need to download it again";

/// The id of the last delete: a reply carries it, so one left behind by a
/// closed menu never answers a later delete.
static LAST_ID: AtomicU64 = AtomicU64::new(0);

/// What a delete removes.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum DeleteTarget {
    /// A model of the local Ollama server, by the name it lists. `size` is
    /// what it lists, `None` when it listed none.
    Ollama { name: String, size: Option<u64> },
    /// A llama.cpp model file the scan listed.
    Gguf(GgufFile),
}

/// A llama.cpp model file to delete, with every part of a split model.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GgufFile {
    /// The path the scan listed: the row's key.
    pub path: String,
    /// The sum of its parts, `None` until the runtime measured them. In a
    /// delete's reply, what the delete freed.
    pub size: Option<u64>,
    /// How many files it has: more than one for a split model.
    pub parts: usize,
    /// The model paths the config uses: the runtime refuses to delete any
    /// of them, however it is spelled.
    pub in_use: Vec<PathBuf>,
}

impl GgufFile {
    /// Whether the file sits in the Hugging Face cache layout
    /// (`models--<org>--<repo>/snapshots/<commit>/`).
    pub fn in_hub_cache(&self) -> bool {
        let names: Vec<&str> = self.path.split('/').collect();
        names
            .windows(3)
            .any(|w| w[0].starts_with("models--") && w[1] == "snapshots")
    }

    fn folder(&self) -> String {
        Path::new(&self.path)
            .parent()
            .map_or_else(String::new, |p| p.display().to_string())
    }
}

impl DeleteTarget {
    /// The name the row shows: a file's name, without its folder.
    pub fn name(&self) -> &str {
        match self {
            Self::Ollama { name, .. } => name,
            Self::Gguf(file) => file.path.rsplit('/').next().unwrap_or(&file.path),
        }
    }

    /// The row's key: the name of an Ollama model, the path of a file.
    pub fn key(&self) -> &str {
        match self {
            Self::Ollama { name, .. } => name,
            Self::Gguf(file) => &file.path,
        }
    }

    fn engine(&self) -> &'static str {
        match self {
            Self::Ollama { .. } => "ollama",
            Self::Gguf(_) => "llamacpp",
        }
    }

    /// The size line of the confirmation.
    fn size_line(&self) -> String {
        match self {
            Self::Ollama {
                size: Some(size), ..
            } => format!(
                "size: {} (models share layers, so it may free less)",
                hf::size_label(*size)
            ),
            Self::Ollama { size: None, .. } => "size: unknown".to_owned(),
            Self::Gguf(GgufFile { size: None, .. }) => "size: measuring…".to_owned(),
            Self::Gguf(GgufFile {
                size: Some(size),
                parts,
                ..
            }) => match parts {
                0 | 1 => format!("size: {}", hf::size_label(*size)),
                n => format!("size: {} in {n} parts", hf::size_label(*size)),
            },
        }
    }

    /// The notice of a delete that worked.
    pub fn deleted(&self) -> String {
        match self {
            Self::Ollama {
                name,
                size: Some(size),
            } => format!(
                "deleted {} · freed up to {}",
                shown(name),
                hf::size_label(*size)
            ),
            Self::Ollama { name, size: None } => format!("deleted {}", shown(name)),
            Self::Gguf(file) => {
                let parts = match file.parts {
                    0 | 1 => String::new(),
                    n => format!(" ({n} parts)"),
                };
                let freed = file
                    .size
                    .map(|size| format!(" · freed {}", hf::size_label(size)))
                    .unwrap_or_default();
                format!("deleted {}{parts}{freed}", shown(self.name()))
            }
        }
    }

    /// The notice of a delete that failed.
    pub fn failed(&self, why: &str) -> String {
        format!("could not delete {}: {why}", shown(self.name()))
    }
}

/// A delete the runtime is asked to carry out.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DeleteAsk {
    pub id: u64,
    pub target: DeleteTarget,
}

/// What can keep a model from being deleted, read when `d` and `y` are
/// pressed.
#[derive(Clone, Copy, Debug)]
pub struct InUse<'a> {
    pub draft: &'a AiConfig,
    pub saved: &'a AiConfig,
    /// `(engine, model)` of the chat turns running now.
    pub turns: &'a [(String, String)],
}

/// The confirmation of one delete.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Delete {
    engine: String,
    target: DeleteTarget,
    /// The cursor on the model rows, to come back near the row even if it
    /// is gone by then.
    row: usize,
    /// Whether the screen was drawn: a key queued before it must not answer
    /// a question never seen.
    armed: bool,
    /// The id of the delete that runs; any other reply is stale.
    in_flight: Option<u64>,
    /// The id of the measure of a llama.cpp file not answered yet.
    measuring: Option<u64>,
}

impl Delete {
    pub fn title(&self) -> String {
        "Delete a model".to_owned()
    }

    pub fn hint(&self) -> &'static str {
        if self.in_flight.is_some() {
            HINT_DELETING
        } else if self.measuring.is_some() {
            HINT_MEASURING
        } else {
            HINT_DELETE
        }
    }

    /// The id of the delete that runs, if any.
    pub fn in_flight(&self) -> Option<u64> {
        self.in_flight
    }

    /// The id of the measure the screen waits for, if any.
    pub fn measuring(&self) -> Option<u64> {
        self.measuring
    }

    /// The screen was drawn: its answers count.
    pub(super) fn arm(&mut self) {
        self.armed = true;
    }

    pub fn rows(&self) -> Vec<PickRow> {
        let lines = if self.in_flight.is_some() {
            vec![format!("deleting {}…", self.target.name())]
        } else {
            let mut lines = vec![format!("delete {}?", self.target.name())];
            if let DeleteTarget::Gguf(file) = &self.target {
                lines.push(format!("folder: {}", file.folder()));
            }
            lines.push(self.target.size_line());
            if matches!(&self.target, DeleteTarget::Gguf(file) if file.in_hub_cache()) {
                lines.push(SHARED_CACHE.to_owned());
            }
            lines.push("This cannot be undone.".to_owned());
            lines
        };
        lines
            .iter()
            .flat_map(|line| wrap_note(&shown(line)))
            .map(info)
            .collect()
    }
}

impl Picker {
    pub fn delete(&self) -> Option<&Delete> {
        match &self.level {
            Level::Delete(delete) => Some(delete),
            _ => None,
        }
    }

    /// What `d` would delete on the row under the cursor: a downloaded
    /// model of the Ollama server, or a llama.cpp file the model scan found.
    /// A model a running llama-server lists has no file behind it, and the
    /// managed model is listed even when the scan did not find it: neither
    /// goes.
    pub fn delete_target(
        &self,
        draft: &AiConfig,
        detection: Option<&Detection>,
    ) -> Option<DeleteTarget> {
        let Level::Models { engine } = &self.level else {
            return None;
        };
        if engine != "ollama" && engine != "llamacpp" {
            return None;
        }
        let rows = self.rows(draft, detection);
        let row = rows.get(self.cursor)?;
        let PickKey::Model(name) = &row.key else {
            return None;
        };
        if engine == "ollama" {
            return (row.mark == Mark::Downloaded).then(|| DeleteTarget::Ollama {
                name: name.clone(),
                size: detection.and_then(|d| d.ollama_size(name)),
            });
        }
        let scanned = detection.is_some_and(|d| {
            d.local_models
                .iter()
                .any(|m| m.as_os_str() == name.as_str())
        });
        (scanned && row.mark == Mark::Status(MANAGED_MARK.to_owned())).then(|| {
            DeleteTarget::Gguf(GgufFile {
                path: name.clone(),
                size: None,
                parts: 1,
                in_use: Vec::new(),
            })
        })
    }

    /// Why `target` cannot go now, as the outcome that says so.
    fn refusal(
        &self,
        target: &DeleteTarget,
        uses: InUse<'_>,
        detection: Option<&Detection>,
    ) -> Option<Outcome> {
        if self.download.is_some() {
            return Some(Outcome::Busy);
        }
        let (engine, key, name) = (target.engine(), target.key(), target.name());
        let runs_on = |e: &str, m: &str| e == engine && same_model(engine, m, key);
        if uses.turns.iter().any(|(e, m)| runs_on(e, m)) {
            return Some(Outcome::Refused(format!(
                "{} is in use by a running chat turn",
                shown(name)
            )));
        }
        if let DeleteTarget::Gguf(file) = target {
            let path = Path::new(&file.path);
            let managed = [uses.draft, uses.saved]
                .iter()
                .any(|config| config.managed().is_some_and(|m| m.model == path));
            if managed {
                return Some(Outcome::Refused(format!(
                    "{} is the model of the managed llama-server · choose another one for it first",
                    shown(name)
                )));
            }
            let served = detection.is_some_and(|d| {
                d.engines.iter().any(|s| {
                    s.id == "llamacpp"
                        && s.status == Status::Available
                        && s.models.iter().any(|m| m == key)
                })
            });
            if served {
                return Some(Outcome::Refused(format!(
                    "{} is loaded by the running llama-server",
                    shown(name)
                )));
            }
        }
        // The saved config counts too: until a save, it is what runs.
        let tasks: Vec<&str> = Task::ALL
            .into_iter()
            .filter(|task| {
                [uses.draft, uses.saved].iter().any(|config| {
                    let slot = config.get(*task);
                    matches!(
                        (slot.engine.as_deref(), slot.model.as_deref()),
                        (Some(e), Some(m)) if runs_on(e, m)
                    )
                })
            })
            .map(Task::id)
            .collect();
        if tasks.is_empty() {
            return None;
        }
        let noun = if tasks.len() == 1 { "task" } else { "tasks" };
        Some(Outcome::Refused(format!(
            "{} is used by {noun} {} · assign another model first",
            shown(name),
            tasks.join(", ")
        )))
    }

    /// `d`: open the confirmation of the row under the cursor, or say why
    /// the model cannot go. A llama.cpp file is measured first.
    pub fn open_delete(&mut self, uses: InUse<'_>, detection: Option<&Detection>) -> Outcome {
        let Some(mut target) = self.delete_target(uses.draft, detection) else {
            return Outcome::Stay;
        };
        if let Some(refused) = self.refusal(&target, uses, detection) {
            return refused;
        }
        let Level::Models { engine } = &self.level else {
            return Outcome::Stay;
        };
        let measuring = match &mut target {
            DeleteTarget::Gguf(file) => {
                file.in_use = in_use_paths(uses);
                Some(LAST_ID.fetch_add(1, Ordering::Relaxed) + 1)
            }
            DeleteTarget::Ollama { .. } => None,
        };
        self.level = Level::Delete(Delete {
            engine: engine.clone(),
            target: target.clone(),
            row: self.cursor,
            armed: false,
            in_flight: None,
            measuring,
        });
        self.cursor = 0;
        self.offset = 0;
        match measuring {
            Some(id) => Outcome::Measure(DeleteAsk { id, target }),
            None => Outcome::Stay,
        }
    }

    /// The runtime measured the file of the screen: its target now carries
    /// the size and the parts. Any other answer changes nothing.
    pub fn delete_measured(&mut self, ask: DeleteAsk) {
        if let Level::Delete(delete) = &mut self.level
            && delete.measuring == Some(ask.id)
            && delete.target.key() == ask.target.key()
        {
            delete.target = ask.target;
            delete.measuring = None;
        }
    }

    /// A key on the confirmation: `yes` is an unmodified `y`. A key pressed
    /// before the screen was drawn answers nothing, and neither does one
    /// pressed while the delete runs.
    pub fn delete_answer(
        &mut self,
        yes: bool,
        uses: InUse<'_>,
        detection: Option<&Detection>,
    ) -> Outcome {
        let Level::Delete(delete) = &self.level else {
            return Outcome::Stay;
        };
        if !delete.armed || delete.in_flight.is_some() {
            return Outcome::Stay;
        }
        if !yes {
            self.close_delete(uses.draft, detection);
            return Outcome::Stay;
        }
        // The size is shown before a file can go (D4).
        if delete.measuring.is_some() {
            return Outcome::Stay;
        }
        let mut target = delete.target.clone();
        if let Some(refused) = self.refusal(&target, uses, detection) {
            self.close_delete(uses.draft, detection);
            return refused;
        }
        if let DeleteTarget::Gguf(file) = &mut target {
            file.in_use = in_use_paths(uses);
        }
        let id = LAST_ID.fetch_add(1, Ordering::Relaxed) + 1;
        if let Level::Delete(delete) = &mut self.level {
            delete.in_flight = Some(id);
        }
        Outcome::Delete(DeleteAsk { id, target })
    }

    /// The reply of delete `id`, or a measure that failed: back on the model
    /// rows, near the row. A reply that is not the delete in flight, or the
    /// measure not answered yet, changes nothing.
    pub fn delete_reply(&mut self, id: u64, draft: &AiConfig, detection: Option<&Detection>) {
        if self
            .delete()
            .is_some_and(|d| d.in_flight == Some(id) || d.measuring == Some(id))
        {
            self.close_delete(draft, detection);
        }
    }

    /// Back to the model rows, the cursor on the model's row while it is
    /// still listed.
    pub(super) fn close_delete(&mut self, draft: &AiConfig, detection: Option<&Detection>) {
        let Level::Delete(delete) = &self.level else {
            return;
        };
        let key = PickKey::Model(delete.target.key().to_owned());
        self.cursor = delete.row;
        self.level = Level::Models {
            engine: delete.engine.clone(),
        };
        self.reanchor(Some(key), draft, detection);
    }
}

/// The model files the config uses: the managed llama-server's, draft and
/// saved, and every llama.cpp task assignment.
fn in_use_paths(uses: InUse<'_>) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    for config in [uses.draft, uses.saved] {
        let assigned = Task::ALL.into_iter().filter_map(|task| {
            let slot = config.get(task);
            (slot.engine.as_deref() == Some("llamacpp"))
                .then(|| slot.model.as_deref().map(PathBuf::from))
                .flatten()
        });
        let managed = config.managed().map(|m| m.model.clone());
        for path in managed.into_iter().chain(assigned) {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    paths
}
