# engram 3.4.0 spike findings (change `engram-memory`, M0)

Recorded against a throwaway `engram 3.4.0`: `ENGRAM_DATA_DIR` in a scratch dir, `ENGRAM_HTTP_TOKEN=throwaway`, `ENGRAM_NO_UPDATE_CHECK=1`, port 17437 (plus a second token-less instance on 17438 for one fixture). The real `~/.engram` and `:7437` were never targeted (`.instance-id` mtime and sha256 identical before and after). Scratch paths were rewritten to `/tmp/spike/...` and `Content-Length` recomputed; everything else is exactly what the server sent (CRLF, `Date` header included). `../.gitattributes` marks `*.http` as `-text` so git never rewrites them.

Each `.http` file is the raw HTTP response only (status line, headers, blank line, body). The request that produced it is in the tables below.

## E-2: DELETE token header

Answer: `Authorization: Bearer <token>`. Confirmed by the server's own challenge `Www-Authenticate: Bearer realm="engram"` on every 401.

| Request | Result | Fixture |
|---------|--------|---------|
| `DELETE /observations/2?expected_project=widget`, no token | 401 `{"error":"authorization required"}` | `delete_401_no_token.http` |
| same, `Authorization: Bearer wrong` | 401 `{"error":"invalid token"}` | `delete_401_wrong_token.http` |
| same, `Authorization: throwaway` (no scheme) | 401 `authorization required` | `delete_401_raw_authorization.http` |
| same, `Authorization: Basic ...` | 401 `authorization required` | `delete_401_basic_scheme.http` |
| same, `Authorization: Bearer throwaway` | 200 `{"hard_delete":false,"id":2,"status":"deleted"}` (soft delete) | `delete_200.http` |
| Bearer ok, `expected_project=nope` | 409 `expected_project does not match observation owner` | `delete_409_wrong_expected_project.http` |
| Bearer ok, no `expected_project` | 400 `expected_project must be a valid non-empty project name` | `delete_400_no_expected_project.http` |
| Bearer ok, already deleted or unknown id | 404 `observation not found` | `delete_404_already_deleted.http` |
| server started WITHOUT a token, DELETE with no header | 200 deleted | `delete_200_server_without_token.http` |

Notes:
- A 403 was NOT observed anywhere. Missing or wrong token is always 401, with two messages (`authorization required` vs `invalid token`). `delete_403.http` from the task list is therefore not produced; M2/M3a/M9 map 401 (not 403) to "token required or wrong".
- `expected_project` is mandatory for DELETE and checked against the owner (409 on mismatch, 400 if absent).
- Only the scheme `Bearer` is accepted. M2 uses exactly `Authorization: Bearer <token>` (matches the design assumption).
- If the server has no `ENGRAM_HTTP_TOKEN`, DELETE needs no header. Sending a Bearer header to a token-less server was not recorded; M9 must not rely on that case.
- After a soft delete, `GET /observations/{id}` returns 404.

## E-3: session re-registration and ended sessions

Verdict: the "one engram session per conversation" model WORKS with one rule for the worker. The fallback "fresh session id per visit" is NOT needed.

| Step | Request | Result | Fixture |
|------|---------|--------|---------|
| (a) | `POST /sessions` twice, same id and project | 201 `{"status":"created"}` both times (idempotent) | `session_reregister_same_201.http` |
| (a') | `POST /sessions` same id, DIFFERENT project | 201, session keeps its original project (later saves for the original project still succeed) | `session_reregister_other_project_201.http` |
| (b) | `POST /sessions/{id}/end {summary}` | 200 `{"status":"completed"}`; ending twice also 200, the LAST summary wins | `session_end_200.http`, `session_end_twice_200.http` |
| (b') | `POST /sessions` again for an ENDED id | 409 `{"code":"session_already_ended","error":"session has already ended"}` | `session_reregister_after_end_409.http` |
| (c) | `POST /observations` into the ended session WITHOUT re-registering | 201 saved | `save_into_ended_no_reregister_201.http` |
| (d) | `POST /observations` after the (failed) re-register | 201 saved | `save_into_ended_after_reregister_attempt_201.http` |
| extra | `/end` again after resuming | 200, new summary replaces the old one, `ended_at` unchanged | `session_end_after_resume_200.http`, `sessions_recent.http` |
| extra | save with a never registered session id | 404 `{"error":"session not found"}` | `save_unregistered_session_404.http` |

Rules for M3b/M7b:
1. On resume of a conversation whose session already ended, `POST /sessions` returns 409 with `code == "session_already_ended"`. Treat exactly that code as SUCCESS (the session exists). Any other 4xx stays an error.
2. Saving into an ended session is allowed, so no new session id is needed.
3. Ending again is allowed and overwrites the summary (latest summary = resume anchor, as designed).
4. The `project` sent on every save must equal the session's project, else 400 `session_project_mismatch`.

## E-4: compact `/context` item marker

Request: `GET /context?project=widget&scope=project&compact=true&observations=8&sessions=2&prompts=0&max_bytes=2000` (`context_compact.http`). Body is JSON `{"context":"<markdown>"}`:

```
## Memory from Previous Sessions

### Recent Sessions
- **widget** (2026-10-12 22:02:51) [4 observations]
- **widget** (2026-10-12 22:03:06): <summary> [2 observations]

### Recent Observations
- [pattern] **Error enum per module**
- [bugfix] **Fix parser panic**
...
```

Answer: a note is a line of the form `- [<kind>] **<title>**` (starts with `- [`) under `### Recent Observations`. Session lines start with `- **` (no bracketed kind) and must NOT be counted. Count = lines in the `### Recent Observations` section that start with `- [`.

Related behaviour:
- Compact items are one line each, no body. Non-compact (`context_full.http`) appends `: <content>`, which may span several lines; count only the compact format.
- A project with sessions but no observations has no `### Recent Observations` section (`context_sessions_only.http`): 0 notes, block still non-empty.
- An unknown project is NOT an empty body: 404 `{"code":"unknown_project","available_projects":[...]}` (`context_empty.http`). M1/M3a treat 404 `unknown_project` as "no memory yet" (`None`).
- `max_bytes` is enforced by the server and the output ends with `\n[truncated]\n`; the cut can land mid-line (`context_tiny_budget.http`, `max_bytes=200`), so the client still does its own line-end/char-boundary cut (M1 2.7).

## Other recorded behaviour

| Topic | Result | Fixture |
|-------|--------|---------|
| `/health` 3.4.0 | 200 with `capabilities{isolated_session_registration,root_session_resume,runtime_session_resolution}`, `instance_id`, `service:"engram"`, `status:"ok"`, `version:"3.4.0"` | `health_3_4_0.http` |
| `/health` 1.20 | hand written from `{"service":"engram","status":"ok","version":"0.1.0"}` (no `capabilities`, no `instance_id`) | `health_1_20.http` |
| `/health` HTML / other service | hand written | `health_html.http`, `health_other_service.http` |
| Save | 201 `{"id":1,"status":"saved"}` | `observation_saved_201.http` |
| `topic_key` upsert | same `topic_key` in the same session returns 201 with the SAME id; `revision_count` becomes 2, content replaced | `observation_upsert_rev2.http`, `observation_get.http`, `search_hit.http` |
| Search | JSON array of observation objects; no hit is `[]` | `search_hit.http`, `search_empty.http` |
| Project mismatch | 400 `{"code":"session_project_mismatch","error":"session project does not match requested project","project":"other-proj","session_project":"widget"}` | `observation_400_session_project_mismatch.http` |
| `/project/current` git repo with remote | 200, `project_source:"git_remote"`, project from the remote name (`widget`) | `project_200.http` |

### Deviation from the plan: `/project/current` never returns 404 or 409

The task list expected `project_404.http` and `project_409.http`. Actual 3.4.0 behaviour:
- No git project (or nonexistent / relative `cwd`): 200 with `project_source:"dir_basename"` and the directory basename as project (`project_dir_basename_200.http`, `project_nonexistent_dir_200.http`). No 404.
- Ambiguous (several git repos below `cwd`): 200 with `project:""`, `project_source:"ambiguous"`, `error_hint` and `available_projects` (`project_ambiguous_200.http`). No 409.

Consequence for M1/M3a: do not classify by HTTP status. Unresolved means `project == ""` or `project_source == "ambiguous"`. Whether `dir_basename` (a weak guess such as `tmp`) counts as resolved is a design decision for the design owner.
