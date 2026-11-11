//! A curated catalog of existing MCP servers (`mcp-catalog`, C2: lazarobox
//! reimplements none of them). Pure static data, no IO. A server is a catalog
//! entry only when the user declared it under the entry's id in
//! `[[ai.mcp.servers]]`; the catalog never adds, removes or rewrites a declared
//! server, and it never writes `config.toml` or `secrets.toml` (C4, C6).
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
    /// Opt-out of confidentiality: a `secret` value is confidential (never
    /// drawn) unless this is set (D23).
    pub shown: bool,
    /// A value the template fills in (`GSC_SKIP_OAUTH = "true"`).
    pub preset: Option<&'static str>,
    /// The preset is a path under `$HOME`, written absolute (a server may not
    /// expand `~`).
    pub under_home: bool,
}

impl Var {
    /// A confidential `secrets.toml` value with no preset.
    pub const SECRET: Var = Var {
        name: "",
        secret: true,
        shown: false,
        preset: None,
        under_home: false,
    };

    /// Whether the value is never drawn.
    pub fn confidential(&self) -> bool {
        self.secret && !self.shown
    }

    /// The preset to save for `home`: a path preset becomes absolute, and
    /// none is given for a `home` that is not absolute.
    pub fn preset_for(&self, home: &str) -> Option<String> {
        let preset = self.preset?;
        if !self.under_home {
            return Some(preset.to_owned());
        }
        home.starts_with('/')
            .then(|| format!("{}/{preset}", home.trim_end_matches('/')))
    }
}

/// One way to fill a service's values, with its own presets and login.
#[derive(Clone, Copy, Debug)]
pub struct Way {
    pub title: &'static str,
    pub vars: &'static [Var],
    pub login: Option<Login>,
}

impl Way {
    /// Each name this way fills and the value to save for `home`; empty when
    /// the user types it.
    pub fn values(&self, home: &str) -> Vec<(&'static str, String)> {
        let value = |v: &Var| v.preset_for(home).unwrap_or_default();
        self.vars.iter().map(|v| (v.name, value(v))).collect()
    }
}

/// How the user links the account.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Link {
    None,
    /// A login line typed in the pane.
    Login,
    /// The link session calls `start_google_auth` on the server.
    GoogleAuth,
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
    /// Every name the server gets: the union of the ways' names.
    pub env: &'static [Var],
    /// Alternative ways to fill `env`; empty for a single-way entry.
    pub ways: &'static [Way],
    pub login: Option<Login>,
    pub link: Link,
    /// Tools never registered in a chat (by server name).
    pub hidden: &'static [&'static str],
    /// Only the Gmail tools of [`GMAIL_ALLOWLIST`] are registered.
    pub gmail_allowlist: bool,
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
    /// The names the user still has to fill. With ways, the first way the
    /// table completes decides (so an S5 service-account table is complete),
    /// else the first (default) way names what is missing. Without ways, the
    /// names the user types.
    pub fn missing(&self, has: impl Fn(&str) -> bool) -> Vec<&'static str> {
        let unset = |vars: &'static [Var], typed_only: bool| -> Vec<&'static str> {
            vars.iter()
                .filter(|v| v.secret && !(typed_only && v.preset.is_some()))
                .filter(|v| !has(v.name))
                .map(|v| v.name)
                .collect()
        };
        let mut per_way = self.ways.iter().map(|w| unset(w.vars, false));
        match per_way.next() {
            None => unset(self.env, true),
            Some(first) if first.is_empty() => first,
            Some(first) => per_way.find(Vec::is_empty).unwrap_or(first),
        }
    }

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

/// The Gmail tools a declared `google` registers (C9: read and draft only).
pub const GMAIL_ALLOWLIST: [&str; 8] = [
    "search_gmail_messages",
    "get_gmail_message_content",
    "get_gmail_messages_content_batch",
    "get_gmail_attachment_content",
    "draft_gmail_message",
    "get_gmail_thread_content",
    "get_gmail_threads_content_batch",
    "list_gmail_labels",
];

/// Whether lazarobox registers `tool` of `server`: not one the entry hides,
/// and, for an entry with the Gmail allowlist, no Gmail tool (name containing
/// `gmail`, ASCII case-insensitive) outside it. Keyed by the server's name; a
/// server of another name is untouched. It runs before the 64-tool cap.
pub fn admits(server: &McpServer, tool: &str) -> bool {
    let Some(entry) = entry(&server.name) else {
        return true;
    };
    let gmail = entry.gmail_allowlist
        && tool.to_ascii_lowercase().contains("gmail")
        && !GMAIL_ALLOWLIST.contains(&tool);
    !(gmail || entry.hidden.contains(&tool))
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
            quoted(v.preset.filter(|_| !v.under_home).unwrap_or(""))
        ));
    }
    Some(out)
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod workspace_tests;
