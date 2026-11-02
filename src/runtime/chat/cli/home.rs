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
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, symlink},
    path::{Path, PathBuf},
};

use crate::{
    core::ai::{
        codex_install::codex_home_of,
        jail::{Cli, broad_place, is_canonical_form},
    },
    runtime::jail::resolve,
};

use super::{CliEnv, write_private};

/// The private `CODEX_HOME`, inside the turn's temp folder.
const CODEX_HOME_DIR: &str = "codex-home";
/// The private `GEMINI_CLI_HOME`, inside the turn's temp folder. gemini keeps
/// its state in `<GEMINI_CLI_HOME>/.gemini`.
const GEMINI_HOME_DIR: &str = "gemini-home";
/// The folder gemini starts in, inside the turn's temp folder.
const GEMINI_CWD_DIR: &str = "gemini-cwd";

/// The login files codex reads from its state folder.
const CODEX_LOGIN: [&str; 1] = ["auth.json"];
/// The login files gemini reads from its state folder: OAuth credentials, the
/// cached account, the file-based credential store and the `.env` that can
/// hold an API key.
const GEMINI_LOGIN: [&str; 3] = [
    "oauth_creds.json",
    "google_accounts.json",
    "gemini-credentials.json",
];

/// The only keys of the user's `~/.gemini/.env` that reach gemini. gemini sets
/// every key of a `.env` that is not yet in its environment, so a link would
/// undo the scrub of variables such as `GEMINI_SYSTEM_MD`.
const DOTENV_KEYS: [&str; 7] = [
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
    "GOOGLE_GENAI_USE_VERTEXAI",
    "GOOGLE_GENAI_USE_GCA",
    "GOOGLE_CLOUD_PROJECT",
    "GOOGLE_CLOUD_LOCATION",
    "GOOGLE_APPLICATION_CREDENTIALS",
];
/// The keys of [`DOTENV_KEYS`] that log gemini in.
const DOTENV_LOGIN: [&str; 5] = [
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
    "GOOGLE_GENAI_USE_VERTEXAI",
    "GOOGLE_GENAI_USE_GCA",
    "GOOGLE_APPLICATION_CREDENTIALS",
];
/// The most of the user's `.env` that is read.
const DOTENV_CAP: u64 = 64 * 1024;
/// The longest path shown in a notice.
const PATH_CAP: usize = 160;

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
    /// The folder gemini runs in when it is not the shared empty one, holding
    /// the private `.env` that stops gemini's walk upwards for one.
    pub cwd: Option<PathBuf>,
    /// The text of that `.env` (gemini only).
    pub dotenv: Option<String>,
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
        ..Prepared::default()
    })
}

fn gemini(env: &CliEnv, tmp: &Path, process_vars: &[OsString]) -> io::Result<Prepared> {
    // A `$GEMINI_CLI_HOME` of the user's is where the login lives, resolved as
    // the jail does; one that cannot be resolved is not guessed at.
    let root = match env.gemini_home.as_deref() {
        Some(set) => Some(checked_gemini_home(set, env.home.as_deref()).map_err(io::Error::other)?),
        None => env.home.clone(),
    };
    let state = root.map(|r| r.join(".gemini"));
    let private_root = tmp.join(GEMINI_HOME_DIR);
    let home = private_root.join(".gemini");
    private_dir(&home)?;
    let mut linked = link_login(state.as_deref(), &home, &GEMINI_LOGIN)?;
    let mut notes = Vec::new();
    // The `.env` is rewritten with the allowlisted keys, never linked.
    let (dotenv, env_has_login) = match state.as_deref().map(|d| read_dotenv(&d.join(".env"))) {
        Some(Ok(parsed)) => parsed,
        Some(Err(why)) => {
            notes.push(format!("gemini .env skipped: {why}"));
            (String::new(), false)
        }
        None => (String::new(), false),
    };
    write_private(&home.join(".env"), dotenv.as_bytes())?;
    // gemini looks for a `.env` from its working folder upwards, and would
    // reach the user's real home: a private one in the folder it starts in
    // stops the walk there.
    let cwd = tmp.join(GEMINI_CWD_DIR);
    private_dir(&cwd)?;
    write_private(&cwd.join(".env"), dotenv.as_bytes())?;
    linked += usize::from(env_has_login);
    let from_env = env
        .inject
        .iter()
        .map(|(name, _)| name)
        .chain(process_vars)
        .any(|name| is_login_variable(name));
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
        cwd: Some(cwd),
        dotenv: Some(dotenv),
    })
}

/// The user's `$GEMINI_CLI_HOME`, resolved, or why it cannot be trusted. Both
/// the jail layout and the private home read through it, so a hostile value
/// is refused before any grant or link is made. It must be an absolute path
/// without `.` or `..`, a folder of the user's that others cannot write, not
/// `/` and not a broad place, and hold a real `.gemini` folder of the user's.
pub(super) fn checked_gemini_home(set: &Path, home: Option<&Path>) -> Result<PathBuf, String> {
    let bad = |why: &str| format!("$GEMINI_CLI_HOME {why}");
    if !is_canonical_form(set) {
        return Err(bad("must be an absolute path without . or .. parts"));
    }
    let dir = resolve(set).ok_or_else(|| bad("does not exist or cannot be resolved"))?;
    if dir == Path::new("/") {
        return Err(bad("must not be /"));
    }
    // SAFETY: geteuid has no preconditions and cannot fail.
    let me = unsafe { libc::geteuid() };
    let meta = fs::metadata(&dir).map_err(|_| bad("cannot be read"))?;
    if !meta.is_dir() || meta.uid() != me {
        return Err(bad("must be a folder of yours"));
    }
    if meta.mode() & 0o022 != 0 {
        return Err(bad("must not be writable by group or others"));
    }
    let state =
        fs::symlink_metadata(dir.join(".gemini")).map_err(|_| bad("has no .gemini folder"))?;
    if !state.is_dir() || state.uid() != me {
        return Err(bad("must hold a real .gemini folder of yours"));
    }
    let real_home = home.and_then(resolve);
    // The user's own home is the default; any other broad place is refused.
    if Some(&dir) != real_home.as_ref() {
        let homes: Vec<&Path> = home.into_iter().chain(real_home.as_deref()).collect();
        if broad_place(&dir, &homes) {
            return Err(bad("is too broad a place"));
        }
    }
    Ok(dir)
}

/// The allowlisted part of the user's `.env`, rewritten as `KEY='value'`
/// lines, and whether it holds a login. The file is opened without following
/// a link and must be a regular file of the user's, at most 64 KiB; a missing
/// file is an empty result.
fn read_dotenv(path: &Path) -> Result<(String, bool), String> {
    let file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((String::new(), false)),
        Err(e) if e.raw_os_error() == Some(libc::ELOOP) => {
            return Err("it is not a regular file of yours".into());
        }
        Err(e) => return Err(format!("it cannot be opened ({})", e.kind())),
    };
    let meta = file
        .metadata()
        .map_err(|e| format!("it cannot be read ({})", e.kind()))?;
    // SAFETY: geteuid has no preconditions and cannot fail.
    if !meta.is_file() || meta.uid() != unsafe { libc::geteuid() } {
        return Err("it is not a regular file of yours".into());
    }
    let mut bytes = Vec::new();
    file.take(DOTENV_CAP + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("it cannot be read ({})", e.kind()))?;
    if bytes.len() as u64 > DOTENV_CAP {
        return Err("it is larger than 64 KiB".into());
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut out = String::new();
    let mut login = false;
    for (key, value) in parse_dotenv(&text) {
        if !DOTENV_KEYS.contains(&key.as_str()) {
            continue;
        }
        // A value gemini's parser could read differently is left out.
        let quoted = if !value.contains('\'') && !value.contains(['\n', '\r']) {
            format!("'{value}'")
        } else if !value.contains(['"', '\\', '\n', '\r']) {
            format!("\"{value}\"")
        } else {
            continue;
        };
        login |= DOTENV_LOGIN.contains(&key.as_str());
        out.push_str(&format!("{key}={quoted}\n"));
    }
    Ok((out, login))
}

/// The `KEY=VALUE` pairs of dotenv text: blank lines and `#` lines skipped, an
/// optional `export `, quoted values taken up to the closing quote, an
/// unquoted value ending at ` #`. One line per pair (no multi-line values).
pub(super) fn parse_dotenv(text: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let line = line.strip_prefix("export ").map_or(line, str::trim_start);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, rest)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let valid = key
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !valid {
            continue;
        }
        let rest = rest.trim_start();
        let value = match rest.chars().next() {
            Some(q @ ('"' | '\'')) => match rest[1..].find(q) {
                Some(end) => rest[1..=end].to_owned(),
                None => rest[1..].to_owned(),
            },
            _ => match rest.find(" #").or_else(|| rest.find("\t#")) {
                Some(at) => rest[..at].trim_end().to_owned(),
                None => rest.trim_end().to_owned(),
            },
        };
        pairs.push((key.to_owned(), value));
    }
    pairs
}

/// A variable that logs gemini in.
fn is_login_variable(name: &OsStr) -> bool {
    name == "GEMINI_API_KEY" || name.as_encoded_bytes().starts_with(b"GOOGLE_")
}

/// `path` with the home folder as `~`, for a message, capped.
fn shown(path: &Path, home: Option<&Path>) -> String {
    let text = match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    };
    if text.chars().count() <= PATH_CAP {
        return text;
    }
    let head: String = text.chars().take(PATH_CAP).collect();
    format!("{head}...")
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
