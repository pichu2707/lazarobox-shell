//! The orchestrator in a chat pane (agent-orchestrator D2, D5): the pane's
//! ORCH mode, its Ctrl+O toggle, the task a send runs on, the one-time
//! provider notice (D12), the run's events and its failure alert. State and
//! keys only; the badge, the hint, the notice and the alert are drawn by the
//! UI.

use super::*;
use crate::core::ai::{
    chat::{
        Entry, OrchAlert, OrchNote,
        engine::{Availability, OrchSend, Tools, availability_for, tools_for},
        transcript::worker_label,
    },
    orchestra::{Toolset, run::Roster},
};

/// Why an orchestrator turn is refused when the user's tools are off: it
/// could not offer the orchestration tools.
const TOOLS_OFF: &str =
    "ORCH needs tools: turn them on in the menu (Ctrl+Space m), or press Ctrl+O to chat";

/// The provider notice (agent-orchestrator D12) with the keys that answer
/// it: Enter accepts, Esc dismisses (`y` is typed).
const NOTICE: &str = "Providers recommend API keys for automation; \
     consumer-account logins carry risk. Enter accept \u{b7} Esc dismiss";

/// The keys of the failure alert: `r` retries, `c` or Esc cancels.
const ALERT_KEYS: &str = "c/Esc cancel";
const RETRY_KEY: &str = "r retry";
/// Said when `r` is pressed again after a retry that failed.
const RETRY_AGAIN: &str = "r try again";
/// Said when the run manager dropped an answer: its run is over.
const NO_ORCHESTRA: &str = "orchestration is not available: your answer was not applied";
const MOVED_ON: &str = "an earlier run already moved on: your answer was not applied";

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

    /// Whether a run is active in pane `id`: from its orchestrator turn
    /// until the run's `Ended`, since a worker can outlive the turn.
    pub(super) fn orch_run_active(&self, id: PaneId) -> bool {
        self.panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .is_some_and(|chat| chat.orch_turn_in_flight() || chat.orch_run().is_some())
    }

    /// What the run manager said about the run of chat `id`. A failure shows
    /// its alert, which stays until it is answered or the run ends: rows,
    /// other output and time leave it as it is. Events of a pane that is not
    /// a chat any more are dropped.
    pub(super) fn on_orchestra(&mut self, id: PaneId, event: OrchestraEvent) -> Vec<Effect> {
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        match event {
            OrchestraEvent::Worker(row) => {
                chat.set_orch_run(Some(row.run));
                chat.worker_row(row);
            }
            OrchestraEvent::Halted {
                run,
                worker,
                class,
                text,
            } => {
                chat.set_orch_run(Some(run));
                chat.set_orch_alert(Some(OrchAlert {
                    run,
                    worker,
                    class,
                    text,
                }));
            }
            // The answer did not go through: the alert comes back and says why.
            OrchestraEvent::Declined {
                run,
                worker,
                class,
                text,
                reason,
            } => {
                chat.set_orch_run(Some(run));
                chat.set_orch_note(Some(OrchNote::RetryFailed(reason)));
                chat.set_orch_alert(Some(OrchAlert {
                    run,
                    worker,
                    class,
                    text,
                }));
            }
            // The run it names is over: the pane's own alert is not touched.
            // Said only when the pane has no other run, so the line is not
            // read as part of a newer one.
            OrchestraEvent::Dropped { run } => {
                if chat.orch_run().is_none_or(|current| current == run) {
                    chat.push_system_line(MOVED_ON.to_owned());
                }
            }
            OrchestraEvent::Unavailable => chat.push_system_line(NO_ORCHESTRA.to_owned()),
            // Only the run it names ends: a replaced run ends after the new
            // one started.
            OrchestraEvent::Ended { run } => {
                if chat.orch_run() == Some(run) {
                    chat.set_orch_run(None);
                }
                if chat.orch_alert().is_some_and(|alert| alert.run == run) {
                    chat.drop_orch_alert();
                }
            }
        }
        self.dirty = true;
        Vec::new()
    }

    /// A key on chat `id` for the failure alert shown there: `r` retries the
    /// worker, `c` or Esc cancels the run. The letters act only with an
    /// empty composer, so text being typed never picks an option (key audit,
    /// 2026-10-29). Only a fresh, unmodified press answers it; `None` leaves
    /// every other key to the chat.
    pub(super) fn answer_orch_alert(
        &mut self,
        id: PaneId,
        key: &KeyEvent,
        repeat: bool,
    ) -> Option<Vec<Effect>> {
        let fresh = !repeat && key.kind == KeyEventKind::Press;
        if !fresh || key.modifiers != KeyModifiers::NONE {
            return None;
        }
        let chat = self.panes.get_mut(&id).and_then(PaneState::chat_mut)?;
        let alert = chat.orch_alert()?;
        let empty = chat.composer().text().is_empty();
        let choice = match key.code {
            KeyCode::Char('r') if empty => UserChoice::Retry,
            KeyCode::Char('c') if empty => UserChoice::Cancel,
            KeyCode::Esc => UserChoice::Cancel,
            _ => return None,
        };
        let run = alert.run;
        chat.drop_orch_alert();
        self.dirty = true;
        Some(vec![Effect::OrchDecide {
            pane: id,
            run,
            choice,
        }])
    }

    /// What pane `id` shows above its composer about its run, in the order
    /// the keys answer: the provider notice, then the failure alert. `None`
    /// when neither is up. Like the `engram serve` offer it is guidance, so
    /// paging counts its rows.
    pub(super) fn orch_guidance(&self, id: PaneId) -> Option<String> {
        let chat = self.panes.get(&id)?.chat_state()?;
        let notice = self.orch_notice(id).then(|| NOTICE.to_owned());
        let alert = chat.orch_alert().map(|alert| alert_text(chat, alert));
        match (notice, alert) {
            (Some(notice), Some(alert)) => Some(format!("{notice} | {alert}")),
            (notice, alert) => notice.or(alert),
        }
    }

    /// Whether the provider notice is shown in pane `id`.
    pub fn orch_notice(&self, id: PaneId) -> bool {
        self.orch_notice == Some(id)
    }

    /// A key on chat `id` for the provider notice shown there: Enter accepts
    /// it (written at once; `y` is typed, user decision 2026-10-29), Esc dismisses it (it shows again at the
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
            KeyCode::Enter => true,
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

/// The failure alert as one paragraph: the run is paused, the worker by its
/// row's label and how it ended, the error as it came (a control character,
/// a line break included, drawn as a space) and the keys.
fn alert_text(chat: &ChatState, alert: &OrchAlert) -> String {
    let row = chat.entries().iter().find_map(|entry| match entry {
        Entry::Worker(row) if (row.run, row.id) == (alert.run, alert.worker) => Some(row),
        _ => None,
    });
    let (worker, ended) = row.map_or_else(
        || (format!("{}", alert.worker.0), "failed"),
        |row| (worker_label(row), row.status.label()),
    );
    let text: String = alert
        .text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let mut line = format!("run paused: worker {worker} {ended}: {text}");
    let retry = match chat.orch_note() {
        Some(OrchNote::RetryFailed(reason)) => {
            line.push_str(&format!(" \u{b7} retry failed: {reason}"));
            RETRY_AGAIN
        }
        None => RETRY_KEY,
    };
    format!("{line} \u{b7} {retry} \u{b7} {ALERT_KEYS}")
}
