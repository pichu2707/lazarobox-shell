//! Windows drives under WSL: which `/mnt/<letter>` folders are drvfs mounts,
//! which Windows profile is the user's own, and the read policy on them
//! (openspec `wsl-drive-access`, agent-tools "WSL drive read policy").
//!
//! Pure: the runtime reads `/proc/mounts` and asks Windows for
//! `%USERPROFILE%`, and hands the text here.

use std::path::{Component, Path, PathBuf};

/// The drvfs drives and the user's own Windows profile. Empty everywhere but
/// WSL, so no path is ever on a drive there.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct WinDrives {
    mounts: Vec<PathBuf>,
    profile: Option<PathBuf>,
}

/// The verdict of the drive policy on a resolved path.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DriveVerdict {
    /// Not below a drive mount: the `$HOME` rules decide.
    NotOnDrive,
    Allowed,
    Refused,
}

/// Folders directly on a drive that are never read (compared ASCII
/// case-insensitively, as drvfs folds case).
const SYSTEM_FOLDERS: &[&str] = &[
    "windows",
    "program files",
    "program files (x86)",
    "programdata",
    "$recycle.bin",
    "system volume information",
    "recovery",
    // Left by a Windows upgrade: copies of the old profiles and hives.
    "windows.old",
    "$windows.~bt",
    "$windows.~ws",
    "$winreagent",
];

/// Files directly on a drive that are never read.
const SYSTEM_FILES: &[&str] = &["pagefile.sys", "hiberfil.sys", "swapfile.sys"];

impl WinDrives {
    pub fn new(mounts: Vec<PathBuf>, profile: Option<PathBuf>) -> Self {
        Self { mounts, profile }
    }

    pub fn is_empty(&self) -> bool {
        self.mounts.is_empty()
    }

    /// The mount `real` lies on, if any.
    pub fn drive_of(&self, real: &Path) -> Option<&Path> {
        self.mounts
            .iter()
            .map(PathBuf::as_path)
            .find(|mount| real.starts_with(mount))
    }

    /// The names below the drive mount, `None` off the drives or for a part
    /// that is not a plain UTF-8 name.
    fn below_mount<'a>(&'a self, real: &Path) -> Option<(&'a Path, Vec<String>)> {
        let mount = self.drive_of(real)?;
        let names = normal_names(real.strip_prefix(mount).ok()?)?;
        Some((mount, names))
    }

    /// Whether `name`, a profile folder on `mount`, is readable: `Public`
    /// or the user's own profile.
    fn readable_profile(&self, mount: &Path, name: &str) -> bool {
        name.eq_ignore_ascii_case("public")
            || self.profile.as_deref().is_some_and(|own| {
                own.parent()
                    .is_some_and(|users| users == mount.join("Users"))
                    && own
                        .file_name()
                        .is_some_and(|own| own.eq_ignore_ascii_case(name))
            })
    }

    /// Judges an already resolved path. `below_profile` judges the part
    /// below a readable profile with the `$HOME` secret rules.
    pub fn judge(&self, real: &Path, below_profile: impl Fn(&Path) -> bool) -> DriveVerdict {
        if self.drive_of(real).is_none() {
            return DriveVerdict::NotOnDrive;
        }
        let Some((mount, names)) = self.below_mount(real) else {
            return DriveVerdict::Refused;
        };
        // drvfs resolves DOS 8.3 aliases (`PROGRA~3` is `ProgramData`) and
        // the real path keeps them, so no name list could catch them.
        if names.iter().any(|name| is_short_alias(name)) {
            return DriveVerdict::Refused;
        }
        let lower: Vec<String> = names.iter().map(|n| n.to_ascii_lowercase()).collect();
        let allowed = match lower.as_slice() {
            [] => true,
            [first, ..] if SYSTEM_FOLDERS.contains(&first.as_str()) => false,
            [only] if SYSTEM_FILES.contains(&only.as_str()) => false,
            [users] if users == "users" => true,
            [users, profile, rest @ ..] if users == "users" => {
                self.readable_profile(mount, &names[1])
                    && !profile.is_empty()
                    && match rest {
                        [] => true,
                        [top, ..] if top == "appdata" || top.starts_with("ntuser.dat") => false,
                        _ => below_profile(&names[2..].iter().collect::<PathBuf>()),
                    }
            }
            _ => !names_excluded(&names),
        };
        if allowed {
            DriveVerdict::Allowed
        } else {
            DriveVerdict::Refused
        }
    }

    /// Whether `real` is a broad drive folder: a drive root, its `Users`
    /// folder or a profile root. Accepted as a chat folder only when typed.
    pub fn is_broad(&self, real: &Path) -> bool {
        self.below_mount(real)
            .is_some_and(|(_, names)| match names.as_slice() {
                [] => true,
                [users] | [users, _] => users.eq_ignore_ascii_case("users"),
                _ => false,
            })
    }
}

/// The drvfs mounts in the text of `/proc/mounts`: a mount point
/// `/mnt/<one ASCII letter>` of type `drvfs`, or `9p` with `aname=drvfs`.
pub fn parse_mounts(text: &str) -> Vec<PathBuf> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let (_, point, kind, options) = (
                fields.next()?,
                fields.next()?,
                fields.next()?,
                fields.next()?,
            );
            let letter = point.strip_prefix("/mnt/")?;
            let drive = letter.len() == 1 && letter.as_bytes()[0].is_ascii_alphabetic();
            let drvfs = kind == "drvfs"
                || (kind == "9p"
                    && options
                        .split([',', ';'])
                        .any(|option| option == "aname=drvfs"));
            (drive && drvfs).then(|| PathBuf::from(point))
        })
        .collect()
}

/// The own profile from the output of `cmd.exe /d /c echo %USERPROFILE%`:
/// `<letter>:\Users\<name>` becomes `/mnt/<letter>/Users/<name>`. Anything
/// else is `None`.
pub fn parse_profile(out: &str) -> Option<PathBuf> {
    let line = out.strip_suffix('\n').unwrap_or(out);
    let line = line.strip_suffix('\r').unwrap_or(line);
    let mut chars = line.chars();
    let letter = chars.next().filter(char::is_ascii_alphabetic)?;
    let name = chars.as_str().strip_prefix(":\\Users\\")?;
    let plain = !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['\\', '/', '%', '\0', '\n', '\r']);
    plain.then(|| {
        PathBuf::from(format!("/mnt/{}", letter.to_ascii_lowercase()))
            .join("Users")
            .join(name)
    })
}

/// Whether `name` looks like a DOS 8.3 alias: a `~` followed by a digit.
fn is_short_alias(name: &str) -> bool {
    name.as_bytes()
        .windows(2)
        .any(|pair| pair[0] == b'~' && pair[1].is_ascii_digit())
}

/// Whether any name is excluded wherever it sits (`.env`, `.netrc`…).
fn names_excluded(names: &[String]) -> bool {
    names
        .iter()
        .any(|name| super::paths::is_excluded_name(name))
}

#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;

fn normal_names(rest: &Path) -> Option<Vec<String>> {
    rest.components()
        .map(|part| match part {
            Component::Normal(name) => name.to_str().map(str::to_owned),
            _ => None,
        })
        .collect()
}
