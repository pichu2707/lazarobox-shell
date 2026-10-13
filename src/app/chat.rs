//! The live chat pane: opening it, its key table, yank, scrolling and the
//! engine check before a send.

use super::*;
use crate::core::ai::chat::{
    SendOutcome,
    engine::{Availability, availability},
    guidance::guidance,
    prompts::Resolved,
    streams::StreamInfo,
    yank,
};

impl App {
    /// `Prefix c`: a chat pane as a right split of the focused pane, opened
    /// from that pane's cwd. A split the layout refuses opens nothing. An
    /// opened chat asks for one engine detection (single flight).
    pub(super) fn open_chat_pane(&mut self) -> Vec<Effect> {
        let origin = self.focused_state().effective_cwd().map(Path::to_path_buf);
        let before = self.panes.len();
        let mut effects = self.split_with(Axis::X, NewPane::Chat(Box::new(ChatState::new(origin))));
        if self.panes.len() > before {
            effects.extend(self.request_detection());
            effects.push(Effect::PreparePrompt {
                pane: self.tab().focus,
                profile: self.ai.profile(),
                origin: self
                    .focused_state()
                    .chat_state()
                    .and_then(|c| c.origin_cwd().map(Path::to_path_buf)),
                context: self.ai.context(),
            });
        }
        effects
    }

    /// Whether the focused pane is a chat.
    pub fn focused_is_chat(&self) -> bool {
        self.focused_state().chat_state().is_some()
    }

    /// Whether any chat pane, in any tab, has a stream running. The runtime
    /// keeps the animation tick armed exactly while this holds.
    pub fn animating(&self) -> bool {
        self.panes
            .values()
            .filter_map(PaneState::chat_state)
            .any(|chat| chat.stream_phase().is_some())
    }

    /// The running streams of every chat pane in every tab.
    pub fn streams(&self) -> Vec<StreamInfo> {
        self.panes
            .iter()
            .filter_map(|(&pane, state)| {
                let chat = state.chat_state()?;
                Some(StreamInfo {
                    pane,
                    started: chat.stream_started()?,
                    phase: chat.stream_phase()?,
                    model: chat.stream_model().to_owned(),
                })
            })
            .collect()
    }

    /// The animation frame the indicator is drawn at.
    pub fn anim_frame(&self) -> usize {
        self.anim_frame
    }

    /// The injected clock, for the indicator's elapsed time.
    pub fn now(&self) -> Instant {
        (self.clock)()
    }

    /// An animation step: the frame advances and the screen is redrawn, but
    /// only while a stream runs (a late tick after the last one ended is
    /// dropped).
    pub(super) fn on_anim_tick(&mut self) -> Vec<Effect> {
        if self.animating() {
            self.anim_frame = self.anim_frame.wrapping_add(1);
            self.dirty = true;
        }
        Vec::new()
    }

    /// A key in TERMINAL with a chat pane focused. Nothing here reaches a
    /// PTY, and a key the table does not list is swallowed. A held key keeps
    /// editing and scrolling but never sends, cancels, yanks or toggles.
    pub(super) fn on_chat_key(&mut self, key: KeyEvent, repeat: bool) -> Vec<Effect> {
        let id = self.tab().focus;
        let mods = key.modifiers;
        let plain = mods - KeyModifiers::SHIFT == KeyModifiers::NONE;
        let debounced = matches!(
            (key.code, mods),
            (KeyCode::Esc, KeyModifiers::NONE)
                | (
                    KeyCode::Char('c' | 'y' | 'b' | 'r' | 'n' | 's'),
                    KeyModifiers::CONTROL
                )
        );
        // A held key arrives as Repeat (Kitty) or as Presses close together.
        let repeat = if debounced {
            self.held_chat_key_dropped(&key, repeat)
        } else {
            self.held_chat_key = None;
            repeat
        };
        if self.note_editor_open(id) {
            return self.on_note_key(id, &key, repeat);
        }
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
            (KeyCode::Char('n'), KeyModifiers::CONTROL) => one_shot(App::chat_note_key, self),
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

    /// Whether `key`, a debounced one-shot chat key, is part of a hold: a
    /// Repeat, or the same key again within `HELD_EXIT_WINDOW` of the last
    /// such event (every event of the hold renews the window).
    fn held_chat_key_dropped(&mut self, key: &KeyEvent, repeat: bool) -> bool {
        let now = (self.clock)();
        let held = self.held_chat_key.is_some_and(|h| {
            h.code == key.code
                && h.modifiers == key.modifiers
                && now.saturating_duration_since(h.last) <= HELD_EXIT_WINDOW
        });
        self.held_chat_key = Some(HeldExit {
            code: key.code,
            modifiers: key.modifiers,
            last: now,
        });
        repeat || held
    }

    /// A paste into the focused chat's composer.
    pub(super) fn chat_paste(&mut self, text: &str) {
        let id = self.tab().focus;
        if self.note_editor_open(id) {
            self.note_paste(id, text);
            return;
        }
        if let Some(chat) = self.focused_state_mut().chat_mut() {
            chat.composer_mut().paste(text);
            self.dirty = true;
            self.drop_parked_send(id);
        }
    }

    /// A parked send is the text as it was at Enter: editing the composer
    /// afterwards, or cancelling, drops it. The text stays in the composer.
    pub(super) fn drop_parked_send(&mut self, id: PaneId) {
        self.waiting_sends.retain(|(pane, _)| *pane != id);
    }

    fn edit_composer(
        &mut self,
        edit: impl FnOnce(&mut crate::core::ai::chat::composer::Composer),
    ) -> Vec<Effect> {
        let id = self.tab().focus;
        if let Some(chat) = self.focused_state_mut().chat_mut() {
            let before = chat.composer().text();
            edit(chat.composer_mut());
            if chat.composer().text() != before {
                self.drop_parked_send(id);
            }
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
        let text = chat.composer().text();
        if availability == Availability::Checking || chat.prompt().is_none() {
            if !text.trim().is_empty() {
                self.drop_parked_send(id);
                self.waiting_sends.push((id, text));
            }
            return Vec::new();
        }
        self.send_text(id, &text, &availability)
    }

    /// Sends `text` from pane `id` and clears its composer when it went.
    /// Nothing goes before the conversation's prompt is prepared.
    fn send_text(&mut self, id: PaneId, text: &str, availability: &Availability) -> Vec<Effect> {
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        let Some(system) = chat.prompt().map(str::to_owned) else {
            return Vec::new();
        };
        match chat.send(text, availability, &system) {
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

    /// The first detection or a conversation's prompt arrived: the sends that
    /// waited for them go now, those still missing one of them keep waiting.
    pub(super) fn run_waiting_sends(&mut self) -> Vec<Effect> {
        let availability = self.chat_availability();
        // A send whose pane is gone is dropped here, not kept.
        let (ready, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.waiting_sends)
            .into_iter()
            .filter(|(id, _)| self.panes.contains_key(id))
            .partition(|(id, _)| {
                availability != Availability::Checking
                    && self
                        .panes
                        .get(id)
                        .and_then(PaneState::chat_state)
                        .is_some_and(|chat| chat.prompt().is_some())
            });
        self.waiting_sends = waiting;
        ready
            .into_iter()
            .flat_map(|(id, text)| self.send_text(id, &text, &availability))
            .collect()
    }

    /// The runtime prepared the system prompt of chat `id`. A pane that is
    /// gone in the meantime drops it.
    pub(super) fn on_prompt_ready(&mut self, id: PaneId, resolved: Resolved) -> Vec<Effect> {
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        chat.set_prompt(resolved);
        self.dirty = true;
        self.run_waiting_sends()
    }

    /// Esc and Ctrl+C: stop the stream and drop a parked send, or do nothing
    /// when idle.
    fn chat_cancel_key(&mut self, id: PaneId) -> Vec<Effect> {
        self.drop_parked_send(id);
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

    /// The transcript rows really drawn in chat `id`: the pane less its
    /// composer and its guidance. Paging and anchoring use this height.
    pub(super) fn chat_viewport(&self, id: PaneId) -> usize {
        let guidance = self.chat_guidance(id);
        self.panes
            .get(&id)
            .and_then(|state| {
                let size = state.size;
                let chat = state.chat_state()?;
                Some(usize::from(chat.viewport_rows(
                    size.cols,
                    size.rows,
                    guidance.as_deref(),
                )))
            })
            .unwrap_or(0)
    }

    /// PageUp and PageDown: one page of the transcript.
    fn chat_page(&mut self, up: bool) -> Vec<Effect> {
        let size = self.focused_state().size;
        let rows = self.chat_viewport(self.tab().focus);
        if let Some(chat) = self.focused_state_mut().chat_mut() {
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
        let rows = self.chat_viewport(self.tab().focus);
        if let Some(chat) = self.focused_state_mut().chat_mut() {
            chat.toggle_reasoning_at(usize::from(size.cols), rows);
        }
        Vec::new()
    }
}
