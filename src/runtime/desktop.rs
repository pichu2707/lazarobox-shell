//! The thin IO layer of `lazarobox-shell install-desktop`: it reads the
//! process environment, writes (or removes) the launcher and refreshes the
//! desktop database. Every decision lives in `core::desktop`.
//!
//! `execute` takes all its inputs through `Context`, so tests drive it with a
//! temp directory and a fake `PATH`; only `main` touches the real process.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{BufRead, IsTerminal, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::core::desktop::{
    ALTERNATIVES, Choice, Entry, FILE_NAME, ICONS, Options, Parsed, RECORDS_URL, USAGE,
    applications_dir, choose_terminal, data_home, icon_theme_dir, is_wsl, parse_args, parse_choice,
    picker_choices, plan_install, target_name,
};

/// Everything `execute` reads from the outside world.
pub struct Context<'a> {
    /// `$TERMINAL`.
    pub env_terminal: Option<String>,
    pub xdg_data_home: Option<OsString>,
    pub home: Option<OsString>,
    /// Absolute, canonical path of the running binary.
    pub exe: PathBuf,
    /// The working directory, for a relative `--dir`.
    pub cwd: PathBuf,
    /// Running inside WSL (adds the WSL notes).
    pub wsl: bool,
    /// Resolves a command to an absolute executable path.
    pub lookup: &'a dyn Fn(&str) -> Option<PathBuf>,
    /// Best-effort `update-desktop-database <applications dir>`.
    pub refresh_db: &'a dyn Fn(&Path),
    /// Best-effort `gtk-update-icon-cache` of `<data>/icons/hicolor`, only
    /// when that folder already has a cache that would go stale.
    pub refresh_icons: &'a dyn Fn(&Path),
}

/// Entry point for `install-desktop <args>`; returns the exit status.
pub fn main(args: &[OsString]) -> i32 {
    let options = match parse_args(args) {
        Ok(Options::Help) => {
            println!("{USAGE}");
            return 0;
        }
        Ok(options) => options,
        Err(why) => {
            eprintln!("error: {why}\n{USAGE}");
            return 2;
        }
    };
    let exe = match std::env::current_exe().and_then(fs::canonicalize) {
        Ok(exe) => exe,
        Err(why) => {
            eprintln!("error: cannot resolve the lazarobox-shell binary: {why}");
            return 1;
        }
    };
    let lookup = |cmd: &str| which(cmd, std::env::var_os("PATH").as_deref());
    let refresh_db = |dir: &Path| {
        if let Some(tool) = lookup("update-desktop-database") {
            refresh_with(tool, &[dir.as_os_str()]);
        }
    };
    let refresh_icons = |dir: &Path| {
        // A theme without a cache is scanned directly, so there is nothing to
        // refresh (and creating a cache would need an index.theme).
        if !dir.join("icon-theme.cache").exists() {
            return;
        }
        if let Some(tool) = lookup("gtk-update-icon-cache") {
            let flags = ["--force", "--ignore-theme-index"].map(OsStr::new);
            refresh_with(tool, &[flags[0], flags[1], dir.as_os_str()]);
        }
    };
    let ctx = Context {
        env_terminal: std::env::var("TERMINAL").ok(),
        xdg_data_home: std::env::var_os("XDG_DATA_HOME"),
        home: std::env::var_os("HOME"),
        exe,
        cwd: std::env::current_dir().unwrap_or_default(),
        wsl: is_wsl(
            std::env::var("WSL_DISTRO_NAME").ok().as_deref(),
            fs::read_to_string("/proc/sys/kernel/osrelease")
                .ok()
                .as_deref(),
        ),
        lookup: &lookup,
        refresh_db: &refresh_db,
        refresh_icons: &refresh_icons,
    };
    // Ask only when a person is at the keyboard and reading the screen.
    let stdin = std::io::stdin();
    let asks = stdin.is_terminal() && std::io::stdout().is_terminal();
    let mut locked = stdin.lock();
    let input = asks.then_some(&mut locked as &mut dyn BufRead);
    match execute(&options, &ctx, input, &mut std::io::stdout()) {
        Ok(()) => 0,
        Err(why) => {
            eprintln!("error: {why}");
            1
        }
    }
}

/// Installs or removes the launcher, reporting to `out`. `input` is `Some`
/// only when stdin and stdout are a TTY; it enables the terminal picker, which
/// `--terminal` skips.
pub fn execute(
    options: &Options,
    ctx: &Context,
    input: Option<&mut dyn BufRead>,
    out: &mut dyn Write,
) -> Result<(), String> {
    let data = data_home(ctx.xdg_data_home.as_deref(), ctx.home.as_deref())?;
    let apps_dir = applications_dir(&data);
    let target = apps_dir.join(FILE_NAME);
    match options {
        Options::Help => writeln!(out, "{USAGE}").map_err(io_error),
        Options::Uninstall => {
            let mut errors = Vec::new();
            let launcher = remove_if_present(&target).unwrap_or_else(|e| {
                errors.push(e);
                false
            });
            let icons = remove_icons(&data, &mut errors);
            if launcher {
                (ctx.refresh_db)(&apps_dir);
            }
            if icons > 0 {
                (ctx.refresh_icons)(&icon_theme_dir(&data));
            }
            if launcher {
                writeln!(out, "Removed {}", target.display()).map_err(io_error)?;
            }
            if icons > 0 {
                let theme = icon_theme_dir(&data);
                writeln!(out, "Removed {icons} icons under {}", theme.display())
                    .map_err(io_error)?;
            }
            if !errors.is_empty() {
                return Err(errors.join("; "));
            }
            if !launcher && icons == 0 {
                writeln!(
                    out,
                    "Nothing to remove: {} does not exist",
                    target.display()
                )
                .map_err(io_error)?;
            }
            Ok(())
        }
        Options::Install { terminal, dir } => {
            // Checked first: a bad folder writes nothing.
            let dir = dir
                .as_deref()
                .map(|raw| install_dir(raw, ctx))
                .transpose()?;
            let (mut choice, mut notes) =
                choose_terminal(terminal.as_deref(), ctx.env_terminal.as_deref(), ctx.lookup)?;
            if let (None, Some(input)) = (terminal, input) {
                let alternative =
                    (ctx.lookup)(ALTERNATIVES).and_then(|path| fs::canonicalize(path).ok());
                let entries = picker_choices(
                    ctx.env_terminal.as_deref(),
                    ctx.lookup,
                    alternative,
                    &choice,
                );
                if let Some(entry) = ask(&entries, input, out)?.filter(|e| !e.default) {
                    choice = entry.choice();
                }
            }
            if let Choice::Terminal {
                name, path, runs, ..
            } = &mut choice
                && name == ALTERNATIVES
            {
                *runs = fs::canonicalize(&*path)
                    .ok()
                    .as_deref()
                    .and_then(target_name);
            }
            let existed = fs::symlink_metadata(&target).is_ok();
            let plan = plan_install(&choice, &ctx.exe, dir.as_deref(), ctx.wsl)?;
            notes.extend(plan.notes);
            write_atomically(&target, plan.contents.as_bytes())
                .map_err(|e| format!("cannot write {}: {e}", target.display()))?;
            (ctx.refresh_db)(&apps_dir);
            let verb = if existed { "Updated" } else { "Installed" };
            let again = if existed { " (already installed)" } else { "" };
            writeln!(out, "{verb} {}{again}", target.display()).map_err(io_error)?;
            writeln!(out, "  Exec: {}", plan.exec).map_err(io_error)?;
            for note in notes {
                writeln!(out, "  note: {note}").map_err(io_error)?;
            }
            // After the launcher: a failed icon never loses the entry, but it
            // is always reported and makes the exit status non-zero.
            let failures = install_icons(&data);
            (ctx.refresh_icons)(&icon_theme_dir(&data));
            if failures.is_empty() {
                return Ok(());
            }
            Err(format!(
                "the launcher is installed, but some icons could not be written: {}",
                failures.join("; ")
            ))
        }
    }
}

/// Lists `entries` and reads answers until one is valid. `Ok(None)` when
/// there is nothing to choose from; end of input cancels with an error, so
/// nothing is written.
fn ask<'e>(
    entries: &'e [Entry],
    input: &mut dyn BufRead,
    out: &mut dyn Write,
) -> Result<Option<&'e Entry>, String> {
    // Nothing to choose from, or only the terminal Enter would pick anyway.
    if entries.is_empty() || matches!(entries, [only] if only.default) {
        return Ok(None);
    }
    writeln!(out, "Choose the terminal that runs lazarobox:").map_err(io_error)?;
    for (i, e) in entries.iter().enumerate() {
        let tested = if e.tested { " (tested)" } else { "" };
        let default = if e.default { " [default]" } else { "" };
        writeln!(out, "  {}) {}{tested}{default}", i + 1, e.name).map_err(io_error)?;
    }
    writeln!(out, "(tested) = has a record: {RECORDS_URL}").map_err(io_error)?;
    loop {
        write!(out, "Number, or Enter for the default: ").map_err(io_error)?;
        out.flush().map_err(io_error)?;
        let mut line = String::new();
        let read = input
            .read_line(&mut line)
            .map_err(|e| format!("cannot read the answer: {e}"))?;
        match parse_choice((read > 0).then_some(line.as_str()), entries) {
            Parsed::Pick(i) => return Ok(entries.get(i)),
            Parsed::Retry(notice) => writeln!(out, "{notice}").map_err(io_error)?,
            Parsed::Cancel => return Err("cancelled: nothing was written".into()),
        }
    }
}

/// The absolute folder for `--dir`: `~` and relative paths resolve now, and
/// it must be an existing directory.
fn install_dir(raw: &str, ctx: &Context) -> Result<PathBuf, String> {
    let home = ctx.home.as_deref().map(Path::new);
    let path = crate::core::start::absolute(raw, &ctx.cwd, home);
    match fs::metadata(&path) {
        Ok(meta) if meta.is_dir() => Ok(path),
        Ok(_) => Err(format!("--dir: not a folder: {}", path.display())),
        Err(_) => Err(format!("--dir: folder not found: {}", path.display())),
    }
}

fn io_error(e: std::io::Error) -> String {
    format!("cannot write to stdout: {e}")
}

/// Longest a refresh helper may run. A real `gtk-update-icon-cache` on a big
/// theme on a cold disk takes several seconds, so this is looser than the
/// 2 s `notify-send` gets; it only has to bound a hang.
const HELPER_DEADLINE: std::time::Duration = std::time::Duration::from_secs(10);

/// Runs `cmd` with no stdio and waits at most `deadline`; a helper still
/// running then is killed and reaped. `Err` says why it did not finish. The
/// exit status is not judged: the helpers are best-effort.
fn run_bounded(mut cmd: Command, deadline: std::time::Duration) -> Result<(), String> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot start: {e}"))?;
    let end = std::time::Instant::now() + deadline;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) if std::time::Instant::now() < end => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("still running after {deadline:?}, killed"));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("cannot wait: {e}"));
            }
        }
    }
}

/// Runs a refresh helper; a failure is a warning on stderr, never an error.
fn refresh_with(tool: PathBuf, args: &[&OsStr]) {
    let mut cmd = Command::new(&tool);
    cmd.args(args);
    if let Err(why) = run_bounded(cmd, HELPER_DEADLINE) {
        eprintln!("warning: {}: {why}", tool.display());
    }
}

/// Writes every embedded icon under `data`; returns one message per failure.
fn install_icons(data: &Path) -> Vec<String> {
    ICONS
        .iter()
        .filter_map(|(relative, bytes)| {
            let path = data.join(relative);
            write_atomically(&path, bytes)
                .err()
                .map(|e| format!("cannot write icon {}: {e}", path.display()))
        })
        .collect()
}

/// Removes the icons `install_icons` writes, trying every one; returns how
/// many went and pushes one message per failure.
fn remove_icons(data: &Path, errors: &mut Vec<String>) -> usize {
    let mut removed = 0;
    for (relative, _) in ICONS {
        match remove_if_present(&data.join(relative)) {
            Ok(true) => removed += 1,
            Ok(false) => {}
            Err(e) => errors.push(e),
        }
    }
    removed
}

/// Removes a file; `false` when it was not there.
fn remove_if_present(path: &Path) -> Result<bool, String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("cannot remove {}: {e}", path.display())),
    }
}

/// Writes a temp file next to the target and renames it over: a file never
/// shows up half written, and a symlinked target is replaced instead of
/// written through. Creates the target's folder.
fn write_atomically(target: &Path, contents: &[u8]) -> std::io::Result<()> {
    let dir = target.parent().unwrap_or(Path::new("."));
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let result = (|| {
        let open = || {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)
        };
        let mut file = match open() {
            // A temp file left by a crashed run with the same pid: drop it
            // and retry once.
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                fs::remove_file(&tmp)?;
                open()?
            }
            other => other?,
        };
        file.write_all(contents)?;
        file.set_permissions(fs::Permissions::from_mode(0o644))?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Resolves `cmd` to an absolute executable: an absolute path is checked as
/// is, a bare name is searched in the absolute entries of `path`. Anything
/// else (a relative path) does not resolve.
pub(crate) fn which(cmd: &str, path: Option<&OsStr>) -> Option<PathBuf> {
    if cmd.contains('/') {
        let p = PathBuf::from(cmd);
        return (p.is_absolute() && is_executable(&p)).then_some(p);
    }
    std::env::split_paths(path?)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(cmd))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests;
