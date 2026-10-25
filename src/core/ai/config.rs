//! The `[ai]` and `[ai.tasks.*]` sections of `config.toml`: the saved engine
//! and model id of each AI task.

use std::path::{Path, PathBuf};

use toml::{Table, Value};

use super::{CATALOG, Kind, ServerPorts};

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

/// What a model's `run_in_pane` does in a chat (`[ai.tools] approval`, design
/// D18 and D19). Only the user switches it: the model can never reach it, so
/// no tool call, request or runtime event carries a mode.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ApprovalMode {
    /// The user confirms each command with `y`. The default.
    #[default]
    Ask,
    /// The command is typed into the pane without a prompt, never followed
    /// by Enter.
    Auto,
    /// A short list of navigation commands runs at once and its output goes
    /// to the model; every other command asks. Routes like `Auto` until the
    /// run guards land.
    Run,
}

/// The exact problem of an unreadable `ai.tools.approval`.
pub const APPROVAL_PROBLEM: &str = "ai.tools.approval: expected \"ask\", \"auto\" or \"run\"";

impl ApprovalMode {
    pub const ALL: [Self; 3] = [Self::Ask, Self::Auto, Self::Run];

    /// The lowercase id: the value of `[ai.tools] approval`.
    pub fn id(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Auto => "auto",
            Self::Run => "run",
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

/// One user MCP server from `[[ai.mcp.servers]]` (design D16). `env` holds
/// variable NAMES only: the values come from lazarobox's own environment when
/// a later slice starts the server. `project` is the absolute git root the
/// entry is limited to; `None` is a global server.
#[derive(Clone, PartialEq, Eq)]
pub struct McpServer {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<String>,
    pub project: Option<PathBuf>,
    pub enabled: bool,
}

/// Prints the name, the project, the flag and the counts, never the command,
/// the arguments or the variable names: a `{:?}` of the config cannot leak them.
impl std::fmt::Debug for McpServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpServer")
            .field("name", &self.name)
            .field("project", &self.project)
            .field("enabled", &self.enabled)
            .field("args", &self.args.len())
            .field("env", &self.env.len())
            .finish_non_exhaustive()
    }
}

/// The longest server name, in characters. It is also a part of a qualified
/// tool name, which is capped at the same length.
const MCP_NAME_MAX: usize = 24;
/// The longest environment variable name, in bytes.
const MCP_ENV_NAME_MAX: usize = 64;

/// The saved AI choice of every task. `None` means unset (a missing key or
/// `""`). The profile, the context flag and the memory table ride along:
/// they live under `[ai]` too.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AiConfig {
    slots: [Assignment; 5],
    profile: Profile,
    context: bool,
    notify: NotifyMode,
    notify_sound: bool,
    tools: bool,
    approval: ApprovalMode,
    mcp_servers: Vec<McpServer>,
    servers: ServerPorts,
    managed: Option<ManagedServer>,
    pub memory: MemoryConfig,
}

/// The port of a llama-server that lazarobox starts itself, unless
/// `[ai.servers.llamacpp] port` says otherwise. Uncommon on purpose, so it
/// does not meet another server's default.
pub const MANAGED_PORT: u16 = 39281;

/// A llama-server lazarobox starts itself (`[ai.servers.llamacpp]` with
/// `managed = true`), with the model file it loads.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ManagedServer {
    pub model: PathBuf,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            slots: Default::default(),
            profile: Profile::default(),
            context: true,
            notify: NotifyMode::Off,
            notify_sound: true,
            tools: true,
            approval: ApprovalMode::Ask,
            mcp_servers: Vec::new(),
            servers: ServerPorts::default(),
            managed: None,
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

    /// Whether lazarobox plays its own sound with the desktop notification
    /// (`auto` and `desktop` modes).
    pub fn notify_sound(&self) -> bool {
        self.notify_sound
    }

    pub fn set_notify_sound(&mut self, sound: bool) {
        self.notify_sound = sound;
    }

    /// Whether the chat may act in the user's panes (`[ai.tools] enabled`).
    /// On by default: nothing is written without the user's `y` anyway.
    pub fn tools_enabled(&self) -> bool {
        self.tools
    }

    pub fn set_tools_enabled(&mut self, on: bool) {
        self.tools = on;
    }

    /// What a model's `run_in_pane` does in a chat opened now
    /// (`[ai.tools] approval`).
    pub fn approval(&self) -> ApprovalMode {
        self.approval
    }

    /// The user MCP servers as declared, global and per-project, in file
    /// order. Read once at startup; the menu never writes them.
    pub fn mcp_servers(&self) -> &[McpServer] {
        &self.mcp_servers
    }

    /// The loopback port of each server engine (`[ai.servers.<id>] port`).
    pub fn servers(&self) -> &ServerPorts {
        &self.servers
    }

    /// The llama-server lazarobox starts itself, when it is turned on.
    pub fn managed(&self) -> Option<&ManagedServer> {
        self.managed.as_ref()
    }

    /// Turns lazarobox's own llama-server on with `model`, as `managed = true`
    /// and `model` in the config would: llamacpp moves to [`MANAGED_PORT`]
    /// unless it already has a port of its own.
    pub fn manage(&mut self, model: PathBuf) {
        if self.managed.is_none() && !self.servers.is_set("llamacpp") {
            self.servers = self.servers.clone().with("llamacpp", MANAGED_PORT);
        }
        self.managed = Some(ManagedServer { model });
    }

    /// The managed llama-server and the ports of `other`, as they were before
    /// [`AiConfig::manage`].
    pub fn keep_servers_of(&mut self, other: &AiConfig) {
        self.managed = other.managed.clone();
        self.servers = other.servers.clone();
    }

    /// Whether a task, its own or through general, uses the managed
    /// llama-server's model.
    pub fn uses_managed(&self) -> bool {
        let Some(server) = &self.managed else {
            return false;
        };
        Task::ALL.into_iter().any(|task| {
            let (a, _) = self.resolve(task);
            a.engine.as_deref() == Some("llamacpp")
                && a.model.as_deref().map(Path::new) == Some(server.model.as_path())
        })
    }

    #[cfg(test)]
    pub fn with_managed(mut self, managed: ManagedServer) -> Self {
        self.managed = Some(managed);
        self
    }

    #[cfg(test)]
    pub fn with_servers(mut self, servers: ServerPorts) -> Self {
        self.servers = servers;
        self
    }

    pub fn set_approval(&mut self, mode: ApprovalMode) {
        self.approval = mode;
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

/// Reads `notify_sound`: a missing key is `true`; a value that is not a
/// boolean falls back to `true` with a problem.
fn read_notify_sound(section: &Table, problems: &mut Vec<String>) -> bool {
    match section.get("notify_sound") {
        None => true,
        Some(Value::Boolean(on)) => *on,
        Some(_) => {
            problems.push("ai.notify_sound: expected a boolean".to_owned());
            true
        }
    }
}

/// Reads `[ai]` (the general task, the profile and the context flag and the notify settings) and `[ai.tasks.<id>]`
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
    config.notify_sound = read_notify_sound(section, problems);
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
    config.tools = read_tools(table, problems);
    config.approval = read_approval(table, problems);
    config.mcp_servers = read_mcp_servers(table, problems);
    (config.servers, config.managed) = read_servers(table, problems);
    config.memory = read_memory(table, problems, port_env);
    config
}

/// Reads `[ai.tools] enabled`: a missing table or key is `true`; a value of
/// the wrong type falls back to `true` with a problem. Unknown keys are
/// ignored. The menu row (`Tools`) edits the same key.
fn read_tools(table: &Table, problems: &mut Vec<String>) -> bool {
    let Some(ai) = table.get("ai").and_then(Value::as_table) else {
        return true;
    };
    let section = match ai.get("tools") {
        None => return true,
        Some(Value::Table(section)) => section,
        Some(_) => {
            problems.push("ai.tools: expected a table".to_owned());
            return true;
        }
    };
    match section.get("enabled") {
        None => true,
        Some(Value::Boolean(on)) => *on,
        Some(_) => {
            problems.push("ai.tools.enabled: expected a boolean".to_owned());
            true
        }
    }
}

/// Reads `[ai.tools] approval`. A missing table or key is `ask`; a value that
/// is not exactly `"ask"`, `"auto"` or `"run"` falls back to `ask` with a problem. A
/// table that is not a table was already reported by `read_tools`.
fn read_approval(table: &Table, problems: &mut Vec<String>) -> ApprovalMode {
    let section = table
        .get("ai")
        .and_then(Value::as_table)
        .and_then(|ai| ai.get("tools"))
        .and_then(Value::as_table);
    match section.and_then(|section| section.get("approval")) {
        None => ApprovalMode::Ask,
        Some(Value::String(id)) if ApprovalMode::from_id(id).is_some() => {
            ApprovalMode::from_id(id).unwrap_or_default()
        }
        Some(_) => {
            problems.push(APPROVAL_PROBLEM.to_owned());
            ApprovalMode::Ask
        }
    }
}

/// Reads `[[ai.mcp.servers]]`. An invalid entry is skipped whole and each of
/// its problems is reported; the others are kept. A wrong `enabled` falls
/// back to `true` on its own. A name is unique per scope (the global list, or
/// one project root), so a project entry may reuse a global name: it wins at
/// resolution.
fn read_mcp_servers(table: &Table, problems: &mut Vec<String>) -> Vec<McpServer> {
    let Some(ai) = table.get("ai").and_then(Value::as_table) else {
        return Vec::new();
    };
    let mcp = match ai.get("mcp") {
        None => return Vec::new(),
        Some(Value::Table(mcp)) => mcp,
        Some(_) => {
            problems.push("ai.mcp: expected a table".to_owned());
            return Vec::new();
        }
    };
    let entries = match mcp.get("servers") {
        None => return Vec::new(),
        Some(Value::Array(entries)) => entries,
        Some(_) => {
            problems.push("ai.mcp.servers: expected an array of tables".to_owned());
            return Vec::new();
        }
    };
    let mut servers: Vec<McpServer> = Vec::new();
    let mut shape_reported = false;
    for (index, entry) in entries.iter().enumerate() {
        let Value::Table(entry) = entry else {
            if !shape_reported {
                problems.push("ai.mcp.servers: expected an array of tables".to_owned());
                shape_reported = true;
            }
            continue;
        };
        let Some(server) = read_mcp_entry(entry, &format!("ai.mcp.servers[{index}]"), problems)
        else {
            continue;
        };
        if servers
            .iter()
            .any(|kept| kept.name == server.name && kept.project == server.project)
        {
            problems.push(format!("ai.mcp.servers[{index}].name: duplicate name"));
            continue;
        }
        servers.push(server);
    }
    servers
}

/// One entry, or `None` after reporting every field problem it has.
fn read_mcp_entry(entry: &Table, path: &str, problems: &mut Vec<String>) -> Option<McpServer> {
    let mut valid = true;
    let name = match entry.get("name") {
        Some(Value::String(name)) if valid_mcp_name(name) => name.clone(),
        _ => {
            valid = false;
            problems.push(format!(
                "{path}.name: expected a name of lowercase letters, digits and - (1 to 24 characters)"
            ));
            String::new()
        }
    };
    let command = match entry.get("command") {
        Some(Value::String(command)) if !command.trim().is_empty() => command.clone(),
        _ => {
            valid = false;
            problems.push(format!("{path}.command: expected a non-empty string"));
            String::new()
        }
    };
    let args = match string_list(entry.get("args"), |_| true) {
        Some(args) => args,
        None => {
            valid = false;
            problems.push(format!("{path}.args: expected an array of strings"));
            Vec::new()
        }
    };
    // A variable NAME only: `NAME=value` and anything else that is not an
    // identifier is refused, so a secret pasted here is never kept or echoed.
    let env = match string_list(entry.get("env"), valid_env_name) {
        Some(env) => env,
        None => {
            valid = false;
            problems.push(format!("{path}.env: expected an array of variable names"));
            Vec::new()
        }
    };
    let enabled = match entry.get("enabled") {
        None => true,
        Some(Value::Boolean(on)) => *on,
        Some(_) => {
            problems.push(format!("{path}.enabled: expected a boolean"));
            true
        }
    };
    // An unreadable project must skip the entry: reading it as global would
    // widen where the server runs.
    let project = match entry.get("project") {
        None => None,
        Some(Value::String(root)) if PathBuf::from(root).is_absolute() => Some(PathBuf::from(root)),
        Some(_) => {
            valid = false;
            problems.push(format!("{path}.project: expected an absolute path"));
            None
        }
    };
    valid.then_some(McpServer {
        name,
        command,
        args,
        env,
        project,
        enabled,
    })
}

fn valid_mcp_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= MCP_NAME_MAX
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn valid_env_name(name: &str) -> bool {
    if name.len() > MCP_ENV_NAME_MAX {
        return false;
    }
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// An optional array whose elements are all strings accepted by `keep`.
/// Absent is empty; any other shape is `None`.
fn string_list(value: Option<&Value>, keep: fn(&str) -> bool) -> Option<Vec<String>> {
    match value {
        None => Some(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| item.as_str().filter(|text| keep(text)).map(str::to_owned))
            .collect(),
        Some(_) => None,
    }
}

/// Reads `[ai.servers.<id>] port` for the server engines, and `managed` and
/// `model` for llamacpp. A wrong port falls back on its own, with a problem:
/// to the catalog port, or to [`MANAGED_PORT`] for a managed llama-server. A
/// wrong `managed` or `model` leaves it unmanaged. Unknown ids and keys are
/// ignored, like unknown names under `ai.tasks`. The menu writes only
/// `managed` and `model` of llamacpp, when a local model is chosen.
fn read_servers(table: &Table, problems: &mut Vec<String>) -> (ServerPorts, Option<ManagedServer>) {
    let mut ports = ServerPorts::default();
    let mut managed = None;
    let section = match table.get("ai").and_then(Value::as_table) {
        None => return (ports, managed),
        Some(ai) => match ai.get("servers") {
            None => return (ports, managed),
            Some(Value::Table(section)) => section,
            Some(_) => {
                problems.push("ai.servers: expected a table".to_owned());
                return (ports, managed);
            }
        },
    };
    for engine in CATALOG {
        if !matches!(engine.kind, Kind::Server { .. }) {
            continue;
        }
        let id = engine.id;
        let server = match section.get(id) {
            None => continue,
            Some(Value::Table(server)) => server,
            Some(_) => {
                problems.push(format!("ai.servers.{id}: expected a table"));
                continue;
            }
        };
        let port = match server.get("port") {
            None => None,
            Some(Value::Integer(n)) if (1..=65535).contains(n) => Some(*n as u16),
            Some(_) => {
                problems.push(format!(
                    "ai.servers.{id}.port: expected an integer from 1 to 65535"
                ));
                None
            }
        };
        // Only llamacpp's own port defaults to the managed one.
        let mut fallback = None;
        if id == "llamacpp" {
            managed = read_managed(server, problems);
            fallback = managed.as_ref().map(|_| MANAGED_PORT);
        }
        if let Some(port) = port.or(fallback) {
            ports = ports.with(id, port);
        }
    }
    (ports, managed)
}

/// `managed` and `model` of `[ai.servers.llamacpp]`: `Some` only with
/// `managed = true` and a model path.
fn read_managed(server: &Table, problems: &mut Vec<String>) -> Option<ManagedServer> {
    let on = match server.get("managed") {
        None => false,
        Some(Value::Boolean(on)) => *on,
        Some(_) => {
            problems.push("ai.servers.llamacpp.managed: expected true or false".to_owned());
            false
        }
    };
    let model = match server.get("model") {
        None => None,
        Some(Value::String(model)) => (!model.is_empty()).then(|| PathBuf::from(model)),
        Some(_) => {
            problems.push("ai.servers.llamacpp.model: expected a path to a model file".to_owned());
            return None;
        }
    };
    match (on, model) {
        (false, _) => None,
        (true, Some(model)) => Some(ManagedServer { model }),
        (true, None) => {
            problems.push("ai.servers.llamacpp.model: required when managed = true".to_owned());
            None
        }
    }
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

    fn ports_of(text: &str) -> (Vec<Option<u16>>, Vec<String>) {
        let (ai, problems) = read_text(text);
        let ports = ["ollama", "lmstudio", "llamacpp", "vllm", "claude", "nope"]
            .iter()
            .map(|id| ai.servers().port(id))
            .collect();
        (ports, problems)
    }

    // Spec: Server ports default to the catalog.
    #[test]
    fn server_ports_default_to_the_catalog_ports() {
        let want = vec![Some(11434), Some(1234), Some(8080), Some(8000), None, None];
        assert_eq!(ports_of(""), (want.clone(), vec![]));
        assert_eq!(ports_of("[ai]\n"), (want.clone(), vec![]));
        assert_eq!(ports_of("[ai.servers]\n"), (want, vec![]));
    }

    // Spec: Server ports are read.
    #[test]
    fn a_server_port_overrides_its_catalog_port_only() {
        let text = "[ai.servers.llamacpp]\nport = 39080\n\n[ai.servers.ollama]\nport = 11500\n";
        assert_eq!(
            ports_of(text),
            (
                vec![Some(11500), Some(1234), Some(39080), Some(8000), None, None],
                vec![]
            )
        );
    }

    // Spec: Unknown servers and keys are ignored.
    #[test]
    fn unknown_servers_and_keys_are_ignored() {
        let text = "[ai.servers.claude]\nport = 9\n\n[ai.servers.nope]\nport = 9\n\n[ai.servers.vllm]\nport = 8001\nhost = \"0.0.0.0\"\n";
        assert_eq!(
            ports_of(text),
            (
                vec![Some(11434), Some(1234), Some(8080), Some(8001), None, None],
                vec![]
            )
        );
    }

    // Spec: A wrong server value falls back alone.
    #[test]
    fn a_wrong_server_value_falls_back_alone_with_its_problem() {
        let defaults = vec![Some(11434), Some(1234), Some(8080), Some(8000), None, None];
        for (text, problem) in [
            ("[ai]\nservers = 1\n", "ai.servers: expected a table"),
            (
                "[ai.servers]\nllamacpp = 39080\n",
                "ai.servers.llamacpp: expected a table",
            ),
            (
                "[ai.servers.llamacpp]\nport = 0\n",
                "ai.servers.llamacpp.port: expected an integer from 1 to 65535",
            ),
            (
                "[ai.servers.llamacpp]\nport = 70000\n",
                "ai.servers.llamacpp.port: expected an integer from 1 to 65535",
            ),
            (
                "[ai.servers.llamacpp]\nport = \"39080\"\n",
                "ai.servers.llamacpp.port: expected an integer from 1 to 65535",
            ),
        ] {
            assert_eq!(
                ports_of(text),
                (defaults.clone(), vec![problem.to_owned()]),
                "{text}"
            );
        }
        let mixed = "[ai.servers.lmstudio]\nport = -1\n\n[ai.servers.vllm]\nport = 8001\n";
        assert_eq!(
            ports_of(mixed),
            (
                vec![Some(11434), Some(1234), Some(8080), Some(8001), None, None],
                vec!["ai.servers.lmstudio.port: expected an integer from 1 to 65535".to_owned()]
            )
        );
    }

    /// The managed llama-server, llamacpp's port and the problems of `text`.
    fn managed_of(text: &str) -> (Option<ManagedServer>, Option<u16>, Vec<String>) {
        let (ai, problems) = read_text(text);
        (
            ai.managed().cloned(),
            ai.servers().port("llamacpp"),
            problems,
        )
    }

    fn managed(model: &str) -> Option<ManagedServer> {
        Some(ManagedServer {
            model: PathBuf::from(model),
        })
    }

    // Spec: A managed llama-server is opt-in.
    #[test]
    fn no_llama_server_is_managed_unless_it_is_turned_on() {
        for text in [
            "",
            "[ai.servers.llamacpp]\nport = 39080\n",
            "[ai.servers.llamacpp]\nmodel = \"/m/q.gguf\"\n",
            "[ai.servers.llamacpp]\nmanaged = false\nmodel = \"/m/q.gguf\"\n",
        ] {
            let port = if text.contains("39080") { 39080 } else { 8080 };
            assert_eq!(managed_of(text), (None, Some(port), vec![]), "{text}");
        }
    }

    // Spec: A managed llama-server has its own port.
    #[test]
    fn a_managed_llama_server_reads_its_model_and_takes_the_managed_port() {
        let text = "[ai.servers.llamacpp]\nmanaged = true\nmodel = \"/m/q.gguf\"\n";
        assert_eq!(
            managed_of(text),
            (managed("/m/q.gguf"), Some(MANAGED_PORT), vec![])
        );
        assert_eq!(MANAGED_PORT, 39281);
        let with_port = format!("{text}port = 39080\n");
        assert_eq!(
            managed_of(&with_port),
            (managed("/m/q.gguf"), Some(39080), vec![])
        );
    }

    // Spec S3: choosing a local model turns the managed server on, on the
    // managed port unless the config names one.
    #[test]
    fn managing_a_model_takes_the_managed_port_like_the_config_does() {
        let mut config = read_text("").0;
        config.manage(PathBuf::from("/m/q.gguf"));
        assert_eq!(config.managed(), managed("/m/q.gguf").as_ref());
        assert_eq!(config.servers().port("llamacpp"), Some(MANAGED_PORT));
        let mut config = read_text("[ai.servers.llamacpp]\nport = 39080\n").0;
        config.manage(PathBuf::from("/m/q.gguf"));
        assert_eq!(config.servers().port("llamacpp"), Some(39080));
        // Another model of a server already managed keeps its port.
        let mut config = read_text(
            "[ai.servers.llamacpp]\nmanaged = true\nmodel = \"/m/a.gguf\"\nport = 39090\n",
        )
        .0;
        config.manage(PathBuf::from("/m/b.gguf"));
        assert_eq!(config.managed(), managed("/m/b.gguf").as_ref());
        assert_eq!(config.servers().port("llamacpp"), Some(39090));
        // The other servers never move.
        assert_eq!(config.servers().port("vllm"), Some(8000));
    }

    // Spec: A managed llama-server is opt-in (other servers).
    #[test]
    fn managed_under_another_server_is_ignored() {
        let text = "[ai.servers.vllm]\nmanaged = true\nmodel = \"m\"\n";
        assert_eq!(managed_of(text), (None, Some(8080), vec![]));
        assert_eq!(ports_of(text).0[3], Some(8000));
    }

    // Spec: A managed llama-server has its own port (only llamacpp's).
    #[test]
    fn a_managed_llama_server_never_moves_another_server() {
        let managed = "[ai.servers.llamacpp]\nmanaged = true\nmodel = \"/m/q.gguf\"\n\n";
        for (vllm, problems) in [
            ("[ai.servers.vllm]\n# port = 8001\n", vec![]),
            ("[ai.servers.vllm]\nmodel = \"x\"\n", vec![]),
            (
                "[ai.servers.vllm]\nport = \"8001\"\n",
                vec!["ai.servers.vllm.port: expected an integer from 1 to 65535".to_owned()],
            ),
        ] {
            let text = format!("{managed}{vllm}");
            let (ports, got) = ports_of(&text);
            assert_eq!(
                (ports[2], ports[3], got),
                (Some(MANAGED_PORT), Some(8000), problems),
                "{text}"
            );
        }
    }

    // Spec: A wrong managed value turns it off with its problem.
    #[test]
    fn a_wrong_managed_value_turns_it_off_with_its_problem() {
        for (text, problem) in [
            (
                "[ai.servers.llamacpp]\nmanaged = \"yes\"\nmodel = \"/m/q.gguf\"\n",
                "ai.servers.llamacpp.managed: expected true or false",
            ),
            (
                "[ai.servers.llamacpp]\nmanaged = true\n",
                "ai.servers.llamacpp.model: required when managed = true",
            ),
            (
                "[ai.servers.llamacpp]\nmanaged = true\nmodel = \"\"\n",
                "ai.servers.llamacpp.model: required when managed = true",
            ),
            (
                "[ai.servers.llamacpp]\nmanaged = true\nmodel = 1\n",
                "ai.servers.llamacpp.model: expected a path to a model file",
            ),
        ] {
            assert_eq!(
                managed_of(text),
                (None, Some(8080), vec![problem.to_owned()]),
                "{text}"
            );
        }
        // A wrong model is a problem even while managed is off.
        assert_eq!(
            managed_of("[ai.servers.llamacpp]\nmodel = 1\n"),
            (
                None,
                Some(8080),
                vec!["ai.servers.llamacpp.model: expected a path to a model file".to_owned()]
            )
        );
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
    fn notify_sound_defaults_to_true_and_reads_a_boolean() {
        assert!(AiConfig::default().notify_sound());
        for (text, want) in [
            ("", true),
            ("[ai]\nnotify_sound = false\n", false),
            ("[ai]\nnotify_sound = true\n", true),
        ] {
            let (config, problems) = read_text(text);
            assert_eq!(config.notify_sound(), want, "{text:?}");
            assert_eq!(problems, Vec::<String>::new());
        }
    }

    #[test]
    fn a_non_boolean_notify_sound_falls_back_to_true_with_the_exact_problem() {
        for value in ["\"no\"", "0", "[false]"] {
            let (config, problems) = read_text(&format!("[ai]\nnotify_sound = {value}\n"));
            assert!(config.notify_sound(), "{value}");
            assert_eq!(
                problems,
                vec!["ai.notify_sound: expected a boolean".to_owned()],
                "{value}"
            );
        }
    }

    // Spec (configuration): `[ai.tools] enabled`, default and parse.
    #[test]
    fn tools_default_to_enabled() {
        assert!(AiConfig::default().tools_enabled());
        for text in ["", "[ai]\n", "[ai.tools]\n", "[ai.tools]\nother = 1\n"] {
            let (config, problems) = read_text(text);
            assert!(config.tools_enabled(), "{text:?}");
            assert!(problems.is_empty(), "{text:?}");
        }
    }

    #[test]
    fn a_boolean_tools_flag_is_read_without_a_problem() {
        for (value, want) in [("true", true), ("false", false)] {
            let (config, problems) = read_text(&format!("[ai.tools]\nenabled = {value}\n"));
            assert_eq!(config.tools_enabled(), want, "{value}");
            assert!(problems.is_empty(), "{value}");
        }
    }

    #[test]
    fn a_bad_tools_value_falls_back_to_enabled_with_the_exact_problem() {
        for value in ["1", "\"no\"", "[]"] {
            let (config, problems) = read_text(&format!("[ai.tools]\nenabled = {value}\n"));
            assert!(config.tools_enabled(), "{value}");
            assert_eq!(
                problems,
                vec!["ai.tools.enabled: expected a boolean".to_owned()],
                "{value}"
            );
        }
        let (config, problems) = read_text("[ai]\ntools = 3\n");
        assert!(config.tools_enabled());
        assert_eq!(problems, vec!["ai.tools: expected a table".to_owned()]);
    }

    #[test]
    fn the_tools_problem_follows_the_task_ones() {
        let (_, problems) = read_text("[ai]\nnotify = 1\n[ai.tools]\nenabled = 1\n");
        assert_eq!(
            problems,
            vec![
                NOTIFY_PROBLEM.to_owned(),
                "ai.tools.enabled: expected a boolean".to_owned(),
            ]
        );
    }

    // Spec (configuration): `[ai.tools] approval`, default and parse (D18).
    #[test]
    fn approval_defaults_to_ask() {
        assert_eq!(AiConfig::default().approval(), ApprovalMode::Ask);
        for text in [
            "",
            "[ai]\n",
            "[ai.tools]\n",
            "[ai.tools]\nenabled = false\n",
        ] {
            let (config, problems) = read_text(text);
            assert_eq!(config.approval(), ApprovalMode::Ask, "{text:?}");
            assert!(problems.is_empty(), "{text:?}");
        }
    }

    #[test]
    fn approval_reads_its_three_ids() {
        for (id, want) in [
            ("ask", ApprovalMode::Ask),
            ("auto", ApprovalMode::Auto),
            ("run", ApprovalMode::Run),
        ] {
            let (config, problems) = read_text(&format!("[ai.tools]\napproval = \"{id}\"\n"));
            assert_eq!(config.approval(), want, "{id}");
            assert!(problems.is_empty(), "{id}");
        }
    }

    #[test]
    fn a_bad_approval_falls_back_to_ask_with_the_exact_problem() {
        for value in [
            "\"AUTO\"",
            "\"RUN\"",
            "\"Run\"",
            "\"yes\"",
            "true",
            "1",
            "\" auto\"",
        ] {
            let (config, problems) = read_text(&format!("[ai.tools]\napproval = {value}\n"));
            assert_eq!(config.approval(), ApprovalMode::Ask, "{value}");
            assert_eq!(problems, vec![APPROVAL_PROBLEM.to_owned()], "{value}");
        }
    }

    #[test]
    fn the_approval_problem_follows_the_enabled_one() {
        let (_, problems) = read_text("[ai.tools]\napproval = 1\nenabled = 1\n");
        assert_eq!(
            problems,
            vec![
                "ai.tools.enabled: expected a boolean".to_owned(),
                APPROVAL_PROBLEM.to_owned(),
            ]
        );
    }

    #[test]
    fn a_non_table_tools_reports_only_the_table_problem() {
        let (config, problems) = read_text("[ai]\ntools = \"auto\"\n");
        assert_eq!(config.approval(), ApprovalMode::Ask);
        assert_eq!(problems, vec!["ai.tools: expected a table".to_owned()]);
    }

    #[test]
    fn the_approval_problem_names_all_three_values() {
        assert_eq!(
            APPROVAL_PROBLEM,
            "ai.tools.approval: expected \"ask\", \"auto\" or \"run\""
        );
    }

    #[test]
    fn the_approval_mode_names_itself_and_round_trips() {
        assert_eq!(
            ApprovalMode::ALL,
            [ApprovalMode::Ask, ApprovalMode::Auto, ApprovalMode::Run]
        );
        for mode in ApprovalMode::ALL {
            assert_eq!(ApprovalMode::from_id(mode.id()), Some(mode));
        }
        assert_eq!(ApprovalMode::from_id("Auto"), None);
        assert_eq!(ApprovalMode::from_id("RUN"), None);
        assert_eq!(ApprovalMode::from_id(" run"), None);
    }

    #[test]
    fn the_approval_mode_cycles_ask_auto_run_both_ways() {
        let mut mode = ApprovalMode::Ask;
        for want in [ApprovalMode::Auto, ApprovalMode::Run, ApprovalMode::Ask] {
            mode = mode.cycled(true);
            assert_eq!(mode, want);
        }
        for want in [ApprovalMode::Run, ApprovalMode::Auto, ApprovalMode::Ask] {
            mode = mode.cycled(false);
            assert_eq!(mode, want);
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

    // Spec (configuration, MCP server lists): the `[[ai.mcp.servers]]` parse.
    fn mcp_of(text: &str) -> (Vec<McpServer>, Vec<String>) {
        let (config, problems) = read_text(text);
        (config.mcp_servers().to_vec(), problems)
    }

    fn server(name: &str, command: &str) -> McpServer {
        McpServer {
            name: name.to_owned(),
            command: command.to_owned(),
            args: Vec::new(),
            env: Vec::new(),
            project: None,
            enabled: true,
        }
    }

    #[test]
    fn no_mcp_table_gives_no_servers_and_no_notice() {
        assert_eq!(mcp_of(""), (Vec::new(), Vec::new()));
        assert_eq!(mcp_of("[ai]\nnotify = true\n").0, Vec::new());
    }

    #[test]
    fn an_entry_is_read_with_its_args_env_and_project() {
        let text = "[[ai.mcp.servers]]\nname = \"fs\"\ncommand = \"mcp-fs\"\n\
            args = [\"--root\", \"/home/u/dev\"]\nenv = [\"GITHUB_TOKEN\"]\n\n\
            [[ai.mcp.servers]]\nname = \"docs\"\ncommand = \"mcp-docs\"\n\
            project = \"/home/u/p\"\nenabled = false\ntimeout = 5\n";
        let (servers, problems) = mcp_of(text);
        assert_eq!(problems, Vec::<String>::new());
        let mut fs = server("fs", "mcp-fs");
        fs.args = vec!["--root".into(), "/home/u/dev".into()];
        fs.env = vec!["GITHUB_TOKEN".into()];
        let mut docs = server("docs", "mcp-docs");
        docs.project = Some("/home/u/p".into());
        docs.enabled = false;
        assert_eq!(servers, vec![fs, docs]);
    }

    #[test]
    fn an_invalid_entry_is_skipped_whole_and_the_others_are_kept() {
        let text = "[[ai.mcp.servers]]\nname = \"a\"\ncommand = 5\n\n\
            [[ai.mcp.servers]]\nname = \"b\"\ncommand = \"mcp-b\"\n";
        let (servers, problems) = mcp_of(text);
        assert_eq!(servers, vec![server("b", "mcp-b")]);
        assert_eq!(
            problems,
            vec!["ai.mcp.servers[0].command: expected a non-empty string".to_owned()]
        );
    }

    #[test]
    fn each_field_has_its_own_exact_problem() {
        let cases = [
            (
                "name = \"a b\"\ncommand = \"x\"",
                "name: expected a name of lowercase letters, digits and - (1 to 24 characters)",
            ),
            (
                "name = \"\"\ncommand = \"x\"",
                "name: expected a name of lowercase letters, digits and - (1 to 24 characters)",
            ),
            (
                "command = \"x\"",
                "name: expected a name of lowercase letters, digits and - (1 to 24 characters)",
            ),
            (
                "name = \"Fs\"\ncommand = \"x\"",
                "name: expected a name of lowercase letters, digits and - (1 to 24 characters)",
            ),
            (
                "name = \"a_b\"\ncommand = \"x\"",
                "name: expected a name of lowercase letters, digits and - (1 to 24 characters)",
            ),
            (
                "name = \"aaaaaaaaaaaaaaaaaaaaaaaaa\"\ncommand = \"x\"",
                "name: expected a name of lowercase letters, digits and - (1 to 24 characters)",
            ),
            (
                "name = \"a\"\ncommand = \"  \"",
                "command: expected a non-empty string",
            ),
            (
                "name = \"a\"\ncommand = \"x\"\nargs = \"--x\"",
                "args: expected an array of strings",
            ),
            (
                "name = \"a\"\ncommand = \"x\"\nargs = [1]",
                "args: expected an array of strings",
            ),
            (
                "name = \"a\"\ncommand = \"x\"\nenv = [1]",
                "env: expected an array of variable names",
            ),
            (
                "name = \"a\"\ncommand = \"x\"\nproject = \"rel/p\"",
                "project: expected an absolute path",
            ),
            (
                "name = \"a\"\ncommand = \"x\"\nproject = 3",
                "project: expected an absolute path",
            ),
        ];
        for (body, want) in cases {
            let (servers, problems) = mcp_of(&format!("[[ai.mcp.servers]]\n{body}\n"));
            assert_eq!(servers, Vec::new(), "{body}");
            assert_eq!(
                problems,
                vec![format!("ai.mcp.servers[0].{want}")],
                "{body}"
            );
        }
    }

    #[test]
    fn a_secret_value_in_env_is_refused_and_never_echoed() {
        for env in [
            "[\"\u{ff27}\u{ff29}\u{ff34}\"]",
            "[\"\u{410}BC\"]",
            "[\"A\u{200b}B\"]",
            "[\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\"]",
            "{ GITHUB_TOKEN = \"abc\" }",
            "[\"GITHUB_TOKEN=abc\"]",
            "[\"a b\"]",
            "[\"\"]",
        ] {
            let text = format!("[[ai.mcp.servers]]\nname = \"a\"\ncommand = \"x\"\nenv = {env}\n");
            let (servers, problems) = mcp_of(&text);
            assert_eq!(servers, Vec::new(), "{env}");
            assert_eq!(
                problems,
                vec!["ai.mcp.servers[0].env: expected an array of variable names".to_owned()],
                "{env}"
            );
            assert!(!problems.concat().contains("abc"), "{env}");
        }
    }

    #[test]
    fn a_wrong_enabled_falls_back_alone_and_keeps_the_entry() {
        let text = "[[ai.mcp.servers]]\nname = \"a\"\ncommand = \"x\"\nenabled = \"no\"\n";
        let (servers, problems) = mcp_of(text);
        assert_eq!(servers, vec![server("a", "x")]);
        assert_eq!(
            problems,
            vec!["ai.mcp.servers[0].enabled: expected a boolean".to_owned()]
        );
    }

    #[test]
    fn a_duplicate_name_skips_the_later_entry_but_a_project_may_reuse_a_global_name() {
        let text = "[[ai.mcp.servers]]\nname = \"fs\"\ncommand = \"one\"\n\n\
            [[ai.mcp.servers]]\nname = \"fs\"\ncommand = \"two\"\n";
        let (servers, problems) = mcp_of(text);
        assert_eq!(servers, vec![server("fs", "one")]);
        assert_eq!(
            problems,
            vec!["ai.mcp.servers[1].name: duplicate name".to_owned()]
        );
        let text = format!(
            "{text}\n[[ai.mcp.servers]]\nname = \"fs\"\ncommand = \"three\"\nproject = \"/p\"\n"
        );
        let (servers, problems) = mcp_of(&text);
        assert_eq!(servers.len(), 2);
        assert_eq!(problems.len(), 1, "{problems:?}");
    }

    #[test]
    fn wrong_container_types_are_one_problem_each() {
        assert_eq!(
            mcp_of("[ai]\nmcp = 3\n"),
            (Vec::new(), vec!["ai.mcp: expected a table".to_owned()])
        );
        assert_eq!(
            mcp_of("[ai.mcp]\nservers = 3\n"),
            (
                Vec::new(),
                vec!["ai.mcp.servers: expected an array of tables".to_owned()]
            )
        );
        assert_eq!(
            mcp_of("[ai.mcp]\nservers = [1]\n"),
            (
                Vec::new(),
                vec!["ai.mcp.servers: expected an array of tables".to_owned()]
            )
        );
    }

    #[test]
    fn mcp_problems_follow_the_tools_problems_in_the_notice_order() {
        let text = "[ai.tools]\nenabled = 1\napproval = 2\n\n\
            [[ai.mcp.servers]]\nname = \"a\"\ncommand = 5\n";
        assert_eq!(
            read_text(text).1,
            vec![
                "ai.tools.enabled: expected a boolean".to_owned(),
                APPROVAL_PROBLEM.to_owned(),
                "ai.mcp.servers[0].command: expected a non-empty string".to_owned(),
            ]
        );
        let (_, _, notice) = crate::core::config::Config::load_with_ai(Ok(text.to_owned()), None);
        assert_eq!(
            notice.as_deref(),
            Some("config (3 problems): ai.tools.enabled: expected a boolean")
        );
    }

    #[test]
    fn a_name_of_24_characters_and_a_variable_of_64_bytes_are_the_limits() {
        let name = format!("{}-9", "a".repeat(22));
        let var = "V".repeat(64);
        let text =
            format!("[[ai.mcp.servers]]\nname = \"{name}\"\ncommand = \"x\"\nenv = [\"{var}\"]\n");
        let (servers, problems) = mcp_of(&text);
        assert_eq!(problems, Vec::<String>::new());
        assert_eq!(servers[0].name, name);
        assert_eq!(servers[0].env, vec![var]);
    }

    #[test]
    fn debug_never_prints_the_command_args_or_env() {
        let mut one = server("fs", "/secret/bin/mcp");
        one.args = vec!["--token=hunter2".into()];
        one.env = vec!["GITHUB_TOKEN".into(), "OTHER".into()];
        one.project = Some("/home/u/p".into());
        let text = format!("{one:?}");
        for hidden in ["secret", "hunter2", "GITHUB_TOKEN", "OTHER"] {
            assert!(!text.contains(hidden), "{text}");
        }
        for shown in ["fs", "/home/u/p", "enabled: true", "args: 1", "env: 2"] {
            assert!(text.contains(shown), "{text}");
        }
    }
}
