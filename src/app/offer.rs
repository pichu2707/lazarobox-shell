//! The `engram serve` offer (engram-memory M5 worker side, M6 app side). The
//! worker makes the offer once per run; the app holds it until a chat view is
//! in front of the user, shows it there, and the next key answers it: an
//! unmodified `y` starts the server, anything else declines.

use super::*;
use crate::core::ai::{
    chat::memory::Readiness,
    memory::{
        offer::Offer,
        port::{Reason, ServerOutcome},
    },
};

impl App {
    /// The worker found nothing on the port and a usable `engram`. Held for
    /// the run; shown at once when a chat is focused, else when a chat view
    /// opens or a memory key is pressed. Ignored with memory off, and after
    /// the run's one offer.
    pub(super) fn on_server_offer(&mut self, offer: &Offer) -> Vec<Effect> {
        if !self.ai.memory.active() || !self.server_offer.accept(offer.clone()) {
            return Vec::new();
        }
        if self.focused_is_chat() {
            self.show_server_offer();
        }
        Vec::new()
    }

    /// Puts the held offer in front of the user, in the focused chat. Nothing
    /// when none is held or it is already shown.
    pub(super) fn show_server_offer(&mut self) {
        if !self.ai.memory.active()
            || self.server_offer.pending().is_none()
            || !self.focused_is_chat()
        {
            return;
        }
        self.offer_shown = true;
        self.offer_pane = Some(self.tab().focus);
        self.dirty = true;
    }

    /// The question to show in chat `id`, while the offer is on screen there.
    pub(super) fn server_offer_prompt(&self, id: PaneId) -> Option<String> {
        if !self.ai.memory.active() || !self.offer_shown || self.offer_pane != Some(id) {
            return None;
        }
        self.server_offer.pending().map(Offer::prompt)
    }

    /// A key with the offer on screen: it answers and goes no further. `y`
    /// starts the server; every other key declines. Returns `None` when no
    /// offer is on screen for the focused chat (the key is for the chat).
    pub(super) fn answer_server_offer(&mut self, key: KeyEvent) -> Option<Vec<Effect>> {
        let id = self.tab().focus;
        self.server_offer_prompt(id)?;
        self.offer_shown = false;
        self.dirty = true;
        let start = self.server_offer.answer(key);
        Some(start.map(Effect::Memory).into_iter().collect())
    }

    /// How `engram serve` ended. A started server is a system line in the chat
    /// that showed the offer (a notice when it is gone); a failure says why.
    /// Views that found engram down look again.
    pub(super) fn on_server_outcome(&mut self, outcome: &ServerOutcome) -> Vec<Effect> {
        let line = match outcome {
            ServerOutcome::Failed(reason) => Some(reason.line()),
            other => other.notice(),
        };
        if let Some(line) = line {
            let target = self
                .offer_pane
                .filter(|id| self.panes.get(id).is_some_and(|p| p.chat_state().is_some()))
                .or_else(|| self.first_chat());
            match target.and_then(|id| self.panes.get_mut(&id).and_then(PaneState::chat_mut)) {
                Some(chat) => chat.push_system(line),
                None => self.notice = Some(line),
            }
            self.dirty = true;
        }
        match outcome {
            ServerOutcome::Started { .. } | ServerOutcome::Adopted => {
                // A server answers now: whatever kept one from being offered
                // is history.
                self.no_server = None;
                self.look_again()
            }
            ServerOutcome::Failed(_) => Vec::new(),
        }
    }

    /// The first chat pane, by id.
    fn first_chat(&self) -> Option<PaneId> {
        self.panes
            .iter()
            .filter(|(_, pane)| pane.chat_state().is_some())
            .map(|(id, _)| *id)
            .min()
    }

    /// A server answers on the port now: every view that found engram not
    /// running asks for its project again (its context follows as for any
    /// view). The line that said engram was down stays: it was true then.
    fn look_again(&mut self) -> Vec<Effect> {
        let now = (self.clock)();
        let down = Reason::NotRunning.text();
        let mut ids: Vec<PaneId> = self.panes.keys().copied().collect();
        ids.sort();
        let mut effects = Vec::new();
        for id in ids {
            let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
                continue;
            };
            if chat.readiness() != &Readiness::Unavailable(down.clone()) {
                continue;
            }
            let Some(cwd) = chat.origin_cwd().map(|p| p.to_string_lossy().into_owned()) else {
                continue;
            };
            chat.start_resolving_at(false, now);
            effects.push(Effect::Memory(MemCmd::Resolve { pane: id, cwd }));
        }
        effects
    }
}
