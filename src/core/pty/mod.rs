//! PTY port: what the app layer needs to know about the child process.

use std::{io, path::PathBuf};

use crate::core::pane::PaneSize;

pub mod fake;
pub mod portable;

/// Shell used when `$SHELL` is unset or empty.
const FALLBACK_SHELL: &str = "/bin/sh";

/// Everything needed to start the child.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SpawnSpec {
    pub program: PathBuf,
    /// Arguments of `program`, passed as they are (no shell parses them).
    /// Empty for a shell.
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Variables set on top of the inherited environment.
    pub env: Vec<(String, String)>,
    /// Variables that are not necessarily text (a `PATH` with raw bytes),
    /// set after `env`.
    pub env_os: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    pub size: PaneSize,
    /// Ask the adapter for a [`PtyEvent::Status`] before `Exited`. Off for
    /// shells: only a pane that runs one job cares how it ended.
    pub report_status: bool,
}

impl SpawnSpec {
    /// Spec for the user's shell (`$SHELL`, or `/bin/sh`) in `cwd`.
    pub fn new(shell: Option<&str>, cwd: PathBuf, size: PaneSize) -> Self {
        let program = shell
            .filter(|s| !s.is_empty())
            .unwrap_or(FALLBACK_SHELL)
            .into();
        Self {
            program,
            args: Vec::new(),
            cwd,
            env_os: Vec::new(),
            env: vec![
                ("TERM".into(), "xterm-256color".into()),
                ("COLORTERM".into(), "truecolor".into()),
                (
                    crate::core::nesting::ENV_VAR.into(),
                    std::process::id().to_string(),
                ),
            ],
            size,
            report_status: false,
        }
    }
}

/// Something the child process did, as reported by the PTY adapter.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PtyEvent {
    Output(Vec<u8>),
    /// How the child ended (its exit code), sent just before `Exited` and
    /// only for a spec with `report_status`.
    Status(u32),
    Exited,
}

/// Receives child events from the reader; returns `false` when nobody is
/// listening any more, so the reader can stop.
pub type PtySink = Box<dyn FnMut(PtyEvent) -> bool + Send>;

/// A running child behind a PTY.
pub trait PtyHandle: Send {
    /// Queues bytes for the child. Never blocks.
    fn write(&mut self, bytes: Vec<u8>);
    fn resize(&mut self, size: PaneSize) -> io::Result<()>;
    fn pid(&self) -> Option<u32>;
    /// Whether the shell itself, and not a program it started, owns the
    /// terminal now: the foreground process group of the PTY is the shell's.
    /// `None` when that cannot be told (an error, a target without process
    /// groups); the caller treats it as "not at the prompt".
    fn shell_in_foreground(&self) -> Option<bool> {
        None
    }
    /// The shell's real working directory, read from `/proc/<pid>/cwd` (the
    /// kernel resolves the link without touching the disk). `None` when it
    /// cannot be read: no pid, a platform without `/proc`, a process that is
    /// gone.
    fn cwd(&self) -> Option<PathBuf> {
        std::fs::read_link(format!("/proc/{}/cwd", self.pid()?)).ok()
    }
    /// Terminates the child. Idempotent; implementations also call it on drop.
    fn kill(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::pane::PaneSize;

    const SIZE: PaneSize = PaneSize { rows: 23, cols: 80 };

    fn env_of<'a>(spec: &'a SpawnSpec, key: &str) -> Option<&'a str> {
        spec.env
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    // Spec: SHELL set.
    #[test]
    fn spawn_spec_uses_shell_cwd_and_terminal_env() {
        let spec = SpawnSpec::new(Some("/bin/zsh"), "/work".into(), SIZE);
        assert_eq!(spec.program, PathBuf::from("/bin/zsh"));
        assert_eq!(spec.cwd, PathBuf::from("/work"));
        assert_eq!(spec.size, SIZE);
        assert_eq!(env_of(&spec, "TERM"), Some("xterm-256color"));
        assert_eq!(env_of(&spec, "COLORTERM"), Some("truecolor"));
    }

    // Spec: Nesting marker.
    #[test]
    fn spawn_spec_marks_the_child_as_inside_lazarobox() {
        let spec = SpawnSpec::new(None, "/".into(), SIZE);
        let pid = std::process::id().to_string();
        assert_eq!(
            env_of(&spec, crate::core::nesting::ENV_VAR),
            Some(pid.as_str())
        );
    }

    #[test]
    fn spawn_spec_honours_another_shell() {
        let spec = SpawnSpec::new(Some("/usr/bin/fish"), "/".into(), SIZE);
        assert_eq!(spec.program, PathBuf::from("/usr/bin/fish"));
    }

    // Spec: SHELL unset or empty.
    #[test]
    fn spawn_spec_falls_back_to_sh_when_shell_is_unset() {
        let spec = SpawnSpec::new(None, "/".into(), SIZE);
        assert_eq!(spec.program, PathBuf::from("/bin/sh"));
    }

    #[test]
    fn spawn_spec_falls_back_to_sh_when_shell_is_empty() {
        let spec = SpawnSpec::new(Some(""), "/".into(), SIZE);
        assert_eq!(spec.program, PathBuf::from("/bin/sh"));
    }
}
