//! `/write` and `/exec`: the chat grant of Write and of Exec (modal-input:
//! either on from the chat is a user action; orchestrator-worker-flow D16).
//! The twin of `/github`: per chat, in memory, one `y` dialog, opened by the
//! user's command and nothing else.

use super::github::{R_FOLDER, R_LIST_OPEN, R_NO_TOOLS, R_TOOLS_OFF};
use super::*;
use crate::core::ai::{
    chat::approval::{
        Approval, ApprovalAsk, EXEC_DECLINED_LINE, EXEC_GRANTED_LINE, Origin, Reply,
        WRITE_DECLINED_LINE, WRITE_GRANTED_LINE,
    },
    config::ToolToggle,
    tools::registry::FamilySet,
};

/// The toggles a chat can be granted.
pub(super) const GRANTS: [ToolToggle; 2] = [ToolToggle::Write, ToolToggle::Exec];

/// Why a grant ended, for the system line `<Name> is off because <why>`.
pub(super) const WHY_FOLDER: &str = "the folder changed";
pub(super) const WHY_TOOLS: &str = "tools were turned off";
pub(super) const WHY_RESUME: &str = "a conversation was resumed";
const NO_ARGUMENTS: &str = "takes no arguments";
const R_NO_FOLDER: &str = "this chat has no folder";
/// What follows the command in the line of a dialog that was withdrawn.
const CANCELLED: &str = " cancelled: ";
/// What follows the command in the line of a command that was refused.
const REFUSED: &str = " refused: ";

/// What the user reads for `toggle`.
fn name(toggle: ToolToggle) -> &'static str {
    match toggle {
        ToolToggle::Exec => "Exec",
        _ => "Write",
    }
}

/// The command that opens the dialog of `toggle`.
fn command(toggle: ToolToggle) -> &'static str {
    match toggle {
        ToolToggle::Exec => "/exec",
        _ => "/write",
    }
}

/// The system line of a grant of `toggle` that ended for `why`.
fn ended_line(toggle: ToolToggle, why: &str) -> String {
    format!("{} is off because {why}", name(toggle))
}

/// What a change of the saved `toggle` says when it ends the grant.
pub(super) fn toggle_why(toggle: ToolToggle) -> String {
    format!("the {} toggle was changed", name(toggle))
}

impl App {
    /// The menu chat `id` follows: the saved one plus each family the chat
    /// holds a grant for (Write by `/write`, Exec by `/exec`) while tools are
    /// on. Nothing else is ever added. The one source of what an
    /// orchestrated turn lets its workers have and of the tools a plain turn
    /// offers.
    pub(super) fn orch_ceiling(&self, id: PaneId) -> FamilySet {
        let saved = self.ai.families();
        if !self.ai.tools_enabled() {
            return saved;
        }
        GRANTS
            .into_iter()
            .filter(|&toggle| self.tool_granted(id, toggle))
            .fold(saved, |set, toggle| set.with(toggle.family()))
    }

    /// Whether chat `id` holds the grant of `toggle`.
    pub(super) fn tool_granted(&self, id: PaneId, toggle: ToolToggle) -> bool {
        (self.panes.get(&id).and_then(PaneState::chat_state))
            .is_some_and(|chat| chat.tool_granted(toggle))
    }

    /// Tells the live run of an orchestrating chat that its menu changed with
    /// the grant: the spawns from now on follow it (D15, D17).
    fn ceiling_effect(&self, id: PaneId) -> Vec<Effect> {
        let ceiling = self.orch_ceiling(id);
        (self.orchestrating(id))
            .then_some(Effect::OrchCeiling { pane: id, ceiling })
            .into_iter()
            .collect()
    }

    /// Chat `id` left its folder: the dialog is taken back, and every grant
    /// held says why it ended.
    pub(super) fn end_write_for_folder(&mut self, id: PaneId) -> Vec<Effect> {
        self.withdraw_write_dialog(id, R_FOLDER);
        self.end_grants(id, &GRANTS, WHY_FOLDER)
    }

    /// Ends the grants of `toggles` that chat `id` holds, each with its own
    /// line, and lowers the menu of its run again, once.
    pub(super) fn end_grants(
        &mut self,
        id: PaneId,
        toggles: &[ToolToggle],
        why: &str,
    ) -> Vec<Effect> {
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        let mut ended = false;
        for &toggle in toggles {
            if !chat.tool_granted(toggle) {
                continue;
            }
            chat.set_tool_granted(toggle, false);
            chat.push_system_line(ended_line(toggle, why));
            ended = true;
        }
        if !ended {
            return Vec::new();
        }
        self.dirty = true;
        self.ceiling_effect(id)
    }

    /// The grants of `toggles` end in every chat (tools turned off, or a saved
    /// toggle changed): each chat that held one says why.
    pub(super) fn end_all_tool_grants(&mut self, toggles: &[ToolToggle], why: &str) -> Vec<Effect> {
        let ids: Vec<PaneId> = self.panes.keys().copied().collect();
        (ids.into_iter())
            .flat_map(|id| self.end_grants(id, toggles, why))
            .collect()
    }

    /// Every grant ends in every chat: tools went off.
    pub(super) fn end_all_write(&mut self, why: &str) -> Vec<Effect> {
        self.end_all_tool_grants(&GRANTS, why)
    }

    /// A resumed conversation starts with no grant.
    pub(super) fn end_grants_for_resume(&mut self, id: PaneId) -> Vec<Effect> {
        self.end_grants(id, &GRANTS, WHY_RESUME)
    }

    /// `/write` or `/exec` followed by text: says so and keeps the composer
    /// text.
    pub(super) fn grant_refused(&mut self, id: PaneId, toggle: ToolToggle) {
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            chat.push_system_line(format!("{} {NO_ARGUMENTS}", command(toggle)));
            self.dirty = true;
        }
    }

    /// Why the command of `toggle` does not apply to chat `id` now, or
    /// `None`: tools off, an engine that offers no tools, or the toggle
    /// already on. An orchestrating chat is the point of the command, so it is
    /// not a reason.
    fn grant_blocker(&self, id: PaneId, toggle: ToolToggle) -> Option<String> {
        if !self.ai.tools_enabled() {
            Some(R_TOOLS_OFF.to_owned())
        } else if !self.offers_tools(id) {
            Some(R_NO_TOOLS.to_owned())
        } else if self.ai.toggle(toggle) || self.tool_granted(id, toggle) {
            Some(format!("{} is already on", name(toggle)))
        } else {
            None
        }
    }

    /// `/write`: see [`Self::grant_submit`].
    pub(super) fn write_submit(&mut self, id: PaneId) -> Vec<Effect> {
        self.grant_submit(id, ToolToggle::Write)
    }

    /// `/exec`: see [`Self::grant_submit`].
    pub(super) fn exec_submit(&mut self, id: PaneId) -> Vec<Effect> {
        self.grant_submit(id, ToolToggle::Exec)
    }

    /// The command of `toggle`: clears the composer and opens the dialog for
    /// the chat folder. Nothing is sent, nothing is stored. A refusal says
    /// why.
    fn grant_submit(&mut self, id: PaneId, toggle: ToolToggle) -> Vec<Effect> {
        self.grant_open(id, toggle, true)
    }

    /// The key of `toggle` (`Ctrl+Space x` or `f`): the same dialog and the
    /// same refusals as the command, with the composer draft kept. Outside a
    /// chat it says so. It never grants: only `y` in the dialog does.
    pub(super) fn grant_key(&mut self, toggle: ToolToggle) -> Vec<Effect> {
        if !self.focused_is_chat() {
            self.notice = Some(format!(
                "{} is for chat panes; open one with Ctrl+Space c",
                name(toggle)
            ));
            return Vec::new();
        }
        let id = self.tab().focus;
        self.grant_open(id, toggle, false)
    }

    /// Opens the dialog of `toggle` in chat `id`; `clear` empties the composer
    /// (the command was typed there).
    fn grant_open(&mut self, id: PaneId, toggle: ToolToggle, clear: bool) -> Vec<Effect> {
        let blocker = self.grant_blocker(id, toggle);
        let list = self.list_open(id);
        let armed = (self.clock)();
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        self.dirty = true;
        // Another confirm owns the keys: the command waits in the composer.
        if chat.approvals().pending().is_some() {
            chat.push_system_line(run::ANSWER_FIRST.to_owned());
            self.notice = Some(run::ANSWER_FIRST.to_owned());
            return Vec::new();
        }
        if clear {
            chat.composer_mut().clear();
        }
        let folder = chat.origin_cwd().map(Path::to_path_buf);
        let why = blocker
            .or_else(|| list.then(|| R_LIST_OPEN.to_owned()))
            .or_else(|| folder.is_none().then(|| R_NO_FOLDER.to_owned()));
        let (Some(folder), None) = (folder, &why) else {
            let why = why.unwrap_or_default();
            chat.push_system_line(format!("{}{REFUSED}{why}", command(toggle)));
            return Vec::new();
        };
        let ask = ApprovalAsk::ToolGrant { toggle, folder };
        let armed = armed + ask.arm_delay();
        if chat.approvals_mut().request(ask, Origin::Block).is_ok() {
            chat.set_armed_at(Some(armed));
        }
        Vec::new()
    }

    /// The toggle of the grant dialog waiting in chat `id`, if that is the
    /// approval waiting.
    fn pending_grant(&self, id: PaneId) -> Option<ToolToggle> {
        let chat = self.panes.get(&id).and_then(PaneState::chat_state)?;
        match chat.approvals().pending()?.ask() {
            ApprovalAsk::ToolGrant { toggle, .. } => Some(*toggle),
            _ => None,
        }
    }

    /// Whether the approval waiting in chat `id` is a `/write` or `/exec`
    /// dialog.
    pub(super) fn write_ask_pending(&self, id: PaneId) -> bool {
        self.pending_grant(id).is_some()
    }

    /// A key while the dialog waits. Same rules as the `/github` dialog: only
    /// an unmodified lowercase `y` grants, only `n` and Esc decline, once the
    /// dialog is armed; Ctrl+C declines and cancels the stream; every other
    /// key is swallowed with a notice. `y` grants only while the chat is in
    /// the folder the dialog names, and only what the prompt shows in full.
    pub(super) fn on_write_key(&mut self, id: PaneId, key: KeyEvent, repeat: bool) -> Vec<Effect> {
        let Some(toggle) = self.pending_grant(id) else {
            return Vec::new();
        };
        let plain = key.modifiers == KeyModifiers::NONE && !repeat;
        let grants = plain && key.code == KeyCode::Char('y');
        let declines = plain && matches!(key.code, KeyCode::Char('n') | KeyCode::Esc);
        let cancels =
            !repeat && key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL;
        if !grants && !declines && !cancels {
            self.ignored_key(&key, run::ANSWER_FIRST);
            return Vec::new();
        }
        if !cancels && !self.approval_armed_at(id) {
            self.ignored_key(&key, run::JUST_OPENED);
            return Vec::new();
        }
        if grants {
            let why = if self.write_folder_moved(id) {
                Some(R_FOLDER.to_owned())
            } else {
                self.grant_blocker(id, toggle)
            };
            if let Some(why) = why {
                self.withdraw_write_dialog(id, &why);
                return Vec::new();
            }
            if !self.prompt_fits(id) {
                self.notice = Some(run::PROMPT_TOO_SMALL.to_owned());
                return Vec::new();
            }
        }
        self.close_approval(
            id,
            if grants {
                Reply::Approve
            } else {
                Reply::Decline
            },
        );
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            chat.set_tool_granted(toggle, grants);
            let line = match (toggle, grants) {
                (ToolToggle::Exec, true) => EXEC_GRANTED_LINE,
                (ToolToggle::Exec, false) => EXEC_DECLINED_LINE,
                (_, true) => WRITE_GRANTED_LINE,
                (_, false) => WRITE_DECLINED_LINE,
            };
            chat.push_system_line(line.to_owned());
        }
        self.dirty = true;
        if cancels {
            return self.chat_cancel_key(id);
        }
        if grants {
            return self.ceiling_effect(id);
        }
        Vec::new()
    }

    /// Whether the dialog of chat `id` names a folder the chat has left.
    fn write_folder_moved(&self, id: PaneId) -> bool {
        let Some(chat) = self.panes.get(&id).and_then(PaneState::chat_state) else {
            return false;
        };
        match chat.approvals().pending().map(Approval::ask) {
            Some(ApprovalAsk::ToolGrant { folder, .. }) => {
                chat.origin_cwd() != Some(folder.as_path())
            }
            _ => false,
        }
    }

    /// Takes the dialog of chat `id` back, if it is open, and says why.
    pub(super) fn withdraw_write_dialog(&mut self, id: PaneId, why: &str) {
        let Some(toggle) = self.pending_grant(id) else {
            return;
        };
        self.close_approval(id, Reply::Decline);
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            chat.push_system_line(format!("{}{CANCELLED}{why}", command(toggle)));
        }
        self.dirty = true;
    }
}
