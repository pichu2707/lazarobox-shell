//! One local note: the schema of a note file and how it is read and written.

use serde_json::{Map, Value};

use crate::core::ai::memory::kind::MemKind;

/// The most characters a note text keeps; the rest is cut.
pub const TEXT_CAP: usize = 1000;
/// The most characters a note read from disk may hold: the largest text any
/// origin writes (a summary).
pub const READ_TEXT_CAP: usize = 1500;
/// The most characters of an area or a topic, of one ref, and of one flag; and
/// how many refs and flags a note may carry, when read from disk.
const AREA_CAP: usize = 200;
const REF_CAP: usize = 300;
const MAX_REFS: usize = 20;
const FLAG_CAP: usize = 32;
const MAX_FLAGS: usize = 8;
/// The visible line for a cut text.
pub const CUT_LINE: &str = "note cut to 1000 characters";
/// The schema version this build reads and writes.
pub const VERSION: u64 = 1;

/// Who wrote the note.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Origin {
    User,
    Model,
    Summary,
}

/// A note as it is on disk. Fields a newer build adds stay in `extra` and are
/// written back unchanged.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LocalNote {
    pub id: String,
    pub area: String,
    pub kind: MemKind,
    pub origin: Origin,
    pub text: String,
    pub refs: Vec<String>,
    pub topic: Option<String>,
    pub created: i64,
    pub updated: i64,
    pub machine: String,
    pub pinned: bool,
    pub flags: Vec<String>,
    extra: Map<String, Value>,
}

/// Why a file is not a usable note.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unreadable {
    Invalid,
    /// Made by a newer lazarobox: skipped, and never rewritten.
    Newer,
}

/// A note to write, checked: a kind from the closed list and a capped text.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NewNote {
    pub kind: MemKind,
    pub area: String,
    pub text: String,
    pub origin: Origin,
    pub topic: Option<String>,
    /// The text was longer than [`TEXT_CAP`] and was cut.
    pub cut: bool,
}

impl NewNote {
    /// A note to write, or the visible line that says why it is refused.
    pub fn new(kind: &str, area: &str, text: &str, origin: Origin) -> Result<NewNote, String> {
        let Some(kind) = MemKind::parse(kind) else {
            let shown: String = kind.chars().filter(|c| !c.is_control()).take(24).collect();
            return Err(format!("note not saved: unknown kind {shown}"));
        };
        let text = strip_controls(text, true);
        let cut = text.chars().count() > TEXT_CAP;
        Ok(NewNote {
            kind,
            area: strip_controls(area, false).chars().take(AREA_CAP).collect(),
            text: text.chars().take(TEXT_CAP).collect(),
            origin,
            topic: None,
            cut,
        })
    }
}

impl Origin {
    fn as_str(self) -> &'static str {
        match self {
            Origin::User => "user",
            Origin::Model => "model",
            Origin::Summary => "summary",
        }
    }

    fn parse(text: &str) -> Option<Origin> {
        [Origin::User, Origin::Model, Origin::Summary]
            .into_iter()
            .find(|o| o.as_str() == text)
    }
}

/// `text` without control characters (ESC, NUL, DEL...) and without the
/// characters that reorder or hide text (bidi controls, zero-width marks, line
/// and paragraph separators). Line breaks and tabs stay when `multiline`.
fn strip_controls(text: &str, multiline: bool) -> String {
    let hidden = |c: char| matches!(c, '\u{200B}'..='\u{200F}' | '\u{2028}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}');
    text.chars()
        .filter(|c| !hidden(*c) && (!c.is_control() || (multiline && matches!(c, '\n' | '\t'))))
        .collect()
}

/// Whether `text` fits in `cap` characters.
fn within(text: &str, cap: usize) -> bool {
    text.chars().count() <= cap
}

/// A note id is a file name later: plain characters only.
fn plain_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl LocalNote {
    pub fn from_new(new: NewNote, id: String, machine: String, now: i64) -> LocalNote {
        LocalNote {
            id,
            area: new.area,
            kind: new.kind,
            origin: new.origin,
            text: new.text,
            refs: Vec::new(),
            topic: new.topic,
            created: now,
            updated: now,
            machine,
            pinned: false,
            flags: Vec::new(),
            extra: Map::new(),
        }
    }

    /// Reads a note file. Missing optional fields take their defaults and
    /// unknown ones are kept; a bad or newer file is an error, never a panic.
    pub fn parse(bytes: &[u8]) -> Result<LocalNote, Unreadable> {
        let Ok(Value::Object(mut o)) = serde_json::from_slice(bytes) else {
            return Err(Unreadable::Invalid);
        };
        match o.remove("v").and_then(|v| v.as_u64()) {
            Some(VERSION) => {}
            Some(v) if v > VERSION => return Err(Unreadable::Newer),
            _ => return Err(Unreadable::Invalid),
        }
        let mut take = |key: &str| o.remove(key);
        let string = |v: Option<Value>| v.and_then(|v| v.as_str().map(str::to_owned));
        let strings = |v: Option<Value>| match v {
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(|i| i.as_str().map(str::to_owned))
                .collect(),
            _ => Vec::new(),
        };
        let number = |v: Option<Value>| v.and_then(|v| v.as_i64()).unwrap_or(0);
        let id = string(take("id")).filter(|id| plain_id(id));
        let kind = string(take("kind")).and_then(|k| MemKind::parse(&k));
        let origin = match string(take("origin")) {
            // The least privileged origin: a hand-written file never ranks as
            // the user's.
            None => Some(Origin::Model),
            Some(name) => Origin::parse(&name),
        };
        let (Some(id), Some(kind), Some(origin), Some(text)) =
            (id, kind, origin, string(take("text")))
        else {
            return Err(Unreadable::Invalid);
        };
        let area =
            string(take("area")).map_or_else(|| ".".to_owned(), |a| strip_controls(&a, false));
        let clean = |items: Vec<String>| -> Vec<String> {
            items.iter().map(|i| strip_controls(i, false)).collect()
        };
        let (refs, topic, flags) = (
            clean(strings(take("refs"))),
            string(take("topic")).map(|t| strip_controls(&t, false)),
            clean(strings(take("flags"))),
        );
        let fits = within(&text, READ_TEXT_CAP)
            && within(&area, AREA_CAP)
            && topic.as_deref().is_none_or(|t| within(t, AREA_CAP))
            && refs.len() <= MAX_REFS
            && refs.iter().all(|r| within(r, REF_CAP))
            && flags.len() <= MAX_FLAGS
            && flags.iter().all(|f| within(f, FLAG_CAP));
        if !fits {
            return Err(Unreadable::Invalid);
        }
        Ok(LocalNote {
            id,
            kind,
            origin,
            text: strip_controls(&text, true),
            area,
            refs,
            topic,
            created: number(take("created")),
            updated: number(take("updated")),
            machine: strip_controls(&string(take("machine")).unwrap_or_default(), false),
            pinned: take("pinned").and_then(|v| v.as_bool()).unwrap_or(false),
            flags,
            extra: std::mem::take(&mut o),
        })
    }

    /// The pretty-printed file text. Keys come out sorted, so a pin changes
    /// one line.
    pub fn to_json(&self) -> String {
        let mut o = self.extra.clone();
        let mut put = |key: &str, value: Value| o.insert(key.to_owned(), value);
        put("v", VERSION.into());
        put("id", self.id.clone().into());
        put("area", self.area.clone().into());
        put("kind", self.kind.as_str().into());
        put("origin", self.origin.as_str().into());
        put("text", self.text.clone().into());
        put("refs", self.refs.clone().into());
        put("created", self.created.into());
        put("updated", self.updated.into());
        put("machine", self.machine.clone().into());
        put("pinned", self.pinned.into());
        put("flags", self.flags.clone().into());
        if let Some(topic) = &self.topic {
            put("topic", topic.clone().into());
        }
        let mut text = serde_json::to_string_pretty(&Value::Object(o)).unwrap_or_default();
        text.push('\n');
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note() -> LocalNote {
        let new = NewNote::new("decision", "frontend", "use Axum", Origin::User).unwrap();
        LocalNote::from_new(new, "n-0123456789abcdef".into(), "m-1".into(), 7)
    }

    #[test]
    fn a_note_survives_a_json_round_trip() {
        let n = note();
        assert_eq!(LocalNote::parse(n.to_json().as_bytes()), Ok(n));
    }

    #[test]
    fn a_kind_outside_the_list_is_refused_with_its_line() {
        let err = NewNote::new("learning", ".", "x", Origin::Model).unwrap_err();
        assert_eq!(err, "note not saved: unknown kind learning");
    }

    #[test]
    fn a_long_text_is_cut_at_1000_characters_and_says_so() {
        let text = "é".repeat(1500);
        let new = NewNote::new("bugfix", ".", &text, Origin::User).unwrap();
        assert_eq!(new.text.chars().count(), TEXT_CAP);
        assert!(new.cut);
        assert_eq!(CUT_LINE, "note cut to 1000 characters");
        let short = NewNote::new("bugfix", ".", "ok", Origin::User).unwrap();
        assert!(!short.cut);
    }

    #[test]
    fn unknown_fields_are_ignored_and_kept_on_rewrite() {
        let json = r#"{"v":1,"id":"n-a","kind":"config","text":"t","future":{"x":1}}"#;
        let n = LocalNote::parse(json.as_bytes()).unwrap();
        assert!(n.to_json().contains("\"future\""));
    }

    #[test]
    fn a_missing_area_reads_as_the_repo_root() {
        let json = r#"{"v":1,"id":"n-a","kind":"config","text":"t"}"#;
        assert_eq!(LocalNote::parse(json.as_bytes()).unwrap().area, ".");
    }

    #[test]
    fn a_missing_origin_is_the_least_privileged_one() {
        let json = r#"{"v":1,"id":"n-a","kind":"config","text":"t"}"#;
        assert_eq!(
            LocalNote::parse(json.as_bytes()).unwrap().origin,
            Origin::Model
        );
    }

    #[test]
    fn a_file_over_the_caps_is_unreadable() {
        let with = |field: &str, value: String| {
            let mut o = serde_json::json!({"v":1,"id":"n-a","kind":"config","text":"t"});
            o[field] = serde_json::from_str(&value).unwrap();
            o.to_string()
        };
        let long = |n: usize| serde_json::to_string(&"x".repeat(n)).unwrap();
        for (field, value) in [
            ("text", long(READ_TEXT_CAP + 1)),
            ("area", long(201)),
            ("topic", long(201)),
            ("refs", serde_json::to_string(&vec!["a"; 21]).unwrap()),
            (
                "refs",
                serde_json::to_string(&vec!["x".repeat(301)]).unwrap(),
            ),
            ("flags", serde_json::to_string(&vec!["f"; 9]).unwrap()),
            (
                "flags",
                serde_json::to_string(&vec!["f".repeat(33)]).unwrap(),
            ),
        ] {
            let json = with(field, value);
            assert_eq!(
                LocalNote::parse(json.as_bytes()),
                Err(Unreadable::Invalid),
                "{field}"
            );
        }
        let at_the_cap = with("text", long(READ_TEXT_CAP));
        assert!(LocalNote::parse(at_the_cap.as_bytes()).is_ok());
    }

    // Spec: a note the chat writes must be one it can read back.
    #[test]
    fn a_deep_area_is_capped_on_write_so_the_note_reads_back() {
        let deep = vec!["folder"; 60].join("/");
        let new = NewNote::new("config", &deep, "t", Origin::User).unwrap();
        assert!(new.area.chars().count() <= 200);
        let note = LocalNote::from_new(new, "n-a".into(), "m-1".into(), 1);
        assert!(LocalNote::parse(note.to_json().as_bytes()).is_ok());
    }

    #[test]
    fn bidi_and_zero_width_characters_are_stripped_from_every_text_field() {
        let bad = "\u{202A}\u{202E}\u{2066}\u{2069}\u{200B}\u{200F}\u{2028}\u{2029}\u{FEFF}\u{1b}";
        let json = serde_json::json!({
            "v": 1, "id": "n-a", "kind": "config",
            "text": format!("a{bad}b"), "area": format!("c{bad}d"),
            "topic": format!("e{bad}f"), "refs": [format!("g{bad}h")],
            "flags": [format!("i{bad}j")], "machine": format!("m{bad}n"),
        });
        let n = LocalNote::parse(json.to_string().as_bytes()).unwrap();
        assert_eq!(n.text, "ab");
        assert_eq!(n.area, "cd");
        assert_eq!(n.topic.as_deref(), Some("ef"));
        assert_eq!(n.refs, ["gh"]);
        assert_eq!(n.flags, ["ij"]);
        assert_eq!(n.machine, "mn");
        let new = NewNote::new(
            "config",
            &format!("a{bad}b"),
            &format!("c{bad}d"),
            Origin::User,
        )
        .unwrap();
        assert_eq!((new.area.as_str(), new.text.as_str()), ("ab", "cd"));
    }

    #[test]
    fn control_characters_are_stripped_from_area_and_text() {
        let json = "{\"v\":1,\"id\":\"n-a\",\"kind\":\"config\",\"area\":\"a\\u001b[31mb\\u0007\",\"text\":\"x\\u001b[2Jy\\nz\\tw\\u0000\"}";
        let n = LocalNote::parse(json.as_bytes()).unwrap();
        assert_eq!(n.area, "a[31mb");
        assert_eq!(n.text, "x[2Jy\nz\tw");
        let new = NewNote::new("config", "a\u{1b}b", "t\u{1b}[0m\u{7f}", Origin::User).unwrap();
        assert_eq!((new.area.as_str(), new.text.as_str()), ("ab", "t[0m"));
    }

    #[test]
    fn an_unknown_kind_or_a_broken_file_is_unreadable() {
        for bad in [
            r#"{"v":1,"id":"n-a","kind":"learning","text":"t"}"#,
            r#"{"v":1,"id":"n-a","kind":"config"}"#,
            r#"{"v":1,"id":"../x","kind":"config","text":"t"}"#,
            "<<<<<<< HEAD\n{}\n=======\n",
            "",
        ] {
            assert_eq!(
                LocalNote::parse(bad.as_bytes()),
                Err(Unreadable::Invalid),
                "{bad:?}"
            );
        }
        let newer = r#"{"v":2,"id":"n-a","kind":"config","text":"t"}"#;
        assert_eq!(LocalNote::parse(newer.as_bytes()), Err(Unreadable::Newer));
    }
}
