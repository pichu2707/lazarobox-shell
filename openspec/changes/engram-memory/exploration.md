## Exploration: engram-memory (lazarobox chat as an engram client)

### Current State
- ai-chat PRs 1-7 landed: text-only chat pane, SQLite store thread (`src/runtime/chat_store/worker.rs`), pure core types under `src/core/ai/chat/`. Hand-written loopback HTTP client: `src/core/ai/http.rs` (parser/decoder, pure) + `src/runtime/loopback.rs` (`post_stream`, POST only so far; GET is not yet there).
- ai-chat D13 tags a conversation with a project (repo-root basename via a `.git` filesystem walk, else cwd basename; worktrees share the main repo name). D15 (shared memory out of scope) is reversed by this change.
- Local engram: binary 3.4.0 (`/home/linuxbrew/.linuxbrew/bin/engram`). **The server on 127.0.0.1:7437 (PID 1064893, started 14:23) is still the OLD 1.20.0 binary (`/proc/1064893/exe -> .../Cellar/engram/1.20.0/bin/engram (deleted)`).** Its `/health` returns `{"service":"engram","status":"ok","version":"0.1.0"}` (no `instance_id`, no `capabilities`). It was NOT touched. The MCP process (PID 1375072, `engram mcp --tools=agent`) is separate and opens the same `~/.engram/engram.db` (WAL) directly.

### Verified engram 3.4.0 facts
Source: `engram --help`, `engram version` (3.4.0); throwaway 3.4.0 instance (`ENGRAM_DATA_DIR=<scratch> engram serve 17437`, killed afterwards, real db untouched); upstream DOCS.md and `internal/server/server.go` (main branch) on GitHub.
- Surface: `serve [port]` (HTTP), `mcp [--tools=agent|admin|all] [--project NAME]`, CLI `save/search/context/timeline/delete/stats/projects/doctor/export/import/compatibility`.
- Env: `ENGRAM_DATA_DIR` (default `~/.engram`, absolute), `ENGRAM_PORT`, `ENGRAM_SOCKET` (POSIX unix socket for serve, exclusive with port), `ENGRAM_PROJECT`, `ENGRAM_HTTP_TOKEN`, `ENGRAM_NO_UPDATE_CHECK`.
- Health (3.4.0, measured): `GET /health` -> `{"status":"ok","service":"engram","version":"3.4.0","instance_id":"...","capabilities":{"isolated_session_registration":true,"root_session_resume":true,"runtime_session_resolution":true}}`. Detect old servers by missing `instance_id`/`capabilities` (old reports `0.1.0`).
- Routes (server.go): `POST /sessions {id,project,directory?,ownership_mode?}` -> `{"id","status":"created"}`; `POST /sessions/{id}/end {summary}` -> `completed`; `GET /sessions/recent`; `POST /observations {session_id,type,title,content,project?,scope?,topic_key?,operation_id?,capture_prompt?}` -> 201 `{"id","status":"saved"}`; `GET /search?q=&project=&type=&scope=&limit=&match_mode=all|any`; `GET /observations/{id}`; `PATCH /observations/{id}?expected_project=X`; `DELETE /observations/{id}?expected_project=X` (needs token if set); `GET /observations/recent`; `GET /context?project=&scope=&observations=N&prompts=N&sessions=N&pinned=N&compact=bool&max_bytes=N` -> `{"context": "<markdown>"}`; `POST /topic-keys/suggest`; `GET /project/current?cwd=` -> `{project,project_source,project_path,cwd,available_projects}`; `GET /observations/save-result?operation_id=`; `GET /stats`, `/projects`, `/doctor`, `/compatibility`.
- Types documented: decision, architecture, bugfix, pattern, config, discovery, learning. **Bogus type `zzz` was accepted (201)**: type is free-form, not validated server-side.
- **There is NO tags field.** `AddObservationParams` = session_id, type, title, content, tool_name, project, scope, topic_key, operation_id. "Categories" = `type`, "tags" must be encoded in `topic_key` (e.g. `chat/<area>/<slug>`) and/or a line in content. Scope: project | personal | global.
- topic_key upsert verified: second POST with same `project+scope+topic_key` returned the same id, `revision_count: 2`, content replaced.
- Session is mandatory for a save: `POST /observations` without a registered session fails; `project` must equal the session's project (else 400 `session_project_mismatch`). lazarobox must register one session per chat conversation (id = e.g. `lazarobox-<conv uuid>`) and end it with a summary.
- Project resolution: explicit `project` > `ENGRAM_PROJECT` > cwd detection (git remote / `.engram/config.json`; binding stored in shared git metadata, survives remote rename and worktrees). `/project/current?cwd=` gave `lazarobox-shell`, `project_source: git_remote`. Reads without `project` on the HTTP server resolve against the SERVER cwd (search without project returned `[]`), so lazarobox MUST always pass `project` explicitly. Unknown project -> 404 `unknown_project` (+ `available_projects`); ambiguous -> 409 `ambiguous_project`. This is different from ai-chat's tag rule (basename of repo root), so lazarobox should ask `GET /project/current?cwd=` and use that name as the engram project (ai-chat's tag can be kept as a display/filter tag).
- Privacy: `<private>...</private>` is stripped in the store layer: `key <private>sk-abc123</private> end` was stored as `key [REDACTED] end` (measured). No automatic secret detection otherwise. Content limit 50000 bytes (truncation reported).
- Auth: bind 127.0.0.1 only by default. `ENGRAM_HTTP_TOKEN` (optional) protects ONLY delete/export/import/rescue routes; reads and saves are open to any local process (any local user on a shared host). Unix socket option (`ENGRAM_SOCKET`) gives filesystem-permission isolation but breaks the "default port" convention.
- Performance (3.4.0 throwaway, loopback): save ~12 ms total (curl), search 1-2.5 ms warm; earlier measurement on 1.20: CLI ~190 ms, HTTP 17-35 ms.
- Files: db and WAL/shm are 0600 (under 3.4.0).
- Versioning: no API version number. Stability signalled by `/health.capabilities` and the MCP tool contract baseline (`internal/mcp/testdata/tool-contract-v1.json`). HTTP routes used here (sessions, observations, search, context, health) existed in 1.20 too, but 3.x adds ownership rules (project_owned, 409 `project_ownership_*`) and `operation_id` replay.
- Who starts the server: nobody automatically for lazarobox. Claude Code's plugin hooks call `/health` and expect a running `engram serve`; here it was started manually/by a service (1.20 process lives since 14:23). `engram mcp` does not use the HTTP server (direct db). Concurrency: server and MCP are separate processes on one SQLite WAL db, which engram already supports.

### Affected Areas
- `src/core/ai/` new pure port (e.g. `memory/`: `MemoryPort` trait-less command/reply enums like `StoreCmd`/`StoreReply`, types `MemKind`, `MemNote`, index formatting, token budget, `<private>` pre-redaction) - pure, testable.
- `src/runtime/` new adapter (`engram.rs`/`engram/`): HTTP over the loopback client (needs GET + JSON bodies; `loopback.rs` currently POST-stream oriented), a worker thread/task, health probe cache.
- `src/runtime/chat/*`, `src/core/ai/chat/state*` - chat open (inject context), "save as note" key, end-of-conversation summary.
- Config (`[ai.memory] enabled, endpoint, token_env`), README privacy section, menu row.
- NOT touching `src/runtime/chat_store*` (remains the raw conversation store, 0600).

### Approaches
| | HTTP to `engram serve` | MCP over stdio (`engram mcp`) | CLI (`engram save/search/context`) |
|---|---|---|---|
| Latency | 1-12 ms (measured) | spawn once ~tens of ms, then fast | ~190 ms/call (1.20 measurement) |
| Robustness | needs a running server; down = degrade | child lifecycle owned by lazarobox; JSON-RPC handshake, tool schema drift | no daemon; always works if binary on PATH; per-call spawn |
| Coupling | stable simple REST; no version field, use `/health` capabilities | tool contract v1 is test-enforced upstream (most stable), but heavy protocol | stdout text parsing, brittle; flags vary |
| Complexity | Low: reuses hand-written loopback client + JSON | High: JSON-RPC framing, process mgmt, stdin/stdout pipes | Lowest, but parsing text and slow |
| Fit | best | second | fallback/bootstrap only |

### Recommendation
HTTP to `engram serve` on `127.0.0.1:7437` (override via config/`ENGRAM_PORT`), behind a pure port in core and an adapter in runtime; never on the render thread (same pattern as the store thread: command channel in, reply channel out, bounded timeouts of ~500 ms, reads cached). Degrade silently to "memory unavailable" (status line hint) when `/health` fails or lacks `instance_id`. lazarobox should NOT spawn `engram serve` in the first slice: it would own a daemon that outlives it and race on the port with the user's service; instead show guidance ("run `engram serve`"). Optional later slice: opt-in autostart of a child on a free port with the same data dir. The `engram mcp` option is rejected for P2 (no tools yet), but P3 tools can simply map to the same port calls, so no rework.

Memory model: one engram observation per distilled item, `type` in {decision, architecture, bugfix, pattern, config, discovery, learning}; `topic_key` = `chat/<category>/<slug>` for upsert of evolving topics; tags as a `**Tags**:` line in content (searchable via FTS) since engram has no tags column; always `scope=project`, `project` from `/project/current`. A per-conversation session `lazarobox-<uuid>` with `POST /sessions` and `/end {summary}` (the summary doubles as the resume anchor). Context at chat open: `GET /context?project=P&compact=true&observations=8&sessions=2&prompts=0&max_bytes=2000` (about 500 tokens), inject as system context; detail on demand via `GET /observations/{id}` when the user opens a note (no automatic search).

Distillation in P2 (text-only):
1. Manual "save as note" key on the last answer or selection: modal asks type + title + tags, content is the selected text, `<private>` wrapped regions honored. Zero extra model cost, user-controlled, no raw reasoning stored. Recommended first.
2. End-of-conversation summary: one extra engine call with a fixed "summarize decisions/facts, no reasoning" prompt on the final turns, shown for approval before save (never auto-saved). Costs tokens, needs approval UI; second slice.
3. P3: model calls a `mem_save`/`mem_search` tool through the agent loop and approval model; maps 1:1 to the same port. Not now.

### Risks
- The running server is 1.20 on :7437 against a db that 3.4.0 (MCP) already opens: schema migration performed by 3.4.0 may make 1.20 behave unexpectedly (it was not verified; user should restart the service with 3.4.0). The adapter must tolerate old servers (missing capabilities).
- HTTP save/search endpoints are unauthenticated on loopback; any local process can read all project memory. Cannot be fixed on our side except unix socket; README must say so.
- Prompt injection: engram content is data from other agents and past chats; injecting it into the system prompt needs the sanitizer (`core/ai/sanitize`), a clear delimiter and a length cap; a poisoned note could steer the model.
- Secrets: only `<private>` is stripped; lazarobox should add its own redaction pass (patterns for keys/tokens) before send, since chat.db is 0600 but engram's db is shared with other agents and optionally synced to cloud/git (Git Sync exports chunks) - leaving the 0600 boundary.
- Project name mismatch with ai-chat's tag rule causes memory split; mitigate with `/project/current` and a user override.
- No tags/type validation upstream: free-form types fragment categories; lazarobox must enforce its own closed enum.
- `GET` in loopback client and JSON decoding are new work; ordering with ai-chat C4/PR tasks (see `tasks.md`).
- Concurrent edits to ai-chat spec by another agent (indicator spec) may change chat state shapes this change hooks into.

### PR ordering
Pure port + types + context formatter can start now (independent). The adapter needs the loopback client extended with GET (check ai-chat C4 status in tasks.md). UI hooks (open-context injection, save-note key) need the conversation view from ai-chat PR 8+ (chat pane list/conversation views) and at least one engine adapter (Ollama) merged so injected context has an effect.

### Open product questions
1. Should memory be opt-in per project/globally ("[ai.memory] enabled", off by default) or on by default when engram is detected?
2. Is manual "save as note" enough for the first release, or must an end-of-conversation summary (extra model call, with approval) ship in the same change?
3. Do you accept that notes leave lazarobox's 0600 store and become visible to every local process and every agent using engram (and to Git/Cloud sync if enabled)? Should lazarobox warn at first save?
4. When engram is not running: only guide the user, or should lazarobox offer to start `engram serve` itself?
5. Which categories should the chat expose (default: decision, architecture, bugfix, pattern, config, discovery, learning) and should chat-originated notes be distinguishable (e.g. topic_key prefix `chat/`, tag `source:lazarobox`)?

### Ready for Proposal
Yes. Recommend an interactive question round on the 5 questions above, then sdd-propose.
