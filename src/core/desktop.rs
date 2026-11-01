//! Pure decisions behind `lazarobox-shell install-desktop`: argument parsing,
//! the data directory, the terminal choice, the per-terminal launch syntax and
//! the rendered `.desktop` file. Nothing here touches the filesystem, the
//! process environment or `PATH`; the caller injects every input, and no
//! terminal binary is ever executed.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// First argument that selects the installer instead of the TUI.
pub const SUBCOMMAND: &str = "install-desktop";

/// File name of the launcher inside `<data_home>/applications`.
pub const FILE_NAME: &str = "lazarobox.desktop";

/// The WM class the launcher asks the terminal for (and matches on).
pub const WM_CLASS: &str = "lazarobox";

/// Known terminals probed in `PATH`, in order of preference.
pub const KNOWN_TERMINALS: [&str; 9] = [
    "kitty",
    "ghostty",
    "wezterm",
    "alacritty",
    "foot",
    "gnome-terminal",
    "konsole",
    "xfce4-terminal",
    "x-terminal-emulator",
];

const TEMPLATE: &str = include_str!("desktop/lazarobox.desktop");

pub const USAGE: &str =
    "usage: lazarobox-shell install-desktop [--terminal <cmd>] [--dir <folder>] [--uninstall]";

/// Where the terminal records live; a cargo-installed user has no
/// `docs/terminals/` folder on disk.
pub const RECORDS_URL: &str =
    "https://github.com/pichu2707/lazarobox-shell/tree/main/docs/terminals";

/// Whether this is WSL: `$WSL_DISTRO_NAME` is set, or the kernel release
/// (`/proc/sys/kernel/osrelease`) names Microsoft.
pub fn is_wsl(distro: Option<&str>, osrelease: Option<&str>) -> bool {
    distro.is_some_and(|v| !v.is_empty())
        || osrelease.is_some_and(|r| r.to_ascii_lowercase().contains("microsoft"))
}

/// Terminals that have a record under `docs/terminals/<name>/`.
pub const RECORDED_TERMINALS: [&str; 2] = ["kitty", "zutty"];

/// What the launcher says about the host terminal; `None` is the terminal the
/// desktop picks (unknown here).
pub fn terminal_notice(name: Option<&str>) -> String {
    match name {
        Some(name) if RECORDED_TERMINALS.contains(&name) => {
            format!("{name} has a record: {RECORDS_URL}")
        }
        _ => format!(
            "{} has no test record yet; see {RECORDS_URL}. Some keys or the OSC 52 clipboard may not work.",
            name.unwrap_or("the terminal your desktop picks")
        ),
    }
}

/// Said in WSL after the launcher is written (see `plan_install`). Assumed,
/// not yet verified on a machine (task 3.8): WSLg lists user entries in the
/// Start menu, and runs `Exec` as a Linux GUI app, so Windows Terminal is not
/// used and a Linux GUI terminal is required in the distro.
const WSL_NOTES: [&str; 2] = [
    "WSL: this is the Linux launcher; it may not appear in the Windows Start menu.",
    "WSL: WSLg runs it as a Linux GUI app, so a Linux GUI terminal (kitty, foot, gnome-terminal...) must be installed in this distro; otherwise run `lazarobox-shell` from a WSL shell.",
];

/// One installed terminal offered by the picker.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    /// Has a record under `docs/terminals/`.
    pub tested: bool,
    /// The terminal the automatic choice picks.
    pub default: bool,
}

impl Entry {
    /// The decision for this entry, as if it had been passed to `--terminal`.
    pub fn choice(&self) -> Choice {
        candidate(&self.name, self.path.clone(), Source::Flag)
    }
}

/// What the user typed at the picker.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Parsed {
    Pick(usize),
    Retry(String),
    Cancel,
}

/// The terminals the picker offers: `$TERMINAL`, `xdg-terminal-exec`, then
/// `KNOWN_TERMINALS`, then the target `x-terminal-emulator` resolves to
/// (`resolved_alternative`, only when its launch syntax is known). Only
/// commands `lookup` finds are listed, once each by basename. Nothing is
/// executed: candidates come from a path lookup alone, because some
/// terminals open a window even for `--version`.
pub fn picker_choices(
    env_terminal: Option<&str>,
    lookup: &dyn Fn(&str) -> Option<PathBuf>,
    resolved_alternative: Option<PathBuf>,
    automatic: &Choice,
) -> Vec<Entry> {
    let wanted = env_terminal
        .filter(|c| !c.is_empty())
        .into_iter()
        .chain(["xdg-terminal-exec"])
        .chain(KNOWN_TERMINALS)
        .filter_map(|cmd| Some((base_name(cmd), lookup(cmd)?)))
        // A path that cannot go into `Exec` would be offered but never work.
        .filter(|(_, path)| validate_path(path).is_ok())
        .chain(resolved_alternative.and_then(|path| {
            let name = base_name(path.to_str()?);
            exec_syntax(&name).map(|_| (name, path))
        }));
    let auto_name = match automatic {
        Choice::Terminal { name, .. } => Some(name),
        Choice::Fallback => None,
    };
    let mut entries: Vec<Entry> = Vec::new();
    for (name, path) in wanted {
        if entries.iter().all(|e| e.name != name) {
            entries.push(Entry {
                tested: RECORDED_TERMINALS.contains(&name.as_str()),
                default: auto_name == Some(&name),
                name,
                path,
            });
        }
    }
    entries
}

/// Reads one answer (`None` is end of input). Enter picks the default, a
/// number picks that entry, anything else asks again with a notice.
pub fn parse_choice(input: Option<&str>, entries: &[Entry]) -> Parsed {
    let Some(input) = input else {
        return Parsed::Cancel;
    };
    let text = input.trim();
    if text.is_empty() {
        return Parsed::Pick(entries.iter().position(|e| e.default).unwrap_or(0));
    }
    match text.parse::<usize>() {
        Ok(n) if (1..=entries.len()).contains(&n) => Parsed::Pick(n - 1),
        _ => Parsed::Retry(format!(
            "{text:?} is not valid: type a number from 1 to {}, or press Enter for the default",
            entries.len()
        )),
    }
}

/// What the user asked for.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Options {
    Install {
        terminal: Option<String>,
        /// Folder the launcher starts in, as typed (resolved by the caller).
        dir: Option<String>,
    },
    Uninstall,
    Help,
}

/// How a terminal receives the program to run: the arguments placed between
/// the terminal executable and the lazarobox binary.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExecSyntax {
    pub args: &'static [&'static str],
    /// Whether `args` sets the window class to `WM_CLASS`, so the launcher
    /// may declare `StartupWMClass`.
    pub sets_class: bool,
}

/// How the terminal was picked.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Source {
    Flag,
    EnvTerminal,
    XdgTerminalExec,
    Path,
}

/// The decision: which terminal runs lazarobox.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Choice {
    Terminal {
        /// Basename the syntax table is keyed on.
        name: String,
        /// Absolute path written to `Exec`.
        path: PathBuf,
        /// `false` when the name is not in the table (generic `-e`).
        known: bool,
        source: Source,
    },
    /// No terminal found: `Terminal=true` and the desktop picks one.
    Fallback,
}

/// The finished launcher plus what to tell the user.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Plan {
    pub contents: String,
    pub exec: String,
    pub notes: Vec<String>,
}

/// Parses what follows `install-desktop`.
pub fn parse_args(args: &[OsString]) -> Result<Options, String> {
    let mut terminal = None;
    let mut dir = None;
    let mut uninstall = false;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let arg = arg.to_str().ok_or("arguments must be valid UTF-8")?;
        match arg {
            "-h" | "--help" => return Ok(Options::Help),
            "--uninstall" => uninstall = true,
            "--terminal" => {
                let value = it.next().and_then(|v| v.to_str());
                terminal = Some(terminal_value(value)?);
            }
            "--dir" => dir = Some(dir_value(it.next().map(OsString::as_os_str))?),
            _ => match (arg.strip_prefix("--terminal="), arg.strip_prefix("--dir=")) {
                (Some(value), _) => terminal = Some(terminal_value(Some(value))?),
                (_, Some(value)) => dir = Some(dir_value(Some(OsStr::new(value)))?),
                _ => return Err(format!("unknown argument: {arg}")),
            },
        }
    }
    match (uninstall, terminal.is_some() || dir.is_some()) {
        (true, true) => Err("--uninstall cannot be combined with --terminal or --dir".into()),
        (true, false) => Ok(Options::Uninstall),
        (false, _) => Ok(Options::Install { terminal, dir }),
    }
}

fn dir_value(value: Option<&OsStr>) -> Result<String, String> {
    match value.map(|v| v.to_str().ok_or(v)) {
        Some(Ok(v)) if !v.is_empty() && !v.starts_with('-') => Ok(v.to_string()),
        Some(Err(_)) => Err("--dir: folder name is not valid UTF-8".into()),
        _ => Err("--dir needs a folder".into()),
    }
}

fn terminal_value(value: Option<&str>) -> Result<String, String> {
    match value {
        Some(v) if !v.is_empty() && !v.starts_with('-') => Ok(v.to_string()),
        _ => Err("--terminal needs a command".into()),
    }
}

/// `$XDG_DATA_HOME` only when absolute (the XDG spec says to ignore a
/// relative one), else `$HOME/.local/share`.
pub fn data_home(xdg_data_home: Option<&OsStr>, home: Option<&OsStr>) -> Result<PathBuf, String> {
    if let Some(xdg) = xdg_data_home.map(Path::new).filter(|p| p.is_absolute()) {
        return Ok(xdg.to_path_buf());
    }
    match home.map(Path::new).filter(|p| p.is_absolute()) {
        Some(home) => Ok(home.join(".local/share")),
        None => Err("cannot find the data directory: HOME is unset or not absolute".into()),
    }
}

/// `<data_home>/applications`.
pub fn applications_dir(data_home: &Path) -> PathBuf {
    data_home.join("applications")
}

/// The launch syntax of a known terminal, keyed on its basename. Sources:
/// kitty, alacritty, wezterm, foot, gnome-terminal, xfce4-terminal,
/// xdg-terminal-exec and ghostty `-e` from their manuals; konsole `-e` and
/// x-terminal-emulator `-e` (Debian alternatives convention) are not checked
/// against a manual here.
pub fn exec_syntax(name: &str) -> Option<ExecSyntax> {
    let (args, sets_class): (&'static [&'static str], bool) = match name {
        "kitty" => (&["--class", WM_CLASS, "--title", "Lazarobox"], true),
        // Ghostty's class must be a valid GTK application id; a dotless name
        // is unverified, so no class is requested.
        "ghostty" => (&["-e"], false),
        "wezterm" => (&["start", "--class", WM_CLASS, "--"], true),
        "alacritty" => (&["--class", WM_CLASS, "-e"], true),
        "foot" => (&["--app-id", WM_CLASS], true),
        "gnome-terminal" => (&["--"], false),
        "konsole" => (&["-e"], false),
        "xfce4-terminal" => (&["-x"], false),
        "x-terminal-emulator" => (&["-e"], false),
        "xdg-terminal-exec" => (&[], false),
        _ => return None,
    };
    Some(ExecSyntax { args, sets_class })
}

/// Syntax for a terminal that is not in the table.
const GENERIC: ExecSyntax = ExecSyntax {
    args: &["-e"],
    sets_class: false,
};

/// Picks the terminal. `lookup` resolves a command (a bare name searched in
/// `PATH`, or an absolute path) to an absolute executable path.
///
/// Order: `--terminal`, `$TERMINAL`, `xdg-terminal-exec`, `KNOWN_TERMINALS`,
/// fallback. An unusable `--terminal` is an error; an unusable `$TERMINAL`
/// is skipped with a note.
pub fn choose_terminal(
    flag: Option<&str>,
    env_terminal: Option<&str>,
    lookup: &dyn Fn(&str) -> Option<PathBuf>,
) -> Result<(Choice, Vec<String>), String> {
    let mut notes = Vec::new();
    if let Some(cmd) = flag {
        let path = lookup(cmd).ok_or_else(|| format!("terminal not found: {cmd}"))?;
        return Ok((candidate(cmd, path, Source::Flag), notes));
    }
    if let Some(cmd) = env_terminal.filter(|c| !c.is_empty()) {
        match lookup(cmd) {
            Some(path) => return Ok((candidate(cmd, path, Source::EnvTerminal), notes)),
            None => notes.push(format!("ignoring $TERMINAL={cmd}: not found")),
        }
    }
    if let Some(path) = lookup("xdg-terminal-exec") {
        let choice = candidate("xdg-terminal-exec", path, Source::XdgTerminalExec);
        return Ok((choice, notes));
    }
    for name in KNOWN_TERMINALS {
        if let Some(path) = lookup(name) {
            return Ok((candidate(name, path, Source::Path), notes));
        }
    }
    Ok((Choice::Fallback, notes))
}

fn base_name(cmd: &str) -> String {
    Path::new(cmd)
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or(cmd)
        .to_string()
}

fn candidate(cmd: &str, path: PathBuf, source: Source) -> Choice {
    let name = base_name(cmd);
    let known = exec_syntax(&name).is_some();
    Choice::Terminal {
        name,
        path,
        known,
        source,
    }
}

/// Desktop Entry reserved characters (plus `%` field codes and control
/// characters): such paths are refused instead of escaped.
const RESERVED: &str = " \t\n\"'\\`$%|&;<>~*?#()[]";

/// A path that can go into `Exec` verbatim.
pub fn validate_path(path: &Path) -> Result<(), String> {
    let text = path
        .to_str()
        .ok_or_else(|| format!("unsupported path (not UTF-8): {}", path.display()))?;
    if !path.is_absolute() || text.chars().any(|c| c.is_control() || RESERVED.contains(c)) {
        return Err(format!(
            "unsupported path (must be absolute, no special characters): {text}"
        ));
    }
    Ok(())
}

/// Whether `path` lives in a cargo `target/debug` or `target/release` dir,
/// which `cargo clean` removes.
pub fn is_build_artefact(path: &Path) -> bool {
    let parts: Vec<_> = path.components().map(|c| c.as_os_str()).collect();
    parts
        .windows(2)
        .any(|w| w[0] == "target" && (w[1] == "debug" || w[1] == "release"))
}

const BUILD_ARTEFACT_NOTE: &str = "the launcher points at a build artefact that `cargo clean` removes; run install-desktop from the installed binary (cargo install) instead.";

/// A Desktop Entry *string* value: `\` is `\\`, a leading space is `\s`;
/// control characters are refused. Spaces elsewhere are kept verbatim.
fn entry_string(text: &str) -> Result<String, String> {
    if text.chars().any(char::is_control) {
        return Err(format!("unsupported folder (control characters): {text:?}"));
    }
    let escaped = text.replace('\\', "\\\\");
    Ok(match escaped.strip_prefix(' ') {
        Some(rest) => format!("\\s{rest}"),
        None => escaped,
    })
}

/// ` --dir=<dir>` as an `Exec` argument: quoted when it holds a reserved
/// character, then escaped as a string value.
fn dir_argument(dir: &str) -> Result<String, String> {
    let arg = format!("--dir={dir}");
    let quoted = if arg.chars().any(|c| RESERVED.contains(c)) {
        let mut out = String::from("\"");
        for c in arg.chars() {
            match c {
                '"' | '`' | '$' | '\\' => out.extend(['\\', c]),
                '%' => out.push_str("%%"),
                _ => out.push(c),
            }
        }
        out.push('"');
        out
    } else {
        arg
    };
    Ok(format!(" {}", entry_string(&quoted)?))
}

/// Renders the launcher for `choice` running `bin`, starting in `dir` (an
/// absolute, existing folder; the caller checks it).
pub fn plan_install(
    choice: &Choice,
    bin: &Path,
    dir: Option<&Path>,
    wsl: bool,
) -> Result<Plan, String> {
    validate_path(bin)?;
    let (path_line, dir_arg) = match dir {
        None => (None, String::new()),
        Some(dir) => {
            let text = dir
                .to_str()
                .filter(|t| dir.is_absolute() && t.trim() == *t)
                .ok_or_else(|| {
                    format!(
                        "unsupported folder (must be absolute, no leading or trailing whitespace): {}",
                        dir.display()
                    )
                })?;
            (
                Some(format!("Path={}", entry_string(text)?)),
                dir_argument(text)?,
            )
        }
    };
    let bin_text = bin.to_string_lossy().into_owned();
    let mut notes = Vec::new();
    if is_build_artefact(bin) {
        notes.push(BUILD_ARTEFACT_NOTE.to_string());
    }
    let (exec, terminal_flag, wm_class) = match choice {
        Choice::Terminal {
            name, path, known, ..
        } => {
            validate_path(path)?;
            let syntax = exec_syntax(name).unwrap_or(GENERIC);
            if !known {
                notes.push(format!(
                    "no launch syntax known for {name}; using `{} -e <bin>`",
                    path.display()
                ));
            }
            notes.push(terminal_notice(Some(name)));
            let mut words = vec![path.to_string_lossy().into_owned()];
            words.extend(syntax.args.iter().map(|a| a.to_string()));
            words.push(bin_text);
            (words.join(" "), "false", syntax.sets_class)
        }
        Choice::Fallback => {
            notes.push(
                "no terminal found; the launcher uses Terminal=true and your desktop picks one"
                    .to_string(),
            );
            notes.push(terminal_notice(None));
            (bin_text, "true", false)
        }
    };
    if wsl {
        notes.extend(WSL_NOTES.map(String::from));
    }
    let contents = render(
        &exec,
        &dir_arg,
        path_line.as_deref(),
        terminal_flag,
        wm_class,
    );
    Ok(Plan {
        contents,
        exec: format!("{exec}{dir_arg}"),
        notes,
    })
}

fn render(
    exec: &str,
    dir_arg: &str,
    path_line: Option<&str>,
    terminal: &str,
    wm_class: bool,
) -> String {
    let mut out = String::new();
    for line in TEMPLATE.lines() {
        match line {
            "@STARTUP_WM_CLASS@" if !wm_class => continue,
            "@PATH@" => match path_line {
                Some(path) => out.push_str(path),
                None => continue,
            },
            "@STARTUP_WM_CLASS@" => out.push_str(&format!("StartupWMClass={WM_CLASS}")),
            _ => substitute(
                line,
                &[
                    ("@EXEC@", exec),
                    ("@DIR_ARG@", dir_arg),
                    ("@TERMINAL@", terminal),
                ],
                &mut out,
            ),
        }
        out.push('\n');
    }
    out
}

/// Replaces each placeholder of `line` in one left-to-right pass, so text
/// that was substituted in is never scanned again.
fn substitute(line: &str, vars: &[(&str, &str)], out: &mut String) {
    let mut rest = line;
    while let Some((at, key, value)) = vars
        .iter()
        .filter_map(|(k, v)| rest.find(k).map(|at| (at, *k, *v)))
        .min_by_key(|(at, ..)| *at)
    {
        out.push_str(&rest[..at]);
        out.push_str(value);
        rest = &rest[at + key.len()..];
    }
    out.push_str(rest);
}

#[cfg(test)]
mod tests;
