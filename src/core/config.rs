//! User configuration (`config.toml`). Pure: parsing and defaults only; the
//! file lookup and read live at the runtime edge.

use std::{
    borrow::Cow,
    ffi::OsString,
    fmt,
    io::{self, ErrorKind},
    path::PathBuf,
};

use toml::{Table, Value};

use super::ai::{
    self,
    config::{AiConfig, ApprovalMode, Task, ToolToggle},
};

/// Which edge of the screen a bar sits on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BarPosition {
    Top,
    #[default]
    Bottom,
}

/// Where the statusline and the tab bar go.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BarPositions {
    pub statusline: BarPosition,
    pub tabbar: BarPosition,
}

impl Default for BarPositions {
    fn default() -> Self {
        Self {
            statusline: BarPosition::Bottom,
            tabbar: BarPosition::Top,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Config {
    pub bars: BarPositions,
    /// Whether the host terminal reports the mouse wheel to the shell
    /// (default on; `mouse = false` leaves the terminal's own handling).
    pub mouse: bool,
    /// Whether a background check may ask crates.io for a newer version
    /// (default on; `[updates] check = false` makes no network call at all).
    pub update_check: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bars: BarPositions::default(),
            mouse: true,
            update_check: true,
        }
    }
}

/// One changed, menu-owned key to write back to `config.toml`:
/// `[table] key = "value"`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ConfigEdit {
    pub table: &'static str,
    pub key: &'static str,
    pub value: Cow<'static, str>,
}

impl ConfigEdit {
    /// An edit of an owned memory flag (`ai.memory.enabled` or
    /// `.acknowledged`).
    pub fn memory_flag(key: &'static str, on: bool) -> Self {
        Self {
            table: MEMORY_TABLE,
            key,
            value: Cow::Borrowed(if on { "true" } else { "false" }),
        }
    }

    /// The `ai.tools.enabled` edit: the only key the menu writes about the
    /// agent tools. `ai.mcp.*` is never written by the menu.
    pub fn tools_flag(on: bool) -> Self {
        Self {
            table: TOOLS_TABLE,
            key: "enabled",
            value: Cow::Borrowed(if on { "true" } else { "false" }),
        }
    }

    /// The edit of one coding-tools family switch (`ai.tools.<key>`): a
    /// boolean, written only when the menu changed it.
    pub fn toggle(toggle: ToolToggle, on: bool) -> Self {
        Self {
            table: TOOLS_TABLE,
            key: toggle.key(),
            value: Cow::Borrowed(if on { "true" } else { "false" }),
        }
    }

    /// The `ai.tools.approval` edit: a string, `ask`, `auto` or `run`, written even
    /// when it equals the default.
    pub fn approval(mode: ApprovalMode) -> Self {
        Self {
            table: TOOLS_TABLE,
            key: "approval",
            value: Cow::Borrowed(mode.id()),
        }
    }

    /// An edit of an owned orchestrator flag (`ai.orchestrator.enabled` or
    /// `.acknowledged`). `worker_timeout_secs` is owned but never edited.
    pub fn orchestrator_flag(key: &'static str, on: bool) -> Self {
        Self {
            table: ORCHESTRATOR_TABLE,
            key,
            value: Cow::Borrowed(if on { "true" } else { "false" }),
        }
    }

    /// Whether the edit deletes its key instead of writing it: a cleared
    /// `orchestrate` slot is absent from the file, not a pair of empty
    /// strings.
    pub fn is_removal(&self) -> bool {
        self.table == Task::Orchestrate.table() && self.value.is_empty()
    }

    /// Whether the key is a TOML integer (written as a number, not as a
    /// string): `ai.servers.llamacpp.idle_minutes`.
    pub fn is_integer(&self) -> bool {
        self.table == LLAMACPP_TABLE && self.key == "idle_minutes"
    }

    /// Whether the key is a TOML boolean (written as `true` / `false`, not
    /// as a string): the root `mouse`, the memory flags, `ai.context`,
    /// `ai.notify_sound`, `ai.tools.enabled`, the five family switches,
    /// `ai.servers.llamacpp.managed` and the orchestrator flags. Every other
    /// owned key holds a string; `ai.memory.port` is owned but never edited.
    pub fn is_flag(&self) -> bool {
        (self.table.is_empty() && self.key == "mouse")
            || (self.table == MEMORY_TABLE && matches!(self.key, "enabled" | "acknowledged"))
            || (self.table == "ai" && matches!(self.key, "context" | "notify_sound"))
            || (self.table == TOOLS_TABLE
                && (self.key == "enabled" || ToolToggle::ALL.iter().any(|t| t.key() == self.key)))
            || (self.table == LLAMACPP_TABLE && self.key == "managed")
            || (self.table == ORCHESTRATOR_TABLE && matches!(self.key, "enabled" | "acknowledged"))
    }
}

/// The dotted table of the llama-server lazarobox manages.
pub const LLAMACPP_TABLE: &str = "ai.servers.llamacpp";

/// The dotted table of the memory keys.
pub const MEMORY_TABLE: &str = "ai.memory";

/// The dotted table of the orchestrator keys.
pub const ORCHESTRATOR_TABLE: &str = "ai.orchestrator";

/// The dotted table of the agent tools keys.
pub const TOOLS_TABLE: &str = "ai.tools";

/// Why a config file was rejected; the text is one short line.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ConfigError(String);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A readable file: the config, plus one line per key that was invalid and
/// fell back to its default.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Parsed {
    pub config: Config,
    pub ai: AiConfig,
    pub problems: Vec<String>,
}

impl Config {
    /// Parses `config.toml`. Missing keys take their defaults and unknown
    /// keys are ignored, so older builds can read newer files.
    ///
    /// The result does not depend on the environment: the memory port default
    /// is 7437. See [`Config::parse_with_port_env`].
    pub fn parse(text: &str) -> Result<Parsed, ConfigError> {
        Self::parse_with_port_env(text, None)
    }

    /// Like [`Config::parse`], with the value of `ENGRAM_PORT` given: a valid
    /// one is the default memory port. Callers read the environment once.
    pub fn parse_with_port_env(text: &str, port_env: Option<&str>) -> Result<Parsed, ConfigError> {
        let table: Table = text.parse().map_err(|error: toml::de::Error| {
            let message = error.message().lines().next().unwrap_or_default().trim();
            ConfigError(
                if message.is_empty() {
                    "invalid TOML"
                } else {
                    message
                }
                .to_owned(),
            )
        })?;
        let defaults = Config::default();
        let mut problems = Vec::new();
        let mut position = |section: &str, default: BarPosition| {
            read_position(&table, section, &mut problems).unwrap_or(default)
        };
        let statusline = position("statusline", defaults.bars.statusline);
        let tabbar = position("tabbar", defaults.bars.tabbar);
        let mouse = match table.get("mouse") {
            None => defaults.mouse,
            Some(Value::Boolean(on)) => *on,
            Some(_) => {
                problems.push("mouse: expected true or false".to_owned());
                defaults.mouse
            }
        };
        let update_check =
            read_update_check(&table, &mut problems).unwrap_or(defaults.update_check);
        let ai = ai::config::read_with_port_env(&table, &mut problems, port_env);
        Ok(Parsed {
            config: Self {
                bars: BarPositions { statusline, tabbar },
                mouse,
                update_check,
            },
            ai,
            problems,
        })
    }

    /// The config for the result of reading the file, plus the startup
    /// notice when something needs the user's attention.
    pub fn load(read: io::Result<String>) -> (Self, Option<String>) {
        let (config, _, notice) = Self::load_with_ai(read, None);
        (config, notice)
    }

    /// Like [`Config::load`], and the saved AI choice too. `port_env` is the
    /// value of `ENGRAM_PORT`, read once by the caller at startup.
    pub fn load_with_ai(
        read: io::Result<String>,
        port_env: Option<&str>,
    ) -> (Self, AiConfig, Option<String>) {
        let text = match read {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return (Self::default(), AiConfig::default(), None);
            }
            Err(error) => {
                return (
                    Self::default(),
                    AiConfig::default(),
                    Some(format!("config: {error}")),
                );
            }
        };
        match Self::parse_with_port_env(&text, port_env) {
            Ok(Parsed {
                config,
                ai,
                problems,
            }) if !problems.is_empty() => {
                let head = match problems.len() {
                    1 => "config".to_owned(),
                    n => format!("config ({n} problems)"),
                };
                (config, ai, Some(format!("{head}: {}", problems[0])))
            }
            Ok(Parsed { config, ai, .. }) if ai.memory.enabled && !ai.memory.acknowledged => (
                config,
                ai,
                Some(ai::config::UNACKNOWLEDGED_NOTICE.to_owned()),
            ),
            Ok(Parsed { config, ai, .. }) => (config, ai, None),
            Err(error) => (
                Self::default(),
                AiConfig::default(),
                Some(format!("config: {error}")),
            ),
        }
    }
}

/// `updates.check`, or `None` (with a problem noted) when it is missing or
/// invalid, so the caller falls back to the default.
fn read_update_check(table: &Table, problems: &mut Vec<String>) -> Option<bool> {
    let section = match table.get("updates")? {
        Value::Table(section) => section,
        _ => {
            problems.push("updates: expected a table".to_owned());
            return None;
        }
    };
    match section.get("check")? {
        Value::Boolean(on) => Some(*on),
        _ => {
            problems.push("updates.check: expected true or false".to_owned());
            None
        }
    }
}

/// `section.position`, or `None` (with a problem noted) when it is missing
/// or invalid, so the caller falls back to the default for that key only.
fn read_position(table: &Table, section: &str, problems: &mut Vec<String>) -> Option<BarPosition> {
    let section_table = match table.get(section) {
        None => return None,
        Some(Value::Table(section_table)) => section_table,
        Some(_) => {
            problems.push(format!("{section}: expected a table"));
            return None;
        }
    };
    match section_table.get("position")?.as_str() {
        Some("top") => Some(BarPosition::Top),
        Some("bottom") => Some(BarPosition::Bottom),
        _ => {
            problems.push(format!(
                "{section}.position: expected \"top\" or \"bottom\""
            ));
            None
        }
    }
}

/// `$XDG_CONFIG_HOME/lazarobox/config.toml`, else `$HOME/.config/...`. Per
/// the XDG spec, an empty or relative `XDG_CONFIG_HOME` or `HOME` is ignored.
pub fn config_path(xdg: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let base = xdg
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            let home = PathBuf::from(home?);
            home.is_absolute().then(|| home.join(".config"))
        })?;
    Some(base.join("lazarobox").join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use BarPosition::{Bottom, Top};

    fn bars(statusline: BarPosition, tabbar: BarPosition) -> BarPositions {
        BarPositions { statusline, tabbar }
    }

    fn parse(text: &str) -> Config {
        Config::parse(text).unwrap().config
    }

    fn problems(text: &str) -> Vec<String> {
        Config::parse(text).unwrap().problems
    }

    // Spec: Defaults.
    #[test]
    fn defaults_are_statusline_bottom_tabbar_top_mouse_on() {
        let expected = Config {
            bars: bars(Bottom, Top),
            mouse: true,
            update_check: true,
        };
        assert_eq!(Config::default(), expected);
        assert_eq!(parse(""), expected);
        assert_eq!(parse("[statusline]\n[tabbar]\n"), expected);
    }

    // Spec: Each position value.
    #[test]
    fn each_position_is_read() {
        assert_eq!(
            parse("[statusline]\nposition = \"top\"").bars.statusline,
            Top
        );
        assert_eq!(
            parse("[statusline]\nposition = \"bottom\"").bars.statusline,
            Bottom
        );
        assert_eq!(parse("[tabbar]\nposition = \"top\"").bars.tabbar, Top);
        assert_eq!(parse("[tabbar]\nposition = \"bottom\"").bars.tabbar, Bottom);
    }

    #[test]
    fn a_partial_file_keeps_the_other_defaults() {
        let c = parse("[tabbar]\nposition = \"bottom\"");
        assert_eq!(c.bars, bars(Bottom, Bottom));
    }

    #[test]
    fn mouse_is_read_at_the_top_level() {
        assert!(parse("mouse = true").mouse);
        assert!(!parse("mouse = false").mouse);
    }

    // Spec: Invalid values fall back per key.
    #[test]
    fn only_unparseable_toml_is_an_error() {
        for text in ["[statusline]\nposition = ", "not toml at all", "[tabbar"] {
            let e = Config::parse(text).unwrap_err().to_string();
            assert!(!e.is_empty() && !e.contains('\n'), "{text:?}: {e:?}");
        }
    }

    #[test]
    fn an_invalid_value_falls_back_for_that_key_only() {
        let text =
            "mouse = true\n[statusline]\nposition = \"middle\"\n[tabbar]\nposition = \"bottom\"";
        let parsed = Config::parse(text).unwrap();
        // The bad key is the default; every other key survives.
        assert_eq!(parsed.config.bars, bars(Bottom, Bottom));
        assert!(parsed.config.mouse);
        assert_eq!(
            parsed.problems,
            [r#"statusline.position: expected "top" or "bottom""#]
        );
    }

    #[test]
    fn each_invalid_key_and_type_is_reported_with_its_key() {
        for (text, expected) in [
            (
                "[statusline]\nposition = \"middle\"",
                r#"statusline.position: expected "top" or "bottom""#,
            ),
            (
                "[tabbar]\nposition = \"TOP\"",
                r#"tabbar.position: expected "top" or "bottom""#,
            ),
            (
                "[tabbar]\nposition = 3",
                r#"tabbar.position: expected "top" or "bottom""#,
            ),
            ("mouse = \"yes\"", "mouse: expected true or false"),
            ("statusline = 3", "statusline: expected a table"),
        ] {
            assert_eq!(problems(text), [expected], "{text:?}");
            assert_eq!(parse(text), Config::default(), "{text:?}");
        }
    }

    #[test]
    fn a_bad_mouse_keeps_the_valid_positions() {
        let c = parse("mouse = 1\n[statusline]\nposition = \"top\"");
        assert_eq!(c.bars.statusline, Top);
        // A bad value falls back to the default, which is on.
        assert!(c.mouse);
    }

    #[test]
    fn valid_files_have_no_problems() {
        assert!(problems("[statusline]\nposition = \"top\"\nx = 1").is_empty());
    }

    // Spec: Unknown keys ignored.
    #[test]
    fn unknown_keys_and_tables_are_ignored() {
        let c = parse("theme = \"x\"\n[statusline]\nposition = \"top\"\ncolor = 1\n[other]\na = 1");
        assert_eq!(c.bars.statusline, Top);
    }

    // Spec: Missing file, unreadable file, invalid file, mouse notice.
    #[test]
    fn a_missing_file_is_silent_defaults() {
        let err = io::Error::from(ErrorKind::NotFound);
        assert_eq!(Config::load(Err(err)), (Config::default(), None));
    }

    #[test]
    fn an_unreadable_file_gives_defaults_and_a_notice() {
        let err = io::Error::from(ErrorKind::PermissionDenied);
        let (config, notice) = Config::load(Err(err));
        assert_eq!(config, Config::default());
        let notice = notice.unwrap();
        assert!(notice.starts_with("config: "), "{notice}");
        assert!(!notice.contains('\n'));
    }

    #[test]
    fn unparseable_toml_gives_all_defaults_and_a_notice() {
        let (config, notice) = Config::load(Ok("mouse = false\n[tabbar\n".into()));
        assert_eq!(config, Config::default());
        let notice = notice.unwrap();
        assert!(notice.starts_with("config: "), "{notice}");
        assert!(!notice.contains('\n'), "{notice}");
    }

    #[test]
    fn an_invalid_key_keeps_the_rest_and_the_notice_names_it() {
        let text = "[statusline]\nposition = \"top\"\n[tabbar]\nposition = \"left\"";
        let (config, notice) = Config::load(Ok(text.into()));
        assert_eq!(config.bars, bars(Top, Top));
        assert_eq!(
            notice.as_deref(),
            Some(r#"config: tabbar.position: expected "top" or "bottom""#)
        );
    }

    #[test]
    fn several_invalid_keys_report_the_count_then_the_first() {
        let text = "mouse = 3\n[statusline]\nposition = \"x\"\n[tabbar]\nposition = \"y\"";
        let (config, notice) = Config::load(Ok(text.into()));
        assert_eq!(config, Config::default());
        assert_eq!(
            notice.as_deref(),
            Some(r#"config (3 problems): statusline.position: expected "top" or "bottom""#)
        );
    }

    #[test]
    fn a_problem_notice_is_the_only_notice_with_mouse_on() {
        let (config, notice) = Config::load(Ok("mouse = true\n[tabbar]\nposition = 1".into()));
        assert!(config.mouse);
        assert!(notice.unwrap().contains("tabbar.position"));
    }

    #[test]
    fn a_valid_file_loads_without_a_notice() {
        let (config, notice) = Config::load(Ok("[statusline]\nposition = \"top\"".into()));
        assert_eq!(config.bars.statusline, Top);
        assert_eq!(notice, None);
    }

    // Spec: mouse is supported, so neither value gives a notice.
    #[test]
    fn mouse_true_is_accepted_without_a_notice() {
        let text = "mouse = true\n[tabbar]\nposition = \"bottom\"";
        let (config, notice) = Config::load(Ok(text.into()));
        assert!(config.mouse);
        assert_eq!(config.bars.tabbar, Bottom);
        assert_eq!(notice, None);
    }

    #[test]
    fn mouse_false_is_accepted_without_a_notice() {
        let (config, notice) = Config::load(Ok("mouse = false\n".into()));
        assert!(!config.mouse);
        assert_eq!(notice, None);
    }

    #[test]
    fn an_empty_file_turns_the_mouse_on_without_a_notice() {
        let (config, notice) = Config::load(Ok(String::new()));
        assert!(config.mouse);
        assert_eq!(notice, None);
    }

    #[test]
    fn a_bad_mouse_falls_back_to_on_and_names_the_key() {
        let (config, notice) = Config::load(Ok("mouse = \"yes\"\n".into()));
        assert!(config.mouse);
        assert_eq!(
            notice.as_deref(),
            Some("config: mouse: expected true or false")
        );
    }

    #[test]
    fn the_update_check_is_on_by_default() {
        assert!(parse("").update_check);
        assert!(parse("[updates]\n").update_check);
    }

    #[test]
    fn the_update_check_can_be_turned_off() {
        assert!(!parse("[updates]\ncheck = false").update_check);
        assert!(parse("[updates]\ncheck = true").update_check);
    }

    #[test]
    fn a_bad_update_check_falls_back_to_on_with_the_usual_notice() {
        let (config, notice) = Config::load(Ok("[updates]\ncheck = \"no\"\n".into()));
        assert!(config.update_check);
        assert_eq!(
            notice.as_deref(),
            Some("config: updates.check: expected true or false")
        );
        let (config, notice) = Config::load(Ok("updates = 1\n".into()));
        assert!(config.update_check);
        assert_eq!(notice.as_deref(), Some("config: updates: expected a table"));
    }

    #[test]
    fn a_bad_update_check_keeps_the_other_keys() {
        let c = parse("mouse = false\n[updates]\ncheck = 3");
        assert!(!c.mouse);
    }

    fn ai(engine: Option<&str>, model: Option<&str>) -> AiConfig {
        AiConfig::general(engine, model)
    }

    // Spec: Defaults (AI unset).
    #[test]
    fn an_empty_file_has_unset_ai() {
        assert_eq!(Config::parse("").unwrap().ai, AiConfig::default());
        let (config, loaded, notice) = Config::load_with_ai(Ok(String::new()), None);
        assert_eq!(
            (config, loaded, notice),
            (Config::default(), ai(None, None), None)
        );
    }

    // Spec: AI keys parsed.
    #[test]
    fn load_with_ai_returns_the_saved_choice_without_a_notice() {
        let text =
            "[ai]\nengine = \"ollama\"\nmodel = \"llama3:8b\"\n[tabbar]\nposition = \"bottom\"";
        let (config, loaded, notice) = Config::load_with_ai(Ok(text.into()), None);
        assert_eq!(config.bars.tabbar, Bottom);
        assert_eq!(loaded, ai(Some("ollama"), Some("llama3:8b")));
        assert_eq!(notice, None);
    }

    // Spec: Invalid AI types fall back per key.
    #[test]
    fn a_bad_ai_key_keeps_the_rest_and_the_notice_names_it() {
        let text = "[ai]\nengine = 3\n[tabbar]\nposition = \"bottom\"";
        let (config, loaded, notice) = Config::load_with_ai(Ok(text.into()), None);
        assert_eq!(config.bars.tabbar, Bottom);
        assert_eq!(loaded, ai(None, None));
        assert_eq!(
            notice.as_deref(),
            Some("config: ai.engine: expected a string")
        );
    }

    // Spec: Non-table ai.
    #[test]
    fn a_non_table_ai_gives_one_notice() {
        let (_, loaded, notice) = Config::load_with_ai(Ok("ai = 3".into()), None);
        assert_eq!(loaded, ai(None, None));
        assert_eq!(notice.as_deref(), Some("config: ai: expected a table"));
    }

    #[test]
    fn a_valid_ai_table_survives_another_invalid_key() {
        let text = "mouse = 3\n[ai]\nengine = \"ollama\"\nmodel = \"llama3:8b\"";
        let (config, loaded, notice) = Config::load_with_ai(Ok(text.into()), None);
        assert_eq!(config.mouse, Config::default().mouse);
        assert_eq!(loaded, ai(Some("ollama"), Some("llama3:8b")));
        assert_eq!(
            notice.as_deref(),
            Some("config: mouse: expected true or false")
        );
    }

    #[test]
    fn task_problems_count_after_the_general_ones() {
        let text = "[ai.tasks.code]\nengine = 5\n[ai.tasks.chat]\nmodel = 6\n";
        let (_, loaded, notice) = Config::load_with_ai(Ok(text.into()), None);
        assert_eq!(loaded, ai(None, None));
        assert_eq!(
            notice.as_deref(),
            Some("config (2 problems): ai.tasks.code.engine: expected a string")
        );
    }

    // Spec: Non-table ai.tasks.
    #[test]
    fn a_non_table_tasks_key_gives_one_notice_and_keeps_general() {
        let (_, loaded, notice) =
            Config::load_with_ai(Ok("[ai]\nengine = \"claude\"\ntasks = 3".into()), None);
        assert_eq!(loaded, ai(Some("claude"), None));
        assert_eq!(
            notice.as_deref(),
            Some("config: ai.tasks: expected a table")
        );
    }

    #[test]
    fn ai_problems_count_with_the_others() {
        let text = "mouse = 3\n[ai]\nengine = 1\nmodel = 2";
        let (_, _, notice) = Config::load_with_ai(Ok(text.into()), None);
        assert_eq!(
            notice.as_deref(),
            Some("config (3 problems): mouse: expected true or false")
        );
    }

    // Spec (configuration): Unknown profile value; Non-string profile; the
    // other keys of the file are respected.
    #[test]
    fn a_bad_profile_gives_one_notice_and_keeps_the_rest() {
        use crate::core::ai::config::Profile;
        let text =
            "[ai]\nprofile = \"wizard\"\nengine = \"ollama\"\n[tabbar]\nposition = \"bottom\"";
        let (config, loaded, notice) = Config::load_with_ai(Ok(text.into()), None);
        assert_eq!(config.bars.tabbar, Bottom);
        assert_eq!(loaded.profile(), Profile::Programmer);
        assert_eq!(
            loaded
                .get(crate::core::ai::config::Task::General)
                .engine
                .as_deref(),
            Some("ollama")
        );
        assert_eq!(
            notice.as_deref(),
            Some("config: ai.profile: expected \"analyst\" or \"programmer\"")
        );
        let (_, _, notice) = Config::load_with_ai(Ok("[ai]\nprofile = 3".into()), None);
        assert_eq!(
            notice.as_deref(),
            Some("config: ai.profile: expected a string")
        );
    }

    // Spec (configuration): a valid profile never raises a notice.
    #[test]
    fn a_valid_profile_raises_no_notice() {
        use crate::core::ai::config::Profile;
        for (value, want) in [
            ("analyst", Profile::Analyst),
            ("programmer", Profile::Programmer),
            ("", Profile::Programmer),
        ] {
            let (_, loaded, notice) =
                Config::load_with_ai(Ok(format!("[ai]\nprofile = \"{value}\"")), None);
            assert_eq!((loaded.profile(), notice), (want, None), "{value:?}");
        }
    }

    // Spec (configuration): the profile counts with the other problems.
    #[test]
    fn a_profile_problem_counts_with_the_others() {
        let text = "[statusline]\nposition = \"middle\"\n[ai]\nprofile = 3";
        let (_, _, notice) = Config::load_with_ai(Ok(text.into()), None);
        assert_eq!(
            notice.as_deref(),
            Some("config (2 problems): statusline.position: expected \"top\" or \"bottom\"")
        );
        let (_, loaded, notice) = Config::load_with_ai(Ok("ai = 3\n".into()), None);
        assert_eq!(
            loaded.profile(),
            crate::core::ai::config::Profile::Programmer
        );
        assert_eq!(notice.as_deref(), Some("config: ai: expected a table"));
    }

    // Spec: File location.
    #[test]
    fn the_path_prefers_xdg_then_home() {
        let p =
            |x: Option<&str>, h: Option<&str>| config_path(x.map(Into::into), h.map(Into::into));
        let xdg = Some("/x/lazarobox/config.toml".into());
        let home = Some("/h/.config/lazarobox/config.toml".into());
        assert_eq!(p(Some("/x"), Some("/h")), xdg);
        assert_eq!(p(None, Some("/h")), home);
        assert_eq!(p(Some(""), Some("/h")), home);
        assert_eq!(p(Some("rel"), Some("/h")), home);
        assert_eq!(p(None, None), None);
        // Spec: Relative or empty HOME is ignored.
        assert_eq!(p(None, Some("")), None);
        assert_eq!(p(None, Some("rel")), None);
        assert_eq!(p(Some("/x"), Some("")), xdg);
    }

    // Spec (chat-memory): enabled without the acknowledgement.
    #[test]
    fn enabled_memory_without_the_acknowledgement_gives_the_privacy_notice() {
        let (_, loaded, notice) =
            Config::load_with_ai(Ok("[ai.memory]\nenabled = true\n".into()), None);
        assert!(loaded.memory.enabled && !loaded.memory.active());
        assert_eq!(
            notice.as_deref(),
            Some("memory: turn it on from the menu to review the privacy note")
        );
    }

    #[test]
    fn an_acknowledged_or_disabled_memory_gives_no_notice() {
        for text in [
            "[ai.memory]\nenabled = true\nacknowledged = true\n",
            "[ai.memory]\nacknowledged = true\n",
            "[ai.memory]\nenabled = false\n",
            "",
        ] {
            assert_eq!(
                Config::load_with_ai(Ok(text.into()), None).2,
                None,
                "{text:?}"
            );
        }
    }

    #[test]
    fn the_privacy_notice_is_shown_with_mouse_on() {
        let text = "mouse = true\n[ai.memory]\nenabled = true\n";
        let (config, _, notice) = Config::load_with_ai(Ok(text.into()), None);
        assert!(config.mouse);
        assert_eq!(notice.as_deref(), Some(ai::config::UNACKNOWLEDGED_NOTICE));
    }

    // The root `mouse` key is a boolean, written as `true` / `false`.
    #[test]
    fn the_root_mouse_edit_is_a_flag() {
        let edit = |table, key| ConfigEdit {
            table,
            key,
            value: Cow::Borrowed("false"),
        };
        assert!(edit("", "mouse").is_flag());
        assert!(!edit("", "other").is_flag());
        assert!(!edit("tabbar", "mouse").is_flag());
    }

    // The environment is a parameter, never read here.
    #[test]
    fn load_with_ai_takes_the_port_default_from_its_argument() {
        let port = |env| Config::load_with_ai(Ok(String::new()), env).1.memory.port;
        assert_eq!(port(None), 7437);
        assert_eq!(port(Some("8123")), 8123);
        assert_eq!(port(Some(" 8123\n")), 8123);
        assert_eq!(port(Some("abc")), 7437);
    }

    // Spec: Counted with other problems.
    #[test]
    fn a_memory_problem_is_counted_like_any_other() {
        let text = "[ai.memory]\nport = 0\n[statusline]\nposition = \"middle\"\n";
        let (config, loaded, notice) = Config::load_with_ai(Ok(text.into()), None);
        assert_eq!(config, Config::default());
        assert_eq!(loaded.memory.port, 7437);
        assert!(
            notice.unwrap().starts_with("config (2 problems): "),
            "counted"
        );
    }

    // Spec: Non-table ai.memory.
    #[test]
    fn a_non_table_memory_gives_one_config_notice() {
        let (_, loaded, notice) = Config::load_with_ai(Ok("[ai]\nmemory = 3\n".into()), None);
        assert_eq!(loaded.memory, Default::default());
        assert_eq!(
            notice.as_deref(),
            Some("config: ai.memory: expected a table")
        );
    }
}
