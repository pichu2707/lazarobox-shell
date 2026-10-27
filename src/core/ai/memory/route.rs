//! The engram calls as data: method, target and JSON body. Pure; the runtime
//! sends them.

use super::{
    port::Note,
    redact::redact,
    slug::{clean_title, topic_key, valid_topic_key},
};
use serde_json::json;

/// The methods an engram call uses.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Method {
    Get,
    Post,
    Delete,
}

/// One request to the engram server.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Call {
    pub method: Method,
    /// Path and query, percent-encoded.
    pub target: String,
    pub body: Option<Vec<u8>>,
    /// Whether the bearer token goes on this call (delete only).
    pub auth: bool,
}

/// The most content bytes sent in one note.
pub const CONTENT_CAP: usize = 50_000;

fn call(method: Method, target: String, body: Option<serde_json::Value>) -> Call {
    Call {
        method,
        target,
        body: body.map(|value| value.to_string().into_bytes()),
        auth: false,
    }
}

/// Encodes every byte but the RFC 3986 unreserved ones, so the text is one
/// query value or one path segment and cannot add a header, a parameter or a
/// segment.
pub fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// `text` redacted, then cut to `CONTENT_CAP` bytes on a character boundary.
/// The cut comes last, so it only ever removes already-masked text.
fn safe_text(text: &str) -> String {
    let redacted = redact(text).text;
    let mut end = redacted.len().min(CONTENT_CAP);
    while !redacted.is_char_boundary(end) {
        end -= 1;
    }
    redacted[..end].to_string()
}

pub fn health() -> Call {
    call(Method::Get, "/health".into(), None)
}

pub fn project_current(cwd: &str) -> Call {
    let target = format!("/project/current?cwd={}", percent_encode(cwd));
    call(Method::Get, target, None)
}

pub fn context(project: &str) -> Call {
    let target = format!(
        "/context?project={}&scope=project&compact=true&observations=8&sessions=2&prompts=0&max_bytes=2000",
        percent_encode(project)
    );
    call(Method::Get, target, None)
}

/// The session id was not `lazarobox-` and 32 lowercase hex digits; no call
/// is built for it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BadSessionId;

fn valid_session_id(id: &str) -> bool {
    id.strip_prefix("lazarobox-").is_some_and(|hex| {
        hex.len() == 32 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    })
}

pub fn register_session(id: &str, project: &str, directory: &str) -> Result<Call, BadSessionId> {
    if !valid_session_id(id) {
        return Err(BadSessionId);
    }
    let body = json!({"id": id, "project": project, "directory": directory});
    Ok(call(Method::Post, "/sessions".into(), Some(body)))
}

pub fn end_session(id: &str, summary: &str) -> Result<Call, BadSessionId> {
    if !valid_session_id(id) {
        return Err(BadSessionId);
    }
    let target = format!("/sessions/{}/end", percent_encode(id));
    Ok(call(
        Method::Post,
        target,
        Some(json!({"summary": safe_text(summary)})),
    ))
}

/// The save call. `topic_key` is a caller-supplied key: a save under the same
/// key updates the note instead of adding one. A key that is blank, too long,
/// holds control characters or changes under redaction is dropped and the key
/// derived from the title is used, so a bad key never blocks or leaks.
pub fn save(
    session_id: &str,
    project: &str,
    note: &Note,
    topic_key_override: Option<&str>,
) -> Call {
    let title = clean_title(&redact(&note.title).text);
    let key = topic_key_override
        .filter(|key| valid_topic_key(key) && redact(key).text == *key)
        .map_or_else(|| topic_key(note.kind, &title), str::to_owned);
    let body = json!({
        "session_id": session_id,
        "type": note.kind.as_str(),
        "title": title,
        "content": safe_text(&note.content),
        "project": project,
        "scope": "project",
        "topic_key": key,
    });
    call(Method::Post, "/observations".into(), Some(body))
}

pub fn delete(id: i64, project: &str) -> Call {
    let target = format!(
        "/observations/{id}?expected_project={}",
        percent_encode(project)
    );
    Call {
        auth: true,
        ..call(Method::Delete, target, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ai::memory::kind::MemKind;
    use serde_json::{Value, json};

    fn json_body(call: &Call) -> Value {
        serde_json::from_slice(call.body.as_ref().expect("body")).expect("json")
    }

    const SID: &str = "lazarobox-0123456789abcdef0123456789abcdef";

    fn note(title: &str, content: &str) -> Note {
        Note {
            kind: MemKind::Decision,
            title: title.into(),
            content: content.into(),
        }
    }

    #[test]
    fn percent_encoding_keeps_only_unreserved_bytes() {
        assert_eq!(percent_encode("aZ09-._~"), "aZ09-._~");
        assert_eq!(percent_encode("a b&c=d/é"), "a%20b%26c%3Dd%2F%C3%A9");
    }

    #[test]
    fn health_is_a_bare_get() {
        let call = health();
        assert_eq!(
            (call.method, call.target.as_str(), call.body, call.auth),
            (Method::Get, "/health", None, false)
        );
    }

    #[test]
    fn cwd_is_one_encoded_query_value() {
        let call = project_current("/tmp/a b&c\r\nX: y");
        assert_eq!(call.method, Method::Get);
        assert_eq!(
            call.target,
            "/project/current?cwd=%2Ftmp%2Fa%20b%26c%0D%0AX%3A%20y"
        );
        assert!(call.target.bytes().all(|b| b.is_ascii_graphic()));
        assert_eq!(call.target.matches('?').count(), 1);
        assert_eq!(call.target.matches('&').count(), 0);
    }

    #[test]
    fn context_query_is_fixed_and_the_project_encoded() {
        let call = context("my proj&x");
        assert_eq!(call.method, Method::Get);
        assert_eq!(
            call.target,
            "/context?project=my%20proj%26x&scope=project&compact=true&observations=8&sessions=2&prompts=0&max_bytes=2000"
        );
        assert!(!call.auth && call.body.is_none());
    }

    #[test]
    fn session_body_carries_id_project_and_directory() {
        let call = register_session(SID, "widget", "/tmp/x y").expect("call");
        assert_eq!(
            (call.method, call.target.as_str()),
            (Method::Post, "/sessions")
        );
        assert_eq!(
            json_body(&call),
            json!({"id": SID, "project": "widget", "directory": "/tmp/x y"})
        );
        assert!(!call.auth);
    }

    #[test]
    fn end_posts_the_redacted_capped_summary_to_the_session() {
        let call = end_session(SID, "Notes: A; password=hunter2").expect("call");
        assert_eq!(
            (call.method, call.target.as_str()),
            (Method::Post, format!("/sessions/{SID}/end").as_str())
        );
        let body = json_body(&call);
        let summary = body["summary"].as_str().expect("summary");
        assert!(summary.starts_with("Notes: A; "));
        assert!(!summary.contains("hunter2"));
        assert_eq!(body.as_object().expect("object").len(), 1);

        let long = end_session(SID, &"é".repeat(30_000)).expect("call");
        let kept = json_body(&long)["summary"].as_str().expect("s").len();
        assert_eq!(kept, 50_000);
    }

    #[test]
    fn a_session_id_of_another_shape_builds_no_call() {
        let bad = [
            "",
            "lazarobox-ab",
            "a/../b?x=1 y",
            "lazarobox-0123456789ABCDEF0123456789abcdef",
            "lazarobox-0123456789abcdef0123456789abcde",
            "lazarobox-0123456789abcdef0123456789abcdef0",
            "lazarobox-0123456789abcdef0123456789abcde\n",
            "xlazarobox-0123456789abcdef0123456789abcdef",
        ];
        for id in bad {
            assert_eq!(register_session(id, "p", "/x"), Err(BadSessionId), "{id:?}");
            assert_eq!(end_session(id, "s"), Err(BadSessionId), "{id:?}");
        }
        assert!(register_session(SID, "p", "/x").is_ok());
    }

    #[test]
    fn the_end_summary_never_reveals_a_secret_cut_by_the_cap() {
        let secret = "sk-ant-api03-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789";
        for pad in [49_980, 49_990, 49_999, 50_000] {
            let summary = format!("{} {secret} tail", "a".repeat(pad));
            let call = end_session(SID, &summary).expect("call");
            let body = json_body(&call);
            let kept = body["summary"].as_str().expect("summary");
            assert!(kept.len() <= CONTENT_CAP);
            assert!(!kept.contains("sk-ant"), "pad {pad}");
            assert!(!kept.contains("AbCdEf"), "pad {pad}");
        }
    }

    #[test]
    fn save_body_has_exactly_the_engram_fields() {
        let call = save(
            "lazarobox-ab",
            "widget",
            &note("Use Axum", "We chose Axum."),
            None,
        );
        assert_eq!(
            (call.method, call.target.as_str(), call.auth),
            (Method::Post, "/observations", false)
        );
        assert_eq!(
            json_body(&call),
            json!({
                "session_id": "lazarobox-ab",
                "type": "decision",
                "title": "Use Axum",
                "content": "We chose Axum.",
                "project": "widget",
                "scope": "project",
                "topic_key": "chat/decision/use-axum",
            })
        );
    }

    #[test]
    fn save_redacts_before_building_the_body() {
        let call = save("s", "p", &note("t", "keep this, password=hunter2 ok"), None);
        let raw = String::from_utf8(call.body.expect("body")).expect("utf8");
        assert!(!raw.contains("hunter2"));
        assert!(raw.contains("[REDACTED]") && raw.contains("keep this"));
    }

    #[test]
    fn save_cleans_the_title_and_derives_the_key_from_it() {
        let call = save("s", "p", &note("  Fix\u{1b}[31m  the Parser\n", "c"), None);
        let body = json_body(&call);
        let title = body["title"].as_str().expect("title");
        assert!(!title.chars().any(char::is_control));
        assert_eq!(body["topic_key"], "chat/decision/fix-31m-the-parser");
    }

    #[test]
    fn save_never_sends_a_secret_from_the_title() {
        let key = "sk-ant-api03-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789";
        for title in [
            "Config password=hunter2 done".to_string(),
            format!("Key {key} leaked"),
        ] {
            let call = save(SID, "p", &note(&title, "c"), None);
            let raw = String::from_utf8(call.body.expect("body")).expect("utf8");
            assert!(!raw.contains("hunter2"), "{title}");
            assert!(
                !raw.contains("sk-ant") && !raw.contains("AbCdEf"),
                "{title}"
            );
        }
    }

    #[test]
    fn save_cuts_content_at_50000_bytes_on_a_char_boundary() {
        let three = save("s", "p", &note("t", &"日".repeat(17_000)), None);
        let content = json_body(&three)["content"]
            .as_str()
            .expect("c")
            .to_string();
        assert_eq!(content.chars().count(), 16_666);
        assert_eq!(content.len(), 49_998);

        let small = save("s", "p", &note("t", &"日".repeat(16_000)), None);
        let kept = json_body(&small)["content"].as_str().expect("c").len();
        assert_eq!(kept, 48_000);

        let ascii = save("s", "p", &note("t", &"a".repeat(60_000)), None);
        let kept = json_body(&ascii)["content"].as_str().expect("c").len();
        assert_eq!(kept, CONTENT_CAP);
    }

    #[test]
    fn delete_names_the_expected_project_and_wants_the_token() {
        let call = delete(7, "my proj");
        assert_eq!(call.method, Method::Delete);
        assert_eq!(call.target, "/observations/7?expected_project=my%20proj");
        assert!(call.auth && call.body.is_none());
    }

    #[test]
    fn save_upserts_under_the_key_the_caller_gives() {
        let call = save(
            SID,
            "p",
            &note("Chat summary", "c"),
            Some("chat/12/summary"),
        );
        let body = json_body(&call);
        assert_eq!(body["topic_key"], "chat/12/summary");
        // The title is still the note's own.
        assert_eq!(body["title"], "Chat summary");
    }

    #[test]
    fn save_without_a_caller_key_derives_it_as_before() {
        let call = save(SID, "p", &note("Use Axum", "c"), None);
        assert_eq!(json_body(&call)["topic_key"], "chat/decision/use-axum");
    }

    #[test]
    fn an_invalid_caller_key_is_dropped_for_the_derived_one() {
        let secret = "chat/sk-ant-api03-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789/summary";
        let long = "k".repeat(201);
        let bad = ["", "   ", "a\nb", "k\u{1b}[0m", "a\u{200b}b", &long, secret];
        for key in bad {
            let call = save(SID, "p", &note("Use Axum", "c"), Some(key));
            let raw = String::from_utf8(call.body.clone().expect("body")).expect("utf8");
            assert_eq!(
                json_body(&call)["topic_key"],
                "chat/decision/use-axum",
                "{key:?}"
            );
            assert!(!raw.contains("sk-ant"), "{key:?}");
        }
    }

    #[test]
    fn a_caller_key_at_the_cap_is_kept() {
        let key = "k".repeat(200);
        let call = save(SID, "p", &note("t", "c"), Some(&key));
        assert_eq!(json_body(&call)["topic_key"], key.as_str());
    }

    #[test]
    fn only_delete_asks_for_auth() {
        let calls = [
            health(),
            project_current("/x"),
            context("p"),
            register_session(SID, "p", "/x").expect("call"),
            end_session(SID, "t").expect("call"),
            save("s", "p", &note("t", "c"), None),
        ];
        assert!(calls.iter().all(|c| !c.auth));
    }
}
