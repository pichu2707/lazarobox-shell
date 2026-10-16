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

/// The persona given to the model. It changes only the prompts.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Profile {
    Analyst,
    #[default]
    Programmer,
}

impl Profile {
    pub const ALL: [Self; 2] = [Self::Analyst, Self::Programmer];

    /// The lowercase id: the value of `[ai] profile`.
    pub fn id(self) -> &'static str {
        match self {
            Self::Analyst => "analyst",
            Self::Programmer => "programmer",
        }
    }

    /// The profile of an id; case-sensitive, no trimming.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.id() == id)
    }

    /// The next profile, wrapping.
    pub fn cycled(self, forward: bool) -> Self {
        let len = Self::ALL.len();
        let step = if forward { 1 } else { len - 1 };
        Self::ALL[(self as usize + step) % len]
    }
}

/// How a finished chat turn reaches a user who is not looking
/// (`[ai] notify`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum NotifyMode {
    /// The bell always, plus a desktop notification when one is available.
    Auto,
    /// A desktop notification; the bell only when that is unavailable or
    /// fails.
    Desktop,
    /// The terminal bell alone.
    Bell,
    /// Nothing. The default.
    #[default]
    Off,
}

/// The exact problem of an unreadable `ai.notify`.
pub const NOTIFY_PROBLEM: &str =
    "ai.notify: expected true, false, \"auto\", \"desktop\", \"bell\" or \"off\"";

impl NotifyMode {
    pub const ALL: [Self; 4] = [Self::Auto, Self::Desktop, Self::Bell, Self::Off];

    /// The lowercase id: the value of `[ai] notify`.
    pub fn id(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Desktop => "desktop",
            Self::Bell => "bell",
            Self::Off => "off",
        }
    }

    /// The mode of an id; case-sensitive, no trimming.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.id() == id)
    }

    /// The next mode, wrapping.
    pub fn cycled(self, forward: bool) -> Self {
        let len = Self::ALL.len();
        let step = if forward { 1 } else { len - 1 };
        Self::ALL[(self as usize + step) % len]
    }
}

/// An engine id plus a model name; either may be unset.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Assignment {
    pub engine: Option<String>,
    pub model: Option<String>,
}

/// The default engram port, used when neither the file nor `ENGRAM_PORT`
/// gives a valid one.
pub const DEFAULT_ENGRAM_PORT: u16 = 7437;

/// The startup notice for a file that enables memory without the
/// acknowledgement of the privacy note.
pub const UNACKNOWLEDGED_NOTICE: &str =
    "memory: turn it on from the menu to review the privacy note";

/// The `[ai.memory]` table. The menu edits `enabled` and `acknowledged`; the
/// `port` is owned too but only ever hand-edited, so a save never touches it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MemoryConfig {
    /// The raw key. A hand-edited file can hold `enabled = true` without the
    /// acknowledgement, so consumers MUST ask [`MemoryConfig::active`], never
    /// read this flag to decide whether memory works or what to show.
    pub enabled: bool,
    pub acknowledged: bool,
    pub port: u16,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            acknowledged: false,
            port: DEFAULT_ENGRAM_PORT,
        }
    }
}

impl MemoryConfig {
    /// Memory works only when it is on AND the privacy note was accepted.
    pub fn active(&self) -> bool {
        self.enabled && self.acknowledged
    }
}

/// The saved AI choice of every task. `None` means unset (a missing key or
/// `""`). The profile, the context flag and the memory table ride along:
/// they live under `[ai]` too.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AiConfig {
    slots: [Assignment; 5],
    profile: Profile,
    context: bool,
    notify: NotifyMode,
    pub memory: MemoryConfig,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            slots: Default::default(),
            profile: Profile::default(),
            context: true,
            notify: NotifyMode::Off,
            memory: MemoryConfig::default(),
        }
    }
}

impl AiConfig {
    pub fn get(&self, task: Task) -> &Assignment {
        &self.slots[task as usize]
    }

    pub fn set(&mut self, task: Task, assignment: Assignment) {
        self.slots[task as usize] = assignment;
    }

    pub fn profile(&self) -> Profile {
        self.profile
    }

    pub fn set_profile(&mut self, profile: Profile) {
        self.profile = profile;
    }

    /// Whether the workspace context block is appended to the system prompt.
    pub fn context(&self) -> bool {
        self.context
    }

    pub fn set_context(&mut self, context: bool) {
        self.context = context;
    }

    /// How a chat answer that finishes while the user is not looking is
    /// announced.
    pub fn notify(&self) -> NotifyMode {
        self.notify
    }

    pub fn set_notify(&mut self, notify: NotifyMode) {
        self.notify = notify;
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

/// Reads one table's `engine` and `model`. A key of the wrong type falls
/// back to unset on its own and notes a problem under `path`; ids are kept
/// verbatim, known or not.
fn read_assignment(section: &Table, path: &str, problems: &mut Vec<String>) -> Assignment {
    let mut key = |name: &str| match section.get(name) {
        None => None,
        Some(Value::String(id)) => Some(id.clone()).filter(|id| !id.is_empty()),
        Some(_) => {
            problems.push(format!("{path}.{name}: expected a string"));
            None
        }
    };
    let engine = key("engine");
    let model = key("model");
    Assignment { engine, model }
}

/// Reads `profile`: a missing key and an empty string are the default; any
/// other value that is not a profile id falls back to it with a problem.
fn read_profile(section: &Table, problems: &mut Vec<String>) -> Profile {
    match section.get("profile") {
        None => Profile::default(),
        Some(Value::String(id)) if id.is_empty() => Profile::default(),
        Some(Value::String(id)) => Profile::from_id(id).unwrap_or_else(|| {
            problems.push("ai.profile: expected \"analyst\" or \"programmer\"".to_owned());
            Profile::default()
        }),
        Some(_) => {
            problems.push("ai.profile: expected a string".to_owned());
            Profile::default()
        }
    }
}

/// Reads `context`: a missing key is `true`; a value that is not a boolean
/// falls back to `true` with a problem.
fn read_context(section: &Table, problems: &mut Vec<String>) -> bool {
    match section.get("context") {
        None => true,
        Some(Value::Boolean(on)) => *on,
        Some(_) => {
            problems.push("ai.context: expected a boolean".to_owned());
            true
        }
    }
}

/// Reads `notify`: a missing key is `off`; the old booleans stay valid
/// (`true` is `auto`, `false` is `off`); anything else falls back to `off`
/// with a problem.
fn read_notify(section: &Table, problems: &mut Vec<String>) -> NotifyMode {
    match section.get("notify") {
        None => NotifyMode::Off,
        Some(Value::Boolean(true)) => NotifyMode::Auto,
        Some(Value::Boolean(false)) => NotifyMode::Off,
        Some(Value::String(id)) if NotifyMode::from_id(id).is_some() => {
            NotifyMode::from_id(id).unwrap_or_default()
        }
        Some(_) => {
            problems.push(NOTIFY_PROBLEM.to_owned());
            NotifyMode::Off
        }
    }
}

/// Reads `[ai]` (the general task, the profile and the context and notify flags) and `[ai.tasks.<id>]`
/// (the others). Each key falls back to unset on its own; `general` and
/// unknown names under `ai.tasks` are ignored.
fn read_tasks(table: &Table, problems: &mut Vec<String>) -> AiConfig {
    let mut config = AiConfig::default();
    let section = match table.get("ai") {
        None => return config,
        Some(Value::Table(section)) => section,
        Some(_) => {
            problems.push("ai: expected a table".to_owned());
            return config;
        }
    };
    config.set(Task::General, read_assignment(section, "ai", problems));
    config.profile = read_profile(section, problems);
    config.context = read_context(section, problems);
    config.notify = read_notify(section, problems);
    let tasks = match section.get("tasks") {
        None => return config,
        Some(Value::Table(tasks)) => tasks,
        Some(_) => {
            problems.push("ai.tasks: expected a table".to_owned());
            return config;
        }
    };
    for task in &Task::ALL[1..] {
        let path = task.table();
        match tasks.get(task.id()) {
            None => {}
            Some(Value::Table(section)) => {
                config.set(*task, read_assignment(section, path, problems));
            }
            Some(_) => problems.push(format!("{path}: expected a table")),
        }
    }
    config
}

/// Reads the whole `[ai]` tree, the memory table included. `port_env` is the
/// value of `ENGRAM_PORT`, read once by the caller: a valid one is the
/// default memory port.
pub fn read_with_port_env(
    table: &Table,
    problems: &mut Vec<String>,
    port_env: Option<&str>,
) -> AiConfig {
    let mut config = read_tasks(table, problems);
    config.memory = read_memory(table, problems, port_env);
    config
}

/// A valid port in the text of `ENGRAM_PORT`: an integer from 1 to 65535.
fn env_port(text: Option<&str>) -> Option<u16> {
    text?.trim().parse::<u16>().ok().filter(|port| *port != 0)
}

/// Reads `[ai.memory]`. Each key falls back to its default on its own; the
/// port default is a valid `ENGRAM_PORT`, else 7437. Unknown keys are
/// ignored.
fn read_memory(table: &Table, problems: &mut Vec<String>, port_env: Option<&str>) -> MemoryConfig {
    let mut memory = MemoryConfig {
        port: env_port(port_env).unwrap_or(DEFAULT_ENGRAM_PORT),
        ..MemoryConfig::default()
    };
    let section = match table.get("ai").and_then(Value::as_table) {
        None => return memory,
        Some(ai) => match ai.get("memory") {
            None => return memory,
            Some(Value::Table(section)) => section,
            Some(_) => {
                problems.push("ai.memory: expected a table".to_owned());
                return memory;
            }
        },
    };
    let mut flag = |name: &str, slot: &mut bool| match section.get(name) {
        None => {}
        Some(Value::Boolean(on)) => *slot = *on,
        Some(_) => problems.push(format!("ai.memory.{name}: expected a boolean")),
    };
    flag("enabled", &mut memory.enabled);
    flag("acknowledged", &mut memory.acknowledged);
    match section.get("port") {
        None => {}
        Some(Value::Integer(n)) if (1..=65535).contains(n) => memory.port = *n as u16,
        Some(_) => {
            problems.push("ai.memory.port: expected an integer from 1 to 65535".to_owned());
        }
    }
    memory
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_text(text: &str) -> (AiConfig, Vec<String>) {
        let table: Table = text.parse().unwrap();
        let mut problems = Vec::new();
        let ai = read_with_port_env(&table, &mut problems, None);
        (ai, problems)
    }

    fn memory_of(text: &str, env: Option<&str>) -> (MemoryConfig, Vec<String>) {
        let table: Table = text.parse().unwrap();
        let mut problems = Vec::new();
        let ai = read_with_port_env(&table, &mut problems, env);
        (ai.memory, problems)
    }

    fn mem(enabled: bool, acknowledged: bool, port: u16) -> MemoryConfig {
        MemoryConfig {
            enabled,
            acknowledged,
            port,
        }
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

    // Characterization: slots are indexed by discriminant, so `ALL` must
    // list the variants in declaration order.
    #[test]
    fn the_discriminants_follow_the_order_of_all() {
        for (i, task) in Task::ALL.iter().enumerate() {
            assert_eq!(*task as usize, i, "{}", task.id());
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

    // Spec: Task tables parsed.
    #[test]
    fn task_tables_are_read_next_to_general() {
        let text = "[ai]\nengine = \"claude\"\nmodel = \"sonnet\"\n\
                    [ai.tasks.code]\nengine = \"ollama\"\nmodel = \"llama3:8b\"\n";
        assert_eq!(
            read_text(text),
            (
                cfg(&[
                    (Task::General, Some("claude"), Some("sonnet")),
                    (Task::Code, Some("ollama"), Some("llama3:8b")),
                ]),
                vec![]
            )
        );
    }

    #[test]
    fn task_tables_work_without_an_ai_engine() {
        assert_eq!(
            read_text("[ai.tasks.summarize]\nmodel = \"m\"\nengine = \"codex\"\n"),
            (cfg(&[(Task::Summarize, Some("codex"), Some("m"))]), vec![])
        );
    }

    // Spec: Empty task string is unset.
    #[test]
    fn an_empty_task_string_is_unset() {
        assert_eq!(
            read_text("[ai.tasks.chat]\nengine = \"\"\nmodel = \"\"\n"),
            (AiConfig::default(), vec![])
        );
    }

    // Spec: General task table and unknown tasks ignored.
    #[test]
    fn the_general_task_table_and_unknown_tasks_are_ignored() {
        let text = "[ai.tasks.general]\nengine = \"codex\"\n\
                    [ai.tasks.poetry]\nengine = \"claude\"\nmodel = 4\n\
                    [ai]\nengine = \"ollama\"\n";
        assert_eq!(read_text(text), (ai(Some("ollama"), None), vec![]));
    }

    // Spec: Unknown task engine kept.
    #[test]
    fn an_unknown_task_engine_is_kept() {
        assert_eq!(
            read_text("[ai.tasks.review]\nengine = \"future-engine\"\n"),
            (cfg(&[(Task::Review, Some("future-engine"), None)]), vec![])
        );
    }

    // Spec: Invalid task key type falls back per key.
    #[test]
    fn a_non_string_task_key_falls_back_alone() {
        assert_eq!(
            read_text("[ai.tasks.code]\nengine = 5\nmodel = \"opus\"\n"),
            (
                cfg(&[(Task::Code, None, Some("opus"))]),
                vec!["ai.tasks.code.engine: expected a string".to_owned()]
            )
        );
        assert_eq!(
            read_text("[ai.tasks.code]\nengine = \"claude\"\nmodel = false\n"),
            (
                cfg(&[(Task::Code, Some("claude"), None)]),
                vec!["ai.tasks.code.model: expected a string".to_owned()]
            )
        );
    }

    // Spec: Non-table ai.tasks.
    #[test]
    fn a_non_table_tasks_key_leaves_general_and_unsets_the_tasks() {
        assert_eq!(
            read_text("[ai]\nengine = \"claude\"\ntasks = 3\n"),
            (
                ai(Some("claude"), None),
                vec!["ai.tasks: expected a table".to_owned()]
            )
        );
    }

    // Spec: Non-table task.
    #[test]
    fn a_non_table_task_is_unset_and_the_others_survive() {
        assert_eq!(
            read_text("[ai.tasks]\ncode = 3\n[ai.tasks.chat]\nengine = \"codex\"\n"),
            (
                cfg(&[(Task::Chat, Some("codex"), None)]),
                vec!["ai.tasks.code: expected a table".to_owned()]
            )
        );
    }

    // Spec: Several task problems counted (order: general, then Task::ALL).
    #[test]
    fn problems_come_general_first_then_tasks_in_task_order() {
        let text = "[ai.tasks.chat]\nmodel = 6\n[ai.tasks.code]\nengine = 5\n\
                    [ai]\nmodel = 1\n";
        assert_eq!(
            read_text(text),
            (
                AiConfig::default(),
                vec![
                    "ai.model: expected a string".to_owned(),
                    "ai.tasks.code.engine: expected a string".to_owned(),
                    "ai.tasks.chat.model: expected a string".to_owned(),
                ]
            )
        );
    }

    // Spec (prompt-profiles): Profile set integrity.
    #[test]
    fn the_profile_set_is_fixed_ordered_and_unique() {
        let ids: Vec<&str> = Profile::ALL.iter().map(|p| p.id()).collect();
        assert_eq!(ids, ["analyst", "programmer"]);
        for (i, profile) in Profile::ALL.iter().enumerate() {
            assert_eq!(*profile as usize, i, "{}", profile.id());
        }
    }

    // Spec (prompt-profiles): Default.
    #[test]
    fn the_profile_defaults_to_programmer() {
        assert_eq!(Profile::default(), Profile::Programmer);
        assert_eq!(AiConfig::default().profile(), Profile::Programmer);
        for text in ["", "[ai]\n", "[ai]\nprofile = \"\"\n", "ai = 3\n"] {
            assert_eq!(read_text(text).0.profile(), Profile::Programmer, "{text:?}");
        }
    }

    // Spec (configuration): Profile parsed.
    #[test]
    fn both_profile_ids_are_read_without_a_problem() {
        for profile in Profile::ALL {
            let text = format!("[ai]\nprofile = \"{}\"\n", profile.id());
            let (config, problems) = read_text(&text);
            assert_eq!(config.profile(), profile);
            assert_eq!(problems, Vec::<String>::new());
        }
    }

    // Spec (configuration): Unknown profile value, case and spaces.
    #[test]
    fn an_unknown_profile_falls_back_alone_with_the_exact_problem() {
        for value in ["wizard", "Analyst", " analyst", "analyst ", "PROGRAMMER"] {
            let text = format!("[ai]\nprofile = \"{value}\"\nengine = \"ollama\"\n");
            let (config, problems) = read_text(&text);
            assert_eq!(config.profile(), Profile::Programmer, "{value:?}");
            assert_eq!(config.get(Task::General).engine.as_deref(), Some("ollama"));
            assert_eq!(
                problems,
                vec!["ai.profile: expected \"analyst\" or \"programmer\"".to_owned()],
                "{value:?}"
            );
        }
    }

    // Spec (configuration): Non-string profile.
    #[test]
    fn a_non_string_profile_falls_back_with_the_exact_problem() {
        for value in ["3", "true", "[\"analyst\"]", "{ a = 1 }"] {
            let (config, problems) = read_text(&format!("[ai]\nprofile = {value}\n"));
            assert_eq!(config.profile(), Profile::Programmer, "{value}");
            assert_eq!(
                problems,
                vec!["ai.profile: expected a string".to_owned()],
                "{value}"
            );
        }
    }

    // Spec (configuration): problems are counted with the others, general
    // keys first, then the profile, then the tasks.
    #[test]
    fn the_profile_problem_comes_after_the_general_keys_and_before_the_tasks() {
        let text = "[ai.tasks.code]\nengine = 5\n[ai]\nprofile = 1\nmodel = 2\n";
        assert_eq!(
            read_text(text).1,
            vec![
                "ai.model: expected a string".to_owned(),
                "ai.profile: expected a string".to_owned(),
                "ai.tasks.code.engine: expected a string".to_owned(),
            ]
        );
    }

    // The profile is not an assignment: it never touches the tasks.
    #[test]
    fn the_profile_is_independent_of_the_assignments() {
        let (config, _) = read_text("[ai]\nprofile = \"analyst\"\n");
        assert_eq!(config.get(Task::General), &Assignment::default());
        let mut want = AiConfig::default();
        want.set_profile(Profile::Analyst);
        assert_eq!(config, want);
        assert_ne!(config, AiConfig::default());
    }

    #[test]
    fn a_non_table_ai_reports_no_task_problems() {
        assert_eq!(
            read_text("ai = 3\n"),
            (AiConfig::default(), vec!["ai: expected a table".to_owned()])
        );
    }

    // Spec (configuration): Memory keys, Defaults.
    #[test]
    fn an_absent_memory_table_gives_the_defaults() {
        assert_eq!(memory_of("", None), (mem(false, false, 7437), vec![]));
        assert_eq!(memory_of("[ai]\n", None), (mem(false, false, 7437), vec![]));
        assert!(!MemoryConfig::default().active());
    }

    // Spec: Values parsed.
    #[test]
    fn memory_values_are_read() {
        let text = "[ai.memory]\nenabled = true\nacknowledged = true\nport = 8123\n";
        let (memory, problems) = memory_of(text, None);
        assert_eq!(memory, mem(true, true, 8123));
        assert!(memory.active());
        assert!(problems.is_empty());
    }

    // Spec: Enabled without acknowledgement is inactive.
    #[test]
    fn enabled_without_the_acknowledgement_is_not_active() {
        let (memory, problems) = memory_of("[ai.memory]\nenabled = true\n", None);
        assert_eq!(memory, mem(true, false, 7437));
        assert!(!memory.active());
        assert!(problems.is_empty());
    }

    // Spec: Port default from the environment.
    #[test]
    fn the_port_defaults_to_a_valid_engram_port_variable() {
        assert_eq!(memory_of("", Some("8123")).0.port, 8123);
        for bad in ["abc", "0", "70000", "", "-1", "80 80"] {
            assert_eq!(memory_of("", Some(bad)).0.port, 7437, "{bad:?}");
        }
        let with_key = "[ai.memory]\nport = 9000\n";
        assert_eq!(memory_of(with_key, Some("8123")).0.port, 9000);
    }

    // Spec: Wrong types and range.
    #[test]
    fn a_wrong_memory_value_falls_back_alone_with_its_problem() {
        let cases = [
            (
                "[ai.memory]\nenabled = \"yes\"\nacknowledged = true\n",
                mem(false, true, 7437),
                "ai.memory.enabled: expected a boolean",
            ),
            (
                "[ai.memory]\nacknowledged = 1\nenabled = true\n",
                mem(true, false, 7437),
                "ai.memory.acknowledged: expected a boolean",
            ),
            (
                "[ai.memory]\nport = 0\n",
                mem(false, false, 7437),
                "ai.memory.port: expected an integer from 1 to 65535",
            ),
            (
                "[ai.memory]\nport = 70000\n",
                mem(false, false, 7437),
                "ai.memory.port: expected an integer from 1 to 65535",
            ),
            (
                "[ai.memory]\nport = \"7437\"\n",
                mem(false, false, 7437),
                "ai.memory.port: expected an integer from 1 to 65535",
            ),
        ];
        for (text, memory, problem) in cases {
            assert_eq!(memory_of(text, None), (memory, vec![problem.to_owned()]));
        }
    }

    #[test]
    fn a_bad_port_falls_back_to_the_environment_default() {
        let (memory, problems) = memory_of("[ai.memory]\nport = 0\n", Some("8123"));
        assert_eq!(memory.port, 8123);
        assert_eq!(problems.len(), 1);
    }

    // Spec: Non-table ai.memory.
    #[test]
    fn a_non_table_memory_is_one_problem_with_the_defaults() {
        assert_eq!(
            memory_of("[ai]\nmemory = 3\n", None),
            (
                mem(false, false, 7437),
                vec!["ai.memory: expected a table".to_owned()]
            )
        );
    }

    // Spec: Unknown keys ignored.
    #[test]
    fn unknown_memory_keys_are_ignored() {
        let text = "[ai.memory]\nendpoint = \"http://x\"\nenabled = true\n";
        assert_eq!(memory_of(text, None), (mem(true, false, 7437), vec![]));
    }

    // Tasks and memory are independent.
    #[test]
    fn memory_problems_follow_the_task_problems() {
        let table: Table = "[ai]\nmodel = 1\n[ai.memory]\nport = 0\n".parse().unwrap();
        let mut problems = Vec::new();
        read_with_port_env(&table, &mut problems, None);
        assert_eq!(
            problems,
            [
                "ai.model: expected a string",
                "ai.memory.port: expected an integer from 1 to 65535"
            ]
        );
    }

    // Spec (prompt-profiles): Context toggle, default and parse.
    #[test]
    fn the_context_defaults_to_true() {
        assert!(AiConfig::default().context());
        for text in ["", "[ai]\n", "ai = 3\n"] {
            assert!(read_text(text).0.context(), "{text:?}");
        }
    }

    #[test]
    fn a_boolean_context_is_read_without_a_problem() {
        for (value, want) in [("true", true), ("false", false)] {
            let (config, problems) = read_text(&format!("[ai]\ncontext = {value}\n"));
            assert_eq!(config.context(), want, "{value}");
            assert_eq!(problems, Vec::<String>::new());
        }
    }

    #[test]
    fn a_non_boolean_context_falls_back_to_true_with_the_exact_problem() {
        for value in ["\"no\"", "0", "[true]", "{ a = 1 }"] {
            let (config, problems) = read_text(&format!("[ai]\ncontext = {value}\n"));
            assert!(config.context(), "{value}");
            assert_eq!(
                problems,
                vec!["ai.context: expected a boolean".to_owned()],
                "{value}"
            );
        }
    }

    // The context is not an assignment and not the profile.
    #[test]
    fn the_context_is_independent_of_the_profile_and_the_assignments() {
        let (config, _) = read_text("[ai]\ncontext = false\nprofile = \"analyst\"\n");
        let mut want = AiConfig::default();
        want.set_context(false);
        want.set_profile(Profile::Analyst);
        assert_eq!(config, want);
        assert_ne!(config, AiConfig::default());
    }

    #[test]
    fn the_context_problem_comes_after_the_profile_problem() {
        let text = "[ai.tasks.code]\nengine = 5\n[ai]\ncontext = 1\nprofile = 1\n";
        assert_eq!(
            read_text(text).1,
            vec![
                "ai.profile: expected a string".to_owned(),
                "ai.context: expected a boolean".to_owned(),
                "ai.tasks.code.engine: expected a string".to_owned(),
            ]
        );
    }

    // Spec (chat-notify, configuration): the notify mode, default and parse.
    #[test]
    fn the_notify_mode_defaults_to_off_like_the_old_false() {
        assert_eq!(AiConfig::default().notify(), NotifyMode::Off);
        for text in ["", "[ai]\n", "ai = 3\n"] {
            assert_eq!(read_text(text).0.notify(), NotifyMode::Off, "{text:?}");
        }
    }

    #[test]
    fn a_legacy_boolean_maps_true_to_auto_and_false_to_off() {
        for (value, want) in [("true", NotifyMode::Auto), ("false", NotifyMode::Off)] {
            let (config, problems) = read_text(&format!("[ai]\nnotify = {value}\n"));
            assert_eq!(config.notify(), want, "{value}");
            assert_eq!(problems, Vec::<String>::new());
        }
    }

    #[test]
    fn the_four_mode_strings_are_read_without_a_problem() {
        for mode in NotifyMode::ALL {
            let (config, problems) = read_text(&format!("[ai]\nnotify = \"{}\"\n", mode.id()));
            assert_eq!(config.notify(), mode);
            assert_eq!(problems, Vec::<String>::new());
        }
    }

    #[test]
    fn an_unknown_notify_falls_back_to_off_with_the_exact_problem() {
        for value in ["\"yes\"", "\"Auto\"", "\"\"", "1", "[true]", "{ a = 1 }"] {
            let (config, problems) = read_text(&format!("[ai]\nnotify = {value}\n"));
            assert_eq!(config.notify(), NotifyMode::Off, "{value}");
            assert_eq!(problems, vec![NOTIFY_PROBLEM.to_owned()], "{value}");
        }
    }

    #[test]
    fn the_mode_cycles_through_the_four_values_both_ways() {
        let mut mode = NotifyMode::Auto;
        let mut seen = vec![mode];
        for _ in 0..4 {
            mode = mode.cycled(true);
            seen.push(mode);
        }
        assert_eq!(
            seen,
            [
                NotifyMode::Auto,
                NotifyMode::Desktop,
                NotifyMode::Bell,
                NotifyMode::Off,
                NotifyMode::Auto
            ]
        );
        assert_eq!(NotifyMode::Auto.cycled(false), NotifyMode::Off);
    }

    #[test]
    fn the_notify_flag_is_independent_and_its_problem_follows_the_context_one() {
        let (config, _) = read_text("[ai]\nnotify = true\ncontext = false\n");
        let mut want = AiConfig::default();
        want.set_context(false);
        want.set_notify(NotifyMode::Auto);
        assert_eq!(config, want);
        let text = "[ai.tasks.code]\nengine = 5\n[ai]\nnotify = 1\ncontext = 1\n";
        assert_eq!(
            read_text(text).1,
            vec![
                "ai.context: expected a boolean".to_owned(),
                NOTIFY_PROBLEM.to_owned(),
                "ai.tasks.code.engine: expected a string".to_owned(),
            ]
        );
    }
}
