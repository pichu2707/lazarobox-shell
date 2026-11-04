//! The GitHub tools of a chat pane: the families a turn offers, and where the
//! grant of `/github` ends (github-tools, The chat grant).

use super::*;
use crate::core::ai::{
    chat::{
        approval::{
            Approval, ApprovalAsk, GITHUB_DECLINED_LINE, GITHUB_GRANTED_LINE, Origin, Reply,
        },
        engine::{Availability, Tools, tools_for},
    },
    config::ToolToggle,
    tools::{
        github::GhRepo,
        registry::{Family, FamilySet},
    },
};

/// The system line of a grant that a folder change ended.
const FOLDER_ENDED: &str = "GitHub tools are off because the folder changed";
/// The system line of a grant that tools turned off ended.
pub(super) const TOOLS_ENDED: &str = "GitHub tools are off because tools were turned off";
/// The system line of a grant that the saved toggle going off ended.
pub(super) const TOGGLE_ENDED: &str =
    "GitHub tools are off because the GitHub toggle was turned off";
const NO_ARGUMENTS: &str = "/github takes no arguments";
const NO_FOLDER: &str = "/github refused: this chat has no folder";
const CHECKING: &str = "checking origin...";
const STILL_CHECKING: &str = "/github refused: still checking the origin";
/// The line of a dialog that was withdrawn, or an answer that was dropped.
const CANCELLED: &str = "/github cancelled: ";
const REFUSED: &str = "/github refused: ";
/// Why `/github` does not apply to a chat now.
const R_TOOLS_OFF: &str = "tools are off";
const R_ORCHESTRATING: &str = "this chat is orchestrating";
const R_NO_TOOLS: &str = "the engine offers no tools";
const R_ALREADY_ON: &str = "the GitHub tools are already on";
const R_FOLDER: &str = "the folder changed";
const R_LIST_OPEN: &str = "the conversation list is open";
pub(super) const R_RESUMED: &str = "a conversation was resumed";
/// What turning ORCH on says to a chat that holds a grant.
pub(super) const ORCH_KEEPS_GRANT: &str =
    "GitHub grant kept, but orchestrated turns do not offer GitHub tools";

impl App {
    /// The families chat `id` offers its next turn: the saved toggles plus the
    /// GitHub family when this chat holds a grant and tools are on. The one
    /// source of the offered tools and of the context budget.
    pub(super) fn offered_families(&self, id: PaneId) -> FamilySet {
        let saved = self.ai.families();
        let granted = self.ai.tools_enabled()
            && (self.panes.get(&id).and_then(PaneState::chat_state))
                .is_some_and(ChatState::github_granted);
        if granted {
            saved.with(Family::Github)
        } else {
            saved
        }
    }

    /// The folder of chat `id` changed: a grant was for the old repository.
    pub(super) fn end_grant_for_folder(&mut self, id: PaneId) {
        // A dialog names the old repository; so does a probe still on its way.
        self.withdraw_github_dialog(id, R_FOLDER);
        self.github_probing.remove(&id);
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut)
            && chat.github_granted()
        {
            chat.set_github_granted(false);
            chat.push_system_line(FOLDER_ENDED.to_owned());
        }
    }

    /// Every grant ends (tools turned off, or the saved toggle turned off):
    /// each chat that held one says why, `line`.
    pub(super) fn end_all_grants(&mut self, line: &str) {
        for chat in self.panes.values_mut().filter_map(PaneState::chat_mut) {
            if chat.github_granted() {
                chat.set_github_granted(false);
                chat.push_system_line(line.to_owned());
            }
        }
        self.dirty = true;
    }

    /// `/github` followed by text: says so and keeps the composer text.
    pub(super) fn github_refused(&mut self, id: PaneId) {
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            chat.push_system_line(NO_ARGUMENTS.to_owned());
            self.dirty = true;
        }
    }

    /// `/github`: clears the composer and asks the runtime for the `origin` of
    /// the chat folder. Nothing is sent, nothing is stored. A refusal that is
    /// known now says why at once; the dialog opens from the answer.
    pub(super) fn github_submit(&mut self, id: PaneId) -> Vec<Effect> {
        let blocker = self.github_blocker(id);
        let probing = self.github_probing.contains_key(&id);
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
        let refusal = if let Some(why) = blocker {
            Some(format!("{REFUSED}{why}"))
        } else if probing {
            Some(STILL_CHECKING.to_owned())
        } else if folder.is_none() {
            Some(NO_FOLDER.to_owned())
        } else {
            None
        };
        if let Some(line) = refusal {
            chat.push_system_line(line);
            return Vec::new();
        }
        let Some(folder) = folder else {
            return Vec::new();
        };
        chat.push_system_line(CHECKING.to_owned());
        self.github_probing.insert(id, folder.clone());
        vec![Effect::ProbeOrigin {
            pane: id,
            folder,
            dialog: true,
        }]
    }

    /// Why `/github` does not apply to chat `id` now, or `None`: tools off,
    /// an orchestrating chat (its turns never offer the family), an engine
    /// that offers no tools, or the tools already on.
    fn github_blocker(&self, id: PaneId) -> Option<&'static str> {
        let granted = (self.panes.get(&id).and_then(PaneState::chat_state))
            .is_some_and(ChatState::github_granted);
        if !self.ai.tools_enabled() {
            Some(R_TOOLS_OFF)
        } else if self.orchestrating(id) {
            Some(R_ORCHESTRATING)
        } else if !self.offers_tools(id) {
            Some(R_NO_TOOLS)
        } else if self.ai.toggle(ToolToggle::Github) || granted {
            Some(R_ALREADY_ON)
        } else {
            None
        }
    }

    /// Whether the engine a turn of chat `id` goes to offers tools.
    fn offers_tools(&self, id: PaneId) -> bool {
        matches!(
            self.turn_availability(id),
            Availability::Ready { engine, .. }
                if tools_for(&engine, true, None, self.tool_engines) != Tools::Off
        )
    }

    /// The origin of `folder` was read. For `/github`: a repository on GitHub
    /// opens the dialog, anything else says why not. An answer for a folder
    /// the chat has left, or for a chat that cannot be granted now, is dropped.
    pub(super) fn on_origin_probed(
        &mut self,
        id: PaneId,
        folder: &Path,
        dialog: bool,
        repo: GhRepo,
    ) -> Vec<Effect> {
        // The probe of the dialog is over, whatever comes of its answer.
        if dialog && self.github_probing.get(&id).is_some_and(|f| f == folder) {
            self.github_probing.remove(&id);
        }
        let armed = (self.clock)()
            + ApprovalAsk::GithubGrant {
                repo: String::new(),
                folder: PathBuf::new(),
            }
            .arm_delay();
        let blocker = self.github_blocker(id);
        let list = self.list_open(id);
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        let current = chat.origin_cwd() == Some(folder);
        let open = chat.approvals().pending().is_some();
        if !dialog {
            return Vec::new();
        }
        // The user typed the command: an answer that cannot open the dialog
        // says why instead of vanishing.
        let dropped = if !current {
            Some(R_FOLDER)
        } else if let Some(why) = blocker {
            Some(why)
        } else if list {
            Some(R_LIST_OPEN)
        } else {
            None
        };
        self.dirty = true;
        if let Some(why) = dropped {
            chat.push_system_line(format!("{CANCELLED}{why}"));
            return Vec::new();
        }
        match repo {
            GhRepo::Found(slug) if !open => {
                let ask = ApprovalAsk::GithubGrant {
                    repo: slug.to_string(),
                    folder: folder.to_path_buf(),
                };
                if chat.approvals_mut().request(ask, Origin::Block).is_ok() {
                    chat.set_armed_at(Some(armed));
                }
            }
            GhRepo::Found(_) => chat.push_system_line(run::ANSWER_FIRST.to_owned()),
            refused => chat.push_system_line(format!(
                "{REFUSED}{}",
                refused.refusal().unwrap_or_default()
            )),
        }
        Vec::new()
    }

    /// Whether the approval waiting in chat `id` is the `/github` dialog.
    pub(super) fn github_ask_pending(&self, id: PaneId) -> bool {
        self.panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .and_then(|chat| chat.approvals().pending())
            .is_some_and(|approval| matches!(approval.ask(), ApprovalAsk::GithubGrant { .. }))
    }

    /// A key while the dialog waits. Only an unmodified lowercase `y` grants,
    /// only `n` and Esc decline, and only once it is armed (it opens from an
    /// asynchronous answer, so a key typed meanwhile is not an answer). Ctrl+C
    /// always works, like in every confirm: it declines and cancels the
    /// stream. Every other key, a held one included, is swallowed with a
    /// notice. `y` grants only what the prompt shows in full, and only while
    /// the chat is still in the folder the dialog was read for.
    pub(super) fn on_github_key(&mut self, id: PaneId, key: KeyEvent, repeat: bool) -> Vec<Effect> {
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
            if self.github_folder_moved(id) {
                self.withdraw_github_dialog(id, R_FOLDER);
                return Vec::new();
            }
            if !self.prompt_fits(id) {
                self.notice = Some(run::PROMPT_TOO_SMALL.to_owned());
                return Vec::new();
            }
        }
        let reply = if grants {
            Reply::Approve
        } else {
            Reply::Decline
        };
        self.close_approval(id, reply);
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            chat.set_github_granted(grants);
            chat.push_system_line(
                if grants {
                    GITHUB_GRANTED_LINE
                } else {
                    GITHUB_DECLINED_LINE
                }
                .to_owned(),
            );
        }
        self.dirty = true;
        if cancels {
            return self.chat_cancel_key(id);
        }
        Vec::new()
    }

    /// Whether the dialog of chat `id` names a folder the chat has left.
    fn github_folder_moved(&self, id: PaneId) -> bool {
        let Some(chat) = self.panes.get(&id).and_then(PaneState::chat_state) else {
            return false;
        };
        match chat.approvals().pending().map(Approval::ask) {
            Some(ApprovalAsk::GithubGrant { folder, .. }) => {
                chat.origin_cwd() != Some(folder.as_path())
            }
            _ => false,
        }
    }

    /// Takes the dialog of chat `id` back, if it is open, and says why.
    pub(super) fn withdraw_github_dialog(&mut self, id: PaneId, why: &str) {
        if !self.github_ask_pending(id) {
            return;
        }
        self.close_approval(id, Reply::Decline);
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            chat.push_system_line(format!("{CANCELLED}{why}"));
        }
        self.dirty = true;
    }

    /// Chat `id` just gained the focus: a key typed for the pane left may
    /// still be on its way, so the dialog waits its delay again.
    pub(super) fn rearm_github_dialog(&mut self, id: PaneId) {
        if !self.github_ask_pending(id) {
            return;
        }
        let clock = (self.clock)();
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut)
            && let Some(delay) = chat.approvals().pending().map(|a| a.ask().arm_delay())
        {
            chat.set_armed_at(Some(clock + delay));
        }
    }

    /// Whether the dialog of chat `id` may take an answer: its own delay passed.
    fn approval_armed_at(&self, id: PaneId) -> bool {
        self.panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .is_none_or(|chat| chat.armed_at().is_none_or(|at| (self.clock)() >= at))
    }
}
