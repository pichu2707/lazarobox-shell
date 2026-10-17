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
}
