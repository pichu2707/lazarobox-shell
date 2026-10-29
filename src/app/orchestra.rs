//! The orchestrator in a chat pane (agent-orchestrator D2, D5): the pane's
//! ORCH mode, its Ctrl+O toggle, the task a send runs on and the one-time
//! provider notice (D12). State and keys only; the badge, the hint and the
//! notice are drawn by the UI.

use super::*;
use crate::core::ai::{
    chat::engine::{Availability, OrchSend, Tools, availability_for, tools_for},
    orchestra::{Toolset, run::Roster},
};

/// Why an orchestrator turn is refused when the user's tools are off: it
/// could not offer the orchestration tools.
const TOOLS_OFF: &str =
    "ORCH needs tools: turn them on in the menu (Ctrl+Space m), or press Ctrl+O to chat";

/// What the chat footer says about ORCH while the gate is on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OrchHint {
    /// Ctrl+O toggles the pane's mode.
    Toggle,
    /// A run is active in the pane: it must be cancelled before the mode
    /// can change.
    CancelRunFirst,
}

impl App {
    /// The global orchestrator gate (`[ai.orchestrator] enabled`), read from
    /// the live configuration every time.
    fn orch_gate(&self) -> bool {
        self.ai.orchestrator.enabled
    }

    /// Whether pane `id` orchestrates: the global gate and the pane's mode
    /// both on.
    pub fn orchestrating(&self, id: PaneId) -> bool {
        self.orch_gate()
            && self
                .panes
                .get(&id)
                .and_then(PaneState::chat_state)
                .is_some_and(ChatState::orch)
    }

    /// Whether a run is active in pane `id`: in S1 a run lives as long as
    /// its orchestrator turn.
    fn orch_run_active(&self, id: PaneId) -> bool {
        self.panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .is_some_and(ChatState::orch_turn_in_flight)
    }

    /// Whether the provider notice is shown in pane `id`.
    pub fn orch_notice(&self, id: PaneId) -> bool {
        self.orch_notice == Some(id)
    }

    /// A key on chat `id` for the provider notice shown there: Enter or `y`
    /// accepts it (written at once), Esc dismisses it (it shows again at the
    /// next run). Only a fresh, unmodified press with an empty composer
    /// answers it; `None` leaves every other key to the chat.
    pub(super) fn answer_orch_notice(
        &mut self,
        id: PaneId,
        key: &KeyEvent,
        repeat: bool,
    ) -> Option<Vec<Effect>> {
        let fresh = !repeat && key.kind == KeyEventKind::Press;
        if !self.orch_notice(id) || !fresh || key.modifiers != KeyModifiers::NONE {
            return None;
        }
        let chat = self.panes.get(&id).and_then(PaneState::chat_state)?;
        if !chat.composer().text().is_empty() {
            return None;
        }
        let accepted = match key.code {
            KeyCode::Enter | KeyCode::Char('y') => true,
            KeyCode::Esc => false,
            _ => return None,
        };
        self.orch_notice = None;
        self.dirty = true;
        if !accepted {
            return Some(Vec::new());
        }
        self.ai.orchestrator.acknowledged = true;
        self.orch_saves += 1;
        let edit = ConfigEdit::orchestrator_flag("acknowledged", true);
        Some(vec![Effect::SaveConfig(vec![edit])])
    }

    /// The result of the write of an accepted notice. A failed one persisted
    /// nothing: the notice is not accepted, so the next run shows it again,
    /// and the error is told.
    pub(super) fn on_notice_saved(&mut self, failed: Option<String>) -> Vec<Effect> {
        self.orch_saves = self.orch_saves.saturating_sub(1);
        if let Some(message) = failed {
            self.ai.orchestrator.acknowledged = false;
            self.notice = Some(message);
            self.dirty = true;
        }
        Vec::new()
    }

    /// The ORCH hint of the chat footer of pane `id`; `None` with the gate
    /// off or on a pane that is not a chat.
    pub fn orch_hint(&self, id: PaneId) -> Option<OrchHint> {
        self.panes.get(&id)?.chat_state()?;
        if !self.orch_gate() {
            return None;
        }
        Some(if self.orch_run_active(id) {
            OrchHint::CancelRunFirst
        } else {
            OrchHint::Toggle
        })
    }

    /// Ctrl+O on chat `id`: flips its ORCH mode. With the gate off it does
    /// nothing, like any unbound key, and while a run is active the mode
    /// stays (the hint says to cancel the run first).
    pub(super) fn chat_toggle_orch(&mut self, id: PaneId) -> Vec<Effect> {
        if !self.orch_gate() || self.orch_run_active(id) {
            return Vec::new();
        }
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            chat.set_orch(!chat.orch());
            self.dirty = true;
        }
        Vec::new()
    }

    /// The engine a turn of pane `id` goes to: the effective `orchestrate`
    /// assignment (own, else `general`) in an orchestrating pane, never
    /// replaced by another; the `chat` one everywhere else.
    pub(super) fn turn_availability(&self, id: PaneId) -> Availability {
        if self.orchestrating(id) {
            availability_for(
                Task::Orchestrate,
                &self.ai,
                self.engines.as_ref(),
                self.jail,
            )
        } else {
            self.chat_availability()
        }
    }

    /// Why an orchestrator turn of pane `id` on a ready engine cannot go: it
    /// could not offer the orchestration tools. `None` for any other pane.
    pub(super) fn orch_refusal(&self, id: PaneId) -> Option<String> {
        if !self.orchestrating(id) {
            return None;
        }
        let Availability::Ready { engine, .. } = self.turn_availability(id) else {
            return None;
        };
        if !self.ai.tools_enabled() {
            return Some(TOOLS_OFF.to_owned());
        }
        (tools_for(&engine, true, None, self.tool_engines) == Tools::Off).then(|| {
            format!(
                "{engine} cannot use tools in lazarobox yet, so it cannot orchestrate: \
                 assign another engine to orchestrate (Ctrl+Space m), or press Ctrl+O to chat"
            )
        })
    }

    /// The effect that starts turn `turn` of pane `id`: an orchestrator turn
    /// with the roster in an orchestrating pane, a chat turn as before
    /// everywhere else.
    pub(super) fn turn_effect(
        &mut self,
        id: PaneId,
        turn: u64,
        mut request: ChatRequest,
    ) -> Effect {
        if !self.orchestrating(id) {
            return Effect::ChatSend {
                pane: id,
                turn,
                request,
            };
        }
        // The orchestrator session: the read tools and the four orchestration
        // tools, never `run_in_pane`.
        request.families = Toolset::Orchestrator.families();
        let roster = Roster::new(
            Task::ALL
                .iter()
                .map(|&task| {
                    let engines = self.engines.as_ref();
                    (task, availability_for(task, &self.ai, engines, self.jail))
                })
                .collect(),
        );
        let deadline = Duration::from_secs(u64::from(self.ai.orchestrator.worker_timeout_secs));
        let chat = self.panes.get_mut(&id).and_then(PaneState::chat_mut);
        let profile = chat
            .as_ref()
            .map_or_else(|| self.ai.profile(), |c| c.profile());
        if let Some(chat) = chat {
            chat.set_orch_turn(turn);
        }
        // The run starts now, whatever the notice's answer (it never blocks).
        if !self.ai.orchestrator.acknowledged {
            self.orch_notice = Some(id);
            self.dirty = true;
        }
        Effect::OrchestrateSend {
            pane: id,
            turn,
            request,
            orch: Box::new(OrchSend {
                roster,
                profile,
                deadline,
            }),
        }
    }
}
