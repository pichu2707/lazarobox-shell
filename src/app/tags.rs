//! The tag editor in the app (ai-chat C9, chat-history: Edit tags): opened by
//! `t` on a list row or `Ctrl+Space T` in a conversation, saved on Enter.

use super::*;
use crate::core::ai::chat::{
    list::TAGS_FAILED,
    store::TAGS_NOT_SAVED_LINE,
    tag_editor::{TagOutcome, TagTarget},
    tags::Tags,
};

impl App {
    /// Whether chat `id` has its tag editor open.
    pub(super) fn tag_editor_open(&self, id: PaneId) -> bool {
        self.panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .is_some_and(|chat| chat.tag_editor().is_some())
    }

    /// `Prefix T`: opens the tag editor on the conversation of the focused
    /// chat. It needs the history (nothing stores a tag without it), and it
    /// leaves the list (which has its own `t`), the note editor and the summary
    /// draft alone.
    pub(super) fn chat_tags_key(&mut self, id: PaneId) -> Vec<Effect> {
        if !self.history {
            self.notice = Some(TAGS_NEED_HISTORY.to_owned());
            return Vec::new();
        }
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut)
            && chat.open_tag_editor(TagTarget::Pane)
        {
            self.dirty = true;
        }
        Vec::new()
    }

    /// `t` on a list row: the editor opens on the tags of that row.
    pub(super) fn edit_row_tags(&mut self, id: PaneId, conversation: i64) {
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return;
        };
        let Some(tags) = chat.list().and_then(|list| list.tags_of(conversation)) else {
            return;
        };
        chat.open_tag_editor_for(TagTarget::Row(conversation), &tags);
    }

    /// A key while the tag editor of chat `id` is open. Enter stores the tags
    /// at once; Esc drops the edit.
    pub(super) fn on_tag_key(&mut self, id: PaneId, key: &KeyEvent, repeat: bool) -> Vec<Effect> {
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        let Some(editor) = chat.tag_editor_mut() else {
            return Vec::new();
        };
        let outcome = editor.on_key(key, repeat);
        let target = editor.target();
        self.dirty = true;
        match outcome {
            TagOutcome::Handled => Vec::new(),
            TagOutcome::Discard => {
                chat.close_tag_editor();
                Vec::new()
            }
            TagOutcome::Save(tags) => {
                chat.close_tag_editor();
                self.save_tags(id, target, tags)
            }
        }
    }

    /// The store command for edited tags. A row of the list is stored by its
    /// id; the conversation of the pane is applied to the pane and, when it
    /// exists, stored (before it exists the first `Create` carries the tags).
    fn save_tags(&mut self, id: PaneId, target: TagTarget, tags: Tags) -> Vec<Effect> {
        let cmd = match target {
            TagTarget::Row(conversation) => Some(StoreCmd::SetTags {
                pane: id,
                conversation: Some(conversation),
                tags,
            }),
            TagTarget::Pane => {
                let chat = self.panes.get_mut(&id).and_then(PaneState::chat_mut);
                chat.and_then(|chat| {
                    let cmd = chat.edit_tags(id, tags);
                    if cmd.is_none() && chat.store_detached() {
                        chat.push_system(TAGS_NOT_SAVED_LINE);
                    }
                    cmd
                })
            }
        };
        cmd.map(Effect::Store).into_iter().collect()
    }

    /// A paste into the tag editor of chat `id`.
    pub(super) fn tag_paste(&mut self, id: PaneId, text: &str) {
        if let Some(editor) = self
            .panes
            .get_mut(&id)
            .and_then(PaneState::chat_mut)
            .and_then(ChatState::tag_editor_mut)
        {
            editor.paste(text);
            self.dirty = true;
        }
    }

    /// The store's answer to a tag edit.
    pub(super) fn on_tags_reply(&mut self, reply: &StoreReply) {
        match reply {
            StoreReply::TagsSaved {
                conversation,
                tags,
                updated_at,
                ..
            } => {
                for state in self.panes.values_mut() {
                    let Some(chat) = state.chat_mut() else {
                        continue;
                    };
                    if let Some(list) = chat.list_mut() {
                        list.set_tags(*conversation, tags, *updated_at);
                    }
                    if chat.conversation() == Some(*conversation) {
                        chat.set_tags(tags);
                    }
                }
                self.dirty = true;
            }
            StoreReply::TagsFailed { pane } => {
                if self.list_open(*pane) {
                    self.set_list_line(*pane, TAGS_FAILED);
                } else if let Some(chat) = self.panes.get_mut(pane).and_then(PaneState::chat_mut) {
                    chat.push_system(TAGS_FAILED);
                    self.dirty = true;
                }
            }
            _ => {}
        }
    }

    /// A conversation was deleted: an editor on its row has nothing left to
    /// edit.
    pub(super) fn close_tag_editors_of(&mut self, conversation: i64) {
        for state in self.panes.values_mut() {
            if let Some(chat) = state.chat_mut()
                && chat
                    .tag_editor()
                    .is_some_and(|editor| editor.target() == TagTarget::Row(conversation))
            {
                chat.close_tag_editor();
            }
        }
    }
}
