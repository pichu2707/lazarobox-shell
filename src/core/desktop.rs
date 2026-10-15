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

pub const USAGE: &str = "usage: lazarobox-shell install-desktop [--terminal <cmd>] [--uninstall]";

pub const NON_KITTY_NOTICE: &str =
    "lazarobox is tested in Kitty; in other terminals some keys or OSC 52 clipboard may not work.";

/// What the user asked for.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Options {
    Install { terminal: Option<String> },
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
            _ => match arg.strip_prefix("--terminal=") {
                Some(value) => terminal = Some(terminal_value(Some(value))?),
                None => return Err(format!("unknown argument: {arg}")),
            },
        }
    }
    match (uninstall, terminal) {
        (true, Some(_)) => Err("--uninstall cannot be combined with --terminal".into()),
        (true, None) => Ok(Options::Uninstall),
        (false, terminal) => Ok(Options::Install { terminal }),
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

fn candidate(cmd: &str, path: PathBuf, source: Source) -> Choice {
    let name = Path::new(cmd)
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or(cmd)
        .to_string();
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

/// Renders the launcher for `choice` running `bin`.
pub fn plan_install(choice: &Choice, bin: &Path) -> Result<Plan, String> {
    validate_path(bin)?;
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
            if name != "kitty" {
                notes.push(NON_KITTY_NOTICE.to_string());
            }
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
            notes.push(NON_KITTY_NOTICE.to_string());
            (bin_text, "true", false)
        }
    };
    Ok(Plan {
        contents: render(&exec, terminal_flag, wm_class),
        exec,
        notes,
    })
}

fn render(exec: &str, terminal: &str, wm_class: bool) -> String {
    let mut out = String::new();
    for line in TEMPLATE.lines() {
        match line {
            "@STARTUP_WM_CLASS@" if !wm_class => continue,
            "@STARTUP_WM_CLASS@" => out.push_str(&format!("StartupWMClass={WM_CLASS}")),
            _ => substitute(
                line,
                &[("@EXEC@", exec), ("@TERMINAL@", terminal)],
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
