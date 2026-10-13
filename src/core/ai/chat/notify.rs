//! When a finished chat answer rings the terminal bell (chat-notify). Pure:
//! the decision only; the app asks it, the runtime writes the bell.

use crate::core::layout::PaneId;

/// How a chat turn ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ended {
    /// The final event of a normal stream (or of a summary run).
    Done,
    /// The turn failed, was interrupted or hit the answer cap: the user was
    /// waiting for it just the same.
    Error,
    /// The user stopped the turn: their own action, never worth a bell.
    Cancelled,
}

/// Whether the end of the turn of `pane` rings the bell: the flag is on, the
/// turn finished by itself, and the user is not looking at it (the pane is
/// not the focused one, or the terminal window lost the focus).
pub fn should_bell(
    focused_pane: PaneId,
    window_focused: bool,
    pane: PaneId,
    ended: Ended,
    enabled: bool,
) -> bool {
    enabled && ended != Ended::Cancelled && (pane != focused_pane || !window_focused)
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: PaneId = PaneId::FIRST;

    fn other() -> PaneId {
        let mut ids = crate::core::layout::PaneIds::default();
        ids.alloc();
        ids.alloc()
    }

    const ENDS: [Ended; 3] = [Ended::Done, Ended::Error, Ended::Cancelled];

    // Spec (chat-notify): Disabled never rings.
    #[test]
    fn a_disabled_flag_never_rings() {
        for ended in ENDS {
            for window in [true, false] {
                assert!(!should_bell(A, window, other(), ended, false), "{ended:?}");
                assert!(!should_bell(A, window, A, ended, false), "{ended:?}");
            }
        }
    }

    // Spec: Watched answer is silent.
    #[test]
    fn a_watched_answer_is_silent() {
        for ended in [Ended::Done, Ended::Error] {
            assert!(!should_bell(A, true, A, ended, true), "{ended:?}");
        }
    }

    // Spec: Other pane rings.
    #[test]
    fn an_answer_in_another_pane_rings() {
        for ended in [Ended::Done, Ended::Error] {
            assert!(should_bell(A, true, other(), ended, true), "{ended:?}");
        }
    }

    // Spec: Unfocused window rings, even over the focused pane.
    #[test]
    fn an_unfocused_window_rings_even_on_the_focused_pane() {
        for ended in [Ended::Done, Ended::Error] {
            assert!(should_bell(A, false, A, ended, true), "{ended:?}");
            assert!(should_bell(A, false, other(), ended, true), "{ended:?}");
        }
    }

    // Spec: Cancel is silent.
    #[test]
    fn a_cancel_never_rings() {
        for window in [true, false] {
            for pane in [A, other()] {
                assert!(!should_bell(A, window, pane, Ended::Cancelled, true));
            }
        }
    }
}
