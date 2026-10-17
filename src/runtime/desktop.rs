//! The thin IO layer of `lazarobox-shell install-desktop`: it reads the
//! process environment, writes (or removes) the launcher and refreshes the
//! desktop database. Every decision lives in `core::desktop`.
//!
//! `execute` takes all its inputs through `Context`, so tests drive it with a
//! temp directory and a fake `PATH`; only `main` touches the real process.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::core::desktop::{
    FILE_NAME, Options, USAGE, applications_dir, choose_terminal, data_home, parse_args,
    plan_install,
};

/// Everything `execute` reads from the outside world.
pub struct Context<'a> {
    /// `$TERMINAL`.
    pub env_terminal: Option<String>,
    pub xdg_data_home: Option<OsString>,
    pub home: Option<OsString>,
    /// Absolute, canonical path of the running binary.
    pub exe: PathBuf,
    /// Resolves a command to an absolute executable path.
    pub lookup: &'a dyn Fn(&str) -> Option<PathBuf>,
    /// Best-effort `update-desktop-database <applications dir>`.
    pub refresh_db: &'a dyn Fn(&Path),
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
            let _ = Command::new(tool)
                .arg(dir)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    };
    let ctx = Context {
        env_terminal: std::env::var("TERMINAL").ok(),
        xdg_data_home: std::env::var_os("XDG_DATA_HOME"),
        home: std::env::var_os("HOME"),
        exe,
        lookup: &lookup,
        refresh_db: &refresh_db,
    };
    match execute(&options, &ctx, &mut std::io::stdout()) {
        Ok(()) => 0,
        Err(why) => {
            eprintln!("error: {why}");
            1
        }
    }
}

/// Installs or removes the launcher, reporting to `out`.
pub fn execute(options: &Options, ctx: &Context, out: &mut dyn Write) -> Result<(), String> {
    let data = data_home(ctx.xdg_data_home.as_deref(), ctx.home.as_deref())?;
    let apps_dir = applications_dir(&data);
    let target = apps_dir.join(FILE_NAME);
    match options {
        Options::Help => writeln!(out, "{USAGE}").map_err(io_error),
        Options::Uninstall => {
            match fs::remove_file(&target) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return writeln!(
                        out,
                        "Nothing to remove: {} does not exist",
                        target.display()
                    )
                    .map_err(io_error);
                }
                Err(e) => return Err(format!("cannot remove {}: {e}", target.display())),
            }
            (ctx.refresh_db)(&apps_dir);
            writeln!(out, "Removed {}", target.display()).map_err(io_error)
        }
        Options::Install { terminal } => {
            let (choice, mut notes) =
                choose_terminal(terminal.as_deref(), ctx.env_terminal.as_deref(), ctx.lookup)?;
            let plan = plan_install(&choice, &ctx.exe)?;
            notes.extend(plan.notes);
            write_atomically(&apps_dir, &target, &plan.contents)
                .map_err(|e| format!("cannot write {}: {e}", target.display()))?;
            (ctx.refresh_db)(&apps_dir);
            writeln!(out, "Installed {}", target.display()).map_err(io_error)?;
            writeln!(out, "  Exec: {}", plan.exec).map_err(io_error)?;
            for note in notes {
                writeln!(out, "  note: {note}").map_err(io_error)?;
            }
            Ok(())
        }
    }
}

fn io_error(e: std::io::Error) -> String {
    format!("cannot write to stdout: {e}")
}

/// Writes a temp file next to the target and renames it over: a launcher
/// never shows up half written, and a symlinked target is replaced instead
/// of written through.
fn write_atomically(dir: &Path, target: &Path, contents: &str) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{FILE_NAME}.{}.tmp", std::process::id()));
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
        file.write_all(contents.as_bytes())?;
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
