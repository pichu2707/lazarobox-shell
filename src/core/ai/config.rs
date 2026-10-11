//! The `[ai]` and `[ai.tasks.*]` sections of `config.toml`: the saved engine
//! and model id of each AI task.

use toml::{Table, Value};

/// The fixed set of AI tasks, in display order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Task {
    General,
    Code,
    Chat,
    Review,
    Summarize,
}

impl Task {
    pub const ALL: [Self; 5] = [
        Self::General,
        Self::Code,
        Self::Chat,
        Self::Review,
        Self::Summarize,
    ];

    /// The lowercase task id.
    pub fn id(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Code => "code",
            Self::Chat => "chat",
            Self::Review => "review",
            Self::Summarize => "summarize",
        }
    }

    /// The `config.toml` table of the task: `ai` for general, else
    /// `ai.tasks.<id>`.
    pub fn table(self) -> &'static str {
        match self {
            Self::General => "ai",
            Self::Code => "ai.tasks.code",
            Self::Chat => "ai.tasks.chat",
            Self::Review => "ai.tasks.review",
            Self::Summarize => "ai.tasks.summarize",
        }
    }
}

/// An engine id plus a model name; either may be unset.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Assignment {
    pub engine: Option<String>,
    pub model: Option<String>,
}

/// The saved AI choice of every task. `None` means unset (a missing key or
/// `""`).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct AiConfig {
    slots: [Assignment; 5],
}

impl AiConfig {
    pub fn get(&self, task: Task) -> &Assignment {
        &self.slots[task as usize]
    }

    pub fn set(&mut self, task: Task, assignment: Assignment) {
        self.slots[task as usize] = assignment;
    }

    /// The effective assignment of a task and whether it was inherited from
    /// general.
    pub fn resolve(&self, task: Task) -> (Assignment, bool) {
        let own = self.get(task);
        if task != Task::General && own.engine.is_none() {
            (self.get(Task::General).clone(), true)
        } else {
            (own.clone(), false)
        }
    }

    /// The row label of a task.
    pub fn label(&self, task: Task) -> String {
        let own = self.get(task);
        match (&own.engine, &own.model) {
            (None, _) if task == Task::General => "(not set)".to_owned(),
            (None, _) => "(default)".to_owned(),
            (Some(engine), Some(model)) => format!("{engine} › {model}"),
            (Some(engine), None) => engine.clone(),
        }
    }

    /// The label of the effective (resolved) assignment of a task.
    fn effective_label(&self, task: Task) -> String {
        let (assignment, _) = self.resolve(task);
        match (assignment.engine, assignment.model) {
            (None, _) => "(not set)".to_owned(),
            (Some(engine), Some(model)) => format!("{engine} › {model}"),
            (Some(engine), None) => engine,
        }
    }

    /// The `Set all tasks` value: the shared effective label or `(mixed)`.
    pub fn aggregate(&self) -> String {
        let first = self.effective_label(Task::General);
        if Task::ALL.iter().all(|t| self.effective_label(*t) == first) {
            first
        } else {
            "(mixed)".to_owned()
        }
    }

    /// Test helper: only the general task assigned.
    #[cfg(test)]
    pub fn general(engine: Option<&str>, model: Option<&str>) -> Self {
        let mut config = Self::default();
        config.set(
            Task::General,
            Assignment {
                engine: engine.map(str::to_owned),
                model: model.map(str::to_owned),
            },
        );
        config
    }
}

/// Reads `[ai]`. A key of the wrong type falls back to unset on its own and
/// notes a problem; ids are kept verbatim, known or not.
pub fn read(table: &Table, problems: &mut Vec<String>) -> AiConfig {
    let section = match table.get("ai") {
        None => return AiConfig::default(),
        Some(Value::Table(section)) => section,
        Some(_) => {
            problems.push("ai: expected a table".to_owned());
            return AiConfig::default();
        }
    };
    let mut key = |name: &str| match section.get(name) {
        None => None,
        Some(Value::String(id)) => Some(id.clone()).filter(|id| !id.is_empty()),
        Some(_) => {
            problems.push(format!("ai.{name}: expected a string"));
            None
        }
    };
    let engine = key("engine");
    let model = key("model");
    let mut config = AiConfig::default();
    config.set(Task::General, Assignment { engine, model });
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_text(text: &str) -> (AiConfig, Vec<String>) {
        let table: Table = text.parse().unwrap();
        let mut problems = Vec::new();
        let ai = read(&table, &mut problems);
        (ai, problems)
    }

    fn ai(engine: Option<&str>, model: Option<&str>) -> AiConfig {
        AiConfig::general(engine, model)
    }

    /// A config with the given (task, engine, model) slots assigned.
    fn cfg(slots: &[(Task, Option<&str>, Option<&str>)]) -> AiConfig {
        let mut config = AiConfig::default();
        for (task, engine, model) in slots {
            config.set(
                *task,
                Assignment {
                    engine: engine.map(str::to_owned),
                    model: model.map(str::to_owned),
                },
            );
        }
        config
    }

    fn asg(engine: &str, model: &str) -> Assignment {
        Assignment {
            engine: Some(engine.to_owned()),
            model: Some(model.to_owned()),
        }
    }

    // Spec: Task set integrity.
    #[test]
    fn the_task_set_is_fixed_ordered_and_unique() {
        let ids: Vec<&str> = Task::ALL.iter().map(|t| t.id()).collect();
        assert_eq!(ids, ["general", "code", "chat", "review", "summarize"]);
        let tables: Vec<&str> = Task::ALL.iter().map(|t| t.table()).collect();
        assert_eq!(
            tables,
            [
                "ai",
                "ai.tasks.code",
                "ai.tasks.chat",
                "ai.tasks.review",
                "ai.tasks.summarize"
            ]
        );
    }

    // Spec: Effective assignment.
    #[test]
    fn an_unassigned_task_inherits_general() {
        let config = cfg(&[
            (Task::General, Some("claude"), Some("sonnet")),
            (Task::Code, Some("ollama"), Some("llama3:8b")),
        ]);
        assert_eq!(
            config.resolve(Task::Code),
            (asg("ollama", "llama3:8b"), false)
        );
        assert_eq!(config.resolve(Task::Chat), (asg("claude", "sonnet"), true));
        assert_eq!(
            config.resolve(Task::General),
            (asg("claude", "sonnet"), false)
        );
    }

    // Spec: Nothing assigned.
    #[test]
    fn nothing_assigned_resolves_to_nothing_with_default_labels() {
        let config = AiConfig::default();
        for task in Task::ALL {
            assert_eq!(config.resolve(task).0, Assignment::default());
        }
        assert_eq!(config.label(Task::General), "(not set)");
        for task in &Task::ALL[1..] {
            assert_eq!(config.label(*task), "(default)");
        }
    }

    // Spec: Model without engine is unassigned.
    #[test]
    fn a_model_without_an_engine_is_unassigned() {
        let config = cfg(&[
            (Task::General, Some("claude"), Some("opus")),
            (Task::Code, None, Some("x")),
        ]);
        assert_eq!(config.label(Task::Code), "(default)");
        assert_eq!(config.resolve(Task::Code), (asg("claude", "opus"), true));
        assert_eq!(
            cfg(&[(Task::General, None, Some("x"))]).label(Task::General),
            "(not set)"
        );
    }

    // Spec: Row labels.
    #[test]
    fn an_assigned_task_shows_engine_and_model() {
        let config = cfg(&[
            (Task::General, Some("claude"), Some("opus")),
            (Task::Code, Some("ollama"), Some("llama3:8b")),
            (Task::Chat, Some("codex"), None),
        ]);
        assert_eq!(config.label(Task::General), "claude › opus");
        assert_eq!(config.label(Task::Code), "ollama › llama3:8b");
        assert_eq!(config.label(Task::Chat), "codex");
        assert_eq!(config.label(Task::Review), "(default)");
    }

    // Spec: Aggregate values.
    #[test]
    fn the_aggregate_is_the_shared_effective_value_or_mixed() {
        let all = |engine, model| {
            let slots: Vec<_> = Task::ALL.iter().map(|t| (*t, engine, model)).collect();
            cfg(&slots)
        };
        assert_eq!(
            all(Some("claude"), Some("opus")).aggregate(),
            "claude › opus"
        );
        assert_eq!(AiConfig::default().aggregate(), "(not set)");
        assert_eq!(
            cfg(&[(Task::General, Some("claude"), Some("opus"))]).aggregate(),
            "claude › opus"
        );
        assert_eq!(
            cfg(&[
                (Task::General, Some("claude"), Some("opus")),
                (Task::Code, Some("ollama"), Some("llama3:8b")),
            ])
            .aggregate(),
            "(mixed)"
        );
    }

    // Spec: AI keys parsed.
    #[test]
    fn engine_and_model_are_read() {
        assert_eq!(
            read_text("[ai]\nengine = \"ollama\"\nmodel = \"llama3:8b\"\n"),
            (ai(Some("ollama"), Some("llama3:8b")), vec![])
        );
    }

    #[test]
    fn missing_section_and_keys_are_unset() {
        assert_eq!(read_text(""), (AiConfig::default(), vec![]));
        assert_eq!(read_text("[ai]\n"), (AiConfig::default(), vec![]));
        assert_eq!(
            read_text("[ai]\nmodel = \"m\"\n"),
            (ai(None, Some("m")), vec![])
        );
    }

    // Spec: Unknown engine id kept.
    #[test]
    fn an_unknown_engine_id_is_kept_verbatim() {
        assert_eq!(
            read_text("[ai]\nengine = \"future-engine\"\n"),
            (ai(Some("future-engine"), None), vec![])
        );
    }

    // Spec: Empty string is unset.
    #[test]
    fn an_empty_string_is_unset() {
        assert_eq!(
            read_text("[ai]\nengine = \"\"\nmodel = \"\"\n"),
            (AiConfig::default(), vec![])
        );
    }

    // Spec: Invalid AI types fall back per key.
    #[test]
    fn a_non_string_key_falls_back_alone() {
        assert_eq!(
            read_text("[ai]\nengine = 3\nmodel = \"m\"\n"),
            (
                ai(None, Some("m")),
                vec!["ai.engine: expected a string".to_owned()]
            )
        );
        assert_eq!(
            read_text("[ai]\nengine = \"claude\"\nmodel = true\n"),
            (
                ai(Some("claude"), None),
                vec!["ai.model: expected a string".to_owned()]
            )
        );
    }

    // Spec: Non-table ai.
    #[test]
    fn a_non_table_ai_is_one_problem() {
        assert_eq!(
            read_text("ai = 3\n"),
            (AiConfig::default(), vec!["ai: expected a table".to_owned()])
        );
    }
}
