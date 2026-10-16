//! Run in pane: the confirm that types a command of a chat into a shell pane.
//! The confirm only types; lazarobox never sends Enter.

use super::*;
use crate::core::ai::{
    chat::{
        approval::{Approval, ApprovalAsk, ApprovalPrompt, Origin, Press, Reply},
        yank,
    },
    sanitize,
    tools::command::validate,
};

/// Notices for the refusals of `Prefix e`.
const CHAT_ONLY: &str = "Run in pane works from a chat";
const CONVERSATION_ONLY: &str = "Run in pane works in the conversation view";
const TOOLS_OFF: &str = "Tools are off: nothing can run from the chat";
const NO_COMMAND: &str = "No runnable command in the last answer";
const NO_SHELL: &str = "No shell pane in this tab";
const TARGET_GONE: &str = "That pane is gone: press p to pick another";
const NO_PASTE_MODE: &str = "The shell cannot take a multi-line command safely";
const ALREADY_OPEN: &str = "A confirm is already open: answer it first";

impl App {
    /// Brings the chats in line with `[ai.tools]`: tools off ends every
    /// pending confirm (nothing is written) and hides the hint under shell
    /// blocks; tools on shows it again.
    pub(super) fn sync_tools(&mut self) -> Vec<Effect> {
        let on = self.ai.tools_enabled();
        for chat in self.panes.values_mut().filter_map(PaneState::chat_mut) {
            chat.set_run_hint(on);
            if !on && let Some(id) = chat.approvals().pending().map(Approval::id) {
                chat.approvals_mut().withdraw(id);
                chat.set_run_target(None);
            }
        }
        self.dirty = true;
        Vec::new()
    }

    /// Whether chat `id` has an approval waiting for an answer.
    pub(super) fn has_approval(&self, id: PaneId) -> bool {
        self.panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .is_some_and(|chat| chat.approvals().pending().is_some())
    }

    /// A key in a chat with a pending approval. It is routed before the chat
    /// key table: `y` approves, `n` and Esc decline, `p` cycles the target,
    /// Ctrl+C cancels the stream and denies, and every other key is swallowed
    /// (no composer edit, no write, no quit). Only a Press answers.
    pub(super) fn on_approval_key(
        &mut self,
        id: PaneId,
        key: KeyEvent,
        repeat: bool,
    ) -> Vec<Effect> {
        if repeat {
            return Vec::new();
        }
        match (key.code, key.modifiers) {
            (KeyCode::Char('y'), KeyModifiers::NONE) => self.approve_run(id),
            (KeyCode::Char('n') | KeyCode::Esc, KeyModifiers::NONE) => {
                self.close_approval(id, Reply::Decline);
                Vec::new()
            }
            (KeyCode::Char('p'), KeyModifiers::NONE) => {
                self.cycle_run_target(id);
                Vec::new()
            }
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                self.close_approval(id, Reply::Decline);
                self.chat_cancel_key(id)
            }
            _ => Vec::new(),
        }
    }

    /// Ends the pending approval of chat `id` with `reply`, writing nothing.
    fn close_approval(&mut self, id: PaneId, reply: Reply) -> Option<Approval> {
        let chat = self.panes.get_mut(&id)?.chat_mut()?;
        let approval_id = chat.approvals().pending()?.id();
        let (approval, _) = chat
            .approvals_mut()
            .answer(approval_id, reply, Press::Press)?;
        chat.set_run_target(None);
        Some(approval)
    }

    /// `y`: types the command into the target pane. The target is rechecked
    /// now, not when the confirm opened: it must still be a shell pane of
    /// this tab and not run a full-screen app, and the paste framing follows
    /// its present state. One accept is one write; nothing ever sends Enter.
    fn approve_run(&mut self, id: PaneId) -> Vec<Effect> {
        let Some(target) = self
            .panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .and_then(ChatState::run_target)
            .filter(|target| self.shell_panes().contains(target))
        else {
            // The target is gone: the confirm stays so `p` can pick another.
            self.notice = Some(TARGET_GONE.to_owned());
            return Vec::new();
        };
        let Some(approval) = self.close_approval(id, Reply::Approve) else {
            return Vec::new();
        };
        let ApprovalAsk::Run { command } = approval.ask() else {
            return Vec::new();
        };
        let command = match validate(command) {
            Ok(command) => command,
            Err(refusal) => {
                self.notice = Some(format!("Command refused: {}", refusal.reason()));
                return Vec::new();
            }
        };
        let number = self.pane_number(target).unwrap_or(0);
        let Some(pty) = self.panes.get_mut(&target).and_then(PaneState::pty_mut) else {
            return Vec::new();
        };
        if pty.emu.mouse_facts().alt_screen {
            self.notice = Some(format!("Pane {number} runs a full-screen app"));
            return Vec::new();
        }
        let text = command.as_str();
        let modes = pty.emu.modes();
        let bytes = if modes.bracketed_paste {
            encode_paste(text, modes)
        } else if text.contains(['\n', '\t']) {
            self.notice = Some(NO_PASTE_MODE.to_owned());
            return Vec::new();
        } else {
            text.as_bytes().to_vec()
        };
        pty.emu.set_scrollback(0);
        vec![Effect::WritePty(target, bytes)]
    }

    /// `p`: the next shell pane of the tab becomes the target, with wrap.
    fn cycle_run_target(&mut self, id: PaneId) {
        let shells = self.shell_panes();
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return;
        };
        let next = match chat
            .run_target()
            .and_then(|t| shells.iter().position(|p| *p == t))
        {
            Some(at) => shells.get((at + 1) % shells.len()),
            None => shells.first(),
        };
        match next {
            Some(next) => chat.set_run_target(Some(*next)),
            None => self.notice = Some(NO_SHELL.to_owned()),
        }
    }

    /// The 1-based number of `pane` in the active tab.
    fn pane_number(&self, pane: PaneId) -> Option<usize> {
        Some(self.tab().tree.leaves().iter().position(|p| *p == pane)? + 1)
    }

    /// `Prefix e`: opens the confirm for the last shell block of the focused
    /// chat's last answer. Nothing is written; only the confirm can.
    pub(super) fn run_in_pane(&mut self) -> Vec<Effect> {
        let id = self.tab().focus;
        if !self.focused_is_chat() {
            self.notice = Some(CHAT_ONLY.to_owned());
            return Vec::new();
        }
        if self.list_open(id) || self.memory_view_open(id) || self.tag_editor_open(id) {
            self.notice = Some(CONVERSATION_ONLY.to_owned());
            return Vec::new();
        }
        if !self.ai.tools_enabled() {
            self.notice = Some(TOOLS_OFF.to_owned());
            return Vec::new();
        }
        let Some(block) = self
            .focused_state()
            .chat_state()
            .and_then(yank::last_shell_block)
        else {
            self.notice = Some(NO_COMMAND.to_owned());
            return Vec::new();
        };
        let command = match validate(&block) {
            Ok(command) => command,
            Err(refusal) => {
                self.notice = Some(format!("Command refused: {}", refusal.reason()));
                return Vec::new();
            }
        };
        let Some(target) = self.default_run_target() else {
            self.notice = Some(NO_SHELL.to_owned());
            return Vec::new();
        };
        let Some(chat) = self.focused_state_mut().chat_mut() else {
            return Vec::new();
        };
        let ask = ApprovalAsk::Run {
            command: command.as_str().to_owned(),
        };
        match chat.approvals_mut().request(ask, Origin::Block) {
            Ok(_) => chat.set_run_target(Some(target)),
            Err(_) => self.notice = Some(ALREADY_OPEN.to_owned()),
        }
        Vec::new()
    }

    /// The shell panes of the active tab, in layout order.
    fn shell_panes(&self) -> Vec<PaneId> {
        self.tab()
            .tree
            .leaves()
            .into_iter()
            .filter(|pane| self.is_shell(*pane))
            .collect()
    }

    /// Where a command goes unless the user cycles: the last shell that had
    /// the focus in this tab, else the first shell of the tab. A chat or a
    /// pane of another tab is never chosen.
    fn default_run_target(&self) -> Option<PaneId> {
        let shells = self.shell_panes();
        self.tab()
            .last_shell
            .filter(|pane| shells.contains(pane))
            .or_else(|| shells.first().copied())
    }

    /// What chat `id` shows for its pending run-in-pane confirm: the target
    /// pane by its number in the tab and its cwd, and the exact command.
    pub fn approval_prompt(&self, id: PaneId) -> Option<ApprovalPrompt> {
        let chat = self.panes.get(&id)?.chat_state()?;
        let ApprovalAsk::Run { command } = chat.approvals().pending()?.ask() else {
            return None;
        };
        let target = chat.run_target().and_then(|pane| self.pane_label(pane));
        Some(ApprovalPrompt {
            target: target.unwrap_or_else(|| "target pane is gone".to_owned()),
            command: command.clone(),
        })
    }

    /// `run in pane N (<cwd>)` for a shell pane of the active tab; `None`
    /// when it is not one. The cwd is cleaned: it is text from the shell.
    fn pane_label(&self, pane: PaneId) -> Option<String> {
        let number = self.pane_number(pane)?;
        let state = self.panes.get(&pane).filter(|s| s.pty().is_some())?;
        Some(match state.effective_cwd() {
            Some(cwd) => format!(
                "run in pane {number} ({})",
                sanitize::clean(&self.home_label(cwd))
            ),
            None => format!("run in pane {number}"),
        })
    }
}
