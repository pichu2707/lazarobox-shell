//! PTY port: what the app layer needs to know about the child process.

use std::{io, path::PathBuf};

use crate::core::pane::PaneSize;

pub mod fake;

/// Shell used when `$SHELL` is unset or empty.
const FALLBACK_SHELL: &str = "/bin/sh";

/// Everything needed to start the child.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SpawnSpec {
    pub program: PathBuf,
    pub cwd: PathBuf,
    /// Variables set on top of the inherited environment.
    pub env: Vec<(String, String)>,
    pub size: PaneSize,
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
            cwd,
            env: vec![
                ("TERM".into(), "xterm-256color".into()),
                ("COLORTERM".into(), "truecolor".into()),
            ],
            size,
        }
    }
}

/// Something the child process did, as reported by the PTY adapter.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PtyEvent {
    Output(Vec<u8>),
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
