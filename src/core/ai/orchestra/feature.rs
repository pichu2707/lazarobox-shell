//! The feature document of a large goal (agent-orchestrator S5, D15): the
//! user's requirements in the user's words, the tasks that cover them and a
//! log whose first entry is the goal. Pure: the run keeps it, the app stores
//! it under one stable key.

use super::RunId;

/// The most requirements and tasks a document holds.
pub const MAX_ITEMS: usize = 32;

/// The most a rendered document takes, in bytes, the goal left out: it
/// travels in the run ledger of every continuation turn.
pub const MAX_BYTES: usize = 3 * 1024;

/// A requirement. `text` is the user's own words, never rewritten.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Spec {
    pub id: String,
    pub text: String,
}

/// A unit of work and the requirements it covers.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TaskRow {
    pub id: String,
    pub specs: Vec<String>,
    pub title: String,
    pub status: String,
}

/// Why a document was not built or changed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FeatureError {
    /// A correction needs the document first.
    NoDocument,
    /// The run has its document: a change of requirement is a correction.
    Exists,
    NoSpecs,
    TooMany,
    TooBig,
    /// The task (counted from 1) is not `S1,S2: title` with known specs.
    BadTask(usize),
    /// A correction names a requirement the document does not have.
    UnknownSpec,
}

/// The feature document of one run.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FeatureDoc {
    run: RunId,
    specs: Vec<Spec>,
    tasks: Vec<TaskRow>,
    /// `log[0]` is the goal; `L{n}` is `log[n - 1]`.
    log: Vec<String>,
}

impl FeatureDoc {
    /// The document of `goal` with the requirements `specs` (numbered `S1`...)
    /// and `tasks`, each written `S1,S2: title`.
    pub fn build(
        run: RunId,
        goal: &str,
        specs: &[String],
        tasks: &[String],
    ) -> Result<Self, FeatureError> {
        if specs.is_empty() {
            return Err(FeatureError::NoSpecs);
        }
        if specs.len() > MAX_ITEMS || tasks.len() > MAX_ITEMS {
            return Err(FeatureError::TooMany);
        }
        let specs: Vec<Spec> = specs
            .iter()
            .enumerate()
            .map(|(at, text)| Spec {
                id: format!("S{}", at + 1),
                text: text_of(text),
            })
            .collect();
        let mut rows = Vec::new();
        for (at, task) in tasks.iter().enumerate() {
            let bad = FeatureError::BadTask(at + 1);
            let (refs, title) = task.split_once(':').ok_or(bad)?;
            let refs: Vec<String> = refs.split(',').map(|r| r.trim().to_owned()).collect();
            if refs.iter().any(|r| specs.iter().all(|s| &s.id != r)) {
                return Err(bad);
            }
            rows.push(TaskRow {
                id: format!("T{}", at + 1),
                specs: refs,
                title: text_of(title).replace(['\n', '\t'], " ").trim().to_owned(),
                status: "todo".to_owned(),
            });
        }
        let doc = Self {
            run,
            specs,
            tasks: rows,
            log: vec![goal.to_owned()],
        };
        doc.fits()
    }

    /// A user correction: `log` gains `correction` verbatim and requirement
    /// `spec` becomes `text`. Nothing else changes.
    pub fn correct(
        &mut self,
        spec: &str,
        text: &str,
        correction: &str,
    ) -> Result<(), FeatureError> {
        let at = self
            .specs
            .iter()
            .position(|s| s.id == spec)
            .ok_or(FeatureError::UnknownSpec)?;
        let mut next = self.clone();
        next.specs[at].text = text_of(text);
        next.log.push(text_of(correction));
        *self = next.fits()?;
        Ok(())
    }

    fn fits(self) -> Result<Self, FeatureError> {
        if self.render_for_ledger().len() > MAX_BYTES {
            return Err(FeatureError::TooBig);
        }
        Ok(self)
    }

    pub fn specs(&self) -> &[Spec] {
        &self.specs
    }

    pub fn tasks(&self) -> &[TaskRow] {
        &self.tasks
    }

    pub fn log(&self) -> &[String] {
        &self.log
    }

    pub fn run(&self) -> RunId {
        self.run
    }

    /// The whole document, as stored.
    pub fn render(&self) -> String {
        self.draw(None)
    }

    /// The document for the run ledger, which already says the goal.
    pub fn render_for_ledger(&self) -> String {
        self.draw(Some("(the goal above)"))
    }

    /// The count line the ledger keeps when it has no room for the document.
    pub fn short_line(&self) -> String {
        format!(
            "Feature document: {} specs, {} tasks, {} log entries",
            self.specs.len(),
            self.tasks.len(),
            self.log.len()
        )
    }

    fn draw(&self, goal: Option<&str>) -> String {
        let mut out = format!("Feature document (run {})\nSpecs:", self.run.0);
        for spec in &self.specs {
            out.push_str(&format!("\n{}: {}", spec.id, spec.text));
        }
        out.push_str("\nTasks:");
        for task in &self.tasks {
            let covers = task.specs.join(", ");
            out.push_str(&format!(
                "\n{} [{}] ({covers}): {}",
                task.id, task.status, task.title
            ));
        }
        out.push_str("\nLog:");
        for (at, entry) in self.log.iter().enumerate() {
            let entry = if at == 0 {
                goal.unwrap_or(entry)
            } else {
                entry
            };
            out.push_str(&format!("\nL{}: {entry}", at + 1));
        }
        out
    }
}

/// `text` without control characters other than line breaks and tabs: it may
/// reach a model and the engram note. The words are otherwise untouched.
fn text_of(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect()
}
