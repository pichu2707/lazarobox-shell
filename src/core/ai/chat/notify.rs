//! When a finished chat answer notifies the user, and how (chat-notify).
//! Pure: the decisions only; the app asks them, the runtime executes them
//! (writes the bell, runs `notify-send`).

use crate::core::{ai::config::NotifyMode, layout::PaneId};

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

/// Whether the end of the turn of `pane` notifies at all: the mode is not
/// `off`, the turn finished by itself, and the user is not looking at it (the
/// pane is not the focused one, or the terminal window lost the focus).
pub fn should_notify(
    focused_pane: PaneId,
    window_focused: bool,
    pane: PaneId,
    ended: Ended,
    mode: NotifyMode,
) -> bool {
    mode != NotifyMode::Off
        && ended != Ended::Cancelled
        && (pane != focused_pane || !window_focused)
}

/// What a notification does once it is due.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Delivery {
    /// Ring the terminal bell now.
    pub bell: bool,
    /// Send a desktop notification.
    pub desktop: bool,
}

/// The delivery of `mode` given whether a desktop notification can be sent
/// (a display is set and `notify-send` resolves).
///
/// | mode    | desktop available | bell | desktop |
/// |---------|-------------------|------|---------|
/// | auto    | yes               | yes  | yes     |
/// | auto    | no                | yes  | no      |
/// | desktop | yes               | no   | yes     |
/// | desktop | no                | yes  | no      |
/// | bell    | any               | yes  | no      |
/// | off     | any               | no   | no      |
pub fn delivery(mode: NotifyMode, desktop_available: bool) -> Delivery {
    match mode {
        NotifyMode::Auto => Delivery {
            bell: true,
            desktop: desktop_available,
        },
        NotifyMode::Desktop => Delivery {
            bell: !desktop_available,
            desktop: desktop_available,
        },
        NotifyMode::Bell => Delivery {
            bell: true,
            desktop: false,
        },
        NotifyMode::Off => Delivery {
            bell: false,
            desktop: false,
        },
    }
}

/// Whether `mode` rings the bell before anything else is known. Whether the
/// desktop is available is only learned off the UI thread, so `desktop` rings
/// later (through [`bell_after_failed_desktop`]) when it is not.
pub fn rings_at_once(mode: NotifyMode) -> bool {
    delivery(mode, true).bell
}

/// Whether a desktop notification that failed to start rings the bell for
/// that event. Only `desktop` needs it: `auto` rang the bell already.
pub fn bell_after_failed_desktop(mode: NotifyMode) -> bool {
    mode == NotifyMode::Desktop
}

/// Whether the mode shows a desktop notification at all.
pub fn wants_desktop(mode: NotifyMode) -> bool {
    matches!(mode, NotifyMode::Auto | NotifyMode::Desktop)
}

/// Whether lazarobox plays its own sound: the flag is on and the mode shows a
/// desktop notification (`bell` and `off` stay as they are).
pub fn plays_sound(mode: NotifyMode, sound: bool) -> bool {
    sound && wants_desktop(mode)
}

/// How the sound step of the test ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SoundEnd {
    /// A player exited 0.
    Played,
    /// A player was still running at its time limit. It started, so it is
    /// counted as playing, but a player hung while connecting to the audio
    /// server looks exactly the same.
    StillPlaying,
}

/// The footer line of the test action. `desktop` and `sound` are the results
/// of the attempts that were made (`None`: not attempted); an `Err` carries
/// the short reason.
pub fn describe_test(
    mode: NotifyMode,
    desktop: Option<Result<(), String>>,
    sound: Option<Result<SoundEnd, String>>,
) -> String {
    let desktop_up = matches!(desktop, Some(Ok(())));
    let mut sent = Vec::new();
    if delivery(mode, desktop_up).bell {
        sent.push("bell");
    }
    if desktop_up {
        sent.push("desktop");
    }
    if matches!(sound, Some(Ok(SoundEnd::Played))) {
        sent.push("sound");
    }
    let mut parts = Vec::new();
    if !sent.is_empty() {
        parts.push(format!("sent: {}", sent.join(" + ")));
    }
    if let Some(Err(why)) = desktop {
        parts.push(format!("desktop unavailable: {why}"));
    }
    match sound {
        Some(Ok(SoundEnd::StillPlaying)) => parts.push("sound: still playing after 2 s".to_owned()),
        Some(Err(why)) => parts.push(format!("no sound: {why}")),
        _ => {}
    }
    if parts.is_empty() {
        return "notify is off".to_owned();
    }
    parts.join(" · ")
}

/// The fixed text of a desktop notification. It names the pane and how the
/// turn ended, and never carries any of the answer.
pub fn body(pane: PaneId, ended: Ended) -> String {
    let n = pane.number();
    match ended {
        Ended::Done => format!("Answer ready in pane {n}"),
        Ended::Error => format!("Turn failed in pane {n}"),
        // Never asked for (a cancel does not notify), but never a failure.
        Ended::Cancelled => format!("Turn ended in pane {n}"),
    }
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
                assert!(
                    !should_notify(A, window, other(), ended, NotifyMode::Off),
                    "{ended:?}"
                );
                assert!(
                    !should_notify(A, window, A, ended, NotifyMode::Off),
                    "{ended:?}"
                );
            }
        }
    }

    // Spec: Watched answer is silent.
    #[test]
    fn a_watched_answer_is_silent() {
        for ended in [Ended::Done, Ended::Error] {
            assert!(
                !should_notify(A, true, A, ended, NotifyMode::Auto),
                "{ended:?}"
            );
        }
    }

    // Spec: Other pane rings.
    #[test]
    fn an_answer_in_another_pane_rings() {
        for ended in [Ended::Done, Ended::Error] {
            assert!(
                should_notify(A, true, other(), ended, NotifyMode::Auto),
                "{ended:?}"
            );
        }
    }

    // Spec: Unfocused window rings, even over the focused pane.
    #[test]
    fn an_unfocused_window_rings_even_on_the_focused_pane() {
        for ended in [Ended::Done, Ended::Error] {
            assert!(
                should_notify(A, false, A, ended, NotifyMode::Auto),
                "{ended:?}"
            );
            assert!(
                should_notify(A, false, other(), ended, NotifyMode::Auto),
                "{ended:?}"
            );
        }
    }

    // Spec: Cancel is silent.
    #[test]
    fn a_cancel_never_rings() {
        for window in [true, false] {
            for pane in [A, other()] {
                assert!(!should_notify(
                    A,
                    window,
                    pane,
                    Ended::Cancelled,
                    NotifyMode::Auto
                ));
            }
        }
    }

    #[test]
    fn every_enabled_mode_gates_the_same_way() {
        for mode in [NotifyMode::Auto, NotifyMode::Desktop, NotifyMode::Bell] {
            assert!(
                should_notify(A, true, other(), Ended::Done, mode),
                "{mode:?}"
            );
            assert!(!should_notify(A, true, A, Ended::Done, mode), "{mode:?}");
            assert!(
                !should_notify(A, false, A, Ended::Cancelled, mode),
                "{mode:?}"
            );
        }
    }

    // Decision table: mode x desktop availability.
    #[test]
    fn the_delivery_table() {
        let d = |bell, desktop| Delivery { bell, desktop };
        let table = [
            (NotifyMode::Auto, true, d(true, true)),
            (NotifyMode::Auto, false, d(true, false)),
            (NotifyMode::Desktop, true, d(false, true)),
            (NotifyMode::Desktop, false, d(true, false)),
            (NotifyMode::Bell, true, d(true, false)),
            (NotifyMode::Bell, false, d(true, false)),
            (NotifyMode::Off, true, d(false, false)),
            (NotifyMode::Off, false, d(false, false)),
        ];
        for (mode, available, want) in table {
            assert_eq!(delivery(mode, available), want, "{mode:?} {available}");
        }
    }

    // The bell that rings at once is the table's with the desktop up: the
    // availability is only known later, off the UI thread.
    #[test]
    fn the_immediate_bell_follows_the_table() {
        for mode in NotifyMode::ALL {
            assert_eq!(rings_at_once(mode), delivery(mode, true).bell, "{mode:?}");
        }
        assert!(rings_at_once(NotifyMode::Auto) && !rings_at_once(NotifyMode::Desktop));
    }

    #[test]
    fn only_desktop_rings_after_a_failed_spawn() {
        for mode in NotifyMode::ALL {
            assert_eq!(
                bell_after_failed_desktop(mode),
                mode == NotifyMode::Desktop,
                "{mode:?}"
            );
        }
    }

    #[test]
    fn the_body_is_fixed_text_naming_only_the_pane() {
        assert_eq!(body(A, Ended::Done), "Answer ready in pane 1");
        assert_eq!(body(A, Ended::Error), "Turn failed in pane 1");
        // A cancel never notifies; if it ever got here it is not a failure.
        assert_eq!(body(A, Ended::Cancelled), "Turn ended in pane 1");
        assert_eq!(
            body(PaneId::for_test(42), Ended::Done),
            "Answer ready in pane 42"
        );
        for ended in ENDS {
            let text = body(other(), ended);
            assert!(
                text.is_ascii() && !text.contains(['\n', '\x1b']),
                "{text:?}"
            );
        }
    }

    #[test]
    fn the_sound_plays_only_in_auto_and_desktop_and_only_when_enabled() {
        for mode in NotifyMode::ALL {
            let want = matches!(mode, NotifyMode::Auto | NotifyMode::Desktop);
            assert_eq!(plays_sound(mode, true), want, "{mode:?}");
            assert!(!plays_sound(mode, false), "{mode:?}");
        }
    }

    #[test]
    fn the_desktop_is_wanted_in_auto_and_desktop_only() {
        for mode in NotifyMode::ALL {
            assert_eq!(
                wants_desktop(mode),
                matches!(mode, NotifyMode::Auto | NotifyMode::Desktop),
                "{mode:?}"
            );
        }
    }

    fn no<T>(why: &str) -> Option<Result<T, String>> {
        Some(Err(why.to_owned()))
    }

    #[test]
    fn the_test_report_lists_what_was_sent() {
        let ok = Some(Ok(()));
        let played = Some(Ok(SoundEnd::Played));
        assert_eq!(
            describe_test(NotifyMode::Auto, ok.clone(), played),
            "sent: bell + desktop + sound"
        );
        assert_eq!(
            describe_test(NotifyMode::Desktop, ok.clone(), None),
            "sent: desktop"
        );
        // A player still running at the limit is not plain success.
        assert_eq!(
            describe_test(
                NotifyMode::Auto,
                ok.clone(),
                Some(Ok(SoundEnd::StillPlaying))
            ),
            "sent: bell + desktop · sound: still playing after 2 s"
        );
        assert_eq!(describe_test(NotifyMode::Bell, None, None), "sent: bell");
        assert_eq!(describe_test(NotifyMode::Off, None, None), "notify is off");
    }

    #[test]
    fn the_test_report_says_why_the_desktop_or_the_sound_did_not_happen() {
        assert_eq!(
            describe_test(
                NotifyMode::Auto,
                no("no notify-send on PATH"),
                no("no sound player")
            ),
            "sent: bell · desktop unavailable: no notify-send on PATH · no sound: no sound player"
        );
        // `desktop` rings the bell instead of a missing notification.
        assert_eq!(
            describe_test(NotifyMode::Desktop, no("SSH session"), None),
            "sent: bell · desktop unavailable: SSH session"
        );
        assert_eq!(
            describe_test(NotifyMode::Auto, Some(Ok(())), no("no sound player")),
            "sent: bell + desktop · no sound: no sound player"
        );
    }
}
