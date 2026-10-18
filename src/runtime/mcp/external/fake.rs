//! A fake MCP server for the tests of this slice: a shell script that speaks
//! just enough of the protocol over its pipes.

use std::{ffi::OsString, path::PathBuf, time::Duration};

use crate::core::ai::{config::McpServer, tools::mcp::PROTOCOL_VERSION};

use super::super::client::Timeouts;

/// Answers `initialize` and `tools/list` (one tool, named `$2`), then answers
/// every later request with the text `$3` after recording its line in `$4`.
/// `$5` is a delay in seconds before each answer (a hung server).
const SCRIPT: &str = "echo $$ > \"$4.pid\"; \
    read l; printf '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"protocolVersion\":\"%s\"}}\\n' \"$1\"; \
    read l; read l; \
    printf '{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[{\"name\":\"%s\",\"inputSchema\":{\"type\":\"object\"}}]}}\\n' \"$2\"; \
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
        let calls = self.calls(name);
        let args = [
            "-c",
            SCRIPT,
            "sh",
            PROTOCOL_VERSION,
            tool,
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

pub(crate) fn host_env(name: &str) -> Option<OsString> {
    (name == "PATH").then(|| OsString::from("/usr/bin:/bin"))
}

pub(crate) fn timeouts(call: Duration) -> Timeouts {
    Timeouts {
        handshake: Duration::from_secs(10),
        call,
    }
}
