//! The app's side of a model's tool calls that need the user: a call arrives
//! as a `ToolCall` event with an ask, the approval overlay of the chat shows
//! it, and the user's answer leaves as `Effect::ToolDecision` under the call's
//! pane, turn and id.
//!
//! `on_tool_ask` is the single place an ask enters the app. It always opens
//! the overlay, unless `approval::answers_itself` lets a read or a search go
//! (`run`: in the chat's root), or `approval::route` says the command can be
//! typed at once (`auto`, D18) or run after a check (`run`, D21). A user tool
//! and the other commands always wait for `y`.

use super::*;
use crate::core::ai::chat::{
    approval::{
        Approval, ApprovalAsk, AutoCtx, Origin, Route, answers_itself, counts_toward_pause,
        reason_line, route,
    },
    engine::{CallId, Decision, ToolStatus, Tools, tools_for},
};
use crate::core::ai::{
    config::ApprovalMode,
    tools::{exec::Fallback, registry::FamilySet},
};

/// Why a call is denied without the user being asked, or when a refusal has no
/// other reason to give. Short, and it goes back to the model.
pub(super) const DECLINED: &str = "the user declined";
pub(super) const CANCELLED: &str = "cancelled";
const TOOLS_OFF: &str = "tools are off";
const APPROVAL_CHAT_ONLY: &str = "The approval mode is for chat panes; open one with Ctrl+Space c";
const APPROVAL_TOOLS_OFF: &str = "Tools are off: there is no approval mode to change";
const TURN_OVER: &str = "the turn is over";
const NO_SHELL: &str = "there is no shell pane in this tab";
const CHAT_GONE: &str = "the chat is gone";

/// What a tool event says about a call that may wait for the user.
pub(super) enum ToolEvent {
    /// The call is announced and needs the user.
    Ask(CallId, ApprovalAsk),
    /// The call ended: whatever waited for it is stale.
    Over(CallId, ToolStatus),
}

impl ToolEvent {
    pub(super) fn of(event: &StreamEvent) -> Option<Self> {
        match event {
            StreamEvent::ToolCall {
                id,
                approval: Some(ask),
                ..
            } => Some(Self::Ask(*id, (**ask).clone())),
            StreamEvent::ToolResult { id, status, .. } => Some(Self::Over(*id, *status)),
            _ => None,
        }
    }
}

impl App {
    /// The answer to the call an approval waits for, if it waits for one.
    pub(super) fn answer(pane: PaneId, approval: &Approval, decision: Decision) -> Option<Effect> {
        let (turn, call) = approval.call()?;
        Some(Effect::ToolDecision {
            pane,
            turn,
            call,
            decision,
        })
    }

    pub(super) fn deny(pane: PaneId, turn: u64, call: CallId, reason: &'static str) -> Vec<Effect> {
        vec![Effect::ToolDecision {
            pane,
            turn,
            call,
            decision: Decision::Denied(reason),
        }]
    }

    /// What a tool event of turn `turn` does to the approvals of chat `id`.
    /// `live` is whether the turn is the one in flight; `finished` whether this
    /// event ended it.
    pub(super) fn on_tool_event(
        &mut self,
        id: PaneId,
        turn: u64,
        (live, finished): (bool, bool),
        event: Option<ToolEvent>,
    ) -> Vec<Effect> {
        // The turn is over: the runtime denies what still waits, so the prompt
        // has nothing left to answer.
        if finished {
            self.withdraw_tool_approval(id, |(t, _)| t == turn);
            self.drop_run_slots(|slot| slot.chat == id && slot.turn == turn);
        }
        match event {
            Some(ToolEvent::Over(call, status)) => {
                self.withdraw_tool_approval(id, |key| key == (turn, call));
                let ran = self
                    .drop_run_slots(|slot| (slot.chat, slot.turn, slot.call) == (id, turn, call));
                if ran
                    && status == ToolStatus::Ok
                    && let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut)
                {
                    chat.clear_run_fallbacks();
                }
                // A refused write left nothing on the line.
                if status == ToolStatus::Denied {
                    self.clear_line_pending(|marked| marked == call);
                }
                Vec::new()
            }
            Some(ToolEvent::Ask(call, ask)) if live => self.on_tool_ask(id, turn, call, ask),
            Some(ToolEvent::Ask(call, _)) => Self::deny(id, turn, call, TURN_OVER),
            None => Vec::new(),
        }
    }

    /// A tool call needs the user: opens the approval overlay for it, or
    /// denies it at once (tools off, another approval pending, nowhere to type
    /// a command).
    pub(super) fn on_tool_ask(
        &mut self,
        id: PaneId,
        turn: u64,
        call: CallId,
        ask: ApprovalAsk,
    ) -> Vec<Effect> {
        if !self.ai.tools_enabled() {
            return Self::deny(id, turn, call, TOOLS_OFF);
        }
        // A read or a search the mode answers needs no overlay, so a pending
        // approval does not stand in its way. A reply that lets it go is not
        // the user's `y`.
        let mode = (self.panes.get(&id))
            .and_then(PaneState::chat_state)
            .map(ChatState::approval_mode);
        if let Some(decision) = mode.and_then(|mode| answers_itself(&ask, mode)) {
            return vec![Effect::ToolDecision {
                pane: id,
                turn,
                call,
                decision,
            }];
        }
        let target = match &ask {
            ApprovalAsk::Run { .. } => {
                let target = self
                    .tab_of(id)
                    .and_then(|tab| self.default_run_target_in(tab));
                if target.is_none() {
                    return Self::deny(id, turn, call, NO_SHELL);
                }
                target
            }
            // A user tool and a command are never routed by the approval mode (D18): only a
            // `run` can be typed or run by itself. A read or a search the mode
            // did not let go above opens the prompt.
            ApprovalAsk::Read { .. }
            | ApprovalAsk::Search { .. }
            | ApprovalAsk::External { .. }
            | ApprovalAsk::Exec { .. }
            | ApprovalAsk::GitDiff { .. }
            | ApprovalAsk::Batch(_) => None,
        };
        let mut why = None;
        if let (ApprovalAsk::Run { command }, Some(target)) = (&ask, target) {
            match self.auto_route(id, target, &ask, false) {
                Route::AutoType => return self.auto_type(id, (turn, call), target, command),
                Route::AutoRun(safe) => {
                    return self.check_run(id, (turn, call), target, safe.as_str());
                }
                Route::Prompt(fallback) => why = self.fallback_why(id, fallback, command),
            }
        }
        self.open_prompt(id, (turn, call), target, ask, why)
    }

    /// Opens the approval overlay of the call for `ask`, aimed at `target`.
    /// `why` says why a `run` command was not run directly. A request refused
    /// (another approval pending) denies the call.
    pub(super) fn open_prompt(
        &mut self,
        id: PaneId,
        (turn, call): (u64, CallId),
        target: Option<PaneId>,
        ask: ApprovalAsk,
        why: Option<String>,
    ) -> Vec<Effect> {
        let armed_at = (self.clock)() + ask.arm_delay();
        let batch = match &ask {
            ApprovalAsk::Batch(review) => Some(review.clone()),
            _ => None,
        };
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Self::deny(id, turn, call, CHAT_GONE);
        };
        match chat.approvals_mut().request_call(ask, turn, call, why) {
            Ok(_) => {
                chat.set_run_target(target);
                chat.set_armed_at(Some(armed_at));
                if let Some(review) = batch {
                    self.open_review(id, review);
                }
                self.dirty = true;
                Vec::new()
            }
            Err(refusal) => Self::deny(id, turn, call, refusal.reason()),
        }
    }

    /// The reason a `run` command takes the prompt, and the pause count: a
    /// fallback that is the model's doing brings the pause one step closer.
    pub(super) fn fallback_why(
        &mut self,
        id: PaneId,
        fallback: Option<Fallback>,
        command: &str,
    ) -> Option<String> {
        let fallback = fallback?;
        if counts_toward_pause(&fallback)
            && let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut)
        {
            chat.note_run_fallback();
        }
        Some(reason_line(&fallback, command))
    }

    /// What chat `id` does with a model's `ask` for `target`: the mode and the
    /// flood limits decide (see `approval::route`). A pending approval is not
    /// the mode's to answer, so it prompts, and the request is then refused.
    /// `rechecking` is the route after the check of a run, whose own count is
    /// already taken.
    pub(super) fn auto_route<'a>(
        &self,
        id: PaneId,
        target: PaneId,
        ask: &'a ApprovalAsk,
        rechecking: bool,
    ) -> Route<'a> {
        let Some(chat) = self.panes.get(&id).and_then(PaneState::chat_state) else {
            return Route::Prompt(None);
        };
        if chat.approvals().pending().is_some() {
            return Route::Prompt(None);
        }
        let pty = self.panes.get(&target).and_then(PaneState::pty);
        let ctx = AutoCtx {
            typed_this_turn: chat.auto_typed().saturating_sub(u8::from(rechecking)),
            line_pending: pty.is_some_and(|pty| pty.line_pending.is_some()),
            target_focused: self.tab().focus == target,
            // A zoomed tab draws only the focused pane, never the target.
            tab_shown: !self.tab().zoom && self.tab().tree.leaves().contains(&id),
            executing: pty.is_some_and(|pty| pty.run.is_some()),
            cwd_known: self.cwd_readable && pty.is_some_and(|pty| pty.proc_cwd.is_some()),
            paused: chat.run_paused(),
            dirty: pty.is_some_and(|pty| pty.user_dirty),
            utf8: self.utf8_locale,
        };
        route(ask, Origin::Tool, chat.approval_mode(), ctx)
    }

    /// Types a model's command into `target` without a prompt: the same
    /// checks as `y`, then one `Effect::AutoType` and the marks that stop the
    /// next command from landing on this line, all before the runtime acts.
    /// A check that fails denies the call with its reason.
    fn auto_type(
        &mut self,
        id: PaneId,
        (turn, call): (u64, CallId),
        target: PaneId,
        command: &str,
    ) -> Vec<Effect> {
        let bytes = match self.typed_bytes(target, command) {
            Ok(bytes) => bytes,
            Err(refused) => return Self::deny(id, turn, call, refused.reason),
        };
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            chat.note_auto_typed();
        }
        if let Some(pty) = self.panes.get_mut(&target).and_then(PaneState::pty_mut) {
            pty.line_pending = Some(call);
        }
        self.dirty = true;
        vec![Effect::AutoType {
            chat: id,
            target,
            bytes,
            number: self.pane_number(target).unwrap_or(0),
            turn,
            call,
        }]
    }

    /// `Prefix a`: cycles the focused chat through `ask`, `auto` and `run`.
    /// Works in any chat view and with an approval pending (which it leaves
    /// alone); a shell pane, or tools off, only gets a notice.
    pub(super) fn toggle_approval(&mut self) -> Vec<Effect> {
        if !self.focused_is_chat() {
            self.notice = Some(APPROVAL_CHAT_ONLY.to_owned());
            return Vec::new();
        }
        if !self.ai.tools_enabled() {
            self.notice = Some(APPROVAL_TOOLS_OFF.to_owned());
            return Vec::new();
        }
        let Some(chat) = self.focused_state_mut().chat_mut() else {
            return Vec::new();
        };
        let mode = chat.approval_mode().cycled(true);
        chat.set_approval_mode(mode);
        chat.push_system_line(match mode {
            ApprovalMode::Auto => "approval: auto (commands are typed, never run)".to_owned(),
            ApprovalMode::Run => "approval: run (cd, pwd, ls, tree, find, grep, rg and reads inside the chat's folder run at once and their output goes to the model; a cloud chat whose folder is your home or / asks for them; paths outside the folder and other commands, git and mkdir included, ask)".to_owned(),
            ApprovalMode::Ask => "approval: ask".to_owned(),
        });
        self.dirty = true;
        Vec::new()
    }

    /// Clears the line-pending mark of every shell pane whose marked call
    /// `stale` names.
    fn clear_line_pending(&mut self, stale: impl Fn(CallId) -> bool) {
        for pty in self.panes.values_mut().filter_map(PaneState::pty_mut) {
            if pty.line_pending.is_some_and(&stale) {
                pty.line_pending = None;
            }
        }
    }

    /// The user sends a key to the focused pane. Enter, Ctrl+C and Ctrl+U end
    /// the line an auto-typed command waited on. Every key marks the pane
    /// dirty; only Enter and Ctrl+C clear it. Ctrl+U does not: a shell may kill
    /// only the text left of the cursor (design D21).
    pub(super) fn note_user_key(&mut self, key: &KeyEvent) {
        let ctrl =
            |c| key.code == KeyCode::Char(c) && key.modifiers.contains(KeyModifiers::CONTROL);
        // Exact modifiers: Alt+Enter, Shift+Enter and Ctrl+Alt+C end no line.
        let enter = key.code == KeyCode::Enter && key.modifiers.is_empty();
        let menu = matches!(
            key.code,
            KeyCode::Tab
                | KeyCode::BackTab
                | KeyCode::Up
                | KeyCode::Down
                | KeyCode::Left
                | KeyCode::Right
        );
        if let Some(pty) = self.focused_state_mut().pty_mut() {
            // After Tab or an arrow, Enter may only accept a menu entry and
            // leave the line in place: stay dirty until the next bare Enter.
            let ends_line = (enter && !pty.after_menu)
                || (key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL);
            pty.after_menu = menu;
            if key.code == KeyCode::Enter || ctrl('c') || ctrl('u') {
                pty.line_pending = None;
            }
            pty.user_dirty = !ends_line;
        }
        self.note_typed(self.tab().focus);
    }

    /// Marks pane `id` dirty: the user's paste, wheel or prefix literal
    /// reached its line.
    pub(super) fn mark_dirty(&mut self, id: PaneId) {
        if let Some(pty) = self.panes.get_mut(&id).and_then(PaneState::pty_mut) {
            pty.user_dirty = true;
        }
    }

    /// Ends the pending approval of chat `id` when it belongs to a tool call
    /// that `stale` names. A confirm the user opened is never touched.
    fn withdraw_tool_approval(&mut self, id: PaneId, stale: impl Fn((u64, CallId)) -> bool) {
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return;
        };
        let Some(approval) = chat.approvals().pending() else {
            return;
        };
        if approval.call().is_some_and(stale) {
            let approval_id = approval.id();
            chat.approvals_mut().withdraw(approval_id);
            chat.set_run_target(None);
            self.dirty = true;
        }
    }

    /// The request of a turn just sent from chat `id` gets the tools its
    /// engine is verified for and, when it offers tools, the families of the
    /// saved toggles, and the chat remembers that it may hold a tool session
    /// to end.
    pub(super) fn offer_tools(&mut self, id: PaneId, request: &mut ChatRequest) {
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return;
        };
        request.tools = tools_for(
            &request.engine,
            self.ai.tools_enabled(),
            chat.origin_cwd().map(Path::to_path_buf),
            self.tool_engines,
        );
        request.families = if request.tools == Tools::Off {
            FamilySet::LEGACY
        } else {
            self.ai.families()
        };
        chat.set_tool_session(request.tools != Tools::Off);
    }

    /// Stops the turn of chat `id`, and ends its tool session when its last
    /// turn offered tools.
    pub(super) fn cancel_chat(&mut self, id: PaneId) -> Vec<Effect> {
        self.drop_run_slots(|slot| slot.chat == id);
        let session = self
            .panes
            .get_mut(&id)
            .and_then(PaneState::chat_mut)
            .is_some_and(ChatState::take_tool_session);
        Self::stop_chat(id, session)
    }

    pub(super) fn stop_chat(id: PaneId, session: bool) -> Vec<Effect> {
        let mut effects = vec![Effect::ChatCancel(id)];
        if session {
            effects.push(Effect::EndToolSession(id));
        }
        effects
    }

    /// Whether the keys answer the prompt of chat `id`.
    pub(super) fn approval_armed(&self, id: PaneId) -> bool {
        let Some(chat) = self.panes.get(&id).and_then(PaneState::chat_state) else {
            return true;
        };
        let model_opened = chat
            .approvals()
            .pending()
            .is_some_and(|approval| approval.origin() == Origin::Tool);
        !model_opened || chat.armed_at().is_none_or(|at| (self.clock)() >= at)
    }

    /// The tab that holds pane `id`.
    pub(super) fn tab_of(&self, id: PaneId) -> Option<&Tab> {
        self.tabs.iter().find(|tab| tab.tree.leaves().contains(&id))
    }
}
