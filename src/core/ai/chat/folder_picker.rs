//! The folder picker of a chat (chat-folder, design D10). Pure: the runtime
//! reads the disk and the store; this module orders what they found and
//! drives the keys.

use std::path::PathBuf;

use super::folder::{Probed, Refusal, Verdict};

/// The most rows the picker lists.
pub const ROWS_MAX: usize = 12;

/// Where a candidate came from, in the order the rows are listed.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Source {
    /// The folder the chat used last.
    LastUsed,
    /// The git root of a pane of the tab.
    PaneRoot,
    /// A folder an earlier conversation was kept under.
    Recent,
    /// The cwd the tab's last shell had.
    LastShell,
}

/// One row of the picker: a resolved, concrete folder.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Row {
    pub path: PathBuf,
    pub source: Source,
}

/// Orders probed candidates: by source, then as found; the same real path
/// once; only folders a chat may work in (`Concrete`); at most [`ROWS_MAX`].
pub fn merge(found: Vec<(Source, Result<Probed, Refusal>)>) -> Vec<Row> {
    let mut found: Vec<_> = found
        .into_iter()
        .filter_map(|(source, result)| match result {
            Ok(Probed {
                path,
                verdict: Verdict::Concrete,
            }) => Some(Row { path, source }),
            _ => None,
        })
        .collect();
    // Stable: candidates of one source keep the order they were found in.
    found.sort_by_key(|row| row.source);
    let mut rows: Vec<Row> = Vec::new();
    for row in found {
        if rows.len() < ROWS_MAX && rows.iter().all(|seen| seen.path != row.path) {
            rows.push(row);
        }
    }
    rows
}
