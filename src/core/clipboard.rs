//! The local clipboard tools a copy can use besides OSC 52, and what the
//! user is told about each copy. Pure: the runtime looks the tools up on
//! PATH and runs the first one that fits the session.

/// A program that puts text on the clipboard of the machine lazarobox runs
/// on. A terminal never confirms OSC 52, a tool's exit status is the only
/// proof of a copy.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    /// `wl-copy` (Wayland).
    WlCopy,
    /// `xclip` (X11).
    Xclip,
    /// `xsel` (X11).
    Xsel,
    /// `clip.exe` (the Windows clipboard, through the WSL interop).
    ClipExe,
}

/// What the environment says about where lazarobox runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Session {
    /// `SSH_CONNECTION` is set: the clipboard that matters is the remote
    /// terminal's, not this machine's.
    pub ssh: bool,
    /// Running in WSL.
    pub wsl: bool,
    /// `WAYLAND_DISPLAY` is set.
    pub wayland: bool,
    /// `DISPLAY` is set.
    pub display: bool,
}

impl Session {
    /// The session `var` describes (`wsl` comes from the host facts).
    pub fn from_env(var: impl Fn(&str) -> Option<String>, wsl: bool) -> Self {
        let set = |name: &str| var(name).is_some_and(|v| !v.is_empty());
        Self {
            ssh: set("SSH_CONNECTION"),
            wsl,
            wayland: set("WAYLAND_DISPLAY"),
            display: set("DISPLAY"),
        }
    }
}

impl Tool {
    /// The program name, also what the notices call it.
    pub fn name(self) -> &'static str {
        match self {
            Self::WlCopy => "wl-copy",
            Self::Xclip => "xclip",
            Self::Xsel => "xsel",
            Self::ClipExe => "clip.exe",
        }
    }

    /// The arguments the tool takes to read the clipboard text from stdin.
    pub fn args(self) -> &'static [&'static str] {
        match self {
            Self::Xclip => &["-selection", "clipboard"],
            Self::Xsel => &["--clipboard", "--input"],
            Self::WlCopy | Self::ClipExe => &[],
        }
    }

    /// The bytes written to the tool's stdin for `text`.
    ///
    /// `clip.exe` reads UTF-16LE with a byte order mark. That is not yet
    /// verified on a real Windows Terminal (see the WSL record).
    pub fn payload(self, text: &str) -> Vec<u8> {
        match self {
            Self::ClipExe => [0xFF, 0xFE]
                .into_iter()
                .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
                .collect(),
            _ => text.as_bytes().to_vec(),
        }
    }
}

/// The tools that fit `session`, best first. Over SSH there are none: the
/// local clipboard is not the one the user is looking at.
pub fn candidates(session: Session) -> Vec<Tool> {
    use Tool::{ClipExe, WlCopy, Xclip, Xsel};
    if session.ssh {
        return Vec::new();
    }
    // The Linux tools need a display, in WSL too; `clip.exe` does not.
    let linux = if session.wayland {
        vec![WlCopy, Xclip, Xsel]
    } else if session.display {
        vec![Xclip, Xsel]
    } else {
        Vec::new()
    };
    if session.wsl {
        [vec![ClipExe], linux].concat()
    } else {
        linux
    }
}

/// The notice while a local tool runs.
pub fn copying(tool: Tool) -> String {
    format!("Copying ({})", tool.name())
}

/// The notice once a local tool succeeded.
pub fn copied(tool: Tool) -> String {
    format!("Copied ({})", tool.name())
}

/// The notice when a local tool failed; OSC 52 was still sent. The reason
/// goes last: it is the part that varies in length, so it is what gets clipped.
pub fn failed(tool: Tool, reason: &str) -> String {
    format!("{} failed; sent via OSC 52 ({reason})", tool.name())
}

/// The notice when a local tool failed on a text too large for OSC 52:
/// nothing was sent to the terminal either. Reason last, as in [`failed`].
pub fn failed_too_large(tool: Tool, reason: &str) -> String {
    format!(
        "{} failed; not copied: too large for OSC 52 ({reason})",
        tool.name()
    )
}

/// The notice when only OSC 52 was sent: nothing confirms it arrived. The
/// action comes first and the notice is short, so the statusline (about 60
/// columns for a notice at 80) never clips it.
pub fn osc52_only(ssh: bool) -> String {
    if ssh {
        "No paste? Enable OSC 52 in your terminal (sent)".to_owned()
    } else {
        "No paste? Install wl-copy or xclip (OSC 52 sent)".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(ssh: bool, wsl: bool, wayland: bool, display: bool) -> Session {
        Session {
            ssh,
            wsl,
            wayland,
            display,
        }
    }

    #[test]
    fn ssh_has_no_local_tool_whatever_else_is_set() {
        assert_eq!(candidates(session(true, false, true, true)), vec![]);
        assert_eq!(candidates(session(true, true, false, false)), vec![]);
    }

    #[test]
    fn wsl_offers_the_windows_clipboard_then_the_tools_its_display_fits() {
        assert_eq!(
            candidates(session(false, true, false, false)),
            vec![Tool::ClipExe]
        );
        assert_eq!(
            candidates(session(false, true, true, true)),
            vec![Tool::ClipExe, Tool::WlCopy, Tool::Xclip, Tool::Xsel]
        );
        assert_eq!(
            candidates(session(false, true, false, true)),
            vec![Tool::ClipExe, Tool::Xclip, Tool::Xsel]
        );
    }

    #[test]
    fn a_failure_on_a_text_too_large_for_osc52_does_not_claim_it_was_sent() {
        let text = failed_too_large(Tool::WlCopy, "timed out");
        assert_eq!(
            text,
            "wl-copy failed; not copied: too large for OSC 52 (timed out)"
        );
        assert!(!text.contains("sent to the terminal"));
    }

    #[test]
    fn wayland_starts_with_wl_copy() {
        let tools = candidates(session(false, false, true, false));
        assert_eq!(tools.first(), Some(&Tool::WlCopy));
        assert_eq!(tools, vec![Tool::WlCopy, Tool::Xclip, Tool::Xsel]);
    }

    #[test]
    fn a_display_alone_gets_the_x11_tools() {
        assert_eq!(
            candidates(session(false, false, false, true)),
            vec![Tool::Xclip, Tool::Xsel]
        );
    }

    #[test]
    fn no_display_no_tool() {
        assert_eq!(candidates(Session::default()), vec![]);
    }

    #[test]
    fn the_session_reads_the_environment() {
        let env = |name: &str| match name {
            "SSH_CONNECTION" => Some("1.2.3.4 1 5.6.7.8 22".to_owned()),
            "WAYLAND_DISPLAY" => Some("wayland-0".to_owned()),
            "DISPLAY" => Some(String::new()),
            _ => None,
        };
        assert_eq!(
            Session::from_env(env, true),
            session(true, true, true, false)
        );
        assert_eq!(Session::from_env(|_| None, false), Session::default());
    }

    #[test]
    fn the_tools_take_their_text_on_stdin() {
        assert_eq!(Tool::Xclip.args(), ["-selection", "clipboard"]);
        assert_eq!(Tool::Xsel.args(), ["--clipboard", "--input"]);
        assert!(Tool::WlCopy.args().is_empty());
        assert!(Tool::ClipExe.args().is_empty());
    }

    #[test]
    fn the_names_are_the_programs() {
        assert_eq!(Tool::WlCopy.name(), "wl-copy");
        assert_eq!(Tool::Xclip.name(), "xclip");
        assert_eq!(Tool::Xsel.name(), "xsel");
        assert_eq!(Tool::ClipExe.name(), "clip.exe");
    }

    #[test]
    fn linux_tools_get_the_utf8_text() {
        assert_eq!(Tool::WlCopy.payload("a漢"), "a漢".as_bytes());
        assert_eq!(Tool::Xclip.payload("hi"), b"hi");
    }

    /// Unverified until the Windows Terminal record (task 4.7).
    #[test]
    fn clip_exe_gets_utf16le_with_a_byte_order_mark() {
        assert_eq!(
            Tool::ClipExe.payload("a漢😀"),
            [0xFF, 0xFE, 0x61, 0x00, 0x22, 0x6F, 0x3D, 0xD8, 0x00, 0xDE]
        );
    }

    #[test]
    fn the_notices_say_what_happened() {
        assert_eq!(copying(Tool::WlCopy), "Copying (wl-copy)");
        assert_eq!(copied(Tool::WlCopy), "Copied (wl-copy)");
        assert_eq!(copied(Tool::Xclip), "Copied (xclip)");
        assert_eq!(
            failed(Tool::WlCopy, "exit code 1"),
            "wl-copy failed; sent via OSC 52 (exit code 1)"
        );
        assert!(!failed(Tool::Xsel, "timed out").starts_with("Copied"));
    }

    #[test]
    fn osc52_only_has_a_local_and_an_ssh_text() {
        assert_eq!(
            osc52_only(false),
            "No paste? Install wl-copy or xclip (OSC 52 sent)"
        );
        assert_eq!(
            osc52_only(true),
            "No paste? Enable OSC 52 in your terminal (sent)"
        );
    }

    /// Columns the statusline gives the notice at 80 columns: measured on a
    /// drawn frame (mode block, pane name chip and padding taken out), less a
    /// margin for a longer pane name.
    const ROOM_AT_80: usize = 58;

    #[test]
    fn every_notice_keeps_its_action_visible_at_80_columns() {
        // The reason comes last, so a long one is what gets clipped.
        let reason = "killed by a signal";
        let notices = [
            osc52_only(false),
            osc52_only(true),
            failed(Tool::ClipExe, reason),
            failed_too_large(Tool::ClipExe, reason),
            failed(Tool::Xclip, "exit code 255"),
            copying(Tool::Xsel),
            copied(Tool::Xsel),
        ];
        for notice in notices {
            // Everything before the reason must fit; the reason may clip.
            let fixed = notice.rsplit_once(" (").map_or(&*notice, |(head, _)| head);
            assert!(fixed.chars().count() <= ROOM_AT_80, "{notice:?}");
        }
        // Even a long reason leaves the action before it in view.
        let long = failed(Tool::ClipExe, &"x".repeat(80));
        let shown: String = long.chars().take(ROOM_AT_80).collect();
        assert!(shown.contains("sent via OSC 52"), "{shown:?}");
        let long = failed_too_large(Tool::ClipExe, &"x".repeat(80));
        let shown: String = long.chars().take(ROOM_AT_80).collect();
        assert!(shown.contains("too large for OSC 52"), "{shown:?}");
    }
}
