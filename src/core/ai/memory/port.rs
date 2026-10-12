//! The memory port: the commands the app sends to the engram worker and the
//! replies that come back. Plain values, no IO.

use super::kind::MemKind;

/// A note ready to be saved. `content` is NOT yet redacted: the call builder
/// redacts and caps it, so no other path can send raw text.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Note {
    pub kind: MemKind,
    pub title: String,
    pub content: String,
}
