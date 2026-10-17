//! The app's side of a model's tool calls that need the user: a call arrives
//! as a `ToolCall` event with an ask, the approval overlay of the chat shows
//! it, and the user's answer leaves as `Effect::ToolDecision` under the call's
//! pane, turn and id.
//!
//! `on_tool_ask` is the single place an ask enters the app. It always opens
//! the overlay, unless the chat is in `auto` mode and `approval::route` says
//! the command can be typed at once (D18).

use std::time::Duration;

use super::*;
use crate::core::ai::chat::{
    approval::{Approval, ApprovalAsk, AutoCtx, Origin, Route, route},
    engine::{CallId, Decision, ToolStatus, Tools, tools_for},
};
use crate::core::ai::config::ApprovalMode;

/// Why a call is denied without the user being asked, or when a refusal has no
/// other reason to give. Short, and it goes back to the model.
pub(super) const DECLINED: &str = "the user declined";
pub(super) const CANCELLED: &str = "cancelled";
/// How long a prompt the model opened ignores `y`, `n` and Esc, so a key the
/// user was already typing cannot answer it.
pub(super) const ARM_DELAY: Duration = Duration::from_millis(400);
const TOOLS_OFF: &str = "tools are off";
const APPROVAL_CHAT_ONLY: &str = "The approval mode is for chat panes; open one with Ctrl+Space c";
const APPROVAL_TOOLS_OFF: &str = "Tools are off: there is no approval mode to change";
const TURN_OVER: &str = "the turn is over";
const NO_SHELL: &str = "there is no shell pane in this tab";
const NO_USER_TOOLS: &str = "user tools are not available";
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

    fn deny(pane: PaneId, turn: u64, call: CallId, reason: &'static str) -> Vec<Effect> {
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
        }
        match event {
            Some(ToolEvent::Over(call, status)) => {
                self.withdraw_tool_approval(id, |key| key == (turn, call));
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
    /// a command, a kind of call that has no prompt yet).
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
            ApprovalAsk::Read { .. } => None,
            ApprovalAsk::External { .. } => return Self::deny(id, turn, call, NO_USER_TOOLS),
        };
        if let (ApprovalAsk::Run { command }, Some(target)) = (&ask, target)
            && self.auto_route(id, target, &ask) == Route::AutoType
        {
            return self.auto_type(id, (turn, call), target, command);
        }
        let armed_at = (self.clock)() + ARM_DELAY;
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Self::deny(id, turn, call, CHAT_GONE);
        };
        match chat.approvals_mut().request_call(ask, turn, call) {
            Ok(_) => {
                chat.set_run_target(target);
                chat.set_armed_at(Some(armed_at));
                self.dirty = true;
                Vec::new()
            }
            Err(refusal) => Self::deny(id, turn, call, refusal.reason()),
        }
    }

    /// What chat `id` does with a model's `ask` for `target`: the mode and the
    /// flood limits decide (see `approval::route`). A pending approval is not
    /// the mode's to answer, so it prompts, and the request is then refused.
    fn auto_route(&self, id: PaneId, target: PaneId, ask: &ApprovalAsk) -> Route {
        let Some(chat) = self.panes.get(&id).and_then(PaneState::chat_state) else {
            return Route::Prompt;
        };
        if chat.approvals().pending().is_some() {
            return Route::Prompt;
        }
        let ctx = AutoCtx {
            typed_this_turn: chat.auto_typed(),
            line_pending: (self.panes.get(&target).and_then(PaneState::pty))
                .is_some_and(|pty| pty.line_pending.is_some()),
            target_focused: self.tab().focus == target,
            // A zoomed tab draws only the focused pane, never the target.
            tab_shown: !self.tab().zoom && self.tab().tree.leaves().contains(&id),
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
            turn,
            call,
        }]
    }

    /// `Prefix a`: flips the focused chat between `ask` and `auto`.
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
        let mode = chat.approval_mode().flipped();
        chat.set_approval_mode(mode);
        chat.push_system_line(match mode {
            ApprovalMode::Auto => "approval: auto (commands are typed, never run)".to_owned(),
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

    /// The user is about to send to the focused pane: Enter, Ctrl+C or Ctrl+U
    /// ends the line an auto-typed command was waiting on. Read from the key
    /// event, so a child's keyboard protocol does not matter.
    pub(super) fn note_user_key(&mut self, key: &KeyEvent) {
        let sends = match key.code {
            KeyCode::Enter => true,
            KeyCode::Char('c' | 'u') => key.modifiers.contains(KeyModifiers::CONTROL),
            _ => false,
        };
        if sends && let Some(pty) = self.focused_state_mut().pty_mut() {
            pty.line_pending = None;
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
    /// engine is verified for, and the chat remembers that it may hold a tool
    /// session to end.
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
        chat.set_tool_session(request.tools != Tools::Off);
    }

    /// Stops the turn of chat `id`, and ends its tool session when its last
    /// turn offered tools.
    pub(super) fn cancel_chat(&mut self, id: PaneId) -> Vec<Effect> {
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
