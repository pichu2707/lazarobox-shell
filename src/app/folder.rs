//! The chat folder in the app (chat-folder): when it may change, and the
//! change itself.

use super::*;
use crate::core::ai::{
    chat::{
        approval::{ApprovalAsk, Origin, Reply},
        folder::{CdArg, Command, Probed, Purpose, Refusal, Verdict, expand, parse_command, typed},
    },
    sanitize::clean,
};

/// The system line of a `/cd` that cannot go on.
fn refused(reason: &str) -> String {
    format!("/cd refused: {reason}")
}

impl App {
    /// Why chat `id` cannot change folder now (see `ChatState::folder_busy`),
    /// with the run slots the app owns. `None` for a chat that is gone.
    pub(super) fn folder_busy(&self, id: PaneId) -> Option<&'static str> {
        let chat = self.panes.get(&id).and_then(PaneState::chat_state)?;
        let running = self
            .panes
            .values()
            .filter_map(PaneState::pty)
            .any(|pty| pty.run.as_ref().is_some_and(|slot| slot.chat == id));
        chat.folder_busy(running)
    }

    /// Whether `epoch` is the memory epoch chat `id` asks under now.
    pub(super) fn memory_epoch_is(&self, id: PaneId, epoch: u32) -> bool {
        self.panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .is_some_and(|chat| chat.memory_epoch() == epoch)
    }

    /// Makes `new` (probed, resolved) the folder of chat `id` and asks for
    /// what the old one pinned: the system prompt, the memory project and the
    /// tool session (whose approvals belong to the old folder). Every other
    /// consumer reads the folder when it needs it (design D1).
    pub(super) fn apply_folder(&mut self, id: PaneId, new: PathBuf) -> Vec<Effect> {
        let history = self.history;
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        // The session of the old project ends before the chat forgets it.
        let end = chat.take_end().map(|(session, project, summary)| {
            Effect::Memory(MemCmd::End {
                session,
                project,
                summary,
                cwd: None,
            })
        });
        let store = chat.store_set_folder(id, new.clone());
        let change = chat.set_folder(new);
        let session = chat.take_tool_session();
        let mut effects: Vec<Effect> = end.into_iter().collect();
        if history {
            effects.extend(store.map(Effect::Store));
        }
        if change.prompt_now {
            effects.extend(self.prepare_prompt(id));
        }
        if session {
            effects.push(Effect::EndToolSession(id));
        }
        effects.extend(self.begin_memory(id));
        self.dirty = true;
        effects
    }

    /// Enter on a `/cd` text: checks it, then asks the runtime to probe the
    /// folder. Nothing is sent, nothing is stored, and the composer keeps its
    /// text until a change is confirmed.
    pub(super) fn cd_submit(&mut self, id: PaneId, arg: &CdArg) -> Vec<Effect> {
        let busy = self.folder_busy(id);
        let home = self.home.clone();
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        self.dirty = true;
        let line = match (arg, busy) {
            (CdArg::Missing, _) => "usage: /cd <folder>".to_owned(),
            (CdArg::Path(_), Some(reason)) => refused(reason),
            (CdArg::Path(typed), None) => {
                let path = match home.as_deref() {
                    None if typed.starts_with('~') => None,
                    home => expand(typed, home.unwrap_or(Path::new("/")), chat.origin_cwd()),
                };
                match path {
                    Some(path) => {
                        return vec![Effect::ProbeFolder {
                            pane: id,
                            path,
                            purpose: Purpose::Cd,
                        }];
                    }
                    None => refused(Refusal::NotFound.text()),
                }
            }
        };
        chat.push_system_line(line);
        Vec::new()
    }

    /// The runtime judged a folder: a `/cd` goes on to the confirmation, a
    /// refused or unchanged one says why.
    pub(super) fn on_folder_probed(
        &mut self,
        id: PaneId,
        purpose: Purpose,
        result: Result<Probed, Refusal>,
    ) -> Vec<Effect> {
        let busy = self.folder_busy(id);
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        match purpose {
            Purpose::Cd => {
                // The answer of a command the composer no longer holds is stale.
                if !matches!(parse_command(&chat.composer().text()), Command::Cd(_)) {
                    return Vec::new();
                }
                self.dirty = true;
                let verdict = typed(result.as_ref().map(|probed| probed.verdict).map_err(|r| *r));
                let line = match (busy, verdict, result) {
                    (Some(reason), ..) => refused(reason),
                    (None, Err(refusal), _) => refused(refusal.text()),
                    (None, Ok(_), Ok(probed)) if chat.origin_cwd() == Some(&probed.path) => {
                        format!("already in {}", clean(&probed.path.to_string_lossy()))
                    }
                    (None, Ok(verdict), Ok(probed)) => {
                        let ask = ApprovalAsk::Folder {
                            from: chat.origin_cwd().map_or_else(
                                || "unknown".to_owned(),
                                |from| clean(&from.to_string_lossy()),
                            ),
                            to: probed.path,
                            note: (verdict != Verdict::Concrete)
                                .then(|| "broad folder: the tools keep their limits".to_owned()),
                        };
                        match chat.approvals_mut().request(ask, Origin::Block) {
                            Ok(_) => return Vec::new(),
                            Err(_) => refused("an approval is pending"),
                        }
                    }
                    (None, Ok(_), Err(_)) => return Vec::new(),
                };
                chat.push_system_line(line);
                Vec::new()
            }
            Purpose::Resume => Vec::new(),
        }
    }

    /// Whether the approval waiting in chat `id` is the folder confirmation.
    pub(super) fn folder_ask_pending(&self, id: PaneId) -> bool {
        self.panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .and_then(|chat| chat.approvals().pending())
            .is_some_and(|approval| matches!(approval.ask(), ApprovalAsk::Folder { .. }))
    }

    /// A key while the folder confirmation waits: an unmodified `y` changes
    /// the folder, any other key cancels. A bare modifier is not an answer.
    pub(super) fn on_folder_key(&mut self, id: PaneId, key: KeyEvent) -> Vec<Effect> {
        if matches!(key.code, KeyCode::Modifier(_)) {
            return Vec::new();
        }
        let confirmed = key.code == KeyCode::Char('y') && key.modifiers == KeyModifiers::NONE;
        let reply = if confirmed {
            Reply::Approve
        } else {
            Reply::Decline
        };
        let Some(approval) = self.close_approval(id, reply) else {
            return Vec::new();
        };
        self.dirty = true;
        let ApprovalAsk::Folder { to, .. } = approval.ask() else {
            return Vec::new();
        };
        if !confirmed {
            return Vec::new();
        }
        // Busy is checked again now: the chat may have changed while it waited.
        if let Some(reason) = self.folder_busy(id) {
            if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
                chat.push_system_line(refused(reason));
            }
            return Vec::new();
        }
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            chat.composer_mut().clear();
        }
        self.apply_folder(id, to.clone())
    }
}
