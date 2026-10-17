//! The app's side of a model's tool calls that need the user: a call arrives
//! as a `ToolCall` event with an ask, the approval overlay of the chat shows
//! it, and the user's answer leaves as `Effect::ToolDecision` under the call's
//! pane, turn and id.
//!
//! `on_tool_ask` is the single place an ask enters the app. It always opens
//! the overlay today; the chat approval mode (D18) will route there first.

use std::time::Duration;

use super::*;
use crate::core::ai::chat::{
    approval::{Approval, ApprovalAsk, Origin},
    engine::{CallId, Decision, Tools, tools_for},
};

/// Why a call is denied without the user being asked, or when a refusal has no
/// other reason to give. Short, and it goes back to the model.
pub(super) const DECLINED: &str = "the user declined";
pub(super) const CANCELLED: &str = "cancelled";
/// How long a prompt the model opened ignores `y`, `n` and Esc, so a key the
/// user was already typing cannot answer it.
pub(super) const ARM_DELAY: Duration = Duration::from_millis(400);
const TOOLS_OFF: &str = "tools are off";
const TURN_OVER: &str = "the turn is over";
const NO_SHELL: &str = "there is no shell pane in this tab";
const NO_USER_TOOLS: &str = "user tools are not available";
const CHAT_GONE: &str = "the chat is gone";

/// What a tool event says about a call that may wait for the user.
pub(super) enum ToolEvent {
    /// The call is announced and needs the user.
    Ask(CallId, ApprovalAsk),
    /// The call ended: whatever waited for it is stale.
    Over(CallId),
}

impl ToolEvent {
    pub(super) fn of(event: &StreamEvent) -> Option<Self> {
        match event {
            StreamEvent::ToolCall {
                id,
                approval: Some(ask),
                ..
            } => Some(Self::Ask(*id, (**ask).clone())),
            StreamEvent::ToolResult { id, .. } => Some(Self::Over(*id)),
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
            Some(ToolEvent::Over(call)) => {
                self.withdraw_tool_approval(id, |key| key == (turn, call));
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
