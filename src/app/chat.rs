//! The live chat pane: opening it, its key table, yank, scrolling and the
//! engine check before a send.

use super::*;
use crate::core::ai::chat::{
    SendOutcome,
    engine::{Availability, Tools, availability, tools_for},
    folder::{Command, Purpose, parse_command},
    guidance::guidance,
    memory::MemBadge,
    prompts::Resolved,
    streams::StreamInfo,
    usage_view::{Billing, today_segment},
    yank,
};

/// What the last store answer said about the local day: the status segment's
/// figures. They come from the store thread, so drawing never reads SQLite.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) struct UsageToday {
    pub(super) tokens: Option<u64>,
    /// Dollars the engines reported today, per engine.
    pub(super) cost: Vec<(String, u64)>,
    /// The wall-clock second they stop being today's (the next local midnight).
    pub(super) until: i64,
}

impl App {
    /// `Prefix c`: a chat pane as a right split of the focused pane, opened
    /// from that pane's cwd. A split the layout refuses opens nothing. An
    /// opened chat asks for one engine detection (single flight).
    pub(super) fn open_chat_pane(&mut self) -> Vec<Effect> {
        let origin = self.focused_state().effective_cwd().map(Path::to_path_buf);
        let before = self.panes.len();
        let chat = self.new_chat_state(origin.clone());
        let mut effects = self.split_with(Axis::X, NewPane::Chat(Box::new(chat)));
        if self.panes.len() > before {
            effects.extend(self.request_detection());
            // Detection needs no folder. The rest waits until the folder is
            // settled: a broad or unknown one asks the user first.
            let pane = self.tab().focus;
            self.opening.insert(pane);
            match origin {
                Some(path) => effects.push(Effect::ProbeFolder {
                    pane,
                    path,
                    purpose: Purpose::Open,
                }),
                None => effects.extend(self.open_folder_picker(pane)),
            }
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

    /// The memory badge for the statusline: only over a focused chat with
    /// memory active.
    pub fn status_memory(&self) -> Option<MemBadge> {
        if !self.ai.memory.active() {
            return None;
        }
        self.focused_state().chat_state().map(ChatState::mem_badge)
    }

    /// The usage segment for the statusline (` · today: 1234 tok`): only over
    /// a focused chat, with the setting on, and once the store has answered.
    /// Dollars are the spend of the engines in `per-token` mode.
    pub fn status_usage(&self) -> Option<String> {
        let usage = self.ai.usage();
        if !usage.statusline() {
            return None;
        }
        self.focused_state().chat_state()?;
        let today = self.usage_today.as_ref()?;
        // Yesterday's figure is never shown.
        if (self.wall)() >= today.until {
            return None;
        }
        let spend = today
            .cost
            .iter()
            .filter(|(engine, _)| usage.billing(engine) == Some(Billing::PerToken))
            .map(|(_, micros)| *micros)
            .reduce(u64::saturating_add)
            .filter(|micros| *micros > 0);
        Some(today_segment(today.tokens, spend))
    }

    /// Asks the store thread for today's figures when they are missing or no
    /// longer today's: the first time a chat is focused with history on, and
    /// again after the local midnight. One read at a time, and a failed one
    /// waits a minute. Nothing here touches SQLite.
    pub(super) fn usage_read_effects(&mut self) -> Vec<Effect> {
        if !self.history || self.usage_read_pending || !self.ai.usage().statusline() {
            return Vec::new();
        }
        let focus = self.tab().focus;
        if self
            .panes
            .get(&focus)
            .and_then(PaneState::chat_state)
            .is_none()
            || (self.wall)() < self.usage_next_read
        {
            return Vec::new();
        }
        self.usage_read_pending = true;
        vec![Effect::Store(StoreCmd::UsageToday { pane: focus })]
    }

    /// How full the focused chat's context was at its last send, in percent.
    pub fn status_context(&self) -> Option<u8> {
        let Availability::Ready { engine, model } = self.chat_availability() else {
            return None;
        };
        self.focused_state()
            .chat_state()
            .and_then(|chat| chat.context_percent(&engine, model.as_deref()))
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
    /// PTY, and a key the table does not list is swallowed. `repeat` marks a
    /// held key, a Repeat or the same key again within the hold window (see
    /// `Hold`): it keeps editing and scrolling but never sends, cancels,
    /// yanks, toggles, saves or answers. Ctrl+H is Backspace in every field.
    pub(super) fn on_chat_key(&mut self, key: KeyEvent, repeat: bool) -> Vec<Effect> {
        let key = backspace_alias(key);
        let id = self.tab().focus;
        // A pending approval takes every key first.
        if self.has_approval(id) {
            return self.on_approval_key(id, key, repeat);
        }
        // The folder comes first: the picker takes every key, and a chat
        // still probing its folder takes none.
        if self.picker_open(id) {
            return self.on_picker_key(id, &key, repeat);
        }
        if self.folder_pending(id) {
            return Vec::new();
        }
        // The `engram serve` question, when it is on screen, takes the key. It
        // is drawn in the conversation view only, so a list does not give up
        // its keys to a question it hides. A held key never answers it.
        let offered = if repeat {
            KeyEvent {
                kind: KeyEventKind::Repeat,
                ..key
            }
        } else {
            key
        };
        if !self.memory_view_open(id)
            && !self.tag_editor_open(id)
            && !self.list_open(id)
            && !self.cost_open(id)
            && let Some(effects) = self.answer_server_offer(offered)
        {
            return effects;
        }
        let mods = key.modifiers;
        let plain = mods - KeyModifiers::SHIFT == KeyModifiers::NONE;
        // The tag editor is drawn first, so it takes the keys first.
        if self.tag_editor_open(id) {
            return self.on_tag_key(id, &key, repeat);
        }
        if self.note_editor_open(id) {
            let effects = self.on_note_key(id, &key, repeat);
            return self.with_waiting_sends(effects);
        }
        if self.summary_draft_open(id) {
            let effects = self.on_summary_key(id, &key, repeat);
            return self.with_waiting_sends(effects);
        }
        if self.list_open(id) {
            return self.on_list_key(id, &key, repeat);
        }
        if self.cost_open(id) {
            return self.on_cost_key(id, &key);
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
            (KeyCode::Char('g'), KeyModifiers::CONTROL) => one_shot(App::chat_review_orphan, self),
            (KeyCode::PageUp, KeyModifiers::NONE) => self.chat_page(true),
            (KeyCode::PageDown, KeyModifiers::NONE) => self.chat_page(false),
            (KeyCode::Enter, KeyModifiers::ALT) | (KeyCode::Char('j'), KeyModifiers::CONTROL) => {
                self.edit_composer(|c| c.newline())
            }
            (KeyCode::Tab, KeyModifiers::NONE) => one_shot(App::cd_complete, self),
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
        let id = self.tab().focus;
        // A paste never answers an approval and never lands in the composer
        // while one waits.
        if self.has_approval(id) {
            return;
        }
        if self.picker_open(id) {
            self.picker_paste(id, text);
            return;
        }
        // A chat that waits for its folder takes no text either.
        if self.folder_pending(id) {
            return;
        }
        if self.tag_editor_open(id) {
            self.tag_paste(id, text);
            return;
        }
        // The list and the cost view take keys only: a paste is not one.
        if self.list_open(id) || self.cost_open(id) {
            return;
        }
        if self.note_editor_open(id) {
            self.note_paste(id, text);
            return;
        }
        if self.summary_draft_open(id) {
            self.summary_paste(id, text);
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
    pub(super) fn chat_availability(&self) -> Availability {
        availability(&self.ai, self.engines.as_ref(), self.jail)
    }

    /// What a chat pane shows instead of a ready composer, when the engine
    /// cannot be used. `None` while it is ready or the pane is busy.
    pub fn chat_guidance(&self, id: PaneId) -> Option<String> {
        let chat = self.panes.get(&id)?.chat_state()?;
        if let Some(prompt) = self.server_offer_prompt(id) {
            return Some(prompt);
        }
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
        let mut text = chat.composer().text();
        // `/cd` is the user's own command: it never reaches the engine, and
        // works whatever the engine state.
        match parse_command(&text) {
            Command::Cd(arg) => return self.cd_submit(id, &arg),
            Command::Cost => return self.cost_submit(id),
            Command::CostArgs => {
                self.cost_refused(id);
                return Vec::new();
            }
            Command::Escaped(unescaped) => text = unescaped,
            Command::Plain => {}
        }
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
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
        // Memory off means no engram text leaves, whatever the chat holds.
        if !self.ai.memory.active() {
            self.forget_memory(id);
        }
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        let Some(system) = chat.prompt().map(str::to_owned) else {
            return Vec::new();
        };
        // The engine's own window sizes the turn; unknown keeps the char caps.
        let window = match availability {
            Availability::Ready { engine, .. } => {
                self.engines.as_ref().and_then(|d| d.window(engine))
            }
            _ => None,
        };
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        chat.set_window(window);
        // The tool schemas weigh on the budget only when the turn offers them.
        let tools = match availability {
            Availability::Ready { engine, .. } => {
                tools_for(engine, self.ai.tools_enabled(), None, self.tool_engines) != Tools::Off
            }
            _ => false,
        };
        chat.set_tools_offered(tools);
        match chat.send(text, availability, &system) {
            SendOutcome::Sent { turn, mut request } => {
                chat.composer_mut().clear();
                self.offer_tools(id, &mut request);
                self.hide_server_offer(id);
                // The conversation is saved before the engine starts.
                let mut effects = self.store_effects(id);
                effects.push(Effect::ChatSend {
                    pane: id,
                    turn,
                    request,
                });
                effects
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
                    // A note editor or summary draft owns the conversation
                    // until it closes: the send goes then.
                    && !self.memory_view_open(*id)
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

    /// `effects` followed by the parked sends that were only waiting for a
    /// memory view to close.
    pub(super) fn with_waiting_sends(&mut self, mut effects: Vec<Effect>) -> Vec<Effect> {
        effects.extend(self.run_waiting_sends());
        effects
    }

    /// The runtime prepared the system prompt of chat `id`. A pane that is
    /// gone in the meantime drops it.
    pub(super) fn on_prompt_ready(&mut self, id: PaneId, resolved: Resolved) -> Vec<Effect> {
        self.on_prompt_for(id, resolved)
    }

    /// Esc and Ctrl+C: stop the stream and drop a parked send, or do nothing
    /// when idle.
    pub(super) fn chat_cancel_key(&mut self, id: PaneId) -> Vec<Effect> {
        self.drop_parked_send(id);
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        // The line of a cancelled turn follows the settings of now: a menu
        // save may have changed them since the last event of the turn.
        chat.set_usage_config(self.ai.usage().clone());
        let follow = chat.cancel();
        // The partial answer is saved as cancelled.
        let mut effects = self.store_effects(id);
        if follow == Follow::CancelEngine {
            effects.extend(self.cancel_chat(id));
        }
        effects
    }

    /// Ctrl+Y (the last answer) and Ctrl+B (its last code block), from the
    /// composer or chat COPY.
    pub(super) fn chat_yank(&mut self, code_block: bool) -> Vec<Effect> {
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
        self.copy_to_clipboard(&text)
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

    /// `Prefix o`: unfolds or folds the memory line, keeping a scrolled view on
    /// the same content. With no memory line nothing happens and nothing is
    /// emitted.
    pub(super) fn chat_toggle_memory(&mut self) -> Vec<Effect> {
        let size = self.focused_state().size;
        let rows = self.chat_viewport(self.tab().focus);
        if let Some(chat) = self.focused_state_mut().chat_mut() {
            chat.toggle_memory_at(usize::from(size.cols), rows);
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
