//! The GitHub star prompt, app side (goal #744). The runtime's start-up check
//! answers once with `AppEvent::StarChecked`; when it says to ask, one muted
//! line on the composer hint of the focused chat asks for a star, only while
//! that chat is idle and ready, its composer empty, nothing else answers keys
//! there, and no approval waits in any pane. A fresh, unmodified `g` stars the
//! repository with the user's `gh` (or opens its page without `gh`); any other
//! key dismisses the prompt and still reaches the chat. Either answer ends the
//! prompt for this run, and the runtime writes the mark (see `core::star`).

use super::*;
use crate::core::ai::chat::{Phase, guidance::guidance};
use crate::core::star::{Due, REPO, REPO_URL, StarReply};

/// The prompt when `g` stars with the user's `gh`.
pub(super) const ASK_GH: &str =
    "Enjoying lazarobox? g: star it on GitHub with gh · any other key: not now";

/// The prompt when there is no logged-in `gh`: `g` opens the page.
pub(super) const ASK_PAGE: &str =
    "Enjoying lazarobox? g: open its GitHub page to star it · any other key: not now";

/// Where the star prompt stands this run.
#[derive(Default)]
pub(super) struct StarPrompt {
    /// The start-up check said to ask: whether `g` stars with `gh`.
    due: Option<bool>,
    /// The chat that answered, where the outcome of the star is told.
    pane: Option<PaneId>,
}

impl App {
    /// The start-up check answered. Held for the run until answered.
    pub(super) fn on_star_checked(&mut self, due: Due) -> Vec<Effect> {
        if let Due::Ask { gh } = due {
            self.star.due = Some(gh);
            self.dirty = true;
        }
        Vec::new()
    }

    /// The prompt for chat `id`, when it may show there now.
    pub(super) fn star_prompt(&self, id: PaneId) -> Option<&'static str> {
        let gh = self.star.due?;
        let chat = self.panes.get(&id)?.chat_state()?;
        let any_approval = self.panes.keys().any(|pane| self.approval_pending(*pane));
        let quiet = chat.phase() == Phase::Idle
            && chat.composer().text().is_empty()
            && !any_approval
            && !self.picker_open(id)
            && !self.folder_pending(id)
            && !self.run_view_open(id)
            && !self.list_open(id)
            && !self.cost_open(id)
            && !self.memory_view_open(id)
            && !self.tag_editor_open(id)
            // Whatever else answers keys, or says the chat cannot send yet,
            // comes first.
            && self.server_offer_prompt(id).is_none()
            && self.orch_guidance(id).is_none()
            && self.orch_refusal(id).is_none()
            && guidance(&self.turn_availability(id)).is_none()
            && self.delegate_line(id).is_none();
        (id == self.tab().focus && quiet).then_some(if gh { ASK_GH } else { ASK_PAGE })
    }

    /// A key on chat `id` with the prompt on screen. A fresh, unmodified `g`
    /// answers and goes no further (`true`). Any other key dismisses the
    /// prompt and stays for the chat (`false`). A held key, a modifier alone
    /// or no prompt on screen changes nothing.
    pub(super) fn answer_star(
        &mut self,
        id: PaneId,
        key: &KeyEvent,
        repeat: bool,
    ) -> (Vec<Effect>, bool) {
        if repeat
            || key.kind != KeyEventKind::Press
            || matches!(key.code, KeyCode::Modifier(_))
            || self.star_prompt(id).is_none()
        {
            return (Vec::new(), false);
        }
        let Some(gh) = self.star.due.take() else {
            return (Vec::new(), false);
        };
        self.dirty = true;
        if key.code != KeyCode::Char('g') || key.modifiers != KeyModifiers::NONE {
            return (vec![Effect::StarMark { dismissed: true }], false);
        }
        self.star.pane = Some(id);
        if gh {
            return (vec![Effect::StarRepo], true);
        }
        (self.open_repo_page(None), true)
    }

    /// What became of `Effect::StarRepo`: a line in the chat that asked. A
    /// failure opens the repository page instead.
    pub(super) fn on_starred(&mut self, reply: &StarReply) -> Vec<Effect> {
        match reply {
            StarReply::Starred => {
                self.tell_star(format!("Starred {REPO} on GitHub. Thank you!"));
                Vec::new()
            }
            StarReply::NoGh => self.open_repo_page(Some("gh is not logged in")),
            StarReply::Failed(message) => self.open_repo_page(Some(message)),
        }
    }

    /// Opens the repository page (or names it without an opener) and marks
    /// the prompt answered for good. `failure` is why `gh` could not star it.
    fn open_repo_page(&mut self, failure: Option<&str>) -> Vec<Effect> {
        let lead =
            failure.map_or_else(String::new, |why| format!("gh could not star it ({why}). "));
        let mut effects = vec![Effect::StarMark { dismissed: false }];
        if self.opener {
            self.tell_star(format!("{lead}Opened {REPO_URL}: thank you for the star!"));
            effects.push(Effect::OpenUrl(REPO_URL.to_owned()));
        } else {
            self.tell_star(format!("{lead}Star lazarobox at {REPO_URL}. Thank you!"));
        }
        effects
    }

    /// A system line in the chat that answered, else a notice.
    fn tell_star(&mut self, line: String) {
        match self
            .star
            .pane
            .and_then(|id| self.panes.get_mut(&id))
            .and_then(PaneState::chat_mut)
        {
            Some(chat) => chat.push_system(line),
            None => self.notice = Some(line),
        }
        self.dirty = true;
    }
}
