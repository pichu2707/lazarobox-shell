//! The engram calls as data: method, target and JSON body. Pure; the runtime
//! sends them.

use super::{
    port::Note,
    redact::redact,
    slug::{clean_title, topic_key},
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

pub fn register_session(id: &str, project: &str, directory: &str) -> Call {
    let body = json!({"id": id, "project": project, "directory": directory});
    call(Method::Post, "/sessions".into(), Some(body))
}

pub fn end_session(id: &str, summary: &str) -> Call {
    let target = format!("/sessions/{}/end", percent_encode(id));
    call(
        Method::Post,
        target,
        Some(json!({"summary": safe_text(summary)})),
    )
}

pub fn save(session_id: &str, project: &str, note: &Note) -> Call {
    let title = clean_title(&note.title);
    let body = json!({
        "session_id": session_id,
        "type": note.kind.as_str(),
        "title": title,
        "content": safe_text(&note.content),
        "project": project,
        "scope": "project",
        "topic_key": topic_key(note.kind, &title),
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
        let call = register_session("lazarobox-ab", "widget", "/tmp/x y");
        assert_eq!(
            (call.method, call.target.as_str()),
            (Method::Post, "/sessions")
        );
        assert_eq!(
            json_body(&call),
            json!({"id": "lazarobox-ab", "project": "widget", "directory": "/tmp/x y"})
        );
        assert!(!call.auth);
    }

    #[test]
    fn end_posts_the_redacted_capped_summary_to_the_session() {
        let call = end_session("lazarobox-ab", "Notes: A; password=hunter2");
        assert_eq!(
            (call.method, call.target.as_str()),
            (Method::Post, "/sessions/lazarobox-ab/end")
        );
        let body = json_body(&call);
        let summary = body["summary"].as_str().expect("summary");
        assert!(summary.starts_with("Notes: A; "));
        assert!(!summary.contains("hunter2"));
        assert_eq!(body.as_object().expect("object").len(), 1);

        let long = end_session("s", &"é".repeat(30_000));
        let kept = json_body(&long)["summary"].as_str().expect("s").len();
        assert_eq!(kept, 50_000);
    }

    #[test]
    fn a_session_id_cannot_add_a_path_segment_or_query() {
        let call = end_session("a/../b?x=1 y", "s");
        assert_eq!(call.target, "/sessions/a%2F..%2Fb%3Fx%3D1%20y/end");
    }

    #[test]
    fn save_body_has_exactly_the_engram_fields() {
        let call = save(
            "lazarobox-ab",
            "widget",
            &note("Use Axum", "We chose Axum."),
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
        let call = save("s", "p", &note("t", "keep this, password=hunter2 ok"));
        let raw = String::from_utf8(call.body.expect("body")).expect("utf8");
        assert!(!raw.contains("hunter2"));
        assert!(raw.contains("[REDACTED]") && raw.contains("keep this"));
    }

    #[test]
    fn save_cleans_the_title_and_derives_the_key_from_it() {
        let call = save("s", "p", &note("  Fix\u{1b}[31m  the Parser\n", "c"));
        let body = json_body(&call);
        let title = body["title"].as_str().expect("title");
        assert!(!title.chars().any(char::is_control));
        assert_eq!(body["topic_key"], "chat/decision/fix-31m-the-parser");
    }

    #[test]
    fn save_cuts_content_at_50000_bytes_on_a_char_boundary() {
        let three = save("s", "p", &note("t", &"日".repeat(17_000)));
        let content = json_body(&three)["content"]
            .as_str()
            .expect("c")
            .to_string();
        assert_eq!(content.chars().count(), 16_666);
        assert_eq!(content.len(), 49_998);

        let small = save("s", "p", &note("t", &"日".repeat(16_000)));
        let kept = json_body(&small)["content"].as_str().expect("c").len();
        assert_eq!(kept, 48_000);

        let ascii = save("s", "p", &note("t", &"a".repeat(60_000)));
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
    fn only_delete_asks_for_auth() {
        let calls = [
            health(),
            project_current("/x"),
            context("p"),
            register_session("s", "p", "/x"),
            end_session("s", "t"),
            save("s", "p", &note("t", "c")),
        ];
        assert!(calls.iter().all(|c| !c.auth));
    }
}
