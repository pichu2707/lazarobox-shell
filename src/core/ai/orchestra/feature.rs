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
    /// The task (counted from 1) is not written `S1,S2: title`.
    MalformedTask(usize),
    /// The task (counted from 1) covers a spec the document does not have.
    UnknownRef {
        task: usize,
        spec: u32,
    },
    /// Spec `n` (counted from 1) is blank.
    EmptySpec(usize),
    /// A correction without the new wording of the requirement.
    EmptyText,
    /// The task (counted from 1) has no title.
    EmptyTitle(usize),
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
    /// Entries dropped after `log[0]`, the oldest first: numbers stay stable.
    omitted: usize,
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
        if let Some(at) = specs.iter().position(|t| text_of(t).trim().is_empty()) {
            return Err(FeatureError::EmptySpec(at + 1));
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
            let bad = FeatureError::MalformedTask(at + 1);
            let (refs, title) = task.split_once(':').ok_or(bad)?;
            let mut covers = Vec::new();
            for r in refs.split(',').map(str::trim) {
                let number = r.strip_prefix('S').and_then(|n| n.parse::<u32>().ok());
                let number = number.ok_or(bad)?;
                if number == 0 || number as usize > specs.len() {
                    return Err(FeatureError::UnknownRef {
                        task: at + 1,
                        spec: number,
                    });
                }
                covers.push(format!("S{number}"));
            }
            let title = text_of(title).replace(['\n', '\t'], " ").trim().to_owned();
            if title.is_empty() {
                return Err(FeatureError::EmptyTitle(at + 1));
            }
            rows.push(TaskRow {
                id: format!("T{}", at + 1),
                specs: covers,
                title,
                status: "todo".to_owned(),
            });
        }
        let doc = Self {
            run,
            specs,
            tasks: rows,
            log: vec![text_of(goal)],
            omitted: 0,
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
        if text_of(text).trim().is_empty() {
            return Err(FeatureError::EmptyText);
        }
        let mut next = self.clone();
        next.specs[at].text = text_of(text);
        next.log.push(text_of(correction));
        // Keep the goal and the newest entries: drop the oldest until it fits.
        while next.log.len() > 2 && next.render_for_ledger().len() > MAX_BYTES {
            next.log.remove(1);
            next.omitted += 1;
        }
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

    /// The log entries kept: the goal, then the newest ones.
    pub fn log(&self) -> &[String] {
        &self.log
    }

    /// How many entries the log dropped (the oldest after the goal).
    pub fn omitted(&self) -> usize {
        self.omitted
    }

    /// The number of the newest log entry (`L{n}`).
    pub fn entries(&self) -> usize {
        self.log.len() + self.omitted
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
            self.entries()
        )
    }

    fn draw(&self, goal: Option<&str>) -> String {
        let mut out = format!("Feature document (run {})\nSpecs:", self.run.0);
        for spec in &self.specs {
            out.push_str(&format!("\n{}: {}", spec.id, indent(&spec.text)));
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
            if at == 1 && self.omitted > 0 {
                out.push_str(&format!("\n({} older log entries omitted)", self.omitted));
            }
            let entry = if at == 0 {
                goal.unwrap_or(entry)
            } else {
                entry
            };
            let number = if at == 0 { 1 } else { at + 1 + self.omitted };
            out.push_str(&format!("\nL{number}: {}", indent(entry)));
        }
        out
    }
}

/// `text` with every continuation line indented, so that no line of the
/// user's words can start like a line of the document or the ledger.
pub fn indent(text: &str) -> String {
    text.replace('\n', "\n    ")
}

/// `text` without control characters other than line breaks and tabs, and
/// without the bidirectional overrides and isolates: it may reach a model and
/// the engram note. The words are otherwise untouched.
fn text_of(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .filter(|c| !matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'))
        .collect()
}
