//! Parsers for the engram replies. Defensive: any shape that is not the
//! expected one becomes a typed error, never a panic, and no error text echoes
//! user content.

use super::context::{ContextBlock, context_block};
use crate::core::ai::sanitize::clean;

/// The longest failure reason, in characters.
pub const REASON_CAP: usize = 120;

/// What the user sees when engram refuses a delete.
pub const DELETE_REFUSED: &str = "engram refused the delete (check ENGRAM_HTTP_TOKEN)";

/// Why project resolution gave no project.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProjectError {
    /// Not a git project (a directory name, ambiguous, empty or unknown).
    NotGit,
    /// The reply was not the expected shape.
    Invalid,
}

/// A short, sanitized reason a call failed (at most `REASON_CAP` characters,
/// no control character).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Failure {
    pub reason: String,
}

/// The observation a save created or updated.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Saved {
    pub id: i64,
    /// `revision_count` when the server reports it.
    pub revision: Option<u32>,
}

/// What a delete came to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeleteOutcome {
    /// 2xx, or 404 `observation not found` (already gone).
    Deleted,
    /// 401 or 403.
    Refused,
    /// Anything else.
    Failed,
}

/// The longest project name accepted, in bytes.
const PROJECT_CAP: usize = 256;

fn object(body: &[u8]) -> Option<serde_json::Map<String, serde_json::Value>> {
    match serde_json::from_slice(body) {
        Ok(serde_json::Value::Object(map)) => Some(map),
        _ => None,
    }
}

fn code(body: &[u8]) -> Option<String> {
    object(body)?.get("code")?.as_str().map(str::to_string)
}

fn fail(reason: String) -> Failure {
    Failure { reason }
}

/// A short reason for a failed call: fixed text for a `code` in a small
/// allowlist, else `engram error (HTTP <status>)`. The server `error` text and
/// unknown codes are never echoed, and nothing of the request is.
fn failure(status: u16, body: &[u8]) -> Failure {
    let known = match code(body).as_deref() {
        Some("session_project_mismatch") => Some("session project mismatch"),
        Some("session_already_ended") => Some("session already ended"),
        Some("unknown_project") => Some("unknown project"),
        _ => None,
    };
    fail(known.map_or_else(|| format!("engram error (HTTP {status})"), str::to_string))
}

/// Resolves the project of `GET /project/current`. The server never answers
/// 404 or 409 there, so the status is ignored and `project_source` decides: a
/// `git_*` source with a non-empty project resolves, any other source is
/// `NotGit` (so is a JSON object without `project_source` or `project`, or with
/// a `null` project), a reply that is not a JSON object is `Invalid`.
pub fn parse_project(_status: u16, body: &[u8]) -> Result<String, ProjectError> {
    let map = object(body).ok_or(ProjectError::Invalid)?;
    let field = |name: &str| match map.get(name) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(text)) => Ok(Some(text.as_str())),
        Some(_) => Err(ProjectError::Invalid),
    };
    let (Some(source), Some(project)) = (field("project_source")?, field("project")?) else {
        return Err(ProjectError::NotGit);
    };
    if !source.starts_with("git_") {
        return Err(ProjectError::NotGit);
    }
    if project.trim().is_empty() {
        return Err(ProjectError::NotGit);
    }
    if project.len() > PROJECT_CAP
        || project.chars().any(char::is_control)
        || project.trim() != project
        || clean(project) != project
    {
        return Err(ProjectError::Invalid);
    }
    Ok(project.to_string())
}

/// The context of `GET /context`: a block, or `None` when there is no memory
/// yet (an empty context or 404 `unknown_project`).
pub fn parse_context(
    status: u16,
    body: &[u8],
    project: &str,
) -> Result<Option<ContextBlock>, Failure> {
    if status == 404 && code(body).as_deref() == Some("unknown_project") {
        return Ok(None);
    }
    if status != 200 {
        return Err(failure(status, body));
    }
    let raw = object(body)
        .and_then(|m| {
            m.get("context")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .ok_or_else(|| fail("invalid reply from engram".into()))?;
    Ok(context_block(&raw, project))
}

/// `POST /sessions`: registering is idempotent, so a session that already
/// ended (409 `session_already_ended`) counts as registered.
pub fn parse_session(status: u16, body: &[u8]) -> Result<(), Failure> {
    if (200..300).contains(&status)
        || (status == 409 && code(body).as_deref() == Some("session_already_ended"))
    {
        Ok(())
    } else {
        Err(failure(status, body))
    }
}

/// `POST /sessions/{id}/end`.
pub fn parse_end(status: u16, body: &[u8]) -> Result<(), Failure> {
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(failure(status, body))
    }
}

/// `POST /observations`: the id of the observation, and its revision when the
/// server says so.
pub fn parse_saved(status: u16, body: &[u8]) -> Result<Saved, Failure> {
    if !(200..300).contains(&status) {
        return Err(failure(status, body));
    }
    let map = object(body).ok_or_else(|| fail("invalid reply from engram".into()))?;
    let id = map
        .get("id")
        .and_then(|v| v.as_i64())
        .filter(|id| *id > 0)
        .ok_or_else(|| fail("invalid reply from engram".into()))?;
    let revision = map
        .get("revision_count")
        .and_then(|v| v.as_u64())
        .and_then(|n| u32::try_from(n).ok());
    Ok(Saved { id, revision })
}

/// `DELETE /observations/{id}`: a 2xx, or a 404 whose `error` is exactly
/// `observation not found`, is deleted; 401 and 403 are a refusal; anything
/// else (any other 404 included) a failure.
pub fn parse_delete(status: u16, body: &[u8]) -> DeleteOutcome {
    let gone = || {
        object(body).is_some_and(|m| {
            m.get("error").and_then(|v| v.as_str()) == Some("observation not found")
        })
    };
    match status {
        200..=299 => DeleteOutcome::Deleted,
        404 if gone() => DeleteOutcome::Deleted,
        401 | 403 => DeleteOutcome::Refused,
        _ => DeleteOutcome::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ai::memory::testing::split;

    macro_rules! fx {
        ($name:literal) => {
            split(include_bytes!(concat!("fixtures/3.4.0/", $name, ".http")))
        };
    }

    fn project(fixture: (u16, &[u8])) -> Result<String, ProjectError> {
        parse_project(fixture.0, fixture.1)
    }

    fn reason_of(result: Result<impl std::fmt::Debug, Failure>) -> String {
        result.expect_err("a failure").reason
    }

    #[test]
    fn a_git_source_with_a_project_resolves() {
        assert_eq!(project(fx!("project_200")), Ok("widget".into()));
    }

    #[test]
    fn every_other_source_is_not_git() {
        assert_eq!(
            project(fx!("project_dir_basename_200")),
            Err(ProjectError::NotGit)
        );
        assert_eq!(
            project(fx!("project_nonexistent_dir_200")),
            Err(ProjectError::NotGit)
        );
        assert_eq!(
            project(fx!("project_ambiguous_200")),
            Err(ProjectError::NotGit)
        );
    }

    #[test]
    fn project_is_classified_by_source_not_by_status_or_name() {
        let git = |source: &str, project: &str| {
            let body = format!(r#"{{"project":"{project}","project_source":"{source}"}}"#);
            parse_project(200, body.as_bytes())
        };
        assert_eq!(git("git_root", "app"), Ok("app".into()));
        assert_eq!(git("git_worktree", "app"), Ok("app".into()));
        assert_eq!(git("git_remote", ""), Err(ProjectError::NotGit));
        assert_eq!(git("git_remote", "  "), Err(ProjectError::NotGit));
        assert_eq!(git("svn", "app"), Err(ProjectError::NotGit));
        assert_eq!(git("", "app"), Err(ProjectError::NotGit));
        assert_eq!(git("GIT_REMOTE", "app"), Err(ProjectError::NotGit));
    }

    #[test]
    fn a_malformed_project_reply_is_invalid() {
        for body in [
            &b"<html>"[..],
            b"[]",
            b"7",
            br#"{"project":7,"project_source":"git_remote"}"#,
            br#"{"project":"a","project_source":7}"#,
            br#"{"project":"a\u001b[31m","project_source":"git_remote"}"#,
            br#"{"project":"a\nb","project_source":"git_remote"}"#,
            br#"{"project":"a\tb","project_source":"git_remote"}"#,
            br#"{"project":"a\u007fb","project_source":"git_remote"}"#,
            br#"{"project":" app","project_source":"git_remote"}"#,
            br#"{"project":"app ","project_source":"git_remote"}"#,
            br#"{"project":"app\n","project_source":"git_remote"}"#,
        ] {
            assert_eq!(
                parse_project(200, body),
                Err(ProjectError::Invalid),
                "{}",
                String::from_utf8_lossy(body)
            );
        }
        assert_eq!(parse_project(500, b""), Err(ProjectError::Invalid));
    }

    #[test]
    fn a_reply_without_the_project_fields_is_not_git() {
        for body in [
            &b"{}"[..],
            br#"{"project":"a"}"#,
            br#"{"project_source":"git_remote"}"#,
            br#"{"project":null,"project_source":"git_remote"}"#,
            br#"{"project":null,"project_source":"dir_basename"}"#,
        ] {
            assert_eq!(
                parse_project(200, body),
                Err(ProjectError::NotGit),
                "{}",
                String::from_utf8_lossy(body)
            );
        }
    }

    #[test]
    fn context_with_notes_becomes_a_block() {
        let (status, body) = fx!("context_full");
        let block = parse_context(status, body, "widget")
            .expect("ok")
            .expect("block");
        assert!(block.notes > 0);
        assert!(block.text.contains("widget"));
    }

    #[test]
    fn context_with_only_sessions_is_a_block_without_notes() {
        let (status, body) = fx!("context_sessions_only");
        let block = parse_context(status, body, "p")
            .expect("ok")
            .expect("block");
        assert_eq!(block.notes, 0);
    }

    #[test]
    fn an_unknown_project_is_no_memory_yet() {
        let (status, body) = fx!("context_empty");
        assert_eq!(parse_context(status, body, "empty-proj"), Ok(None));
        assert_eq!(parse_context(200, br#"{"context":""}"#, "p"), Ok(None));
    }

    #[test]
    fn other_context_replies_are_failures() {
        assert!(!reason_of(parse_context(404, br#"{"error":"nope"}"#, "p")).is_empty());
        assert!(!reason_of(parse_context(500, b"", "p")).is_empty());
        assert!(!reason_of(parse_context(200, b"<html>", "p")).is_empty());
        assert!(!reason_of(parse_context(200, br#"{"context":5}"#, "p")).is_empty());
        assert!(!reason_of(parse_context(200, br#"{}"#, "p")).is_empty());
    }

    #[test]
    fn a_registered_session_is_ok_including_one_that_ended() {
        for (status, body) in [
            fx!("session_created"),
            fx!("session_reregister_same_201"),
            fx!("session_reregister_other_project_201"),
            fx!("session_reregister_after_end_409"),
        ] {
            assert_eq!(parse_session(status, body), Ok(()), "{status}");
        }
    }

    #[test]
    fn other_session_failures_are_reported() {
        assert!(!reason_of(parse_session(409, br#"{"code":"other"}"#)).is_empty());
        assert!(!reason_of(parse_session(409, b"")).is_empty());
        assert!(!reason_of(parse_session(500, b"boom")).is_empty());
    }

    #[test]
    fn ending_twice_is_ok_and_an_unknown_session_is_not() {
        for (status, body) in [
            fx!("session_end_200"),
            fx!("session_end_twice_200"),
            fx!("session_end_after_resume_200"),
        ] {
            assert_eq!(parse_end(status, body), Ok(()));
        }
        assert!(!reason_of(parse_end(404, br#"{"error":"session not found"}"#)).is_empty());
    }

    #[test]
    fn a_saved_note_gives_its_id() {
        let (status, body) = fx!("observation_saved_201");
        assert_eq!(
            parse_saved(status, body),
            Ok(Saved {
                id: 1,
                revision: None
            })
        );
    }

    #[test]
    fn an_upsert_gives_the_same_id_and_the_revision_when_present() {
        let (status, body) = fx!("observation_upsert_rev2");
        assert_eq!(parse_saved(status, body).expect("saved").id, 1);
        let with = parse_saved(201, br#"{"id":1,"status":"saved","revision_count":2}"#);
        assert_eq!(
            with,
            Ok(Saved {
                id: 1,
                revision: Some(2)
            })
        );
    }

    #[test]
    fn a_malformed_save_reply_is_a_failure() {
        for body in [
            &b""[..],
            b"[]",
            b"{}",
            br#"{"id":"1"}"#,
            br#"{"id":0}"#,
            br#"{"id":-3}"#,
            br#"{"id":1.5}"#,
        ] {
            assert!(
                parse_saved(201, body).is_err(),
                "{}",
                String::from_utf8_lossy(body)
            );
        }
    }

    #[test]
    fn a_session_project_mismatch_is_short_and_does_not_echo_content() {
        let (status, body) = fx!("observation_400_session_project_mismatch");
        let reason = reason_of(parse_saved(status, body));
        assert_eq!(reason, "session project mismatch");
        assert!(!reason.contains("other-proj") && !reason.contains("widget"));
    }

    #[test]
    fn an_unregistered_session_failure_does_not_echo_the_server_error() {
        let (status, body) = fx!("save_unregistered_session_404");
        assert_eq!(
            reason_of(parse_saved(status, body)),
            "engram error (HTTP 404)"
        );
    }

    #[test]
    fn only_allowlisted_codes_get_their_own_text() {
        let reason = |status: u16, body: &str| reason_of(parse_saved(status, body.as_bytes()));
        assert_eq!(
            reason(409, r#"{"code":"session_already_ended"}"#),
            "session already ended"
        );
        assert_eq!(
            reason(404, r#"{"code":"unknown_project"}"#),
            "unknown project"
        );
        assert_eq!(
            reason(400, r#"{"code":"made_up_code","error":"leaks SECRET"}"#),
            "engram error (HTTP 400)"
        );
        assert_eq!(
            reason(500, r#"{"error":"password=hunter2 in /home/x"}"#),
            "engram error (HTTP 500)"
        );
        assert_eq!(reason(502, "<html>"), "engram error (HTTP 502)");
    }

    #[test]
    fn a_save_id_beyond_i64_is_a_failure() {
        for body in [
            &br#"{"id":9223372036854775808}"#[..],
            br#"{"id":18446744073709551615}"#,
            br#"{"id":1e30}"#,
        ] {
            assert!(
                parse_saved(201, body).is_err(),
                "{}",
                String::from_utf8_lossy(body)
            );
        }
        let max = parse_saved(201, br#"{"id":9223372036854775807}"#);
        assert_eq!(max.expect("saved").id, i64::MAX);
    }

    #[test]
    fn failure_reasons_are_capped_and_have_no_control_characters() {
        let long = format!(
            r#"{{"error":"bad \u001b[31m\u0007 {} SECRET-NOTE-456\r\nX: y"}}"#,
            "y".repeat(500)
        );
        let reason = reason_of(parse_saved(400, long.as_bytes()));
        assert!(reason.chars().count() <= REASON_CAP, "{}", reason.len());
        assert!(!reason.chars().any(char::is_control));
        assert!(!reason.is_empty());
    }

    #[test]
    fn a_failure_without_a_usable_body_still_has_a_reason() {
        for body in [&b""[..], b"<html>boom</html>", b"\xff\xfe", b"[1]"] {
            let reason = reason_of(parse_saved(502, body));
            assert!(!reason.is_empty() && reason.chars().count() <= REASON_CAP);
            assert!(!reason.chars().any(char::is_control));
        }
        assert_eq!(reason_of(parse_saved(502, b"")), "engram error (HTTP 502)");
    }

    #[test]
    fn a_delete_that_worked_or_found_nothing_counts_as_deleted() {
        for (status, body) in [
            fx!("delete_200"),
            fx!("delete_200_server_without_token"),
            fx!("delete_404_already_deleted"),
        ] {
            assert_eq!(parse_delete(status, body), DeleteOutcome::Deleted);
        }
        assert_eq!(parse_delete(204, b""), DeleteOutcome::Deleted);
    }

    #[test]
    fn only_an_observation_not_found_404_counts_as_deleted() {
        for body in [
            &b""[..],
            b"<html>",
            b"{}",
            br#"{"error":"project not found"}"#,
            br#"{"error":"observation not found!"}"#,
            br#"{"code":"unknown_project"}"#,
            br#"{"error":7}"#,
        ] {
            assert_eq!(
                parse_delete(404, body),
                DeleteOutcome::Failed,
                "{}",
                String::from_utf8_lossy(body)
            );
        }
        let ok = parse_delete(404, br#"{"error":"observation not found"}"#);
        assert_eq!(ok, DeleteOutcome::Deleted);
    }

    #[test]
    fn a_missing_wrong_or_forbidden_token_is_refused() {
        for (status, body) in [
            fx!("delete_401_no_token"),
            fx!("delete_401_wrong_token"),
            fx!("delete_401_basic_scheme"),
            fx!("delete_401_raw_authorization"),
        ] {
            assert_eq!(parse_delete(status, body), DeleteOutcome::Refused);
        }
        assert_eq!(parse_delete(403, b""), DeleteOutcome::Refused);
        assert_eq!(
            DELETE_REFUSED,
            "engram refused the delete (check ENGRAM_HTTP_TOKEN)"
        );
    }

    #[test]
    fn other_delete_statuses_fail() {
        for (status, body) in [
            fx!("delete_400_no_expected_project"),
            fx!("delete_409_wrong_expected_project"),
        ] {
            assert_eq!(parse_delete(status, body), DeleteOutcome::Failed);
        }
        assert_eq!(parse_delete(500, b""), DeleteOutcome::Failed);
    }
}
