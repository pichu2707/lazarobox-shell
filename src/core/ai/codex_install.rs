//! Which way codex was installed, for messages only.
//!
//! The label never reaches `jail_spec_for` or any other security decision:
//! the jail derives its rules from real paths alone. A test pins that the
//! jail sources do not even name this type.

use std::path::{Path, PathBuf};

/// Where the resolved codex program lives, in a word a person recognises.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CodexInstall {
    /// The official installer: `<CODEX_HOME>/packages/standalone/...`.
    Standalone,
    /// `npm i -g @openai/codex`, or an npm, nvm, volta or fnm prefix.
    Npm,
    /// A Homebrew prefix, cellar or cask.
    Homebrew,
    Other,
}

/// Homebrew prefixes (Apple silicon and Intel).
const BREW_PREFIXES: &[&str] = &[
    "/opt/homebrew",
    "/usr/local/Cellar",
    "/usr/local/Caskroom",
    "/usr/local/Homebrew",
];

/// Folder names that mark a Node version or package manager prefix.
const NODE_MANAGERS: &[&str] = &[".nvm", ".volta", ".fnm", "fnm", ".npm-global", ".npm"];

impl CodexInstall {
    /// Classifies the resolved program `binary` against the resolved
    /// `codex_home`. Pure: nothing is read from disk.
    pub fn detect(binary: &Path, codex_home: &Path) -> Self {
        if binary.starts_with(codex_home.join("packages").join("standalone")) {
            return Self::Standalone;
        }
        if in_openai_package(binary)
            || binary.components().any(|c| {
                NODE_MANAGERS
                    .iter()
                    .any(|m| c.as_os_str() == std::ffi::OsStr::new(m))
            })
        {
            return Self::Npm;
        }
        if BREW_PREFIXES.iter().any(|p| binary.starts_with(p)) {
            return Self::Homebrew;
        }
        Self::Other
    }

    /// The word shown after the version; `None` for [`Self::Other`], which
    /// says nothing useful.
    pub fn label(self) -> Option<&'static str> {
        match self {
            Self::Standalone => Some("standalone"),
            Self::Npm => Some("npm"),
            Self::Homebrew => Some("homebrew"),
            Self::Other => None,
        }
    }
}

/// Whether `path` runs below `node_modules/@openai/codex`.
fn in_openai_package(path: &Path) -> bool {
    let parts: Vec<_> = path.components().map(|c| c.as_os_str()).collect();
    parts
        .windows(3)
        .any(|w| w[0] == "node_modules" && w[1] == "@openai" && w[2] == "codex")
}

/// The codex state folder: `$CODEX_HOME`, else `~/.codex`.
pub fn codex_home_of(env_home: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    env_home
        .map(Path::to_path_buf)
        .or_else(|| home.map(|h| h.join(".codex")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(path: &str) -> CodexInstall {
        CodexInstall::detect(Path::new(path), Path::new("/Users/u/.codex"))
    }

    #[test]
    fn the_official_installer_is_standalone() {
        assert_eq!(
            kind("/Users/u/.codex/packages/standalone/releases/0.153.4/bin/codex"),
            CodexInstall::Standalone
        );
        assert_eq!(
            CodexInstall::detect(
                Path::new("/data/cx/packages/standalone/releases/1/bin/codex"),
                Path::new("/data/cx")
            ),
            CodexInstall::Standalone
        );
    }

    #[test]
    fn a_standalone_path_under_another_home_is_not_standalone() {
        assert_eq!(
            kind("/Users/other/.codex/packages/standalone/releases/1/bin/codex"),
            CodexInstall::Other
        );
    }

    #[test]
    fn npm_prefixes_and_the_openai_package_are_npm() {
        for p in [
            "/usr/lib/node_modules/@openai/codex/bin/codex.js",
            "/opt/homebrew/lib/node_modules/@openai/codex/bin/codex.js",
            "/Users/u/.nvm/versions/node/v22/bin/codex",
            "/Users/u/.volta/bin/codex",
            "/Users/u/.local/share/fnm/node-versions/v22/installation/bin/codex",
            "/Users/u/.npm-global/bin/codex",
        ] {
            assert_eq!(kind(p), CodexInstall::Npm, "{p}");
        }
    }

    #[test]
    fn brew_prefixes_are_homebrew() {
        for p in [
            "/opt/homebrew/bin/codex",
            "/opt/homebrew/Caskroom/codex/0.1/codex",
            "/usr/local/Cellar/codex/0.1/bin/codex",
            "/usr/local/Caskroom/codex/0.1/codex",
        ] {
            assert_eq!(kind(p), CodexInstall::Homebrew, "{p}");
        }
    }

    #[test]
    fn anything_else_is_other_with_no_label() {
        assert_eq!(kind("/usr/local/bin/codex"), CodexInstall::Other);
        assert_eq!(CodexInstall::Other.label(), None);
        assert_eq!(CodexInstall::Standalone.label(), Some("standalone"));
    }

    #[test]
    fn codex_home_falls_back_to_the_dot_codex_folder() {
        let home = Path::new("/Users/u");
        assert_eq!(
            codex_home_of(None, Some(home)),
            Some(PathBuf::from("/Users/u/.codex"))
        );
        assert_eq!(
            codex_home_of(Some(Path::new("/x")), Some(home)),
            Some(PathBuf::from("/x"))
        );
        assert_eq!(codex_home_of(None, None), None);
    }

    /// The label is for messages only: the jail rules come from real paths.
    #[test]
    fn the_jail_sources_never_name_the_install_label() {
        for src in [
            include_str!("jail.rs"),
            include_str!("jail/sbpl.rs"),
            include_str!("jail/rpath.rs"),
        ] {
            let prod = src.split("#[cfg(test)]").next().unwrap();
            assert!(!prod.contains("CodexInstall"));
        }
    }
}
