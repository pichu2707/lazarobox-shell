//! Pure edit of `config.toml` text: changes only the menu-owned keys and
//! keeps everything else byte-for-byte (comments, order, whitespace).

use super::config::ConfigEdit;

/// Why an edit was refused; nothing is written in either case.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditError {
    /// The existing text is not valid TOML.
    Unparseable,
    /// The named entry exists but is not a table.
    NotATable(&'static str),
}

/// Applies `edits` to the existing file text (`None` means no file yet) and
/// returns the new text.
pub fn apply_edit(existing: Option<&str>, edits: &[ConfigEdit]) -> Result<String, EditError> {
    let _ = (existing, edits);
    Ok(String::new())
}
