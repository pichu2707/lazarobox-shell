//! A curated catalog of existing MCP servers (`mcp-catalog`, C2: lazarobox
//! reimplements none of them). Pure static data, no IO. A server is a catalog
//! entry only when the user declared it under the entry's id in
//! `[[ai.mcp.servers]]`; the catalog never adds, removes or rewrites a declared
//! server, and it never writes `config.toml` or `secrets.toml` (C4, C6): the
//! one write to `secrets.toml` is the menu save in `runtime::secrets_write`.
//!
//! The read/write class of a tool is recorded here and read by nobody in
//! approval: every call to a connector tool still needs the user's `y` (C11).

use crate::core::ai::config::McpServer;

pub mod checks;
mod entries;
pub mod presence;

/// Where an entry sits in the Connectors area.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    Analytics,
    Workspace,
}

/// What a tool does to the user's data (C11). Recorded only.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    Read,
    Write,
}

/// One variable the server names in `env`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Var {
    pub name: &'static str,
    /// Whether its value belongs in `secrets.toml`.
    pub secret: bool,
    /// A value the template fills in (`GSC_SKIP_OAUTH = "true"`).
    pub preset: Option<&'static str>,
}

/// The command typed in the login pane. Static text, never user or server
/// text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Login {
    pub line: &'static str,
}

/// One catalog entry.
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    /// A valid server name: what the user declares.
    pub id: &'static str,
    /// The service name the area shows.
    pub title: &'static str,
    pub group: Group,
    /// The server's project and license.
    pub project: &'static str,
    pub license: &'static str,
    /// The pinned install command: a binary installed beforehand, never a
    /// first-run download (it could exceed the 20 s handshake).
    pub install: &'static str,
    pub command: &'static str,
    pub args: &'static [&'static str],
    pub env: &'static [Var],
    pub login: Option<Login>,
    /// The guide, one step or warning per line.
    pub guide: &'static [&'static str],
    /// The guide links, `https://` only.
    pub links: &'static [&'static str],
    /// Where the server keeps its token.
    pub token_location: &'static str,
    /// The known tools and their class.
    pub tools: &'static [(&'static str, Class)],
}

/// The catalog, in the order of C8.
pub fn entries() -> &'static [Entry] {
    entries::ENTRIES
}

/// The entry a declared server is: its `name` equals the entry id exactly.
pub fn entry(name: &str) -> Option<&'static Entry> {
    entries().iter().find(|e| e.id == name)
}

impl Entry {
    /// The class of a tool; one the catalog does not know is `Write` (the
    /// safe default).
    pub fn class(&self, tool: &str) -> Class {
        self.tools
            .iter()
            .find(|(name, _)| *name == tool)
            .map_or(Class::Write, |(_, class)| *class)
    }
}

/// The class of `tool` on `server`: the entry's, or `Write` when the server is
/// not an entry or the tool is unknown.
pub fn class(server: &str, tool: &str) -> Class {
    entry(server).map_or(Class::Write, |e| e.class(tool))
}

/// Whether lazarobox registers `tool` of `server`. Every entry admits every
/// tool until a slice sets a filter flag (S6, the Gmail filter).
pub fn admits(server: &McpServer, tool: &str) -> bool {
    let _ = (server, tool);
    true
}

/// A TOML basic string.
fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

fn list(items: impl Iterator<Item = String>) -> String {
    format!("[{}]", items.collect::<Vec<_>>().join(", "))
}

/// The `[[ai.mcp.servers]]` entry to paste into `config.toml`. `args` and
/// `env` are left out when empty.
pub fn snippet(entry: &Entry) -> String {
    let mut out = format!(
        "[[ai.mcp.servers]]\nname = {}\ncommand = {}\n",
        quoted(entry.id),
        quoted(entry.command)
    );
    if !entry.args.is_empty() {
        let args = list(entry.args.iter().map(|a| quoted(a)));
        out.push_str(&format!("args = {args}\n"));
    }
    if !entry.env.is_empty() {
        let env = list(entry.env.iter().map(|v| quoted(v.name)));
        out.push_str(&format!("env = {env}\n"));
    }
    out
}

/// The `secrets.toml` table to fill, or `None` when no variable is secret.
pub fn secrets_template(entry: &Entry) -> Option<String> {
    let mut vars = entry.env.iter().filter(|v| v.secret).peekable();
    vars.peek()?;
    let mut out = format!("[{}]\n", entry.id);
    for v in vars {
        out.push_str(&format!(
            "{} = {}\n",
            v.name,
            quoted(v.preset.unwrap_or(""))
        ));
    }
    Some(out)
}

#[cfg(test)]
mod tests;
