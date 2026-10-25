//! The pure core of the agent tools: what the chat may ask the user's pane to
//! do. No IO lives here.

pub mod capture;
pub mod command;
pub mod content;
pub mod exec;
pub mod glob;
#[cfg(test)]
mod golden;
pub mod label;
pub mod mcp;
pub mod paths;
pub mod registry;
pub mod report;
pub mod servers;
pub mod settle;
pub mod stage;
pub mod summary;
pub mod wiring;

use std::borrow::Cow;

use serde_json::Value;

use registry::{ArgDef, ArgKind, ArgValue, Family, FamilySet, ToolDef, parse_args, schema};

/// Whether a tool only reads or can change the user's session.
///
/// `Read` does NOT mean free. Whether a call needs the user's `y` is decided
/// by the engine's locality in slice 2e (a cloud engine asks before
/// `read_file`). Nothing may dispatch on `Access` alone before then.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Access {
    Read,
    Write,
}

/// Where what a tool returns goes. Known from the adapter kind (see
/// `Kind::locality`), never configured and never guessed from a model name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Locality {
    /// An HTTP loopback server: file text stays on the machine.
    Local,
    /// A CLI: what a tool returns goes to the provider.
    Cloud,
}

/// One tool as offered to an engine.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub schema: Value,
    pub access: Access,
}

/// A tool from a user-configured MCP server, offered next to the built-ins.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExternalTool {
    pub name: String,
    pub description: String,
    pub schema: Value,
}

/// A window of a file, 1-based: `max` lines from line `start`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lines {
    pub start: u32,
    pub max: u32,
}

/// A validated tool call.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Call {
    FindDir {
        query: String,
    },
    ListDir {
        path: String,
    },
    ReadFile {
        path: String,
        /// The window to read; `None` reads from the start, as in 0.18.0.
        lines: Option<Lines>,
    },
    /// `dir` is the optional sub-directory to search, as the model wrote it.
    Glob {
        pattern: String,
        dir: Option<String>,
    },
    /// `dir` is the optional sub-directory to search, as the model wrote it.
    Grep {
        pattern: String,
        dir: Option<String>,
    },
    RunInPane {
        command: String,
    },
    /// A tool of a user MCP server. It is always a write tool: every call
    /// needs the user's `y`. `name` is the qualified name the model used;
    /// `server` and `tool` come from the lookup (`servers::Qualified`), never
    /// from splitting `name`.
    External {
        name: String,
        server: String,
        tool: String,
        args: Value,
    },
}

impl Call {
    /// The tool's name, as the registry offers it (qualified for a user tool).
    pub fn name(&self) -> &str {
        match self {
            Call::External { name, .. } => name,
            Call::FindDir { .. } => "find_dir",
            Call::ListDir { .. } => "list_dir",
            Call::ReadFile { .. } => "read_file",
            Call::Glob { .. } => "glob",
            Call::Grep { .. } => "grep",
            Call::RunInPane { .. } => "run_in_pane",
        }
    }

    /// The argument of the call: a query, a path or a command (a search adds the
    /// folder it looks in); a user tool
    /// has its arguments as compact JSON.
    pub fn argument(&self) -> Cow<'_, str> {
        match self {
            Call::FindDir { query } => Cow::Borrowed(query),
            Call::ListDir { path } | Call::ReadFile { path, .. } => Cow::Borrowed(path),
            Call::Glob { pattern, dir } | Call::Grep { pattern, dir } => match dir {
                Some(dir) => Cow::Owned(format!("{pattern} in {dir}")),
                None => Cow::Borrowed(pattern),
            },
            Call::RunInPane { command } => Cow::Borrowed(command),
            Call::External { args, .. } => Cow::Owned(args.to_string()),
        }
    }
}

/// Why a call was refused. Short and content-free: it may go back to a model.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToolError {
    UnknownTool,
    NotAnObject,
    MissingArgument,
    WrongType,
    ExtraArgument,
    TooLong,
}

/// The one required string argument of the 0.18.0 tools.
const fn text(name: &'static str) -> ArgDef {
    ArgDef {
        name,
        kind: ArgKind::Str { max: MAX_ARG_BYTES },
        required: true,
        gate: None,
    }
}

/// The most lines one `read_file` window may ask for (and the default).
pub const READ_MAX_LINES: i64 = 2000;

/// The built-ins in offer order. `Access::Read` is only a classification,
/// not an approval rule: see [`Access`].
const BUILT_INS: [ToolDef; 6] = [
    ToolDef {
        name: "find_dir",
        description: "Find directories under the user's home whose name contains the query. Names that look like secrets are left out and the result says so.",
        access: Access::Read,
        family: Family::Read,
        args: &[text("query")],
    },
    ToolDef {
        name: "list_dir",
        description: "List the entries of a directory under the user's home. Names that look like secrets are left out and the result says so.",
        access: Access::Read,
        family: Family::Read,
        args: &[text("path")],
    },
    ToolDef {
        name: "read_file",
        description: "Read a text file under the user's home.",
        access: Access::Read,
        family: Family::Read,
        args: &[
            text("path"),
            ArgDef {
                name: "start_line",
                kind: ArgKind::Int {
                    min: 1,
                    max: u32::MAX as i64,
                },
                required: false,
                gate: Some(Family::Search),
            },
            ArgDef {
                name: "max_lines",
                kind: ArgKind::Int {
                    min: 1,
                    max: READ_MAX_LINES,
                },
                required: false,
                gate: Some(Family::Search),
            },
        ],
    },
    ToolDef {
        name: "glob",
        description: "List the files under the chat's root whose root-relative path matches a glob pattern (`*`, `**`, `?`, `[a-z]`). Names only, never content. Names that look like secrets are left out and the result says so.",
        access: Access::Read,
        family: Family::Search,
        args: &[
            ArgDef {
                name: "pattern",
                kind: ArgKind::Str {
                    max: glob::MAX_PATTERN_BYTES,
                },
                required: true,
                gate: None,
            },
            ArgDef {
                name: "path",
                kind: ArgKind::Str { max: MAX_ARG_BYTES },
                required: false,
                gate: None,
            },
        ],
    },
    ToolDef {
        name: "grep",
        description: "Search the text of the files under the chat's root for a regular expression (linear-time engine, no look-around or backreferences). Returns `path:line:text` for at most 100 matches. Binary files, large files and names that look like secrets are skipped, and secrets in the text are hidden.",
        access: Access::Read,
        family: Family::Search,
        args: &[
            ArgDef {
                name: "pattern",
                kind: ArgKind::Str {
                    max: glob::MAX_PATTERN_BYTES,
                },
                required: true,
                gate: None,
            },
            ArgDef {
                name: "path",
                kind: ArgKind::Str { max: MAX_ARG_BYTES },
                required: false,
                gate: None,
            },
        ],
    },
    ToolDef {
        name: "run_in_pane",
        description: "Type a shell command into the user's pane. The result says whether it was only typed or typed and run, and carries the output of a command that ran, which may be \"not available\". A single cd, pwd, ls, tree or mkdir with plain or quoted paths (no &&, ;, |, $ or redirections) can run directly when the user allows it; anything else waits for the user to approve it and press Enter. Prefer one simple command per call. You cannot change files yourself. Only mkdir can run directly; any other change needs a command the user reviews and runs.",
        access: Access::Write,
        family: Family::Pane,
        args: &[text("command")],
    },
];

/// The tools offered at [`FamilySet::LEGACY`], the 0.18.0 set: see
/// [`registry_with`].
pub fn registry(external: &[ExternalTool]) -> Vec<ToolSpec> {
    registry_with(external, FamilySet::LEGACY)
}

/// The tools offered to an engine: the built-ins whose family is in `set`,
/// then the external ones (which belong to no family: pass `&[]` to drop
/// them), each name once and none shadowing a built-in. The names are expected to be
/// server-qualified already (see `servers::qualify`); a raw colliding name is
/// dropped here as a last defence. An external tool is never `Read`: it runs
/// code the user did not write here.
pub fn registry_with(external: &[ExternalTool], set: FamilySet) -> Vec<ToolSpec> {
    let built_in = BUILT_INS
        .iter()
        .filter(|def| set.contains(def.family))
        .map(|def| ToolSpec {
            name: def.name.to_owned(),
            description: def.description.to_owned(),
            schema: schema(def, set),
            access: def.access,
        });
    let mut seen: Vec<&str> = Vec::new();
    let external = external
        .iter()
        .filter(|tool| !BUILT_INS.iter().any(|def| def.name == tool.name))
        .filter(|tool| {
            let fresh = !seen.contains(&tool.name.as_str());
            seen.push(&tool.name);
            fresh
        })
        .map(|tool| ToolSpec {
            name: tool.name.clone(),
            description: tool.description.clone(),
            schema: tool.schema.clone(),
            access: Access::Write,
        });
    built_in.chain(external).collect()
}

/// The longest `path`, `query` or `command` argument accepted, in bytes. It
/// bounds the decoded text only; the cap on the raw wire message belongs to
/// slices 2b and 2e.
pub const MAX_ARG_BYTES: usize = 4096;

/// Checks a call by name against the built-in argument schemas: an object
/// with the one required string argument and no other key, at most
/// [`MAX_ARG_BYTES`] long.
///
/// Duplicate JSON keys cannot be seen here: `serde_json::Value` has already
/// collapsed them to the last one. They are rejected at the raw wire layer by
/// [`mcp::Frame::parse`] (slice 2c) on the MCP path; any other entry point
/// (slice 2e) MUST do the same before the arguments become a `Value`.
pub fn parse_call(name: &str, args: &Value) -> Result<Call, ToolError> {
    parse_built_in(name, args, FamilySet::LEGACY)
}

/// A built-in call by name. One whose family is not in `set` is an unknown
/// tool, the same refusal as a name that does not exist.
fn parse_built_in(name: &str, args: &Value, set: FamilySet) -> Result<Call, ToolError> {
    let def = BUILT_INS
        .iter()
        .filter(|def| set.contains(def.family))
        .find(|def| def.name == name)
        .ok_or(ToolError::UnknownTool)?;
    let parsed = parse_args(def.args, args, set)?;
    let Some(ArgValue::Str(text)) = def.args.first().and_then(|arg| parsed.get(arg.name)) else {
        return Err(ToolError::MissingArgument);
    };
    let text = text.clone();
    match name {
        "find_dir" => Ok(Call::FindDir { query: text }),
        "list_dir" => Ok(Call::ListDir { path: text }),
        "read_file" => Ok(Call::ReadFile {
            path: text,
            lines: window(&parsed),
        }),
        "glob" => Ok(Call::Glob {
            pattern: text,
            dir: match parsed.get("path") {
                Some(ArgValue::Str(dir)) => Some(dir.clone()),
                _ => None,
            },
        }),
        "grep" => Ok(Call::Grep {
            pattern: text,
            dir: match parsed.get("path") {
                Some(ArgValue::Str(dir)) => Some(dir.clone()),
                _ => None,
            },
        }),
        "run_in_pane" => Ok(Call::RunInPane { command: text }),
        // Unreachable: the name was found in `BUILT_INS` above and every
        // built-in has an arm. Kept so a new built-in cannot become a
        // `run_in_pane` by falling through.
        _ => Err(ToolError::UnknownTool),
    }
}

/// The `start_line` / `max_lines` of a `read_file` call, if either was given.
fn window(parsed: &registry::Parsed) -> Option<Lines> {
    let int = |name| match parsed.get(name) {
        Some(ArgValue::Int(n)) => Some(u32::try_from(*n).unwrap_or(u32::MAX)),
        _ => None,
    };
    let (start, max) = (int("start_line"), int("max_lines"));
    (start.is_some() || max.is_some()).then(|| Lines {
        start: start.unwrap_or(1),
        max: max.unwrap_or(READ_MAX_LINES as u32),
    })
}

/// The most a user tool's arguments may take, as compact JSON, in bytes. Low
/// on purpose: every call asks the user, and what the prompt cannot show is
/// what the user cannot judge.
pub const MAX_EXTERNAL_ARGS: usize = 4 * 1024;

/// [`parse_call`] that also knows the tools of the user servers of the turn.
/// A built-in name is always the built-in. Any other name must be in
/// `external` (a lookup, never a split on `__`); its arguments must be an
/// object of at most [`MAX_EXTERNAL_ARGS`] bytes. The server validates their
/// shape: the call is a write that needs the user's `y` first.
pub fn parse_call_with(
    name: &str,
    args: &Value,
    external: &servers::Qualified,
) -> Result<Call, ToolError> {
    parse_call_in(name, args, external, FamilySet::LEGACY)
}

/// [`parse_call_with`] gated by `set`: a built-in outside the set is refused
/// as an unknown tool, so a model cannot call what was not offered.
pub fn parse_call_in(
    name: &str,
    args: &Value,
    external: &servers::Qualified,
    set: FamilySet,
) -> Result<Call, ToolError> {
    // Any built-in name, offered or not, is the built-in and never an
    // external tool; `parse_built_in` then refuses one whose family is off.
    if BUILT_INS.iter().any(|def| def.name == name) {
        return parse_built_in(name, args, set);
    }
    let (server, tool) = external.raw_of(name).ok_or(ToolError::UnknownTool)?;
    if !args.is_object() {
        return Err(ToolError::NotAnObject);
    }
    if args.to_string().len() > MAX_EXTERNAL_ARGS {
        return Err(ToolError::TooLong);
    }
    Ok(Call::External {
        name: name.to_owned(),
        server: server.to_owned(),
        tool: tool.to_owned(),
        args: args.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_call_knows_its_registry_name_and_argument() {
        for (name, arg) in [
            ("find_dir", "q"),
            ("list_dir", "~/p"),
            ("read_file", "~/f"),
            ("run_in_pane", "ls"),
        ] {
            let call = parse_call(name, &json!({ registry_arg(name): arg })).unwrap();
            assert_eq!((call.name(), &*call.argument()), (name, arg));
        }
    }

    fn registry_arg(name: &str) -> &'static str {
        match name {
            "find_dir" => "query",
            "run_in_pane" => "command",
            _ => "path",
        }
    }

    fn names(tools: &[ToolSpec]) -> Vec<&str> {
        tools.iter().map(|t| t.name.as_str()).collect()
    }

    // Spec (agent-tools, registry): the four built-ins with schema and access.
    #[test]
    fn the_registry_lists_the_four_built_ins_with_their_access() {
        let tools = registry(&[]);
        assert_eq!(
            names(&tools),
            ["find_dir", "list_dir", "read_file", "run_in_pane"]
        );
        let access: Vec<Access> = tools.iter().map(|t| t.access).collect();
        assert_eq!(
            access,
            [Access::Read, Access::Read, Access::Read, Access::Write]
        );
        for tool in &tools {
            assert!(!tool.description.is_empty(), "{}", tool.name);
            assert_eq!(tool.schema["type"], "object", "{}", tool.name);
            assert_eq!(tool.schema["additionalProperties"], false, "{}", tool.name);
        }
    }

    // The model reads this before it reads the prompt: what runs directly, and
    // that the result says what happened. The same text in every mode (D18).
    #[test]
    fn run_in_pane_says_what_runs_directly_and_what_waits() {
        let tools = registry(&[]);
        let run = tools.iter().find(|t| t.name == "run_in_pane").unwrap();
        for needle in [
            "only typed or typed and run",
            "carries the output of a command that ran, which may be \"not available\"",
            "cd, pwd, ls, tree or mkdir",
            "plain or quoted paths",
            "no &&, ;, |, $ or redirections",
            "can run directly when the user allows it",
            "anything else waits for the user to approve it and press Enter",
            "one simple command per call",
            "You cannot change files yourself. Only mkdir can run directly; any other change needs a command the user reviews and runs.",
        ] {
            assert!(run.description.contains(needle), "{needle}");
        }
        assert!(run.description.len() < 640, "{}", run.description.len());
        assert!(run.description.is_ascii());
    }

    #[test]
    fn each_schema_requires_exactly_its_one_string_argument() {
        let want = [
            ("find_dir", "query"),
            ("list_dir", "path"),
            ("read_file", "path"),
            ("run_in_pane", "command"),
        ];
        for (tool, (name, arg)) in registry(&[]).iter().zip(want) {
            assert_eq!(tool.name, name);
            assert_eq!(tool.schema["required"], json!([arg]), "{name}");
            assert_eq!(tool.schema["properties"][arg]["type"], "string", "{name}");
            assert_eq!(
                tool.schema["properties"].as_object().unwrap().len(),
                1,
                "{name}"
            );
        }
    }

    // Spec: the MCP and Ollama builders read one list, so it is the same.
    #[test]
    fn every_call_gives_the_same_list() {
        assert_eq!(registry(&[]), registry(&[]));
    }

    #[test]
    fn external_tools_follow_and_never_shadow_a_built_in() {
        let ext = |name: &str| ExternalTool {
            name: name.to_owned(),
            description: "d".to_owned(),
            schema: json!({"type": "object"}),
        };
        let tools = registry(&[ext("read_file"), ext("search"), ext("run_in_pane")]);
        assert_eq!(
            names(&tools),
            ["find_dir", "list_dir", "read_file", "run_in_pane", "search"]
        );
        let built_in = registry(&[]);
        assert_eq!(tools[2], built_in[2], "read_file stays the built-in");
        assert_eq!(tools[4].access, Access::Write, "external is never free");
    }

    #[test]
    fn parse_call_accepts_each_built_in() {
        let cases = [
            (
                "find_dir",
                json!({"query": "src"}),
                Call::FindDir {
                    query: "src".into(),
                },
            ),
            (
                "list_dir",
                json!({"path": "."}),
                Call::ListDir { path: ".".into() },
            ),
            (
                "read_file",
                json!({"path": "a.rs"}),
                Call::ReadFile {
                    path: "a.rs".into(),
                    lines: None,
                },
            ),
            (
                "run_in_pane",
                json!({"command": "ls"}),
                Call::RunInPane {
                    command: "ls".into(),
                },
            ),
        ];
        for (name, args, call) in cases {
            assert_eq!(parse_call(name, &args), Ok(call), "{name}");
        }
    }

    #[test]
    fn parse_call_refuses_an_unknown_tool() {
        for name in ["bash", "", "READ_FILE", "read_file "] {
            assert_eq!(
                parse_call(name, &json!({"path": "a"})),
                Err(ToolError::UnknownTool),
                "{name:?}"
            );
        }
    }

    #[test]
    fn parse_call_refuses_missing_and_mistyped_arguments() {
        assert_eq!(
            parse_call("read_file", &json!({})),
            Err(ToolError::MissingArgument)
        );
        assert_eq!(
            parse_call("read_file", &json!({"file": "a"})),
            Err(ToolError::MissingArgument)
        );
        for bad in [
            json!(1),
            json!(null),
            json!(["a"]),
            json!({"p": 1}),
            json!(true),
        ] {
            let args = json!({ "path": bad });
            assert_eq!(
                parse_call("list_dir", &args),
                Err(ToolError::WrongType),
                "{args}"
            );
        }
        for args in [json!(null), json!("path"), json!([1])] {
            assert_eq!(
                parse_call("list_dir", &args),
                Err(ToolError::NotAnObject),
                "{args}"
            );
        }
    }

    // Spec: extra keys such as `mode` are refused, not ignored.
    #[test]
    fn parse_call_refuses_extra_keys() {
        assert_eq!(
            parse_call("read_file", &json!({"path": "a", "mode": "write"})),
            Err(ToolError::ExtraArgument)
        );
        assert_eq!(
            parse_call("run_in_pane", &json!({"command": "ls", "enter": true})),
            Err(ToolError::ExtraArgument)
        );
    }

    // Review W4: oversized arguments are refused, the limit itself is not.
    #[test]
    fn parse_call_caps_the_argument_length() {
        let arg = |len: usize| "a".repeat(len);
        for (name, key) in [
            ("find_dir", "query"),
            ("list_dir", "path"),
            ("read_file", "path"),
            ("run_in_pane", "command"),
        ] {
            assert!(
                parse_call(name, &json!({ key: arg(MAX_ARG_BYTES) })).is_ok(),
                "{name} at the limit"
            );
            assert_eq!(
                parse_call(name, &json!({ key: arg(MAX_ARG_BYTES + 1) })),
                Err(ToolError::TooLong),
                "{name} over the limit"
            );
        }
    }

    fn qualified() -> servers::Qualified {
        let tool = |name: &str| ExternalTool {
            name: name.to_owned(),
            description: "d".to_owned(),
            schema: json!({"type": "object"}),
        };
        servers::qualify(&[(
            "fs",
            &[tool("write"), tool("read_file"), tool("deep__tool")],
        )])
    }

    // 4c: a qualified name becomes an external call by the lookup, with the
    // raw tool name and the server the lookup holds.
    #[test]
    fn an_external_call_is_routed_by_the_lookup() {
        let q = qualified();
        let args = json!({"path": "~/x", "n": 2});
        for (name, tool) in [
            ("fs__write", "write"),
            ("fs__read_file", "read_file"),
            ("fs__deep__tool", "deep__tool"),
        ] {
            let call = parse_call_with(name, &args, &q).unwrap();
            assert_eq!(
                call,
                Call::External {
                    name: name.to_owned(),
                    server: "fs".to_owned(),
                    tool: tool.to_owned(),
                    args: args.clone(),
                }
            );
            assert_eq!(call.name(), name);
        }
    }

    // Never by splitting on `__`: a name the lookup does not hold is unknown.
    #[test]
    fn a_name_outside_the_lookup_is_unknown_even_if_it_looks_qualified() {
        let q = qualified();
        for name in ["fs__deep", "git__log", "write", "fs__"] {
            assert_eq!(
                parse_call_with(name, &json!({}), &q),
                Err(ToolError::UnknownTool),
                "{name}"
            );
        }
    }

    #[test]
    fn a_built_in_name_is_always_the_built_in() {
        let mut q = qualified();
        q.tools[0].tool.name = "read_file".to_owned();
        assert_eq!(
            parse_call_with("read_file", &json!({"path": "a"}), &q),
            Ok(Call::ReadFile {
                path: "a".into(),
                lines: None
            })
        );
        assert_eq!(
            parse_call_with("read_file", &json!({"x": 1}), &q),
            Err(ToolError::MissingArgument)
        );
    }

    // Case and look-alike variants are other names: only the lookup decides.
    #[test]
    fn case_and_unicode_variants_of_a_qualified_name_are_unknown() {
        let q = qualified();
        for name in [
            "FS__WRITE",
            "Fs__write",
            "fs\u{ff3f}\u{ff3f}write",
            "fs__write ",
        ] {
            assert_eq!(
                parse_call_with(name, &json!({}), &q),
                Err(ToolError::UnknownTool),
                "{name:?}"
            );
        }
    }

    #[test]
    fn external_arguments_must_be_an_object_within_the_cap() {
        let q = qualified();
        assert_eq!(
            parse_call_with("fs__write", &json!([1]), &q),
            Err(ToolError::NotAnObject)
        );
        assert!(matches!(
            parse_call_with("fs__write", &json!(null), &q),
            Err(ToolError::NotAnObject)
        ));
        let at = |len: usize| json!({"k": "a".repeat(len)});
        let base = at(0).to_string().len();
        assert!(parse_call_with("fs__write", &at(MAX_EXTERNAL_ARGS - base), &q).is_ok());
        assert_eq!(
            parse_call_with("fs__write", &at(MAX_EXTERNAL_ARGS - base + 1), &q),
            Err(ToolError::TooLong)
        );
        assert_eq!(MAX_EXTERNAL_ARGS, 4096);
    }

    #[test]
    fn an_external_call_shows_its_arguments_as_one_line_of_json() {
        let call = parse_call_with("fs__write", &json!({"a": 1}), &qualified()).unwrap();
        assert_eq!(call.argument(), r#"{"a":1}"#);
        assert_eq!(
            parse_call("read_file", &json!({"path": "p"}))
                .unwrap()
                .argument(),
            "p"
        );
    }

    // P0a.4: the call carries an optional line window, `None` until a
    // Search-gated argument fills it.
    #[test]
    fn read_file_parses_with_no_line_window() {
        assert_eq!(
            parse_call("read_file", &json!({"path": "a.rs"})),
            Ok(Call::ReadFile {
                path: "a.rs".into(),
                lines: None
            })
        );
    }

    fn only(family: Family) -> FamilySet {
        FamilySet::EMPTY.with(family)
    }

    // P0b.1: the offered list is a filter over families, in table order.
    #[test]
    fn registry_with_offers_only_the_tools_of_the_set() {
        assert_eq!(
            names(&registry_with(&[], only(Family::Read))),
            ["find_dir", "list_dir", "read_file"]
        );
        assert_eq!(
            names(&registry_with(&[], only(Family::Pane))),
            ["run_in_pane"]
        );
        assert!(registry_with(&[], FamilySet::EMPTY).is_empty());
        assert_eq!(registry_with(&[], FamilySet::LEGACY), registry(&[]));
    }

    // External tools do not belong to a family: the set neither adds nor
    // drops them; the caller passes `&[]` to drop them.
    #[test]
    fn external_tools_survive_an_empty_set() {
        let ext = ExternalTool {
            name: "fs__write".to_owned(),
            description: "d".to_owned(),
            schema: json!({"type": "object"}),
        };
        assert_eq!(
            names(&registry_with(&[ext], FamilySet::EMPTY)),
            ["fs__write"]
        );
    }

    // P0b.1 / design D3: a built-in outside the set is refused with exactly
    // what an unknown name gets, so the model cannot call what was not
    // offered, and a name cannot reveal which families exist.
    #[test]
    fn a_built_in_outside_the_set_is_an_unknown_tool() {
        let q = qualified();
        let args = json!({"command": "ls"});
        let unknown = parse_call_in("nope", &args, &q, FamilySet::LEGACY);
        assert_eq!(unknown, Err(ToolError::UnknownTool));
        assert_eq!(
            parse_call_in("run_in_pane", &args, &q, only(Family::Read)),
            unknown
        );
        assert_eq!(
            parse_call_in("read_file", &json!({"path": "a"}), &q, only(Family::Pane)),
            unknown
        );
        // In the set, the same call parses.
        assert_eq!(
            parse_call_in("run_in_pane", &args, &q, only(Family::Pane)),
            Ok(Call::RunInPane {
                command: "ls".into()
            })
        );
    }

    // A built-in name is never an external tool, even with an empty set.
    #[test]
    fn a_built_in_name_never_falls_through_to_an_external_tool() {
        let q = qualified();
        assert_eq!(
            parse_call_in("read_file", &json!({"path": "a"}), &q, FamilySet::EMPTY),
            Err(ToolError::UnknownTool)
        );
        // An external name still routes under an empty set.
        assert!(parse_call_in("fs__write", &json!({}), &q, FamilySet::EMPTY).is_ok());
    }

    // The old entry points mean LEGACY.
    #[test]
    fn the_old_entry_points_delegate_at_legacy() {
        let q = qualified();
        let args = json!({"path": "a"});
        assert_eq!(
            parse_call_with("read_file", &args, &q),
            parse_call_in("read_file", &args, &q, FamilySet::LEGACY)
        );
        assert_eq!(
            parse_call("run_in_pane", &json!({"command": "ls"})),
            parse_call_in(
                "run_in_pane",
                &json!({"command": "ls"}),
                &q,
                FamilySet::LEGACY
            )
        );
    }

    fn search_set() -> FamilySet {
        FamilySet::LEGACY.with(Family::Search)
    }

    fn parse_glob(args: Value, set: FamilySet) -> Result<Call, ToolError> {
        parse_call_in("glob", &args, &servers::Qualified::default(), set)
    }

    // Spec (code-search, Family and toggle): glob exists only with Search on.
    #[test]
    fn glob_is_offered_and_parsed_only_with_search_on() {
        assert!(!names(&registry(&[])).contains(&"glob"));
        let on = registry_with(&[], search_set());
        assert_eq!(
            names(&on),
            [
                "find_dir",
                "list_dir",
                "read_file",
                "glob",
                "grep",
                "run_in_pane"
            ]
        );
        let glob = on.iter().find(|t| t.name == "glob").unwrap();
        assert_eq!(glob.access, Access::Read);
        assert_eq!(
            glob.schema,
            json!({
                "type": "object",
                "properties": {"pattern": {"type": "string"}, "path": {"type": "string"}},
                "required": ["pattern"],
                "additionalProperties": false,
            })
        );
        let args = json!({"pattern": "**/*.rs"});
        assert_eq!(
            parse_glob(args.clone(), FamilySet::LEGACY),
            Err(ToolError::UnknownTool)
        );
        assert_eq!(parse_call("glob", &args), Err(ToolError::UnknownTool));
        assert_eq!(
            parse_glob(args, search_set()),
            Ok(Call::Glob {
                pattern: "**/*.rs".into(),
                dir: None
            })
        );
    }

    #[test]
    fn glob_takes_an_optional_sub_directory() {
        let call = parse_glob(json!({"pattern": "*.rs", "path": "src"}), search_set()).unwrap();
        assert_eq!(
            call,
            Call::Glob {
                pattern: "*.rs".into(),
                dir: Some("src".into())
            }
        );
        assert_eq!((call.name(), &*call.argument()), ("glob", "*.rs in src"));
        let whole = parse_glob(json!({"pattern": "*.rs"}), search_set()).unwrap();
        assert_eq!(whole.argument(), "*.rs");
    }

    fn parse_read(args: Value, set: FamilySet) -> Result<Call, ToolError> {
        parse_call_in("read_file", &args, &servers::Qualified::default(), set)
    }

    // Spec (code-search, read_file paging): the window arguments exist only
    // with Search on, and the schema is untouched with it off.
    #[test]
    fn read_file_paging_arguments_exist_only_with_search_on() {
        let schema_of = |set| {
            let tools = registry_with(&[], set);
            tools
                .into_iter()
                .find(|t| t.name == "read_file")
                .unwrap()
                .schema
        };
        assert_eq!(
            schema_of(FamilySet::LEGACY),
            json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"],
                "additionalProperties": false,
            })
        );
        assert_eq!(
            schema_of(search_set())["properties"],
            json!({
                "path": {"type": "string"},
                "start_line": {"type": "integer", "minimum": 1, "maximum": u32::MAX},
                "max_lines": {"type": "integer", "minimum": 1, "maximum": READ_MAX_LINES},
            })
        );
        let args = json!({"path": "a", "start_line": 3});
        assert_eq!(
            parse_read(args.clone(), FamilySet::LEGACY),
            Err(ToolError::ExtraArgument)
        );
        assert_eq!(
            parse_call("read_file", &args),
            Err(ToolError::ExtraArgument)
        );
    }

    #[test]
    fn read_file_window_defaults_and_clamps() {
        let window = |args: Value| match parse_read(args, search_set()) {
            Ok(Call::ReadFile { lines, .. }) => lines,
            other => panic!("not a read: {other:?}"),
        };
        assert_eq!(window(json!({"path": "a"})), None);
        assert_eq!(
            window(json!({"path": "a", "start_line": 100, "max_lines": 20})),
            Some(Lines {
                start: 100,
                max: 20
            })
        );
        assert_eq!(
            window(json!({"path": "a", "start_line": 7})),
            Some(Lines {
                start: 7,
                max: READ_MAX_LINES as u32
            })
        );
        assert_eq!(
            window(json!({"path": "a", "max_lines": 5})),
            Some(Lines { start: 1, max: 5 })
        );
        assert_eq!(
            window(json!({"path": "a", "start_line": 0, "max_lines": 1_000_000})),
            Some(Lines {
                start: 1,
                max: READ_MAX_LINES as u32
            })
        );
        assert_eq!(
            parse_read(json!({"path": "a", "start_line": "1"}), search_set()),
            Err(ToolError::WrongType)
        );
    }

    // Spec (code-search, grep): same family gate, a 512-byte pattern, and an
    // optional sub-directory.
    #[test]
    fn grep_is_offered_and_parsed_only_with_search_on() {
        assert!(!names(&registry(&[])).contains(&"grep"));
        let on = registry_with(&[], search_set());
        let grep = on.iter().find(|t| t.name == "grep").unwrap();
        assert_eq!(grep.access, Access::Read);
        assert_eq!(
            grep.schema,
            json!({
                "type": "object",
                "properties": {"pattern": {"type": "string"}, "path": {"type": "string"}},
                "required": ["pattern"],
                "additionalProperties": false,
            })
        );
        let parse =
            |args: Value, set| parse_call_in("grep", &args, &servers::Qualified::default(), set);
        let args = json!({"pattern": "fn main", "path": "src"});
        assert_eq!(
            parse(args.clone(), FamilySet::LEGACY),
            Err(ToolError::UnknownTool)
        );
        let call = parse(args, search_set()).unwrap();
        assert_eq!(
            call,
            Call::Grep {
                pattern: "fn main".into(),
                dir: Some("src".into())
            }
        );
        assert_eq!((call.name(), &*call.argument()), ("grep", "fn main in src"));
        let whole = parse(json!({"pattern": "fn main"}), search_set()).unwrap();
        assert_eq!(whole.argument(), "fn main");
        assert_eq!(
            parse(
                json!({"pattern": "a".repeat(glob::MAX_PATTERN_BYTES + 1)}),
                search_set()
            ),
            Err(ToolError::TooLong)
        );
        assert_eq!(
            parse(json!({}), search_set()),
            Err(ToolError::MissingArgument)
        );
    }

    // Spec (agent-tools): the shared parse order and caps apply to glob too.
    #[test]
    fn glob_arguments_are_validated_through_the_table() {
        let bad = |args: Value| parse_glob(args, search_set());
        assert_eq!(bad(json!({})), Err(ToolError::MissingArgument));
        assert_eq!(bad(json!({"pattern": 5})), Err(ToolError::WrongType));
        assert_eq!(
            bad(json!({"pattern": "a", "path": 5})),
            Err(ToolError::WrongType)
        );
        assert_eq!(
            bad(json!({"pattern": "a", "mode": 1})),
            Err(ToolError::ExtraArgument)
        );
        assert_eq!(bad(json!(["a"])), Err(ToolError::NotAnObject));
        assert_eq!(
            bad(json!({"pattern": "a".repeat(glob::MAX_PATTERN_BYTES + 1)})),
            Err(ToolError::TooLong)
        );
        assert!(bad(json!({"pattern": "a".repeat(glob::MAX_PATTERN_BYTES)})).is_ok());
        assert_eq!(
            bad(json!({"pattern": "a", "path": "p".repeat(MAX_ARG_BYTES + 1)})),
            Err(ToolError::TooLong)
        );
    }
}
