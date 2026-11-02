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
//! gemini the one setting that says which login to use.

use std::{
    ffi::OsString,
    fs,
    io::{self, Read},
    os::unix::fs::{DirBuilderExt, symlink},
    path::Path,
};

use crate::core::ai::{codex_install::codex_home_of, jail::Cli};

use super::CliEnv;

/// The private `CODEX_HOME`, inside the turn's temp folder.
const CODEX_HOME_DIR: &str = "codex-home";
/// The private `GEMINI_CLI_HOME`, inside the turn's temp folder. gemini keeps
/// its state in `<GEMINI_CLI_HOME>/.gemini`.
const GEMINI_HOME_DIR: &str = "gemini-home";

/// The login files codex reads from its state folder.
const CODEX_LOGIN: [&str; 1] = ["auth.json"];
/// The login files gemini reads from its state folder: OAuth credentials, the
/// cached account and the file-based credential store.
const GEMINI_LOGIN: [&str; 3] = [
    "oauth_creds.json",
    "google_accounts.json",
    "gemini-credentials.json",
];

/// The most of the user's gemini `settings.json` that is read.
const SETTINGS_CAP: usize = 1 << 20;

/// The variables that give `cli` its private home, which `tmp` (the turn's
/// private folder) holds. Empty for a CLI that is isolated by flags (claude).
pub(super) fn prepare(env: &CliEnv, cli: Cli, tmp: &Path) -> io::Result<Vec<(OsString, OsString)>> {
    match cli {
        Cli::Claude => Ok(Vec::new()),
        Cli::Codex => {
            let home = tmp.join(CODEX_HOME_DIR);
            private_dir(&home)?;
            let real = codex_home_of(env.codex_home.as_deref(), env.home.as_deref());
            link_login(real.as_deref(), &home, &CODEX_LOGIN)?;
            Ok(vec![("CODEX_HOME".into(), home.into_os_string())])
        }
        Cli::Gemini => {
            let root = tmp.join(GEMINI_HOME_DIR);
            let home = root.join(".gemini");
            private_dir(&home)?;
            let real = env.home.as_deref().map(|h| h.join(".gemini"));
            link_login(real.as_deref(), &home, &GEMINI_LOGIN)?;
            if let Some(kind) = real.as_deref().and_then(auth_choice) {
                let settings = serde_json::json!({"security": {"auth": {"selectedType": kind}}});
                fs::write(home.join("settings.json"), settings.to_string())?;
            }
            Ok(vec![("GEMINI_CLI_HOME".into(), root.into_os_string())])
        }
    }
}

/// A new folder, 0700, with its parents.
fn private_dir(path: &Path) -> io::Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
}

/// Links each login file the user has in `real` into `home`.
fn link_login(real: Option<&Path>, home: &Path, files: &[&str]) -> io::Result<()> {
    let Some(real) = real else { return Ok(()) };
    for name in files {
        let source = real.join(name);
        if fs::symlink_metadata(&source).is_ok() {
            symlink(&source, home.join(name))?;
        }
    }
    Ok(())
}

/// Which login gemini is set to use (`security.auth.selectedType`), the only
/// part of the user's settings a turn needs. `None` when the file is missing,
/// unreadable or has none: gemini then falls back to what its environment
/// gives it, or says it has no login.
fn auth_choice(state: &Path) -> Option<String> {
    let mut text = String::new();
    fs::File::open(state.join("settings.json"))
        .ok()?
        .take(SETTINGS_CAP as u64)
        .read_to_string(&mut text)
        .ok()?;
    let settings: serde_json::Value = serde_json::from_str(&text).ok()?;
    settings
        .pointer("/security/auth/selectedType")?
        .as_str()
        .map(str::to_owned)
}
