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

/// The `/cd` a chat waits for the answer of: its token and the argument as
/// submitted, to tell an edited or repeated command from the one that asked.
pub(super) struct CdRequest {
    pub(super) token: u64,
    arg: String,
    /// The folder the user confirmed, while the last check of it runs.
    confirm: Option<PathBuf>,
}

/// A send waits for memory: it must not leave under another folder.
const PARKED: &str = "a message is waiting to be sent";

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
        // A send parked for the memory answer would leave under the new folder.
        let parked = self.waiting_sends.iter().any(|(pane, _)| *pane == id);
        chat.folder_busy(running).or(parked.then_some(PARKED))
    }

    /// Whether `epoch` is the memory epoch chat `id` asks under now.
    pub(super) fn memory_epoch_is(&self, id: PaneId, epoch: u32) -> bool {
        self.panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .is_some_and(|chat| chat.memory_epoch() == epoch)
    }

    /// Whether save `token` of chat `id` was sent before its folder changed.
    pub(super) fn save_is_stale(
        &self,
        id: PaneId,
        token: crate::core::ai::memory::port::SaveId,
    ) -> bool {
        self.panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .is_some_and(|chat| chat.save_is_stale(token))
    }

    /// Makes `new` (probed, resolved) the folder of chat `id` and asks for
    /// what the old one pinned: the system prompt, the memory project and the
    /// tool session (whose approvals belong to the old folder). Every other
    /// consumer reads the folder when it needs it (design D1).
    pub(super) fn apply_folder(&mut self, id: PaneId, new: PathBuf) -> Vec<Effect> {
        self.change_folder(id, new, false)
    }

    /// Puts back the folder a resumed conversation was stored with: no write
    /// to the store (it came from there), no end of the resumed session.
    pub(super) fn restore_folder(&mut self, id: PaneId, new: PathBuf) -> Vec<Effect> {
        self.change_folder(id, new, true)
    }

    fn change_folder(&mut self, id: PaneId, new: PathBuf, restore: bool) -> Vec<Effect> {
        // A resume answer still on its way must not undo this choice.
        self.resume_folders.remove(&id);
        let history = self.history && !restore;
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        // The session of the old project ends before the chat forgets it.
        let end =
            (!restore)
                .then(|| chat.take_end())
                .flatten()
                .map(|(session, project, summary)| {
                    Effect::Memory(MemCmd::End {
                        session,
                        project,
                        summary,
                        cwd: None,
                    })
                });
        let store = chat.store_set_folder(id, new.clone());
        let change = if restore {
            chat.restore_folder(new)
        } else {
            chat.set_folder(new)
        };
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
        // The user's own `/cd` outranks a resume answer still on its way.
        self.resume_folders.remove(&id);
        self.cd_requests.remove(&id);
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
                        self.folder_seq += 1;
                        let token = self.folder_seq;
                        self.cd_requests.insert(
                            id,
                            CdRequest {
                                token,
                                arg: typed.clone(),
                                confirm: None,
                            },
                        );
                        return vec![Effect::ProbeFolder {
                            pane: id,
                            path,
                            purpose: Purpose::Cd(token),
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
        let current = self.cd_requests.get(&id).map(|r| (r.token, r.arg.clone()));
        let confirming = self.cd_requests.get(&id).and_then(|r| r.confirm.clone());
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        match purpose {
            Purpose::Resume(token) => self.on_resume_probed(id, token, result, busy),
            // The picker takes it up (PR B2).
            Purpose::Pick(_) => Vec::new(),
            Purpose::Cd(token) | Purpose::Confirm(token) => {
                // Only the answer of the latest command, still in the
                // composer as it was submitted, counts.
                let same = match (current, parse_command(&chat.composer().text())) {
                    (Some((latest, submitted)), Command::Cd(CdArg::Path(arg))) => {
                        latest == token && submitted == arg
                    }
                    _ => false,
                };
                if !same {
                    return Vec::new();
                }
                if matches!(purpose, Purpose::Confirm(_)) {
                    return self.on_confirm_probed(id, confirming, result, busy);
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
        }
    }

    /// The check made at `y` answered: the folder is changed only when it is
    /// still the one the user saw, and the chat is still free.
    fn on_confirm_probed(
        &mut self,
        id: PaneId,
        confirmed: Option<PathBuf>,
        result: Result<Probed, Refusal>,
        busy: Option<&'static str>,
    ) -> Vec<Effect> {
        self.cd_requests.remove(&id);
        self.dirty = true;
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        let same = result.ok().filter(|probed| {
            Some(&probed.path) == confirmed.as_ref() && typed(Ok(probed.verdict)).is_ok()
        });
        match (busy, same) {
            (Some(reason), _) => chat.push_system_line(refused(reason)),
            (None, None) => chat.push_system_line(refused("folder changed since it was checked")),
            (None, Some(probed)) => {
                chat.composer_mut().clear();
                return self.apply_folder(id, probed.path);
            }
        }
        Vec::new()
    }

    /// A conversation was resumed: its stored folder is judged by the
    /// runtime, or the chat says it has none.
    pub(super) fn probe_stored_folder(
        &mut self,
        id: PaneId,
        conversation: i64,
        folder: Option<&Path>,
    ) -> Option<Effect> {
        let Some(folder) = folder else {
            self.resume_folders.remove(&id);
            let chat = self.panes.get_mut(&id).and_then(PaneState::chat_mut)?;
            chat.push_system_line("no stored folder".to_owned());
            return None;
        };
        self.folder_seq += 1;
        let token = self.folder_seq;
        self.resume_folders
            .insert(id, (token, conversation, folder.to_path_buf()));
        Some(Effect::ProbeFolder {
            pane: id,
            path: folder.to_path_buf(),
            purpose: Purpose::Resume(token),
        })
    }

    /// The probe of a resumed conversation's folder answered: a concrete
    /// folder is taken without asking (it was the user's choice), anything
    /// else leaves the chat where it is and says so.
    fn on_resume_probed(
        &mut self,
        id: PaneId,
        token: u64,
        result: Result<Probed, Refusal>,
        busy: Option<&'static str>,
    ) -> Vec<Effect> {
        // Only the answer of the latest resume counts.
        if self.resume_folders.get(&id).map(|entry| entry.0) != Some(token) {
            return Vec::new();
        }
        let Some((_, conversation, stored)) = self.resume_folders.remove(&id) else {
            return Vec::new();
        };
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        // The answer of a conversation the chat has since left is stale.
        if chat.stored_conversation() != Some(conversation) {
            return Vec::new();
        }
        self.dirty = true;
        let line = match result {
            Ok(probed) if probed.verdict == Verdict::Concrete => {
                if chat.origin_cwd() == Some(&probed.path) {
                    return Vec::new();
                }
                match busy {
                    None => return self.restore_folder(id, probed.path),
                    Some(reason) => format!("stored folder not used: {reason}"),
                }
            }
            _ => format!(
                "stored folder not available: {}",
                clean(&stored.to_string_lossy())
            ),
        };
        chat.push_system_line(line);
        Vec::new()
    }

    /// The pane is gone: nothing it asked about is answered any more.
    pub(super) fn drop_folder_state(&mut self, id: PaneId) {
        self.resume_folders.remove(&id);
        self.cd_requests.remove(&id);
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
        // The folder is resolved once more before it is taken.
        self.folder_seq += 1;
        let token = self.folder_seq;
        let Some(request) = self.cd_requests.get_mut(&id) else {
            return Vec::new();
        };
        request.token = token;
        request.confirm = Some(to.clone());
        vec![Effect::ProbeFolder {
            pane: id,
            path: to.clone(),
            purpose: Purpose::Confirm(token),
        }]
    }
}
