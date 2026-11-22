//! The private ledger: which notes this machine made. Trust comes from here,
//! never from a field of the note, because a committed note can claim
//! anything. The ledger file lives in the data folder, a root tools never
//! read.

use std::{collections::BTreeMap, path::Path};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use super::note::LocalNote;

/// Why a ledger file cannot be used.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LedgerError {
    /// Not a ledger: the file is broken or edited by hand.
    Damaged,
    /// Written by a newer lazarobox: it is not rewritten.
    Newer,
}

/// `{id: content hash}` of the notes this machine wrote. Keys it does not know
/// (a later slice's `trust`, say) are kept when the file is rewritten.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Ledger {
    pub own: BTreeMap<String, String>,
    extra: Map<String, Value>,
}

/// The schema version of the ledger file.
const VERSION: u64 = 1;

impl Ledger {
    /// Reads a ledger file's text. A damaged or newer file is an error: the
    /// caller must say so and must not write over it, or every note this
    /// machine made would turn foreign without a word.
    pub fn parse(text: &str) -> Result<Ledger, LedgerError> {
        let Ok(Value::Object(mut root)) = serde_json::from_str(text) else {
            return Err(LedgerError::Damaged);
        };
        match root.remove("v").and_then(|v| v.as_u64()) {
            Some(VERSION) => {}
            Some(v) if v > VERSION => return Err(LedgerError::Newer),
            _ => return Err(LedgerError::Damaged),
        }
        let Some(Value::Object(own)) = root.remove("own") else {
            return Err(LedgerError::Damaged);
        };
        let mut ledger = Ledger {
            own: BTreeMap::new(),
            extra: root,
        };
        for (id, hash) in own {
            let Value::String(hash) = hash else {
                return Err(LedgerError::Damaged);
            };
            ledger.own.insert(id, hash);
        }
        Ok(ledger)
    }

    pub fn to_json(&self) -> String {
        let mut root = self.extra.clone();
        root.insert("v".to_owned(), VERSION.into());
        root.insert("own".to_owned(), json!(self.own));
        let mut text = serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_default();
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
    fn a_note_that_claims_this_machine_or_another_notes_id_is_still_foreign() {
        let mine = note("mine");
        let mut ledger = Ledger::default();
        ledger.record(&mine);
        // Same machine field, an id the ledger never saw.
        let claims_machine = note("forged");
        assert_eq!(claims_machine.machine, mine.machine);
        assert!(!ledger.is_local(&claims_machine));
        // Mine's id and machine with other content: the hash does not match.
        let json = mine.to_json().replace("mine", "forged");
        assert!(!ledger.is_local(&LocalNote::parse(json.as_bytes()).unwrap()));
    }

    // The hash is part of the on-disk trust: a change to it would turn every
    // existing note foreign, so it is pinned.
    #[test]
    fn the_content_hash_is_pinned_and_ignores_key_order_and_whitespace() {
        let json = r#"{"v":1,"id":"n-0123456789abcdef","kind":"decision","text":"use Axum","area":"frontend","origin":"user","machine":"m-1","created":7,"updated":7}"#;
        let note = LocalNote::parse(json.as_bytes()).unwrap();
        assert_eq!(
            content_hash(&note),
            "029c2f41af13b19777ee2242abc23344113795965300b8f3c9fb3412d5cf35dc"
        );
        let shuffled = "{ \"updated\": 7,\n \"created\": 7, \"machine\": \"m-1\", \"origin\": \"user\",\n\t\"area\": \"frontend\", \"text\": \"use Axum\", \"kind\": \"decision\", \"id\": \"n-0123456789abcdef\", \"v\": 1 }";
        let other = LocalNote::parse(shuffled.as_bytes()).unwrap();
        assert_eq!(content_hash(&other), content_hash(&note));
    }

    #[test]
    fn the_ledger_round_trips_with_its_version_and_keeps_unknown_keys() {
        let mut ledger = Ledger::parse(r#"{"v":1,"own":{},"trust":{"x":1}}"#).unwrap();
        ledger.record(&note("a"));
        let text = ledger.to_json();
        assert!(
            text.contains("\"v\": 1") && text.contains("\"trust\""),
            "{text}"
        );
        assert_eq!(Ledger::parse(&text), Ok(ledger));
    }

    #[test]
    fn a_damaged_or_newer_ledger_is_an_error_never_an_empty_one() {
        for bad in [
            "",
            "{",
            "[]",
            r#"{"own":3}"#,
            r#"{"v":1}"#,
            r#"{"v":1,"own":{"n-1":3}}"#,
        ] {
            assert_eq!(Ledger::parse(bad), Err(LedgerError::Damaged), "{bad:?}");
        }
        assert_eq!(
            Ledger::parse(r#"{"v":2,"own":{}}"#),
            Err(LedgerError::Newer)
        );
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
