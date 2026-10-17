//! The write half of the chat's auto-type (design D18): the command was typed
//! only if the shell, and not a program it started, owns the terminal now.

use crate::core::{ai::chat::engine::Decision, pty::PtyHandle};

/// Why nothing was typed into a pane that no longer exists.
pub const GONE: &str = "the pane is gone";

/// Writes `bytes` to `pane` (which the user knows as number `number`) when its
/// shell is in the foreground and answers `Typed`; otherwise writes nothing
/// and answers `Busy` (or `Denied` for a pane that is gone). An unknown
/// foreground (an error, a target without process groups) is `Busy`.
pub fn type_if_idle(
    pane: Option<&mut Box<dyn PtyHandle>>,
    number: usize,
    bytes: Vec<u8>,
) -> Decision {
    let Some(pane) = pane else {
        return Decision::Denied(GONE);
    };
    if pane.shell_in_foreground() != Some(true) {
        return Decision::Busy(number);
    }
    pane.write(bytes);
    Decision::Typed(number)
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
        let decision = type_if_idle(Some(&mut pane), 3, b"cd x".to_vec());
        assert_eq!(decision, Decision::Typed(3));
        assert_eq!(log.lock().unwrap().writes, vec![b"cd x".to_vec()]);
    }

    #[test]
    fn a_child_in_the_foreground_or_an_unknown_one_is_denied_and_nothing_is_written() {
        for foreground in [Some(false), None] {
            let (mut pane, log) = pty(foreground);
            let decision = type_if_idle(Some(&mut pane), 2, b"cd x".to_vec());
            assert_eq!(decision, Decision::Busy(2), "{foreground:?}");
            assert!(log.lock().unwrap().writes.is_empty(), "{foreground:?}");
        }
    }

    #[test]
    fn a_pane_that_is_gone_is_denied() {
        assert_eq!(
            type_if_idle(None, 1, b"cd x".to_vec()),
            Decision::Denied(GONE)
        );
    }
}
