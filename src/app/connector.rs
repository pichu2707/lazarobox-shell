//! Connectors in the app: the install and login of a catalog entry run as
//! visible text in a new shell tab, and the probe's reports are kept for the
//! session (mcp-connect). The `y` that asks for either is the menu's; this
//! file only acts on the command it emits.

use std::time::{Duration, Instant};

use super::{App, Effect, NewPane, Outcome};
use crate::core::{ai::mcp_catalog::checks::Probed, layout::PaneId};

/// How long a line waits for its shell's first output before it is typed
/// anyway. The runtime's one-second tick checks it, so it is typed between 2
/// and 3 seconds after the tab opens.
const FALLBACK: Duration = Duration::from_secs(2);

/// A catalog line waiting to be typed into the shell of `pane`.
pub(super) struct Typing {
    pane: PaneId,
    /// The line and Enter.
    bytes: Vec<u8>,
    due: Instant,
}

impl App {
    /// Opens a shell tab and types `line` into it, Enter included, when the
    /// shell first speaks (or after [`FALLBACK`]): after the user's `y` the
    /// command runs on its own (D19). The menu closes as it does for an
    /// update, so the tab is in view; unsaved menu changes are discarded.
    pub(super) fn start_connector(&mut self, line: String) -> Vec<Effect> {
        // A catalog line is one line of static text; anything else would
        // run more than the prompt showed.
        if line.is_empty() || line.chars().any(char::is_control) {
            self.notice = Some("connector command refused".to_owned());
            self.dirty = true;
            return Vec::new();
        }
        let mut effects = self.new_tab_with(NewPane::Connector);
        self.typing.push(Typing {
            pane: self.tab().focus,
            bytes: format!("{line}\r").into_bytes(),
            due: (self.clock)() + FALLBACK,
        });
        effects.extend(self.close_menu(Outcome::Reverted));
        effects
    }

    /// The shell of `pane` spoke: its line is typed, once.
    pub(super) fn type_connector(&mut self, pane: PaneId) -> Vec<Effect> {
        let (ready, rest) = std::mem::take(&mut self.typing)
            .into_iter()
            .partition::<Vec<_>, _>(|t| t.pane == pane);
        self.typing = rest;
        ready
            .into_iter()
            .map(|t| Effect::WritePty(t.pane, t.bytes))
            .collect()
    }

    /// The lines whose shell stayed silent for [`FALLBACK`] are typed now; a
    /// pane that is gone types nothing.
    pub(super) fn type_due_connectors(&mut self, now: Instant) -> Vec<Effect> {
        let waiting = std::mem::take(&mut self.typing);
        let mut effects = Vec::new();
        for t in waiting {
            if !self.panes.contains_key(&t.pane) {
                continue;
            }
            if t.due <= now {
                effects.push(Effect::WritePty(t.pane, t.bytes));
            } else {
                self.typing.push(t);
            }
        }
        effects
    }

    /// Whether a line waits: the runtime ticks.
    pub(super) fn typing_waiting(&self) -> bool {
        self.typing.iter().any(|t| self.panes.contains_key(&t.pane))
    }

    /// A probe ended: its report is kept for the session.
    pub(super) fn on_mcp_probe(&mut self, report: Probed) -> Vec<Effect> {
        self.mcp_probes.insert(report.server.clone(), report);
        self.dirty = true;
        Vec::new()
    }

    /// The last probe report of the server `name` this session.
    pub fn mcp_probe(&self, name: &str) -> Option<&Probed> {
        self.mcp_probes.get(name)
    }
}
