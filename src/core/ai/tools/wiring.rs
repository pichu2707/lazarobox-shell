//! How an engine is told about the lazarobox tools. Pure: renderers of what
//! goes into a CLI's argv, environment and config file. Which engines get it
//! is decided elsewhere (`MCP_VERIFIED`); the runtime writes the file and
//! starts the process.

use std::{fmt, path::Path, path::PathBuf, sync::Arc};

use serde_json::json;

/// The name the engine knows the server by; its tools read `mcp__lazarobox__*`.
pub const SERVER: &str = "lazarobox";
/// The prefix of every tool name claude shows for this server.
pub const CLAUDE_TOOL_PREFIX: &str = "mcp__lazarobox__";
/// How long claude waits for one MCP call, in ms: a call may wait for the
/// user's `y` (D13).
const CLAUDE_TOOL_TIMEOUT_MS: &str = "900000";
/// The shim's subcommand and the variable it reads its token from (the same
/// as `runtime::mcp::shim`, which a test pins).
pub const SUBCOMMAND: &str = "__mcp-serve";
pub const TOKEN_ENV: &str = "LAZAROBOX_MCP_TOKEN";

/// A secret that never prints.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(text: &str) -> Self {
        Secret(text.to_owned())
    }

    /// The text, for the one place that writes it to the engine's config.
    pub fn reveal(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(..)")
    }
}

/// How a turn ends its session early: the engine's adapter calls it when the
/// turn runs as text after all (no jail, a layout the jail refuses, no shim
/// program), so the socket and token do not stay live for nothing. It does
/// nothing when the session is already over or was replaced. Equality ignores
/// it.
#[derive(Clone)]
pub struct Unwire(Option<Arc<dyn Fn() + Send + Sync>>);

impl Unwire {
    /// A session nobody needs to end.
    pub fn none() -> Self {
        Self(None)
    }

    pub fn new(end: impl Fn() + Send + Sync + 'static) -> Self {
        Self(Some(Arc::new(end)))
    }

    pub fn call(&self) {
        if let Some(end) = &self.0 {
            end();
        }
    }
}

impl fmt::Debug for Unwire {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Unwire")
    }
}

impl PartialEq for Unwire {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Unwire {}

/// The session a turn's engine connects to.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct McpWire {
    pub socket: PathBuf,
    pub token: Secret,
    pub unwire: Unwire,
}

/// What claude gets for one turn with tools.
#[derive(Clone, PartialEq, Eq)]
pub struct ClaudeWiring {
    /// The content of the config file (it holds the token: write it 0600).
    pub config: String,
    /// Appended to the argv, in the `--flag=value` form: both flags are
    /// variadic in claude, so a separate value would swallow what follows.
    pub args: Vec<String>,
    pub env: Vec<(&'static str, &'static str)>,
}

impl fmt::Debug for ClaudeWiring {
    /// The config holds the token, so it never prints.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClaudeWiring")
            .field("config", &"(..)")
            .field("args", &self.args)
            .field("env", &self.env)
            .finish()
    }
}

/// The wiring for claude: the stdio server entry (the token goes only in its
/// `env`, never argv), the flags that load it and allow its tools, and the
/// call timeout. `None` when a path is not UTF-8.
pub fn claude(shim: &Path, config: &Path, wire: &McpWire) -> Option<ClaudeWiring> {
    let doc = json!({"mcpServers": {SERVER: {
        "type": "stdio",
        "command": shim.to_str()?,
        "args": [SUBCOMMAND, wire.socket.to_str()?],
        "env": {TOKEN_ENV: wire.token.reveal()},
    }}});
    Some(ClaudeWiring {
        config: serde_json::to_string_pretty(&doc).ok()?,
        args: vec![
            format!("--mcp-config={}", config.to_str()?),
            format!("--allowedTools=mcp__{SERVER}"),
        ],
        env: vec![("MCP_TOOL_TIMEOUT", CLAUDE_TOOL_TIMEOUT_MS)],
    })
}

/// What codex gets for one turn with tools: `-c` overrides for the server
/// entry, and the one variable its own process env must hold.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CodexWiring {
    pub args: Vec<String>,
    /// The token's variable and value. `Secret` never prints, so the derived
    /// `Debug` is safe.
    pub env: Vec<(&'static str, Secret)>,
}

/// How long codex waits for one MCP call, in seconds: a call may wait for the
/// user's `y` (D13).
const CODEX_TOOL_TIMEOUT_SEC: u32 = 900;
/// How long codex waits for the shim to start, in seconds.
const CODEX_STARTUP_TIMEOUT_SEC: u32 = 20;

/// A TOML basic string, escaped by the toml crate.
fn toml_string(text: &str) -> String {
    toml::Value::String(text.to_owned()).to_string()
}

/// The wiring for codex: `-c mcp_servers.lazarobox.*` overrides (the value is
/// TOML). The token is NOT in them: argv is visible in `ps`. `env_vars` names
/// the variable, and codex forwards it from its own process env, where the
/// runtime sets it. `None` when a path is not UTF-8.
pub fn codex(shim: &Path, wire: &McpWire) -> Option<CodexWiring> {
    let key = |name: &str| format!("mcp_servers.{SERVER}.{name}");
    let pairs = [
        ("command", toml_string(shim.to_str()?)),
        (
            "args",
            format!(
                "[{}, {}]",
                toml_string(SUBCOMMAND),
                toml_string(wire.socket.to_str()?)
            ),
        ),
        ("env_vars", format!("[{}]", toml_string(TOKEN_ENV))),
        ("tool_timeout_sec", CODEX_TOOL_TIMEOUT_SEC.to_string()),
        ("startup_timeout_sec", CODEX_STARTUP_TIMEOUT_SEC.to_string()),
    ];
    Some(CodexWiring {
        args: pairs
            .into_iter()
            .flat_map(|(name, value)| ["-c".to_owned(), format!("{}={value}", key(name))])
            .collect(),
        env: vec![(TOKEN_ENV, wire.token.clone())],
    })
}

/// The prefix of every tool name gemini shows for this server: its fully
/// qualified name is `mcp_{server}_{tool}` (gemini 0.63.0 docs).
///
/// The real gate on which server a call came from is the policy rule (its
/// exact `mcpName`); the decoder's prefix check is defence in depth.
pub const GEMINI_TOOL_PREFIX: &str = "mcp_lazarobox_";

/// Whether a path survives gemini's reading of `--policy` and `--admin-policy`
/// values, which it splits on `,` and trims per part: a comma or edge
/// whitespace would make it load a different path (`/home/u/a,b` loads
/// `/home/u/a`).
pub fn policy_flag_safe(path: &str) -> bool {
    !path.contains(',') && path.trim() == path
}

/// What gemini gets for one turn with tools.
#[derive(Clone, PartialEq, Eq)]
pub struct GeminiWiring {
    /// The workspace `settings.json` (it holds the token: write it 0600).
    pub settings: String,
    /// The policy file that lets the server's tools through (admin tier).
    pub policy: String,
    /// Appended to the argv, in the `--flag=value` form: both flags are
    /// variadic in gemini, so a separate value would swallow what follows.
    pub args: Vec<String>,
}

impl fmt::Debug for GeminiWiring {
    /// The settings hold the token, so they never print.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GeminiWiring")
            .field("settings", &"(..)")
            .field("policy", &self.policy)
            .field("args", &self.args)
            .finish()
    }
}

/// How long gemini waits for one MCP call, in ms: a call may wait for the
/// user's `y` (D13).
const GEMINI_TOOL_TIMEOUT_MS: u32 = 900_000;

/// The wiring for gemini: the workspace `settings.json` with the stdio server
/// entry (the token goes only in its `env`, never argv; `trust` skips gemini's
/// own confirmation, because the host asks the user), the flag that allows
/// the server, and an admin-tier policy rule that allows its tools. The
/// text-only policy (a deny-all at 999 of the user tier) stays loaded; the
/// admin tier outranks it by gemini's tier table, which a priority inside one
/// file never could (0 to 999). `None` when a path is not UTF-8, when the
/// policy path would be split or trimmed by gemini (see [`policy_flag_safe`]),
/// or when the shim or socket path holds a `$` (gemini expands `$VAR` in the
/// strings of its settings).
pub fn gemini(shim: &Path, policy: &Path, wire: &McpWire) -> Option<GeminiWiring> {
    let (shim, socket) = (shim.to_str()?, wire.socket.to_str()?);
    if !policy_flag_safe(policy.to_str()?) || shim.contains('$') || socket.contains('$') {
        return None;
    }
    let doc = json!({"mcpServers": {SERVER: {
        "command": shim,
        "args": [SUBCOMMAND, socket],
        "env": {TOKEN_ENV: wire.token.reveal()},
        "timeout": GEMINI_TOOL_TIMEOUT_MS,
        "trust": true,
    }}});
    Some(GeminiWiring {
        settings: serde_json::to_string_pretty(&doc).ok()?,
        policy: format!(
            "[[rule]]\nmcpName = {}\ndecision = \"allow\"\npriority = 999\n",
            toml_string(SERVER)
        ),
        args: vec![
            format!("--allowed-mcp-server-names={SERVER}"),
            format!("--admin-policy={}", policy.to_str()?),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wire() -> McpWire {
        McpWire {
            socket: PathBuf::from("/run/user/1000/lazarobox/s-42-1.sock"),
            token: Secret::new("0123456789abcdef0123456789abcdef"),
            unwire: Unwire::none(),
        }
    }

    fn wiring() -> ClaudeWiring {
        claude(
            Path::new("/opt/lb/bin/lazarobox-shell"),
            Path::new("/data/lazarobox/cli-tmp/42-7/mcp.json"),
            &wire(),
        )
        .unwrap()
    }

    #[test]
    fn the_claude_wiring_snapshot() {
        let w = wiring();
        let env: Vec<String> = w.env.iter().map(|(k, v)| format!("{k}={v}")).collect();
        let text = format!(
            "argv: {}\nenv: {}\nmcp.json:\n{}\n",
            w.args.join(" "),
            env.join(" "),
            w.config
        );
        insta::assert_snapshot!("claude_wiring", text);
    }

    #[test]
    fn the_token_is_only_in_the_config_env_entry() {
        let w = wiring();
        let token = wire().token.reveal().to_owned();
        assert!(w.config.contains(&token));
        let doc: serde_json::Value = serde_json::from_str(&w.config).unwrap();
        assert_eq!(doc["mcpServers"][SERVER]["env"][TOKEN_ENV], token);
        assert_eq!(doc["mcpServers"][SERVER]["args"][0], SUBCOMMAND);
        assert!(!w.args.iter().any(|a| a.contains(&token)));
        assert!(
            !w.env
                .iter()
                .any(|(k, v)| k.contains(&token) || v.contains(&token))
        );
        assert!(!format!("{:?}", wire()).contains(&token), "Debug hides it");
    }

    #[test]
    fn the_claude_wiring_never_prints_its_config() {
        let w = wiring();
        let token = wire().token.reveal().to_owned();
        let text = format!("{w:?}");
        assert!(!text.contains(&token), "{text}");
        assert!(text.contains("--allowedTools=mcp__lazarobox"), "{text}");
    }

    #[test]
    fn variadic_flags_use_the_equals_form_so_nothing_after_them_is_swallowed() {
        for arg in wiring().args {
            assert!(arg.starts_with("--") && arg.contains('='), "{arg}");
        }
    }

    #[test]
    fn a_path_that_is_not_utf8_gives_no_wiring() {
        use std::os::unix::ffi::OsStrExt;
        let odd = Path::new(std::ffi::OsStr::from_bytes(b"/a/\xff"));
        assert!(claude(odd, Path::new("/c"), &wire()).is_none());
    }

    fn codex_wiring() -> CodexWiring {
        codex(Path::new("/opt/lb/bin/lazarobox-shell"), &wire()).unwrap()
    }

    #[test]
    fn the_codex_wiring_snapshot() {
        let w = codex_wiring();
        let env: Vec<String> = w
            .env
            .iter()
            .map(|(k, v)| format!("{k}=<{} bytes>", v.reveal().len()))
            .collect();
        let text = format!("argv: {}\nenv: {}\n", w.args.join(" "), env.join(" "));
        insta::assert_snapshot!("codex_wiring", text);
    }

    #[test]
    fn the_codex_token_is_only_in_the_env_never_argv_or_debug() {
        let w = codex_wiring();
        let token = wire().token.reveal().to_owned();
        assert_eq!(w.env.len(), 1);
        assert_eq!(w.env[0].0, TOKEN_ENV);
        assert_eq!(w.env[0].1.reveal(), token);
        assert!(!w.args.iter().any(|a| a.contains(&token)), "{:?}", w.args);
        // The variable is named in `env_vars`, so codex forwards it.
        assert!(
            w.args
                .contains(&format!("mcp_servers.lazarobox.env_vars=[\"{TOKEN_ENV}\"]"))
        );
        assert!(!format!("{w:?}").contains(&token), "{w:?}");
    }

    #[test]
    fn a_codex_value_is_a_toml_string_even_for_an_odd_path() {
        let w = codex(Path::new("/a \"b\"/c\\d\te"), &wire()).unwrap();
        let command = w
            .args
            .iter()
            .find_map(|a| a.strip_prefix("mcp_servers.lazarobox.command="))
            .unwrap();
        let parsed: toml::Value = toml::from_str(&format!("v = {command}")).unwrap();
        assert_eq!(parsed["v"].as_str(), Some("/a \"b\"/c\\d\te"));
    }

    #[test]
    fn a_codex_path_that_is_not_utf8_gives_no_wiring() {
        use std::os::unix::ffi::OsStrExt;
        let odd = Path::new(std::ffi::OsStr::from_bytes(b"/a/\xff"));
        assert!(codex(odd, &wire()).is_none());
    }

    fn gemini_wiring() -> GeminiWiring {
        gemini(
            Path::new("/opt/lb/bin/lazarobox-shell"),
            Path::new("/data/lazarobox/cli-tmp/42-7/gemini-mcp-policy.toml"),
            &wire(),
        )
        .unwrap()
    }

    #[test]
    fn the_gemini_wiring_snapshot() {
        let w = gemini_wiring();
        let text = format!(
            "argv: {}\nsettings.json:\n{}\npolicy:\n{}",
            w.args.join(" "),
            w.settings,
            w.policy
        );
        insta::assert_snapshot!("gemini_wiring", text);
    }

    #[test]
    fn the_gemini_token_is_only_in_the_settings_env_entry() {
        let w = gemini_wiring();
        let token = wire().token.reveal().to_owned();
        let doc: serde_json::Value = serde_json::from_str(&w.settings).unwrap();
        let server = &doc["mcpServers"][SERVER];
        assert_eq!(server["env"][TOKEN_ENV], token);
        assert_eq!(server["args"][0], SUBCOMMAND);
        assert_eq!(server["timeout"], 900_000);
        assert_eq!(server["trust"], true);
        assert!(!w.args.iter().any(|a| a.contains(&token)), "{:?}", w.args);
        assert!(!w.policy.contains(&token));
        assert!(!format!("{w:?}").contains(&token), "Debug hides it");
    }

    #[test]
    fn the_gemini_flags_use_the_equals_form_so_nothing_after_them_is_swallowed() {
        let w = gemini_wiring();
        assert_eq!(w.args.len(), 2);
        for arg in &w.args {
            assert!(arg.starts_with("--") && arg.contains('='), "{arg}");
        }
        assert!(
            w.args
                .contains(&"--allowed-mcp-server-names=lazarobox".to_owned())
        );
    }

    #[test]
    fn the_gemini_policy_is_valid_toml_allowing_only_our_server() {
        let parsed: toml::Value = toml::from_str(&gemini_wiring().policy).unwrap();
        let rules = parsed["rule"].as_array().unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0]["mcpName"].as_str(), Some("lazarobox"));
        assert_eq!(rules[0]["decision"].as_str(), Some("allow"));
        assert!(rules[0].get("toolName").is_none(), "no tool wildcard");
    }

    #[test]
    fn a_gemini_path_that_is_not_utf8_gives_no_wiring() {
        use std::os::unix::ffi::OsStrExt;
        let odd = Path::new(std::ffi::OsStr::from_bytes(b"/a/\xff"));
        assert!(gemini(odd, Path::new("/p"), &wire()).is_none());
        assert!(gemini(Path::new("/s"), odd, &wire()).is_none());
    }

    #[test]
    fn a_gemini_policy_path_it_would_split_or_trim_gives_no_wiring() {
        // gemini splits `--admin-policy` on `,` and trims each part, so
        // `/home/u/a,b` would load `/home/u/a` as an admin-tier folder.
        for bad in [
            "/home/u/a,b/p.toml",
            " /p.toml",
            "/p.toml ",
            "/p.toml\n",
            "\t/p",
        ] {
            assert!(
                gemini(Path::new("/s"), Path::new(bad), &wire()).is_none(),
                "{bad:?}"
            );
        }
        assert!(gemini(Path::new("/s"), Path::new("/a b/p.toml"), &wire()).is_some());
    }

    #[test]
    fn a_gemini_shim_or_socket_path_with_a_dollar_gives_no_wiring() {
        // gemini expands `$VAR` in the strings of settings.json.
        assert!(gemini(Path::new("/o/$HOME/lb"), Path::new("/p"), &wire()).is_none());
        let mut w = wire();
        w.socket = PathBuf::from("/run/${X}/s.sock");
        assert!(gemini(Path::new("/s"), Path::new("/p"), &w).is_none());
    }
}
