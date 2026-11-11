//! A fake MCP server for the tests of this slice: a shell script that speaks
//! just enough of the protocol over its pipes.

use std::{ffi::OsString, path::PathBuf, time::Duration};

use crate::core::ai::{config::McpServer, tools::mcp::PROTOCOL_VERSION};

use super::super::client::Timeouts;

/// Answers `initialize` and `tools/list` (the JSON objects of `$2`), then answers
/// every later request with the text `$3` after recording its line in `$4`.
/// `$5` is a delay in seconds before each answer (a hung server).
const SCRIPT: &str = "echo $$ > \"$4.pid\"; \
    read l; printf '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"protocolVersion\":\"%s\"}}\\n' \"$1\"; \
    read l; read l; \
    printf '{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[%s]}}\\n' \"$2\"; \
    n=3; while read l; do echo \"$l\" >> \"$4\"; sleep \"$5\"; \
    printf '{\"jsonrpc\":\"2.0\",\"id\":%s,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"%s\"}]}}\\n' \"$n\" \"$3\"; \
    n=$((n+1)); done";

pub(crate) struct Lab {
    pub home: PathBuf,
    _root: tempfile::TempDir,
}

pub(crate) fn lab() -> Lab {
    let root = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let home = std::fs::canonicalize(root.path()).unwrap().join("home");
    std::fs::create_dir_all(&home).unwrap();
    Lab { home, _root: root }
}

impl Lab {
    /// Where the fake `name` records the requests it got after the handshake.
    pub fn calls(&self, name: &str) -> PathBuf {
        self.home.join(format!("{name}.calls"))
    }

    /// The text of the requests the fake `name` got, empty when none.
    pub fn received(&self, name: &str) -> String {
        std::fs::read_to_string(self.calls(name)).unwrap_or_default()
    }

    /// A server `name` with one tool `tool` that answers every call `text`.
    pub fn server(&self, name: &str, tool: &str, text: &str) -> McpServer {
        self.slow(name, tool, text, "0")
    }

    /// Like [`Lab::server`], but waits `delay` seconds before each answer.
    pub fn slow(&self, name: &str, tool: &str, text: &str, delay: &str) -> McpServer {
        self.listing(name, &[tool], text, delay)
    }

    /// A server `name` that lists `tools`, each a tool of its own.
    pub fn listing(&self, name: &str, tools: &[&str], text: &str, delay: &str) -> McpServer {
        let calls = self.calls(name);
        let tools = tools
            .iter()
            .map(|t| format!(r#"{{"name":"{t}","inputSchema":{{"type":"object"}}}}"#))
            .collect::<Vec<_>>()
            .join(",");
        let args = [
            "-c",
            SCRIPT,
            "sh",
            PROTOCOL_VERSION,
            &tools,
            text,
            calls.to_str().unwrap(),
            delay,
        ];
        McpServer {
            name: name.to_owned(),
            command: "/bin/sh".to_owned(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            env: Vec::new(),
            project: None,
            enabled: true,
        }
    }
}

impl Lab {
    /// A server that starts and then never answers `initialize`.
    pub fn mute(&self, name: &str) -> McpServer {
        let mut server = self.server(name, "ping", "pong");
        server.args[1] = "echo $$ > \"$4.pid\"; sleep 600".to_owned();
        server
    }

    /// A server that says its own `CLARITY_API_TOKEN` on stderr and dies.
    pub fn noisy(&self, name: &str) -> McpServer {
        let mut server = self.server(name, "ping", "pong");
        server.args[1] = "echo \"the token is $CLARITY_API_TOKEN, sorry\" >&2; exit 1".to_owned();
        server.env = vec!["CLARITY_API_TOKEN".to_owned()];
        server
    }

    /// A server that says `text` on stderr (verbatim: a single
    /// quote is not allowed) and dies; it is given `CLARITY_API_TOKEN`.
    pub fn says(&self, name: &str, text: &str) -> McpServer {
        let mut server = self.server(name, "ping", "pong");
        server.args[1] = format!("printf '%s\\n' '{text}' >&2; exit 1");
        server.env = vec!["CLARITY_API_TOKEN".to_owned()];
        server
    }

    /// A server that answers `initialize` and then `tools/list` with an error
    /// whose text is `pad` characters and then its `CLARITY_API_TOKEN`: the 200
    /// character cut of a message would show the start of the token.
    pub fn leaky(&self, name: &str, pad: usize) -> McpServer {
        let mut server = self.server(name, "ping", "pong");
        let pad = "x".repeat(pad);
        server.args[1] = format!(
            "read l; printf '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{\"protocolVersion\":\"%s\"}}}}\\n' \"$1\"; \
             read l; read l; \
             printf '{{\"jsonrpc\":\"2.0\",\"id\":2,\"error\":{{\"code\":-1,\"message\":\"{pad}%s\"}}}}\\n' \"$CLARITY_API_TOKEN\"; sleep 5"
        );
        server.env = vec!["CLARITY_API_TOKEN".to_owned()];
        server
    }

    /// Like [`Lab::server`], naming `env`, and first writing its whole
    /// environment to [`Lab::env_of`] (a recorder).
    pub fn recorder(&self, name: &str, env: &[&str]) -> McpServer {
        let mut server = self.server(name, "ping", "pong");
        let out = self.home.join(format!("{name}.env"));
        server.args[1] = format!("env > '{}'; {SCRIPT}", out.display());
        server.env = env.iter().map(|n| (*n).to_owned()).collect();
        server
    }

    /// The environment the recorder `name` was started with, one `K=V` a line.
    pub fn env_of(&self, name: &str) -> Vec<String> {
        let text =
            std::fs::read_to_string(self.home.join(format!("{name}.env"))).unwrap_or_default();
        text.lines().map(str::to_owned).collect()
    }
}

pub(crate) fn host_env(name: &str) -> Option<OsString> {
    (name == "PATH").then(|| OsString::from("/usr/bin:/bin"))
}

pub(crate) fn timeouts(call: Duration) -> Timeouts {
    Timeouts {
        handshake: Duration::from_secs(10),
        call,
    }
}
