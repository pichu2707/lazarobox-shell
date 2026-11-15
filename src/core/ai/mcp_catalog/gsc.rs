//! The `gsc` OAuth way (S6b1): when a chat or probe start is refused, whether
//! the account is linked, and the login line of the Link action. Pure: the
//! values and the `stat` are parameters, and nothing here opens a token file.

use std::path::Path;

use crate::core::ai::{
    config::McpServer,
    sanitize::{clean, is_format},
};
use crate::core::opener::leading_args;

pub const UNFILLED: &str = "fill the OAuth values in Connectors (f)";
pub const NOT_LINKED: &str = "not linked; open Connectors and use Link with Google";
pub const FILL_FIRST: &str = "fill the OAuth values first (f)";
pub const PASS_DIR: &str =
    "the declared gsc entry does not pass GSC_CONFIG_DIR; paste the current entry";
pub const PASS_FILE: &str =
    "the declared gsc entry does not pass GSC_OAUTH_CLIENT_SECRETS_FILE; paste the current entry";
pub const OUTSIDE: &str =
    "GSC_CONFIG_DIR must be under ~/.config/mcp-gsc; fix it in Connectors (f)";
pub const BAD_PATH: &str = "a path holds a character the command cannot carry; choose another (f)";
pub const NO_TOOL_DIR: &str = "uv tool dir did not print one absolute folder";
pub const NO_UV: &str = "uv not found: install uv first";
pub const FIX: &str = "uv tool install --force mcp-search-console==0.4.2";

/// The folder and the client file the link line and the server both use.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Plan {
    pub dir: String,
    pub file: String,
}

/// The values a declared `gsc` would get, by the names its `env` lists (a
/// name it does not list never reaches the server, whatever the host has).
struct Got {
    skip: String,
    file: String,
    dir: String,
}

impl Got {
    fn of(server: &McpServer, get: &impl Fn(&str) -> Option<String>) -> Self {
        let own = |name: &str| {
            let listed = server.env.iter().any(|n| n == name);
            listed.then(|| get(name)).flatten().unwrap_or_default()
        };
        Self {
            skip: own("GSC_SKIP_OAUTH"),
            file: own("GSC_OAUTH_CLIENT_SECRETS_FILE"),
            dir: own("GSC_CONFIG_DIR"),
        }
    }

    /// The server would start its browser flow when there is no token.
    fn oauth(&self) -> bool {
        // The server compares `.lower()` with no trim: a padded value is not
        // a skip there, so it is not one here (fail closed).
        let skip = self.skip.to_ascii_lowercase();
        !self.file.is_empty() && !matches!(skip.as_str(), "true" | "1" | "yes")
    }

    /// `<GSC_CONFIG_DIR>/token.json`, only for an absolute folder.
    fn token(&self) -> Option<std::path::PathBuf> {
        let dir = Path::new(&self.dir);
        dir.is_absolute().then(|| dir.join("token.json"))
    }
}

/// `Some(linked)` for a declared `gsc` whose values select the OAuth way;
/// `exists` is a `stat`, the file is never opened.
pub fn linked(
    server: &McpServer,
    get: &impl Fn(&str) -> Option<String>,
    exists: impl Fn(&Path) -> bool,
) -> Option<bool> {
    let got = Got::of(server, get);
    (server.name == "gsc" && got.oauth()).then(|| got.token().is_some_and(|token| exists(&token)))
}

/// Why a start of `server` is refused, if it is: with no usable folder the
/// server would look for its token elsewhere, with a folder the tools could
/// read the token would be exposed to the model, and with no token any call
/// starts a browser flow inside the call.
pub fn start_refusal(
    server: &McpServer,
    home: &Path,
    get: &impl Fn(&str) -> Option<String>,
    exists: impl Fn(&Path) -> bool,
) -> Option<&'static str> {
    let got = Got::of(server, get);
    if server.name != "gsc" || !got.oauth() {
        return None;
    }
    match got.token() {
        None => Some(UNFILLED),
        Some(_) if !denied(Path::new(&got.dir), home) => Some(OUTSIDE),
        Some(token) => (!exists(&token)).then_some(NOT_LINKED),
    }
}

/// Whether the model's tools already refuse to read `dir`: the default token
/// folder of mcp-gsc on Linux or macOS, or a folder inside it. A `..` is
/// refused (the path is not resolved here).
pub fn denied(dir: &Path, home: &Path) -> bool {
    let inside = |rel: &str| dir.starts_with(home.join(rel));
    let plain = !dir
        .components()
        .any(|c| c == std::path::Component::ParentDir);
    plain && (inside(".config/mcp-gsc") || inside("Library/Application Support/mcp-gsc"))
}

/// What the link line is built from, or why there is none: the declared
/// entry must pass both paths, and both must be filled, absolute, and not
/// skip the OAuth flow.
pub fn plan(
    server: &McpServer,
    home: &Path,
    get: &impl Fn(&str) -> Option<String>,
) -> Result<Plan, &'static str> {
    let lists = |name: &str| server.env.iter().any(|n| n == name);
    if !lists("GSC_OAUTH_CLIENT_SECRETS_FILE") {
        return Err(PASS_FILE);
    }
    if !lists("GSC_CONFIG_DIR") {
        return Err(PASS_DIR);
    }
    let got = Got::of(server, get);
    if !got.oauth() || got.token().is_none() || !Path::new(&got.file).is_absolute() {
        return Err(FILL_FIRST);
    }
    if !denied(Path::new(&got.dir), home) {
        return Err(OUTSIDE);
    }
    Ok(Plan {
        dir: got.dir,
        file: got.file,
    })
}

/// The one absolute folder `uv tool dir` printed.
pub fn tool_dir(out: &str) -> Option<String> {
    let dir = out.trim();
    (Path::new(dir).is_absolute() && carries(dir)).then(|| dir.to_owned())
}

/// `Ok` when `uv tool list` shows the pinned version, else the notice with
/// what was found (sanitized, at most 20 characters).
pub fn pinned(list: &str) -> Result<(), String> {
    const NAME: &str = "mcp-search-console v";
    let found = list.lines().find_map(|l| l.trim().strip_prefix(NAME));
    let version = found.map(|rest| rest.split_whitespace().next().unwrap_or(""));
    match version {
        Some("0.4.2") => Ok(()),
        Some(other) => {
            let other: String = clean(other).chars().take(20).collect();
            Err(format!("mcp-search-console {other}; run: {FIX}"))
        }
        None => Err(format!("mcp-search-console not installed; run: {FIX}")),
    }
}

/// A value the line can carry inside single quotes: no control character, no
/// Unicode format character (they reorder or hide text in the prompt), no `$(`.
fn carries(value: &str) -> bool {
    !value.chars().any(|c| c.is_control() || is_format(c)) && !value.contains("$(")
}

fn quoted(value: &str) -> Result<String, &'static str> {
    carries(value)
        .then(|| format!("'{}'", value.replace('\'', "'\\''")))
        .ok_or(BAD_PATH)
}

/// The login line, from resolved values, each single-quoted. It runs the
/// server's own OAuth function outside MCP, with the scrubbed environment the
/// chat server gets, so the token lands where the chat looks for it.
///
/// `opener` is the program lazarobox opens links with (a name from
/// [`candidates`](crate::core::opener::candidates)), passed as `BROWSER`:
/// the OAuth flow opens the browser through Python's `webbrowser`, which on
/// WSL picks `xdg-open` and fails silently when no Linux browser is
/// installed. With `None`, Python picks the browser as it would.
pub fn login_line(
    tool_dir: &str,
    plan: &Plan,
    opener: Option<&str>,
) -> Result<String, &'static str> {
    let python = quoted(&format!("{tool_dir}/mcp-search-console/bin/python"))?;
    let browser = match opener {
        Some(name) => {
            let mut words = vec![name];
            words.extend(leading_args(name));
            words.push("%s");
            format!("BROWSER={} ", quoted(&words.join(" "))?)
        }
        None => String::new(),
    };
    Ok(format!(
        "env -u XDG_CONFIG_HOME -u XDG_DATA_HOME -u GSC_ALLOW_DESTRUCTIVE -u GSC_SKIP_OAUTH {browser}GSC_CONFIG_DIR={} GSC_OAUTH_CLIENT_SECRETS_FILE={} {python} -I -c 'import os; os.umask(0o077); import gsc_server as g; g.get_gsc_service_oauth(); print(\"Search Console linked\")'",
        quoted(&plan.dir)?,
        quoted(&plan.file)?,
    ))
}

#[cfg(test)]
mod tests;
