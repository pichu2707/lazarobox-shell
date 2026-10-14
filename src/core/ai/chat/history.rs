//! The history side of a chat pane: the project tag, the conversation list
//! and, in time, resume and the store link. Pure.

use super::{ChatState, list::ListView};

impl ChatState {
    /// The project tag proposed for this conversation.
    pub fn tag(&self) -> Option<&str> {
        self.tag.as_deref()
    }

    pub fn set_tag(&mut self, tag: Option<String>) {
        self.tag = tag;
    }

    /// The conversation list, while it is shown.
    pub fn list(&self) -> Option<&ListView> {
        self.list.as_ref()
    }

    pub fn list_mut(&mut self) -> Option<&mut ListView> {
        self.list.as_mut()
    }

    /// Shows the list, filtered by the project tag. The rows arrive with the
    /// store's answer.
    pub fn open_list(&mut self) {
        self.list = Some(ListView::new(self.tag.clone()));
    }

    pub fn close_list(&mut self) {
        self.list = None;
    }
}
