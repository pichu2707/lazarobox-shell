//! The pure core of the agent tools: what the chat may ask the user's pane to
//! do. No IO lives here.

pub mod command;
pub mod exec;
pub mod label;
pub mod mcp;
pub mod paths;
pub mod summary;
pub mod wiring;

use serde_json::{Value, json};

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

/// A validated tool call.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Call {
    FindDir { query: String },
    ListDir { path: String },
    ReadFile { path: String },
    RunInPane { command: String },
}

impl Call {
    /// The tool's name, as the registry offers it.
    pub fn name(&self) -> &'static str {
        match self {
            Call::FindDir { .. } => "find_dir",
            Call::ListDir { .. } => "list_dir",
            Call::ReadFile { .. } => "read_file",
            Call::RunInPane { .. } => "run_in_pane",
        }
    }

    /// The one argument of the call: a query, a path or a command.
    pub fn argument(&self) -> &str {
        match self {
            Call::FindDir { query } => query,
            Call::ListDir { path } | Call::ReadFile { path } => path,
            Call::RunInPane { command } => command,
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

/// The built-ins in offer order: name, argument, description, access.
/// `Access::Read` is only a classification, not an approval rule: see
/// [`Access`].
const BUILT_INS: [(&str, &str, &str, Access); 4] = [
    (
        "find_dir",
        "query",
        "Find directories under the user's home whose name contains the query. Names that look like secrets are left out and the result says so.",
        Access::Read,
    ),
    (
        "list_dir",
        "path",
        "List the entries of a directory under the user's home. Names that look like secrets are left out and the result says so.",
        Access::Read,
    ),
    (
        "read_file",
        "path",
        "Read a text file under the user's home.",
        Access::Read,
    ),
    (
        "run_in_pane",
        "command",
        "Type a shell command into the user's pane. The user must approve it and press Enter themselves.",
        Access::Write,
    ),
];

/// The schema of a tool with exactly one required string argument.
fn one_string(arg: &str) -> Value {
    json!({
        "type": "object",
        "properties": { arg: { "type": "string" } },
        "required": [arg],
        "additionalProperties": false,
    })
}

/// The tools offered to every engine: the built-ins, then the external ones
/// whose name does not shadow a built-in. Dropping a colliding external tool
/// is temporary: slice 4a exposes it under its server-qualified name and
/// removes duplicates among externals. An external tool is never `Read`:
/// it runs code the user did not write here.
pub fn registry(external: &[ExternalTool]) -> Vec<ToolSpec> {
    let built_in = BUILT_INS
        .iter()
        .map(|(name, arg, description, access)| ToolSpec {
            name: (*name).to_owned(),
            description: (*description).to_owned(),
            schema: one_string(arg),
            access: *access,
        });
    let external = external
        .iter()
        .filter(|tool| !BUILT_INS.iter().any(|(name, ..)| *name == tool.name))
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
    let (_, arg, ..) = BUILT_INS
        .iter()
        .find(|(known, ..)| *known == name)
        .ok_or(ToolError::UnknownTool)?;
    let object = args.as_object().ok_or(ToolError::NotAnObject)?;
    let value = object.get(*arg).ok_or(ToolError::MissingArgument)?;
    let text = value.as_str().ok_or(ToolError::WrongType)?.to_owned();
    if object.len() > 1 {
        return Err(ToolError::ExtraArgument);
    }
    if text.len() > MAX_ARG_BYTES {
        return Err(ToolError::TooLong);
    }
    match name {
        "find_dir" => Ok(Call::FindDir { query: text }),
        "list_dir" => Ok(Call::ListDir { path: text }),
        "read_file" => Ok(Call::ReadFile { path: text }),
        "run_in_pane" => Ok(Call::RunInPane { command: text }),
        // Unreachable: the name was found in `BUILT_INS` above and every
        // built-in has an arm. Kept so a new built-in cannot become a
        // `run_in_pane` by falling through.
        _ => Err(ToolError::UnknownTool),
    }
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
            assert_eq!((call.name(), call.argument()), (name, arg));
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
}
