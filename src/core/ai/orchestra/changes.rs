//! The change ledger of a `code` worker (S7): the files it staged, each with
//! its size and a hash of its diff, and whether the user's batch review
//! applied it. Pure: the host reports what it staged and what was applied.
//!
//! A worker's writes are staged in its pane's batch and reach the disk only
//! when the user approves that batch: the ledger only records, it never
//! approves anything.

use std::fmt::Write as _;

/// One staged path, as the orchestrator is told about it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Change {
    /// The path relative to the chat's root, one sanitized line.
    pub path: String,
    /// The bytes of the staged text; `None` for a folder.
    pub size: Option<u64>,
    /// [`diff_hash`] of the staged change.
    pub hash: String,
    /// The user's batch review applied it to the disk.
    pub applied: bool,
}

/// The files a worker staged, in the order it first staged them.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ChangeLedger(Vec<Change>);

impl ChangeLedger {
    pub fn changes(&self) -> &[Change] {
        &self.0
    }

    /// Records a staged change. Staging the same path again replaces its
    /// size and hash in place, and it is not applied until a review applies
    /// it again.
    pub fn staged(&mut self, change: Change) {
        let change = Change {
            applied: false,
            ..change
        };
        match self.0.iter_mut().find(|known| known.path == change.path) {
            Some(known) => *known = change,
            None => self.0.push(change),
        }
    }

    /// The user's review applied a batch holding `paths`: those of them this
    /// worker staged are now on disk.
    pub fn applied(&mut self, paths: &[String]) {
        for change in &mut self.0 {
            if paths.contains(&change.path) {
                change.applied = true;
            }
        }
    }

    /// The `files changed` block of the worker's result, `None` when it
    /// staged nothing: one line per path with its size, its diff hash and
    /// whether it was applied.
    pub fn render(&self) -> Option<String> {
        if self.0.is_empty() {
            return None;
        }
        let mut out = String::from("files changed:");
        for change in &self.0 {
            let size = match change.size {
                Some(bytes) => format!("{bytes} bytes"),
                None => "folder".to_owned(),
            };
            let state = if change.applied {
                "applied"
            } else {
                "not applied"
            };
            let _ = write!(
                out,
                "\n- {} ({size}, diff {}): {state}",
                change.path, change.hash
            );
        }
        Some(out)
    }
}

/// A short fingerprint of a staged change: FNV-1a 64 over the text it
/// replaces (none for a create) and its new text, as 16 hex digits. It tells
/// two versions of a change apart; it is no security boundary.
pub fn diff_hash(base: Option<&str>, text: &str) -> String {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    let mut eat = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(PRIME);
        }
    };
    match base {
        None => eat(b"new\0"),
        Some(base) => {
            // Length-prefixed, so no base and text pair reads as another.
            eat(b"old\0");
            eat(&(base.len() as u64).to_le_bytes());
            eat(base.as_bytes());
        }
    }
    eat(text.as_bytes());
    format!("{hash:016x}")
}
