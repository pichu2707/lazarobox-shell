//! The private home of a codex or gemini turn.
//!
//! Both CLIs read the user's own instructions and skills from their state
//! folder (`~/.codex/AGENTS.md`, `~/.gemini/GEMINI.md`, `~/.gemini/skills/`),
//! and neither has a flag that stops it (`codex --ignore-user-config` only
//! skips `config.toml`). The state folder is what they locate through
//! `$CODEX_HOME` and `$GEMINI_CLI_HOME`, so a turn points them at a folder
//! inside its own wiped temp folder that holds only what the login needs:
//! symlinks to the user's login files (never copies, so a login is not
//! duplicated and the user's folder is neither moved nor modified), and for
//! gemini the one setting that says which login to use. A login refresh that
//! writes the file in place follows the link to the real file; one that
//! renames a new file over the link stays in the wiped folder.

use std::{
    ffi::{OsStr, OsString},
    fs,
    io::{self, Read},
    os::unix::fs::{DirBuilderExt, symlink},
    path::{Path, PathBuf},
};

use crate::{
    core::ai::{codex_install::codex_home_of, jail::Cli},
    runtime::jail::resolve,
};

use super::{CliEnv, write_private};

/// The private `CODEX_HOME`, inside the turn's temp folder.
const CODEX_HOME_DIR: &str = "codex-home";
/// The private `GEMINI_CLI_HOME`, inside the turn's temp folder. gemini keeps
/// its state in `<GEMINI_CLI_HOME>/.gemini`.
const GEMINI_HOME_DIR: &str = "gemini-home";

/// The login files codex reads from its state folder.
const CODEX_LOGIN: [&str; 1] = ["auth.json"];
/// The login files gemini reads from its state folder: OAuth credentials, the
/// cached account, the file-based credential store and the `.env` that can
/// hold an API key.
const GEMINI_LOGIN: [&str; 4] = [
    "oauth_creds.json",
    "google_accounts.json",
    "gemini-credentials.json",
    ".env",
];

/// The most of the user's gemini `settings.json` that is read.
const SETTINGS_CAP: usize = 1 << 20;

/// What a chat turn gets for its CLI.
#[derive(Debug, Default)]
pub(super) struct Prepared {
    /// The variables that point the CLI at its private home.
    pub vars: Vec<(OsString, OsString)>,
    /// What the user is told before the turn, when the CLI will probably lack
    /// a login or a setting.
    pub notice: Option<String>,
}

/// The private home of `cli`, which `tmp` (the turn's private folder) holds.
/// Empty for a CLI that is isolated by flags (claude).
pub(super) fn prepare(env: &CliEnv, cli: Cli, tmp: &Path) -> io::Result<Prepared> {
    let names: Vec<OsString> = std::env::vars_os().map(|(name, _)| name).collect();
    prepare_with(env, cli, tmp, &names)
}

/// [`prepare`], given the names of the variables that count as the user's
/// environment (the process's, and what the turn injects).
pub(super) fn prepare_with(
    env: &CliEnv,
    cli: Cli,
    tmp: &Path,
    process_vars: &[OsString],
) -> io::Result<Prepared> {
    match cli {
        Cli::Claude => Ok(Prepared::default()),
        Cli::Codex => codex(env, tmp),
        Cli::Gemini => gemini(env, tmp, process_vars),
    }
}

fn codex(env: &CliEnv, tmp: &Path) -> io::Result<Prepared> {
    let home = tmp.join(CODEX_HOME_DIR);
    private_dir(&home)?;
    let real = codex_home_of(env.codex_home.as_deref(), env.home.as_deref());
    let linked = link_login(real.as_deref(), &home, &CODEX_LOGIN)?;
    let notice = (linked == 0).then(|| {
        let at = real.map_or_else(
            || "~/.codex/auth.json".to_owned(),
            |dir| shown(&dir.join(CODEX_LOGIN[0]), env.home.as_deref()),
        );
        format!("codex has no login at {at}; run codex login")
    });
    Ok(Prepared {
        vars: vec![("CODEX_HOME".into(), home.into_os_string())],
        notice,
    })
}

fn gemini(env: &CliEnv, tmp: &Path, process_vars: &[OsString]) -> io::Result<Prepared> {
    // A `$GEMINI_CLI_HOME` of the user's is where the login lives, resolved as
    // the jail does; one that cannot be resolved is not guessed at.
    let root = match env.gemini_home.as_deref() {
        Some(set) => Some(resolve(set).ok_or_else(|| {
            io::Error::other("$GEMINI_CLI_HOME does not exist or cannot be resolved")
        })?),
        None => env.home.clone(),
    };
    let state = root.map(|r| r.join(".gemini"));
    let private_root = tmp.join(GEMINI_HOME_DIR);
    let home = private_root.join(".gemini");
    private_dir(&home)?;
    let linked = link_login(state.as_deref(), &home, &GEMINI_LOGIN)?;
    let from_env = env
        .inject
        .iter()
        .map(|(name, _)| name)
        .chain(process_vars)
        .any(|name| is_login_variable(name));
    let mut notes = Vec::new();
    match state.as_deref().map(auth_choice) {
        Some(Ok(kind)) => {
            let settings = serde_json::json!({"security": {"auth": {"selectedType": kind}}});
            write_private(&home.join("settings.json"), settings.to_string().as_bytes())?;
        }
        // With an API key in the environment, no auth type is not a problem.
        Some(Err(_)) if from_env => {}
        Some(Err(why)) => notes.push(format!("gemini auth type not read: {why}")),
        None => notes.push("gemini auth type not read: no home folder".to_owned()),
    }
    if linked == 0 && !from_env {
        let at = state.map_or_else(
            || "~/.gemini".to_owned(),
            |d| shown(&d, env.home.as_deref()),
        );
        notes.push(format!(
            "gemini has no login in {at} and no GEMINI_API_KEY or GOOGLE_* in the \
             environment; run gemini to log in"
        ));
    }
    Ok(Prepared {
        vars: vec![("GEMINI_CLI_HOME".into(), private_root.into_os_string())],
        notice: (!notes.is_empty()).then(|| notes.join("; ")),
    })
}

/// A variable that logs gemini in.
fn is_login_variable(name: &OsStr) -> bool {
    name == "GEMINI_API_KEY" || name.as_encoded_bytes().starts_with(b"GOOGLE_")
}

/// `path` with the home folder as `~`, for a message.
fn shown(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// A new folder, 0700 (and its parents, 0700 too).
fn private_dir(path: &Path) -> io::Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
}

/// Links each login file the user has in `real` into `home`; how many.
fn link_login(real: Option<&Path>, home: &Path, files: &[&str]) -> io::Result<usize> {
    let Some(real) = real else { return Ok(0) };
    let mut linked = 0;
    for name in files {
        let source: PathBuf = real.join(name);
        if fs::symlink_metadata(&source).is_ok() {
            symlink(&source, home.join(name))?;
            linked += 1;
        }
    }
    Ok(linked)
}

/// Which login gemini is set to use (`security.auth.selectedType`), the only
/// part of the user's settings a turn needs, or why it cannot be read.
fn auth_choice(state: &Path) -> Result<String, String> {
    let mut text = String::new();
    let file = fs::File::open(state.join("settings.json")).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => "settings.json does not exist".to_owned(),
        kind => format!("settings.json cannot be opened ({kind})"),
    })?;
    file.take(SETTINGS_CAP as u64 + 1)
        .read_to_string(&mut text)
        .map_err(|e| format!("settings.json cannot be read ({})", e.kind()))?;
    if text.len() > SETTINGS_CAP {
        return Err("settings.json is larger than 1 MiB".to_owned());
    }
    let settings: serde_json::Value = serde_json::from_str(&strip_comments(&text))
        .map_err(|_| "settings.json is not valid JSON".to_owned())?;
    settings
        .pointer("/security/auth/selectedType")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "settings.json has no security.auth.selectedType".to_owned())
}

/// `text` without `//` and `/* */` comments outside strings (gemini reads its
/// settings as JSON with comments). A comment becomes whitespace, so a line
/// break stays.
pub(super) fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            match c {
                '\\' => out.extend(chars.next()),
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match (c, chars.peek()) {
            ('"', _) => {
                in_string = true;
                out.push(c);
            }
            ('/', Some('/')) => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut last = ' ';
                for next in chars.by_ref() {
                    if last == '*' && next == '/' {
                        break;
                    }
                    if next == '\n' {
                        out.push('\n');
                    }
                    last = next;
                }
                out.push(' ');
            }
            _ => out.push(c),
        }
    }
    out
}
