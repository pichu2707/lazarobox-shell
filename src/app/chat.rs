//! The live chat pane: opening it, its key table, yank, scrolling and the
//! engine check before a send.

use super::*;
use crate::core::ai::chat::{
    SendOutcome,
    engine::{Availability, availability},
    guidance::guidance,
    yank,
};

impl App {
    /// `Prefix c`: a chat pane as a right split of the focused pane, opened
    /// from that pane's cwd. A split the layout refuses opens nothing. An
    /// opened chat asks for one engine detection (single flight).
    pub(super) fn open_chat_pane(&mut self) -> Vec<Effect> {
        let origin = self.focused_state().effective_cwd().map(Path::to_path_buf);
        let before = self.panes.len();
        let mut effects = self.split_with(Axis::X, NewPane::Chat(ChatState::new(origin)));
        if self.panes.len() > before {
            effects.extend(self.request_detection());
        }
        effects
    }

    /// Whether the focused pane is a chat.
    pub(super) fn focus_is_chat(&self) -> bool {
        self.focused_state().chat_state().is_some()
    }

    /// A key in TERMINAL with a chat pane focused. Nothing here reaches a
    /// PTY, and a key the table does not list is swallowed. A held key keeps
    /// editing and scrolling but never sends, cancels, yanks or toggles.
    pub(super) fn on_chat_key(&mut self, key: KeyEvent, repeat: bool) -> Vec<Effect> {
        let id = self.tab().focus;
        let mods = key.modifiers;
        let plain = mods - KeyModifiers::SHIFT == KeyModifiers::NONE;
        let one_shot = |effects: fn(&mut App, PaneId) -> Vec<Effect>, app: &mut App| {
            if repeat { Vec::new() } else { effects(app, id) }
        };
        match (key.code, mods) {
            (KeyCode::Enter, KeyModifiers::NONE) => one_shot(App::chat_submit, self),
            (KeyCode::Esc, KeyModifiers::NONE) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                one_shot(App::chat_cancel_key, self)
            }
            (KeyCode::Char('y'), KeyModifiers::CONTROL) => {
                one_shot(|app, _| app.chat_yank(false), self)
            }
            (KeyCode::Char('b'), KeyModifiers::CONTROL) => {
                one_shot(|app, _| app.chat_yank(true), self)
            }
            (KeyCode::Char('r'), KeyModifiers::CONTROL) => {
                one_shot(|app, _| app.chat_toggle_reasoning(), self)
            }
            (KeyCode::PageUp, KeyModifiers::NONE) => self.chat_page(true),
            (KeyCode::PageDown, KeyModifiers::NONE) => self.chat_page(false),
            (KeyCode::Enter, KeyModifiers::ALT) | (KeyCode::Char('j'), KeyModifiers::CONTROL) => {
                self.edit_composer(|c| c.newline())
            }
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => self.edit_composer(|c| c.clear()),
            (KeyCode::Char(ch), _) if plain => self.edit_composer(|c| c.insert(ch)),
            (KeyCode::Backspace, _) if plain => self.edit_composer(|c| c.backspace()),
            (KeyCode::Delete, _) if plain => self.edit_composer(|c| c.delete()),
            (KeyCode::Left, _) if plain => self.edit_composer(|c| c.left()),
            (KeyCode::Right, _) if plain => self.edit_composer(|c| c.right()),
            (KeyCode::Home, _) if plain => self.edit_composer(|c| c.home()),
            (KeyCode::End, _) if plain => self.edit_composer(|c| c.end()),
            _ => Vec::new(),
        }
    }

    /// A paste into the focused chat's composer.
    pub(super) fn chat_paste(&mut self, text: &str) {
        if let Some(chat) = self.focused_state_mut().chat_mut() {
            chat.composer_mut().paste(text);
            self.dirty = true;
        }
    }

    fn edit_composer(
        &mut self,
        edit: impl FnOnce(&mut crate::core::ai::chat::composer::Composer),
    ) -> Vec<Effect> {
        if let Some(chat) = self.focused_state_mut().chat_mut() {
            edit(chat.composer_mut());
        }
        Vec::new()
    }

    /// The engine the `chat` task resolves to right now.
    fn chat_availability(&self) -> Availability {
        availability(&self.ai, self.engines.as_ref(), self.jail)
    }

    /// What a chat pane shows instead of a ready composer, when the engine
    /// cannot be used. `None` while it is ready or the pane is busy.
    pub fn chat_guidance(&self, id: PaneId) -> Option<String> {
        let chat = self.panes.get(&id)?.chat_state()?;
        if chat.phase() != Phase::Idle {
            return None;
        }
        guidance(&self.chat_availability())
    }

    /// Enter: sends the composer text to the engine, checked again now. With
    /// the first detection still running the send waits for its result.
    fn chat_submit(&mut self, id: PaneId) -> Vec<Effect> {
        let availability = self.chat_availability();
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        if availability == Availability::Checking {
            if !chat.composer().text().trim().is_empty() && !self.waiting_sends.contains(&id) {
                self.waiting_sends.push(id);
            }
            return Vec::new();
        }
        let text = chat.composer().text();
        match chat.send(&text, &availability, "") {
            SendOutcome::Sent { turn, request } => {
                chat.composer_mut().clear();
                vec![Effect::ChatSend {
                    pane: id,
                    turn,
                    request,
                }]
            }
            SendOutcome::Refused => Vec::new(),
        }
    }

    /// The first detection arrived: the sends that waited for it go now.
    pub(super) fn run_waiting_sends(&mut self) -> Vec<Effect> {
        let waiting = std::mem::take(&mut self.waiting_sends);
        waiting
            .into_iter()
            .flat_map(|id| self.chat_submit(id))
            .collect()
    }

    /// Esc and Ctrl+C: stop the stream, or do nothing when idle.
    fn chat_cancel_key(&mut self, id: PaneId) -> Vec<Effect> {
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        match chat.cancel() {
            Follow::CancelEngine => vec![Effect::ChatCancel(id)],
            Follow::Nothing => Vec::new(),
        }
    }

    /// Ctrl+Y (the last answer) and Ctrl+B (its last code block).
    fn chat_yank(&mut self, code_block: bool) -> Vec<Effect> {
        let Some(chat) = self.focused_state().chat_state() else {
            return Vec::new();
        };
        let text = if code_block {
            yank::last_code_block(chat)
        } else {
            yank::last_answer(chat)
        };
        let Some(text) = text else {
            self.notice = Some("Nothing to copy".to_string());
            return Vec::new();
        };
        match osc52(&text) {
            Ok(sequence) => {
                self.notice = Some(match text.chars().count() {
                    1 => "Copied 1 character".to_string(),
                    n => format!("Copied {n} characters"),
                });
                vec![Effect::SetClipboard(sequence)]
            }
            Err(TooLarge) => {
                self.notice = Some("Selection too large to copy".to_string());
                Vec::new()
            }
        }
    }

    /// PageUp and PageDown: one page of the transcript.
    fn chat_page(&mut self, up: bool) -> Vec<Effect> {
        let size = self.focused_state().size;
        if let Some(chat) = self.focused_state_mut().chat_mut() {
            let rows = usize::from(chat.transcript_rows(size.cols, size.rows));
            let total = chat.lines(usize::from(size.cols)).len();
            let next = if up {
                chat.scroll().page_up(total, rows)
            } else {
                chat.scroll().page_down(total, rows)
            };
            chat.set_scroll(next);
        }
        Vec::new()
    }

    /// Ctrl+R: expands or collapses the latest reasoning block, keeping a
    /// scrolled view on the same content.
    fn chat_toggle_reasoning(&mut self) -> Vec<Effect> {
        let size = self.focused_state().size;
        if let Some(chat) = self.focused_state_mut().chat_mut() {
            let rows = usize::from(chat.transcript_rows(size.cols, size.rows));
            chat.toggle_reasoning_at(usize::from(size.cols), rows);
        }
        Vec::new()
    }
}
