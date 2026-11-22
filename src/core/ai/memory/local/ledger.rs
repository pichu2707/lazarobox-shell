//! The private ledger: which notes this machine made. Trust comes from here,
//! never from a field of the note, because a committed note can claim
//! anything. The ledger file lives in the data folder, a root tools never
//! read.

use std::{collections::BTreeMap, path::Path};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::note::LocalNote;

/// `{id: content hash}` of the notes this machine wrote.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Ledger {
    pub own: BTreeMap<String, String>,
}

impl Ledger {
    /// Reads a ledger file's text; anything unreadable is an empty ledger, so
    /// every note counts as foreign (the safe side).
    pub fn parse(text: &str) -> Ledger {
        let own = match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(mut root)) => match root.remove("own") {
                Some(Value::Object(own)) => own,
                _ => return Ledger::default(),
            },
            _ => return Ledger::default(),
        };
        let own = own
            .into_iter()
            .filter_map(|(id, hash)| Some((id, hash.as_str()?.to_owned())))
            .collect();
        Ledger { own }
    }

    pub fn to_json(&self) -> String {
        let mut text =
            serde_json::to_string_pretty(&json!({ "own": self.own })).unwrap_or_default();
        text.push('\n');
        text
    }

    /// Whether `note` is one this machine wrote and nobody changed since.
    pub fn is_local(&self, note: &LocalNote) -> bool {
        self.own
            .get(&note.id)
            .is_some_and(|hash| *hash == content_hash(note))
    }

    pub fn record(&mut self, note: &LocalNote) {
        self.own.insert(note.id.clone(), content_hash(note));
    }

    /// The union with what another instance wrote meanwhile; on one id, `self`
    /// wins.
    pub fn merged(self, mut disk: Ledger) -> Ledger {
        disk.own.extend(self.own);
        disk
    }
}

/// The hash a ledger keeps for a note: its canonical JSON.
pub fn content_hash(note: &LocalNote) -> String {
    format!("{:x}", Sha256::digest(note.to_json().as_bytes()))
}

/// The first 16 hex digits of the SHA-256 of `parts`, each one length-prefixed.
fn short_hash(parts: &[&[u8]]) -> String {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update((part.len() as u64).to_le_bytes());
        hash.update(part);
    }
    format!("{:x}", hash.finalize())[..16].to_owned()
}

/// `n-` plus 16 hex digits of a hash of the machine, the time, a counter and
/// the text: no randomness is needed.
pub fn note_id(machine: &str, nanos: u128, counter: u64, text: &str) -> String {
    let (nanos, counter) = (nanos.to_le_bytes(), counter.to_le_bytes());
    format!(
        "n-{}",
        short_hash(&[machine.as_bytes(), &nanos, &counter, text.as_bytes()])
    )
}

/// `m-` plus 16 hex digits of a hash of random `entropy`.
pub fn machine_id(entropy: &[u8]) -> String {
    format!("m-{}", short_hash(&[entropy]))
}

/// The ledger file name of a store root: 16 hex digits of its hash.
pub fn ledger_name(root: &Path) -> String {
    format!("{}.json", short_hash(&[root.to_string_lossy().as_bytes()]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ai::memory::local::note::{NewNote, Origin};

    fn note(text: &str) -> LocalNote {
        let new = NewNote::new("decision", ".", text, Origin::User).unwrap();
        LocalNote::from_new(new, note_id("m-1", 1, 1, text), "m-1".into(), 1)
    }

    #[test]
    fn an_own_note_with_the_recorded_hash_is_local() {
        let n = note("use Axum");
        let mut ledger = Ledger::default();
        ledger.record(&n);
        assert!(ledger.is_local(&n));
    }

    #[test]
    fn an_unknown_id_is_foreign() {
        assert!(!Ledger::default().is_local(&note("x")));
    }

    #[test]
    fn a_hand_edited_own_note_is_foreign() {
        let n = note("use Axum");
        let mut ledger = Ledger::default();
        ledger.record(&n);
        let json = n.to_json().replace("use Axum", "use Actix");
        let edited = LocalNote::parse(json.as_bytes()).unwrap();
        assert!(!ledger.is_local(&edited));
    }

    #[test]
    fn a_note_that_claims_this_machine_is_still_foreign() {
        let mut other = note("x");
        other.machine = "m-1".into();
        assert!(!Ledger::default().is_local(&other));
    }

    #[test]
    fn the_ledger_round_trips_and_a_bad_file_is_empty() {
        let mut ledger = Ledger::default();
        ledger.record(&note("a"));
        assert_eq!(Ledger::parse(&ledger.to_json()), ledger);
        for bad in ["", "{", "[]", r#"{"own":3}"#] {
            assert_eq!(Ledger::parse(bad), Ledger::default(), "{bad:?}");
        }
    }

    #[test]
    fn merging_keeps_the_entries_of_both_writers() {
        let (a, b) = (note("a"), note("b"));
        let (mut mine, mut theirs) = (Ledger::default(), Ledger::default());
        mine.record(&a);
        theirs.record(&b);
        let merged = mine.merged(theirs);
        assert!(merged.is_local(&a) && merged.is_local(&b));
    }

    #[test]
    fn ids_have_the_documented_shape_and_differ() {
        let id = note_id("m-1", 5, 1, "t");
        assert_eq!(id.len(), 18);
        assert!(id.starts_with("n-") && id[2..].bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(id, note_id("m-1", 5, 2, "t"));
        let machine = machine_id(b"entropy");
        assert!(machine.starts_with("m-") && machine.len() == 18);
        assert_eq!(ledger_name(Path::new("/r")).len(), 16 + ".json".len());
    }
}
