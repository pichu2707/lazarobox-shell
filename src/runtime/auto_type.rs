//! The write half of the chat's auto-type (design D18): the command was typed
//! only if the shell, and not a program it started, owns the terminal now.

use crate::core::{ai::chat::engine::Decision, pty::PtyHandle};

/// Why nothing was typed into a pane that is busy or cannot be inspected.
pub const BUSY: &str = "the pane is running a program";
/// Why nothing was typed into a pane that no longer exists.
pub const GONE: &str = "the pane is gone";

/// Writes `bytes` to `pane` when its shell is in the foreground and answers
/// `Sent`; otherwise writes nothing and answers `Denied`. An unknown
/// foreground (an error, a target without process groups) is a denial.
pub fn type_if_idle(pane: Option<&mut Box<dyn PtyHandle>>, bytes: Vec<u8>) -> Decision {
    let Some(pane) = pane else {
        return Decision::Denied(GONE);
    };
    if pane.shell_in_foreground() != Some(true) {
        return Decision::Denied(BUSY);
    }
    pane.write(bytes);
    Decision::Sent
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::pty::fake::{FakeLog, FakePty};
    use std::sync::{Arc, Mutex};

    fn pty(foreground: Option<bool>) -> (Box<dyn PtyHandle>, Arc<Mutex<FakeLog>>) {
        let pty = FakePty {
            foreground,
            ..FakePty::default()
        };
        let log = pty.log.clone();
        (Box::new(pty), log)
    }

    // Spec (agent-tools, auto-type): the foreground check.
    #[test]
    fn the_shell_in_the_foreground_gets_the_bytes_and_the_call_is_sent() {
        let (mut pane, log) = pty(Some(true));
        let decision = type_if_idle(Some(&mut pane), b"cd x".to_vec());
        assert_eq!(decision, Decision::Sent);
        assert_eq!(log.lock().unwrap().writes, vec![b"cd x".to_vec()]);
    }

    #[test]
    fn a_child_in_the_foreground_or_an_unknown_one_is_denied_and_nothing_is_written() {
        for foreground in [Some(false), None] {
            let (mut pane, log) = pty(foreground);
            let decision = type_if_idle(Some(&mut pane), b"cd x".to_vec());
            assert_eq!(decision, Decision::Denied(BUSY), "{foreground:?}");
            assert!(log.lock().unwrap().writes.is_empty(), "{foreground:?}");
        }
    }

    #[test]
    fn a_pane_that_is_gone_is_denied() {
        assert_eq!(type_if_idle(None, b"cd x".to_vec()), Decision::Denied(GONE));
    }
}
