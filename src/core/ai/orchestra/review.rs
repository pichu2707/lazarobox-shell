//! The risk of a staged change set (agent-orchestrator S7b, D16): a tier from
//! deterministic path rules, which the orchestrator may only raise, and which
//! is `High` whenever the assessment itself fails. Pure: it reads no file and
//! follows no symlink, it only judges the path strings the ledger holds.
//!
//! Limits, on purpose: a rename or a deletion is assessed by its staged path
//! only, because `Change` has no source path; when S7 stages moves, the old
//! path must be added and judged too (moving `Cargo.lock` away must be `high`).
//! A symlink or hard link is judged by its own staged name, never by what it
//! points to.

use std::fmt;

use super::changes::Change;
use super::ledger::{REASON_BYTES, cut, one_line};
use crate::core::ai::tools::paths::is_excluded_name;

/// How much a change needs a second look. Ordered: `Passive < Medium < High`.
/// The rules produce `Passive` or `High`; `Medium` comes from the orchestrator.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Tier {
    Passive,
    Medium,
    High,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Passive => "passive",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

/// Where the tier of an [`Assessment`] came from.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Source {
    /// The path rules, with nothing raised above them.
    Rules,
    /// The orchestrator raised the tier above the rules; its sanitized reason.
    Raised(String),
    /// The assessment failed, so the tier is `High`; why.
    Failed(String),
}

/// The tier of a change set and its source.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Assessment {
    pub tier: Tier,
    pub source: Source,
}

/// An orchestrator's `raise{tier, reason}`, already checked and sanitized.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Raise {
    tier: Tier,
    reason: String,
}

/// Why a raise was refused; the host shows it to the orchestrator.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RaiseError {
    Tier,
    Reason,
    /// The tier asked for is `passive`: a raise goes up, never down.
    Lowering,
}

impl fmt::Display for RaiseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Tier => "raise refused: tier must be passive, medium or high",
            Self::Reason => "raise refused: a reason is required",
            Self::Lowering => {
                "raise refused: the orchestrator can only raise the tier above passive"
            }
        })
    }
}

impl Raise {
    /// `tier` is `passive`, `medium` or `high` (any case); `reason` is
    /// untrusted text: its first line, without control or bidirectional
    /// format characters, cut like a handoff reason.
    pub fn new(tier: &str, reason: &str) -> Result<Self, RaiseError> {
        let tier = match tier.trim().to_ascii_lowercase().as_str() {
            "passive" => Tier::Passive,
            "medium" => Tier::Medium,
            "high" => Tier::High,
            _ => return Err(RaiseError::Tier),
        };
        if tier == Tier::Passive {
            return Err(RaiseError::Lowering);
        }
        let reason = cut(&one_line(reason), REASON_BYTES).trim_end().to_owned();
        if !reason.chars().any(|c| !c.is_whitespace()) {
            return Err(RaiseError::Reason);
        }
        Ok(Self { tier, reason })
    }

    pub fn tier(&self) -> Tier {
        self.tier
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }
}

/// The tier of `changes`: the highest path tier, raised (never lowered) by
/// `raise`. An empty set, or a path that cannot be judged, is `High`.
pub fn assess(changes: &[Change], raise: Option<&Raise>) -> Assessment {
    let floor = match floor(changes) {
        Ok(tier) => tier,
        Err(why) => {
            return Assessment {
                tier: Tier::High,
                source: Source::Failed(why),
            };
        }
    };
    match raise {
        Some(raise) if raise.tier > floor => Assessment {
            tier: raise.tier,
            source: Source::Raised(raise.reason.clone()),
        },
        _ => Assessment {
            tier: floor,
            source: Source::Rules,
        },
    }
}

fn floor(changes: &[Change]) -> Result<Tier, String> {
    if changes.is_empty() {
        return Err("no staged change to assess".to_owned());
    }
    let mut tier = Tier::Passive;
    for change in changes {
        let parts = parts_of(&change.path)?;
        if risky(&parts) {
            tier = Tier::High;
        }
    }
    Ok(tier)
}

/// The names of a path relative to the chat root: `\` counts as `/`, `.` and
/// empty parts vanish, `..` pops a part, and trailing dots and spaces of a
/// name are dropped (Windows ignores them). A blank, absolute, drive-lettered
/// or NUL-holding path, or one that climbs above the root, cannot be judged.
fn parts_of(path: &str) -> Result<Vec<String>, String> {
    let unix = path.replace('\\', "/");
    if unix.trim().is_empty() {
        return Err("empty path".to_owned());
    }
    if unix.contains('\0') || unix.starts_with('/') {
        return Err("unreadable path".to_owned());
    }
    let mut parts: Vec<String> = Vec::new();
    for (at, part) in unix.split('/').enumerate() {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err("path leaves the root".to_owned());
                }
            }
            _ => {
                if at == 0 && drive_letter(part) {
                    return Err("absolute path".to_owned());
                }
                let name = part.trim_end_matches(['.', ' ']);
                if !name.is_empty() {
                    parts.push(fold(name));
                }
            }
        }
    }
    if parts.is_empty() {
        return Err("empty path".to_owned());
    }
    Ok(parts)
}

/// A name folded for matching: every char goes through its uppercase and back
/// to lowercase, so the KELVIN SIGN (K) and the LONG S (s) land on `k` and
/// `s` and cannot hide `Makefile` or `.ssh`. Plain ASCII folds as before. Not
/// a full Unicode normalization: other confusables (fullwidth letters,
/// homoglyphs) are not folded onto a rule name.
fn fold(name: &str) -> String {
    if name.is_ascii() {
        return name.to_ascii_lowercase();
    }
    name.chars()
        .flat_map(char::to_uppercase)
        .flat_map(char::to_lowercase)
        .collect()
}

fn drive_letter(part: &str) -> bool {
    let bytes = part.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// Folders whose content is CI, build config, credentials or git internals, at any depth.
const RISKY_DIRS: &[&str] = &[
    ".github",
    ".gitlab",
    ".circleci",
    ".buildkite",
    ".gitea",
    ".woodpecker",
    ".git",
    ".cargo",
    ".husky",
    ".githooks",
    ".ssh",
    ".gnupg",
];

/// File names that are build, CI, dependency or lockfile entries.
const RISKY_FILES: &[&str] = &[
    "build.rs",
    "makefile",
    "gnumakefile",
    "justfile",
    "cmakelists.txt",
    "meson.build",
    "build.zig",
    "flake.nix",
    "jenkinsfile",
    "dockerfile",
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "pom.xml",
    "setup.py",
    "pipfile",
    "gemfile",
    "mix.exs",
    "go.mod",
    "go.sum",
    "bun.lockb",
    "gradle.lockfile",
    "pnpm-lock.yaml",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "rakefile",
    "procfile",
    "containerfile",
    "vagrantfile",
    "brewfile",
    "podfile",
    "rust-toolchain",
    ".gitmodules",
    ".gitattributes",
    ".tool-versions",
    ".yarnrc",
    ".pypirc",
    "known_hosts",
    "id_rsa",
    "id_dsa",
    "id_ecdsa",
    "id_ed25519",
];

/// Extensions of config formats, build projects, scripts, locks and key
/// material. Broad on purpose (any `*.json`, `*.xml` or `*.sh` is `high`): a
/// false positive costs one consent prompt, a miss costs an unreviewed
/// change to the build or to code that runs by itself.
const RISKY_EXTENSIONS: &[&str] = &[
    "toml",
    "yaml",
    "yml",
    "json",
    "jsonc",
    "json5",
    "ini",
    "cfg",
    "conf",
    "properties",
    "lock",
    "cmake",
    "dockerfile",
    "pem",
    "key",
    "p12",
    "pfx",
    "gpg",
    "asc",
    "gradle",
    "sbt",
    "csproj",
    "sln",
    "props",
    "targets",
    "xml",
    "mk",
    "tf",
    "sh",
];

/// Whether the normalized, lowercased `parts` name a risky path: a build,
/// CI, dependency, lock, config or secret entry. Matching is by name, case-folded
/// (see [`fold`]), at any depth.
fn risky(parts: &[String]) -> bool {
    let Some((name, dirs)) = parts.split_last() else {
        return false;
    };
    dirs.iter().any(|dir| RISKY_DIRS.contains(&dir.as_str()))
        || RISKY_DIRS.contains(&name.as_str())
        || RISKY_FILES.contains(&name.as_str())
        || name.starts_with("dockerfile.")
        || name.starts_with("requirements") && name.ends_with(".txt")
        || name.starts_with(".gitlab-ci")
        || name.starts_with(".travis")
        || name == "azure-pipelines.yml"
        || name
            .rsplit_once('.')
            .is_some_and(|(stem, ext)| !stem.is_empty() && RISKY_EXTENSIONS.contains(&ext))
        || is_excluded_name(name)
}
