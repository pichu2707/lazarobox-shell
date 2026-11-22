//! The local project memory: small tagged notes kept as one JSON file each
//! under `.lzr/` at the repository root. The note schema, the area of a
//! folder, the private ledger that tells which notes this machine made live
//! here; the file IO of the store is in
//! `runtime/memory_local.rs`.

pub mod area;
pub mod gitignore;
pub mod ledger;
pub mod note;
pub mod port;

/// The store folder at the repository root.
pub const STORE_DIR: &str = ".lzr";
/// Where the notes are, below the store folder.
pub const NOTES_DIR: &str = "memory/notes";
