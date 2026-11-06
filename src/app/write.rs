//! `/write`: the chat grant of Write (modal-input: Write on from the chat is a
//! user action). The twin of `/github`: per chat, in memory, one `y` dialog,
//! opened by the user's command and nothing else.

use super::github::{R_FOLDER, R_LIST_OPEN, R_NO_TOOLS, R_TOOLS_OFF};
use super::*;
use crate::core::ai::{
    chat::approval::{
        Approval, ApprovalAsk, Origin, Reply, WRITE_DECLINED_LINE, WRITE_GRANTED_LINE,
    },
    config::ToolToggle,
};

/// The system line of a grant that a folder change ended.
const FOLDER_ENDED: &str = "Write is off because the folder changed";
/// The system line of a grant that tools turned off ended.
pub(super) const TOOLS_ENDED: &str = "Write is off because tools were turned off";
/// The system line of a grant that a change of the saved toggle ended.
pub(super) const TOGGLE_ENDED: &str = "Write is off because the Write toggle was changed";
/// The system line of a grant that a resumed conversation ended.
pub(super) const RESUME_ENDED: &str = "Write is off because a conversation was resumed";
const NO_ARGUMENTS: &str = "/write takes no arguments";
const R_ALREADY_ON: &str = "Write is already on";
const R_NO_FOLDER: &str = "this chat has no folder";
/// The line of a dialog that was withdrawn.
const CANCELLED: &str = "/write cancelled: ";
const REFUSED: &str = "/write refused: ";

impl App {
    /// Whether chat `id` holds a `/write` grant.
    pub(super) fn write_granted(&self, id: PaneId) -> bool {
        (self.panes.get(&id).and_then(PaneState::chat_state)).is_some_and(ChatState::write_granted)
    }

    /// Chat `id` lost its folder grant: the dialog is taken back, and a grant
    /// held says why it ended.
    pub(super) fn end_write_for_folder(&mut self, id: PaneId) {
        self.withdraw_write_dialog(id, R_FOLDER);
        self.end_write(id, FOLDER_ENDED);
    }

    /// Ends the grant of chat `id`, if it holds one, with `line`.
    pub(super) fn end_write(&mut self, id: PaneId, line: &str) -> bool {
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return false;
        };
        let held = chat.write_granted();
        if held {
            chat.set_write_granted(false);
            chat.push_system_line(line.to_owned());
            self.dirty = true;
        }
        held
    }

    /// Every grant ends (tools turned off, or the saved toggle changed): each
    /// chat that held one says why, `line`.
    pub(super) fn end_all_write(&mut self, line: &str) {
        let ids: Vec<PaneId> = self.panes.keys().copied().collect();
        for id in ids {
            self.end_write(id, line);
        }
    }

    /// `/write` followed by text: says so and keeps the composer text.
    pub(super) fn write_refused(&mut self, id: PaneId) {
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            chat.push_system_line(NO_ARGUMENTS.to_owned());
            self.dirty = true;
        }
    }

    /// Why `/write` does not apply to chat `id` now, or `None`: tools off, an
    /// engine that offers no tools, or Write already on. An orchestrating
    /// chat is the point of the command, so it is not a reason.
    fn write_blocker(&self, id: PaneId) -> Option<&'static str> {
        if !self.ai.tools_enabled() {
            Some(R_TOOLS_OFF)
        } else if !self.offers_tools(id) {
            Some(R_NO_TOOLS)
        } else if self.ai.toggle(ToolToggle::Write) || self.write_granted(id) {
            Some(R_ALREADY_ON)
        } else {
            None
        }
    }

    /// `/write`: clears the composer and opens the dialog for the chat
    /// folder. Nothing is sent, nothing is stored. A refusal says why.
    pub(super) fn write_submit(&mut self, id: PaneId) -> Vec<Effect> {
        let blocker = self.write_blocker(id);
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
        chat.composer_mut().clear();
        let folder = chat.origin_cwd().map(Path::to_path_buf);
        let why = blocker
            .or_else(|| list.then_some(R_LIST_OPEN))
            .or_else(|| folder.is_none().then_some(R_NO_FOLDER));
        let (Some(folder), None) = (folder, why) else {
            chat.push_system_line(format!("{REFUSED}{}", why.unwrap_or_default()));
            return Vec::new();
        };
        let ask = ApprovalAsk::ToolGrant {
            toggle: ToolToggle::Write,
            folder,
        };
        let armed = armed + ask.arm_delay();
        if chat.approvals_mut().request(ask, Origin::Block).is_ok() {
            chat.set_armed_at(Some(armed));
        }
        Vec::new()
    }

    /// Whether the approval waiting in chat `id` is the `/write` dialog.
    pub(super) fn write_ask_pending(&self, id: PaneId) -> bool {
        self.panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .and_then(|chat| chat.approvals().pending())
            .is_some_and(|approval| matches!(approval.ask(), ApprovalAsk::ToolGrant { .. }))
    }

    /// A key while the dialog waits. Same rules as the `/github` dialog: only
    /// an unmodified lowercase `y` grants, only `n` and Esc decline, once the
    /// dialog is armed; Ctrl+C declines and cancels the stream; every other
    /// key is swallowed with a notice. `y` grants only while the chat is in
    /// the folder the dialog names, and only what the prompt shows in full.
    pub(super) fn on_write_key(&mut self, id: PaneId, key: KeyEvent, repeat: bool) -> Vec<Effect> {
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
                Some(R_FOLDER)
            } else {
                self.write_blocker(id)
            };
            if let Some(why) = why {
                self.withdraw_write_dialog(id, why);
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
            chat.set_write_granted(grants);
            let line = if grants {
                WRITE_GRANTED_LINE
            } else {
                WRITE_DECLINED_LINE
            };
            chat.push_system_line(line.to_owned());
        }
        self.dirty = true;
        if cancels {
            return self.chat_cancel_key(id);
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
        if !self.write_ask_pending(id) {
            return;
        }
        self.close_approval(id, Reply::Decline);
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            chat.push_system_line(format!("{CANCELLED}{why}"));
        }
        self.dirty = true;
    }
}
