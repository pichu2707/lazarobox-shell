//! The write half of the chat's auto-type (design D18) and of its run (D21):
//! the command was typed only if the shell, and not a program it started,
//! owns the terminal now.

use std::path::Path;

use tokio::sync::oneshot;

use crate::core::{
    ai::chat::engine::{Answer, Decision},
    pty::PtyHandle,
};

/// Why nothing was typed into a pane that no longer exists.
pub const GONE: &str = crate::app::PANE_GONE;
/// Why a run was not written: the shell is not where the command was checked.
pub const MOVED: &str = "the pane's directory changed since the check";
/// Why a run was not written: its bytes break the one-Enter invariant.
pub const MALFORMED: &str = "the run was not one command and one Enter";
/// Why a run was not written: a program owns the terminal.
pub const BUSY: &str = "the pane is running a program";

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

/// The one write of a run (design D21), with its Enter already in `bytes`.
/// Only if the shell's real directory still is `checked` and the shell owns
/// the terminal (unknown counts as failing); otherwise nothing is written and
/// the reason comes back. The answer is claimed BEFORE the write, so a call
/// that is over gets no bytes. The symlink-swap residual: see `app::auto_run`.
pub fn run_if_idle(
    pane: Option<&mut Box<dyn PtyHandle>>,
    number: usize,
    checked: &Path,
    bytes: Vec<u8>,
    claim: impl FnOnce() -> Option<oneshot::Sender<Answer>>,
) -> Result<(), &'static str> {
    let Some(pane) = pane else {
        return Err(GONE);
    };
    // Refused, not asserted: a bug here must not abort the UI.
    let one_enter = bytes.last() == Some(&b'\r')
        && bytes.iter().filter(|b| matches!(b, b'\r' | b'\n')).count() == 1;
    if !one_enter {
        return Err(MALFORMED);
    }
    if pane.cwd().as_deref() != Some(checked) {
        return Err(MOVED);
    }
    if pane.shell_in_foreground() != Some(true) {
        return Err(BUSY);
    }
    if let Some(answer) = claim() {
        pane.write(bytes);
        let _ = answer.send(Decision::Ran(number).into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::pty::fake::{FakeLog, FakePty};
    use std::path::Path;
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

    // Spec (pane-execution, enter in the same write): the write step.
    fn run_pty(
        cwd: Option<&str>,
        foreground: Option<bool>,
    ) -> (Box<dyn PtyHandle>, Arc<Mutex<FakeLog>>) {
        let pty = FakePty {
            foreground,
            cwd: cwd.map(Into::into),
            ..FakePty::default()
        };
        let log = pty.log.clone();
        (Box::new(pty), log)
    }

    fn claim_of(
        log: &Arc<Mutex<FakeLog>>,
    ) -> (
        tokio::sync::oneshot::Receiver<Answer>,
        impl FnOnce() -> Option<tokio::sync::oneshot::Sender<Answer>>,
    ) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let log = log.clone();
        let claim = move || {
            assert!(
                log.lock().unwrap().writes.is_empty(),
                "claimed before the write"
            );
            Some(tx)
        };
        (rx, claim)
    }

    #[test]
    fn a_run_claims_the_answer_then_writes_once_and_answers_ran() {
        let (mut pane, log) = run_pty(Some("/home/u/dev"), Some(true));
        let (mut rx, claim) = claim_of(&log);
        let done = run_if_idle(
            Some(&mut pane),
            3,
            Path::new("/home/u/dev"),
            b"cd x\r".to_vec(),
            claim,
        );
        assert_eq!(done, Ok(()));
        assert_eq!(log.lock().unwrap().writes, vec![b"cd x\r".to_vec()]);
        assert_eq!(rx.try_recv(), Ok(Decision::Ran(3).into()));
    }

    #[test]
    fn a_moved_or_unreadable_cwd_and_a_busy_foreground_write_nothing_and_claim_nothing() {
        let cases = [
            (Some("/home/u/.ssh"), Some(true), MOVED),
            (None, Some(true), MOVED),
            (Some("/home/u/dev"), Some(false), BUSY),
            (Some("/home/u/dev"), None, BUSY),
        ];
        for (cwd, foreground, want) in cases {
            let (mut pane, log) = run_pty(cwd, foreground);
            let claim = || -> Option<tokio::sync::oneshot::Sender<Answer>> { panic!("claimed") };
            let done = run_if_idle(
                Some(&mut pane),
                2,
                Path::new("/home/u/dev"),
                b"ls\r".to_vec(),
                claim,
            );
            assert_eq!(done, Err(want), "{cwd:?} {foreground:?}");
            assert!(
                log.lock().unwrap().writes.is_empty(),
                "{cwd:?} {foreground:?}"
            );
        }
    }

    // S1: the write site checks the one-Enter invariant itself.
    #[test]
    fn bytes_without_exactly_one_final_enter_are_refused_and_nothing_is_claimed() {
        for bad in [
            &b"ls"[..],
            b"ls\r\r",
            b"ls\n\r",
            b"a\rb\r",
            b"ls\r\n",
            b"\r\rls",
            b"",
        ] {
            let (mut pane, log) = run_pty(Some("/home/u"), Some(true));
            let claim = || -> Option<tokio::sync::oneshot::Sender<Answer>> { panic!("claimed") };
            let done = run_if_idle(
                Some(&mut pane),
                1,
                Path::new("/home/u"),
                bad.to_vec(),
                claim,
            );
            assert_eq!(done, Err(MALFORMED), "{bad:?}");
            assert!(log.lock().unwrap().writes.is_empty(), "{bad:?}");
        }
    }

    #[test]
    fn a_pane_that_is_gone_and_a_call_that_is_over_write_nothing() {
        let claim = || -> Option<tokio::sync::oneshot::Sender<Answer>> { None };
        assert_eq!(
            run_if_idle(None, 1, Path::new("/"), b"ls\r".to_vec(), claim),
            Err(GONE)
        );
        let (mut pane, log) = run_pty(Some("/home/u"), Some(true));
        let over = run_if_idle(
            Some(&mut pane),
            1,
            Path::new("/home/u"),
            b"ls\r".to_vec(),
            claim,
        );
        assert_eq!(over, Ok(()), "nothing owed, nothing to ask");
        assert!(log.lock().unwrap().writes.is_empty());
    }
}
