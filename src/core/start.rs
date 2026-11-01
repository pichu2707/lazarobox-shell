//! The folder lazarobox starts in: the `--dir` flag, else a valid
//! `[workspace] dir`, else the launch cwd. Pure: the caller injects the cwd,
//! `$HOME` and a probe of the filesystem. An invalid source never stops the
//! app (stderr is invisible from a launcher): it starts in the launch cwd
//! with a notice to show inside the app.

use std::path::{Component, Path, PathBuf};

/// The flag that names the start folder.
pub const DIR_FLAG: &str = "--dir";

/// The flag that runs the first-run prefix probe again.
pub const CHECK_PREFIX_FLAG: &str = "--check-prefix";

/// What the command line said about the start folder.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct StartArgs {
    /// The raw value of the last `--dir`.
    pub dir: Option<String>,
    /// `--dir` came without a usable value.
    pub missing_value: bool,
    /// `--check-prefix`: run the prefix probe even with a stored result.
    pub check_prefix: bool,
}

/// What a path is on disk.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Dir,
    File,
    Missing,
}

/// The chosen folder and, when a source was invalid, the notice to show.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StartDir {
    pub path: PathBuf,
    pub notice: Option<String>,
}

/// Reads `--dir X` and `--dir=X` from the leading flags of the plain start
/// (program name excluded). The first non-flag argument is a subcommand and
/// ends the scan. A value never starts with `-`; write `--dir=-x` for that.
pub fn parse_args(argv: &[String]) -> StartArgs {
    let mut out = StartArgs::default();
    let mut it = argv.iter().peekable();
    while let Some(arg) = it.next() {
        if !arg.starts_with('-') {
            break;
        }
        if arg == CHECK_PREFIX_FLAG {
            out.check_prefix = true;
            continue;
        }
        let value = if arg == DIR_FLAG {
            it.next_if(|v| !v.starts_with('-')).cloned()
        } else if let Some(v) = arg.strip_prefix("--dir=") {
            Some(v.to_owned())
        } else {
            continue;
        };
        match value.filter(|v| !v.is_empty()) {
            Some(v) => (out.dir, out.missing_value) = (Some(v), false),
            None => (out.dir, out.missing_value) = (None, true),
        }
    }
    out
}

/// Expands `~`, joins a relative path to `cwd` and drops `.` components.
pub fn absolute(raw: &str, cwd: &Path, home: Option<&Path>) -> PathBuf {
    let joined = match (raw, raw.strip_prefix("~/"), home) {
        ("~", _, Some(home)) => home.to_path_buf(),
        (_, Some(rest), Some(home)) => home.join(rest),
        _ => cwd.join(raw),
    };
    joined
        .components()
        .filter(|c| !matches!(c, Component::CurDir))
        .collect()
}

/// Decides the start folder. `config_dir` is the already absolute value of
/// `[workspace] dir`. An invalid `--dir` does not fall through to the config.
pub fn resolve(
    args: &StartArgs,
    config_dir: Option<&Path>,
    cwd: &Path,
    home: Option<&Path>,
    kind: &dyn Fn(&Path) -> Kind,
) -> StartDir {
    let fallback = |notice: String| StartDir {
        path: cwd.to_path_buf(),
        notice: Some(notice),
    };
    if args.missing_value {
        return fallback(format!("{DIR_FLAG}: missing value"));
    }
    if let Some(raw) = &args.dir {
        if raw.contains('\u{fffd}') {
            return fallback(format!("{DIR_FLAG}: folder name is not valid UTF-8"));
        }
        let path = absolute(raw, cwd, home);
        return match kind(&path) {
            Kind::Dir => StartDir { path, notice: None },
            Kind::File => fallback(format!("{DIR_FLAG}: not a folder: {}", path.display())),
            Kind::Missing => fallback(format!("{DIR_FLAG}: folder not found: {}", path.display())),
        };
    }
    match config_dir {
        Some(dir) if kind(dir) == Kind::Dir => StartDir {
            path: dir.to_path_buf(),
            notice: None,
        },
        Some(_) => fallback("config: workspace.dir: folder not found".to_owned()),
        None => StartDir {
            path: cwd.to_path_buf(),
            notice: None,
        },
    }
}

#[cfg(test)]
mod tests;
