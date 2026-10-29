//! The folder picker in the app (chat-folder, design D10/D11): opening a chat
//! settles its folder first, and the picker runs on the chat's keys.

use super::*;
use crate::core::ai::{
    chat::{
        folder::{CdArg, Command, Probed, Purpose, Refusal, Verdict, parse_command},
        folder_picker::{
            Completion, PickerOutcome, RECENT_ASK, Source, completed, completion_target, merge,
        },
    },
    sanitize::clean,
};

/// The completion id of the composer's `/cd` line: picker ids start at 1.
const COMPOSER: u64 = 0;
const CHOOSE_FIRST: &str = "Choose the chat folder first";
const NO_MATCH: &str = "no matching folder";
const BUSY: &str = "folder check is busy, try again";

impl App {
    /// Whether chat `id` shows its folder picker.
    pub(super) fn picker_open(&self, id: PaneId) -> bool {
        self.panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .is_some_and(|chat| chat.folder_picker().is_some())
    }

    /// Whether chat `id` waits for its folder (the open probe or the picker):
    /// it takes no key but the picker's.
    pub(super) fn folder_pending(&self, id: PaneId) -> bool {
        self.opening.contains(&id) && !self.picker_open(id)
    }

    /// Shows the picker over chat `id` and starts gathering its candidates:
    /// the store's recent folders first (history on), then the judging.
    pub(super) fn open_folder_picker(&mut self, id: PaneId) -> Vec<Effect> {
        let home = self.home.clone().unwrap_or_else(|| PathBuf::from("/"));
        // Session-unique, never per pane: a reopened picker is another one.
        self.folder_seq += 1;
        let picker = self.folder_seq;
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        chat.open_folder_picker(picker, home);
        self.dirty = true;
        if self.history {
            let limit = RECENT_ASK;
            vec![Effect::Store(StoreCmd::RecentFolders { pane: id, limit })]
        } else {
            self.gather(id, Vec::new())
        }
    }

    /// The candidates of chat `id`: the folders of `recent` (the newest is the
    /// last used one), the shells of its tab and the shell used last.
    fn gather(&self, id: PaneId, recent: Vec<PathBuf>) -> Vec<Effect> {
        let mut paths: Vec<(Source, PathBuf)> = recent
            .into_iter()
            .enumerate()
            .map(|(n, path)| {
                (
                    if n == 0 {
                        Source::LastUsed
                    } else {
                        Source::Recent
                    },
                    path,
                )
            })
            .collect();
        let cwd_of = |pane: PaneId| {
            let state = self.panes.get(&pane)?;
            state.effective_cwd().map(Path::to_path_buf)
        };
        if let Some(tab) = self.tabs.iter().find(|tab| tab.tree.leaves().contains(&id)) {
            let panes = self.shell_panes_in(tab);
            paths.extend(
                panes
                    .into_iter()
                    .filter_map(cwd_of)
                    .map(|p| (Source::PaneRoot, p)),
            );
            paths.extend(
                tab.last_shell
                    .and_then(cwd_of)
                    .map(|p| (Source::LastShell, p)),
            );
        }
        let picker = self.picker_id(id).unwrap_or_default();
        vec![Effect::ProbeFolders {
            pane: id,
            id: picker,
            paths,
            git: true,
        }]
    }

    /// The id of the picker chat `id` shows.
    fn picker_id(&self, id: PaneId) -> Option<u64> {
        let chat = self.panes.get(&id).and_then(PaneState::chat_state);
        chat.and_then(|chat| chat.folder_picker()).map(|p| p.id())
    }

    /// The store listed the recent folders: the candidates can be judged.
    pub(super) fn on_recent_folders(&mut self, id: PaneId, folders: Vec<PathBuf>) -> Vec<Effect> {
        let gathering = self
            .panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .and_then(|chat| chat.folder_picker())
            .is_some_and(|picker| picker.gathering());
        if gathering {
            self.gather(id, folders)
        } else {
            Vec::new()
        }
    }

    /// The candidates were judged: they become the rows. A late list for a
    /// picker that is gone, or already filled, is dropped.
    pub(super) fn on_folders_probed(
        &mut self,
        id: PaneId,
        picker_id: u64,
        found: Vec<(Source, Result<Probed, Refusal>)>,
        busy: bool,
    ) -> Vec<Effect> {
        if let Some(picker) = self
            .panes
            .get_mut(&id)
            .and_then(PaneState::chat_mut)
            .and_then(|chat| chat.folder_picker_mut())
            .filter(|picker| picker.gathering())
        {
            picker.set_rows(picker_id, merge(found));
            if busy {
                picker.note(BUSY);
            }
            self.dirty = true;
        }
        Vec::new()
    }

    /// The directory names that complete the field text arrived. Id 0 is the
    /// `/cd` line of the composer, the others name a picker.
    pub(super) fn on_folder_completed(
        &mut self,
        id: PaneId,
        picker_id: u64,
        text: &str,
        found: &Completion,
    ) -> Vec<Effect> {
        if picker_id == COMPOSER {
            return self.on_cd_completed(id, text, found);
        }
        if let Some(picker) = self
            .panes
            .get_mut(&id)
            .and_then(PaneState::chat_mut)
            .and_then(|chat| chat.folder_picker_mut())
        {
            let fresh = picker.on_completed(picker_id, text, found);
            if fresh && found.busy {
                picker.note(BUSY);
            } else if fresh && found.names.is_empty() && !found.truncated {
                picker.note(NO_MATCH);
            }
            self.dirty |= fresh;
        }
        Vec::new()
    }

    /// A key while the picker is shown: the picker takes every one.
    pub(super) fn on_picker_key(
        &mut self,
        id: PaneId,
        key: &KeyEvent,
        repeat: bool,
    ) -> Vec<Effect> {
        let Some(picker) = self
            .panes
            .get_mut(&id)
            .and_then(PaneState::chat_mut)
            .and_then(|chat| chat.folder_picker_mut())
        else {
            return Vec::new();
        };
        let picker_id = picker.id();
        if picker.gathering() && key.code == KeyCode::Tab {
            picker.note("still looking for folders");
            self.dirty = true;
            return Vec::new();
        }
        let outcome = picker.on_key(key, repeat);
        self.dirty = true;
        match outcome {
            PickerOutcome::Handled => Vec::new(),
            PickerOutcome::Probe(path) => vec![Effect::ProbeFolder {
                pane: id,
                path,
                purpose: Purpose::Pick(picker_id),
            }],
            PickerOutcome::Complete { text, dir, prefix } => vec![Effect::CompleteFolder {
                pane: id,
                id: picker_id,
                text,
                dir,
                prefix,
            }],
            PickerOutcome::Cancel => self.cancel_picker(id),
        }
    }

    /// Tags, notes, summaries and the memory switch wait for the folder: they
    /// would open a view the picker hides.
    pub(super) fn refuse_over_folder(&mut self) -> bool {
        let id = self.tab().focus;
        let waits = self.focused_is_chat() && (self.picker_open(id) || self.folder_pending(id));
        if waits {
            self.notice = Some(CHOOSE_FIRST.to_owned());
        }
        waits
    }

    /// A paste into the picker's field.
    pub(super) fn picker_paste(&mut self, id: PaneId, text: &str) {
        if let Some(picker) = self
            .panes
            .get_mut(&id)
            .and_then(PaneState::chat_mut)
            .and_then(|chat| chat.folder_picker_mut())
        {
            picker.paste(text);
            self.dirty = true;
        }
    }

    /// Tab in the composer: a `/cd <path>` line asks which directories
    /// complete the path. Anything else is not a completion.
    pub(super) fn cd_complete(&mut self, id: PaneId) -> Vec<Effect> {
        let home = self.home.clone().unwrap_or_else(|| PathBuf::from("/"));
        let Some(chat) = self.panes.get(&id).and_then(PaneState::chat_state) else {
            return Vec::new();
        };
        let text = chat.composer().text();
        let Command::Cd(CdArg::Path(arg)) = parse_command(&text) else {
            return Vec::new();
        };
        match completion_target(&arg, &home, chat.origin_cwd()) {
            Some((dir, prefix)) if !text.contains('\n') => vec![Effect::CompleteFolder {
                pane: id,
                id: COMPOSER,
                text,
                dir,
                prefix,
            }],
            _ => Vec::new(),
        }
    }

    /// The names that complete a `/cd` line arrived: the line takes them
    /// only if it is still the one that was asked about.
    fn on_cd_completed(&mut self, id: PaneId, text: &str, found: &Completion) -> Vec<Effect> {
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        if chat.composer().text() != text {
            return Vec::new();
        }
        let Command::Cd(CdArg::Path(arg)) = parse_command(text) else {
            return Vec::new();
        };
        self.dirty = true;
        match completed(&arg, found) {
            Some(done) => {
                chat.composer_mut().clear();
                chat.composer_mut().paste(&format!("/cd {done}"));
            }
            None if found.busy => chat.push_system_line(BUSY.to_owned()),
            None if found.names.is_empty() && !found.truncated => {
                chat.push_system_line(NO_MATCH.to_owned());
            }
            None => {}
        }
        Vec::new()
    }

    /// The open probe answered: a concrete folder opens the chat as before,
    /// anything else (broad, unknown, refused) asks the user.
    pub(super) fn on_open_probed(
        &mut self,
        id: PaneId,
        result: Result<Probed, Refusal>,
    ) -> Vec<Effect> {
        if !self.opening.contains(&id) || self.picker_open(id) {
            return Vec::new();
        }
        match result {
            Ok(probed) if probed.verdict == Verdict::Concrete => self.finish_open(id),
            _ => self.open_folder_picker(id),
        }
    }

    /// The probe of the folder the picker chose answered: a folder the chat
    /// may use is applied, a refused one stays in the picker with its reason.
    pub(super) fn on_pick_probed(
        &mut self,
        id: PaneId,
        picker: u64,
        result: Result<Probed, Refusal>,
    ) -> Vec<Effect> {
        let busy = self.folder_busy(id);
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        self.dirty = true;
        let Some(path) = chat
            .folder_picker_mut()
            .and_then(|p| p.on_probed(picker, result))
        else {
            return Vec::new();
        };
        // A busy chat keeps the picker, so the pick is not thrown away.
        if let (Some(reason), Some(picker)) = (busy, chat.folder_picker_mut()) {
            picker.note(reason);
            return Vec::new();
        }
        chat.close_folder_picker();
        let mut effects = if chat.origin_cwd() == Some(&path) {
            Vec::new()
        } else {
            self.apply_folder(id, path)
        };
        effects.extend(self.finish_open(id));
        effects
    }

    /// Esc: the chat keeps the folder it has, and says so.
    fn cancel_picker(&mut self, id: PaneId) -> Vec<Effect> {
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        chat.close_folder_picker();
        let folder = chat
            .origin_cwd()
            .map_or_else(|| "unknown".to_owned(), |p| clean(&p.to_string_lossy()));
        chat.push_system_line(format!(
            "chat folder is {folder}; use /cd <folder> to change it"
        ));
        self.finish_open(id)
    }

    /// The folder is settled: runs the steps `open_chat_pane` held back.
    pub(super) fn finish_open(&mut self, id: PaneId) -> Vec<Effect> {
        if !self.opening.remove(&id) {
            return Vec::new();
        }
        let mut effects = self.begin_memory(id);
        self.show_server_offer();
        effects.extend(self.prepare_prompt(id));
        if self.history {
            let origin = self
                .panes
                .get(&id)
                .and_then(PaneState::chat_state)
                .and_then(|chat| chat.origin_cwd().map(Path::to_path_buf));
            effects.push(Effect::Store(StoreCmd::List { pane: id, origin }));
        }
        effects
    }
}
