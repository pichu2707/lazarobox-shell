# Tasks: Engram Memory (the chat as an engram client)

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | ~3,700 total (tests included), 12 PRs of 150-450 lines, plus 1 no-code manual PR |
| 400-line budget risk | Medium (total per PR); High for M1 (~400-450), M8 (~400-450) and M3b |
| Chained PRs recommended | Yes |
| Suggested split | PR1 (M0) → PR2 (M1) → ... → PR12 (M9) → PR13 (V, manual) (see work units) |
| Delivery strategy | auto-chain |
| Chain strategy | feature-branch-chain (tracker `feat/engram-memory`) |

Decision needed before apply: No
Chained PRs recommended: Yes
Chain strategy: feature-branch-chain
400-line budget risk: Medium

`size:exception` is pre-accepted by the user when tests push a PR over 400 lines (additions + deletions). The forecast stays honest: M1, M3b and M8 may exceed 400, and the fallback split is noted on each.

Rules for every PR:
- Strict TDD: RED with compiling stubs and exact assertions, then GREEN, then refactor. Tests are never trimmed.
- Gate = `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
- One work-unit commit per task group (conventional commits, no AI attribution). Never run git from a task unless the apply phase owns that PR.
- Re-measure with `git diff --shortstat` before opening each PR; above 400 either use the noted fallback split or record `size:exception`.
- PR1 base = `feat/engram-memory`; PR(n) base = PR(n-1) branch. Only the tracker merges to main.
- Memory is opt-in (`enabled = false`): until M4 lands nothing emits `Effect::Memory`; new items carry `cfg_attr(not(test), expect(dead_code))` until their consumer lands.
- Fixtures live in `src/core/ai/memory/fixtures/3.4.0/*.http` (recorded raw responses, loaded with `include_bytes!`).
- Never touch the user's real engram db or `:7437`. Anything that needs a live engram uses an isolated throwaway instance (task 1.1).

### Suggested Work Units

ai-chat chain today: PR7 (#140) → PR8 (#141) → PR9 (live pane, in review fixes) → PR13 (Ollama, reordered ahead of PR10-PR12) → PR10 (list/tags/resume/delete, wires `Effect::Store`) → PR11 → PR12. Chain order below puts the ai-chat-independent slices first.

| Unit | Goal | PR | Est. | Depends on | Order |
|------|------|----|------|-----------|-------|
| M0 | Spike: verify E-2, E-3, E-4 on a throwaway engram 3.4.0, record fixtures | PR1 | ~150 | - | first, blocks M1/M3/M7b |
| M1 | Core: kind, slug/topic_key, redaction, context block, health classifier | PR2 | ~400-450 | M0 | fallback 1a (kind, slug, redact) / 1b (context, health) |
| M2 | Loopback `request` (GET/POST/DELETE, JSON, allow-list) | PR3 | ~250 | - | reorderable with M1 |
| M3a | Call builders, reply parsers, `MemCmd`/`MemReply` | PR4 | ~250 | M0, M1, M2 | fixed |
| M3b | Worker task, caches, sessions, `Effect/AppEvent::Memory` | PR5 | ~300-350 | M3a | fixed |
| M4 | `[ai.memory]` config, persistence, menu row, privacy warning | PR6 | ~350 | M1 | reorderable after M1 |
| M5 | `engram serve` offer: spawn, port race, adopt (worker side + pure decision) | PR7 | ~350 | M3b, M4 | fixed |
| M7a | chat.db v2, link store commands, eviction note count | PR8 | ~250 | M3b, M4, ai-chat PR9 | fixed |
| M7b | Note editor (Ctrl+N), session register/end | PR9 | ~300 | M7a | fixed |
| M6 | Context at open/resume, `Entry::Memory` (Ctrl+O), `ChatRequest.memory`, budget, statusline, offer hook | PR10 | ~350 | M5, M7b, ai-chat PR9 + PR13 | fixed |
| M8 | Summary draft and approval (Ctrl+E) | PR11 | ~400-450 | M6, M7b, ai-chat PR13 | fixed |
| M9 | Delete offer with token, store delete rows, README | PR12 | ~300 | M7a, ai-chat C9 (PR10) | fixed |
| V | Final manual verify, live engram end to end | PR13 | 0 | PR12 | no code |

Gating: PR1-PR7 can start now. PR8-PR9 wait for ai-chat PR9 merged into its tracker. PR10-PR11 wait for ai-chat PR13. PR12 waits for ai-chat PR10. If an ai-chat PR changes shape (indicator work), re-read `src/core/ai/chat/*` before the RED tasks.

## PR1 / M0: Spike on a throwaway engram 3.4.0 (`src/core/ai/memory/fixtures/3.4.0/`)

No production code. The only outputs are fixtures, a findings file and design open-question updates.

- [x] 1.1 [manual] Start an isolated instance: `ENGRAM_DATA_DIR=$(mktemp -d) ENGRAM_HTTP_TOKEN=throwaway ENGRAM_PORT=<free port, never 7437> engram serve <port>` (3.4.0 binary, `engram version` checked first). Install a `trap` that kills it and removes the dir; confirm `/health` has `instance_id` and `capabilities`. Never target `:7437` or `~/.engram`.
- [x] 1.2 Record fixtures (raw status line, headers, body): `health_3_4_0.http`, `health_1_20.http` (hand-written from `{"service":"engram","status":"ok","version":"0.1.0"}`), `health_html.http`, `health_other_service.http`.
- [x] 1.3 Record `project_200.http`, `project_404.http`, `project_409.http` (via a temp git repo, a directory with no project, and an ambiguous one), `session_created.http`, `observation_saved_201.http`, `observation_upsert_rev2.http`, `observation_400_session_project_mismatch.http`.
- [x] 1.4 E-2: `DELETE /observations/{id}?expected_project=P` with the token as `Authorization: Bearer` and, if refused, as the alternatives engram documents; also with no token and with a wrong token. Record `delete_200.http`, `delete_401.http`, `delete_403.http` and note the accepted header name.
- [x] 1.5 E-3: (a) `POST /sessions` twice with the same id; (b) `POST /sessions/{id}/end`, then `POST /sessions` again with the same id; (c) `POST /observations` into the ended session without re-registering; (d) with re-registering. Record every reply as `session_reregister_*.http` and `save_into_ended_*.http`.
- [x] 1.6 E-4: save three observations of different kinds and end a session with a summary, then `GET /context?project=P&scope=project&compact=true&observations=8&sessions=2&prompts=0&max_bytes=2000`. Record `context_compact.http` and `context_empty.http`; write down the exact per-note item marker used for `N notes`.
- [x] 1.7 Write `fixtures/3.4.0/FINDINGS.md` (E-2 header name and statuses, E-3 verdicts, E-4 marker) and update design.md "Open Questions" E-2..E-4 with the results.
- [x] 1.8 Fallback gate for E-3. If re-registering an existing or ended session fails, or saving into an ended session fails: STOP before PR9 (M7b) and ask the user to accept the fallback: a fresh session id per visit (one engram session per visit instead of per conversation, an E6 deviation), where each visit replaces the `memory_sessions` row. The user must accept before M7a/M7b assume it. If E-2 shows a header other than Bearer, M2/M3a use the recorded name.
- [x] 1.9 [manual] Verify cleanup: process gone, temp dir removed, nothing listening on the spike port, `:7437` untouched.
- [x] 1.10 Gate.

## PR2 / M1: Core, pure (`src/core/ai/memory/{mod,kind,slug,redact,context,health}.rs`)

Fallback split if over 400 without exception: 1a = 2.1-2.6, 1b = 2.7-2.10.

- [x] 2.1 RED: `MemKind` has exactly the seven values in order (`decision`, `architecture`, `bugfix`, `discovery`, `pattern`, `config`, `preference`); `parse("learning")` is rejected; `as_str` round-trips (chat-memory "Closed enum").
- [x] 2.2 GREEN: `kind.rs` with `MemKind::ALL`, `as_str`, `parse`.
- [x] 2.3 RED: slug table from the spec (`Use Axum!`, `  A/B  test--x `, `Ünï Cødé 42`, `日本語 メモ`, `../../etc`), 200-letter title cut to 60 with no trailing `-`, `!!! ---` gives `untitled`; `topic_key(Bugfix, "Fix  the Parser") == "chat/bugfix/fix-the-parser"`; a title cannot add a path segment; same slug gives the same key.
- [x] 2.4 GREEN: `slug.rs` (`slug`, `topic_key`, `clean_title` 1-120 chars, control/format chars removed).
- [x] 2.5 RED: redaction tables: each prefix pattern, `Authorization: Bearer abc.def` (and `Basic`, `Token`, any case), URL userinfo, `SECRET_KEY=` style names, quoted values with spaces, secrets after non-ASCII characters, PEM block (terminated and unterminated), `password=hunter2 ok`, `API_KEY: zz9`, `<private>` terminated and unterminated, hit count, plain text unchanged, idempotent on a seeded random corpus, no panic on arbitrary bytes-as-str, linear time on a 1 MiB input.
- [x] 2.6 GREEN: `redact.rs` hand-written scanner returning `Redacted { text, hits }`; no regex crate.
- [x] 2.7 RED: `context_block`: 50,000-byte input with escapes is cut under 2,000 bytes at a line end and a char boundary; every `<` in the data becomes `‹` (so `</PROJECT-MEMORY>` and look-alikes such as `</project\u{34f}-memory>` leave exactly one closing delimiter and no other `<`), and the fillers U+034F, U+115F, U+1160, U+3164, U+FFA0, U+180E are stripped; opening line `<project-memory source="engram" project="P">` plus the fixed data-not-instructions line; empty gives `None`; `notes` counts only lines starting with `- [` under `### Recent Observations` of `context_compact.http` (session lines `- **` are not counted; `context_sessions_only.http` gives 0 notes and a non-empty block); the server `\n[truncated]\n` tail of `context_tiny_budget.http`, which can cut mid-line, is handled by the same cut.
- [x] 2.8 GREEN: `context.rs` (`ContextBlock { text, notes, bytes }`), reusing the CHAT-4 sanitizer.
- [x] 2.9 RED: `classify_health` against the M0 fixtures: 3.4.0 gives `Ready{instance_id,version}`, 1.20 (`health_1_20.http`) gives `Old`, `health_html.http`, `health_other_service.http` and non-200 give `Foreign`, missing `capabilities` gives `Old`.
- [x] 2.10 GREEN: `health.rs`; module wiring in `mod.rs` and `src/core/ai/mod.rs`.
- [x] 2.11 Refactor; Gate.

## PR3 / M2: Loopback `request` (`src/runtime/loopback.rs`, `src/core/ai/http.rs`)

- [x] 3.1 RED (characterization): existing loopback and detection tests stay green after the body-framing helper (`Body` Content-Length/chunked rules) moves from the private enum to a shared helper in `src/core/ai/http.rs`.
- [x] 3.2 GREEN: move the framing helper; `post_stream` and `CHAT_PATHS` untouched.
- [x] 3.3 RED: against the existing fake server: GET/POST/DELETE bytes exact (request line, `Host: 127.0.0.1`, `Accept`, `Connection: close`, `Content-Type`/`Content-Length` only with a body); any status 200-599 is returned with its body; 307 is an error and its Location is never requested.
- [x] 3.4 RED: Content-Length with Transfer-Encoding rejected, 256 KiB plus one byte body rejected in both framings; non-`127.0.0.1` host makes no connection; target with CR, LF, space, or outside `ENGRAM_ROUTES` is `Refused`; `Authorization: Bearer` only when `auth` is set and never `Cookie`.
- [x] 3.5 RED: timeouts on a paused clock: connect 200 ms and total limits (300 ms / 800 ms / 2 s) become `CallError::Timeout`; a silent server never hangs.
- [x] 3.6 GREEN: `loopback::request(port, Method, target, body, auth, limits) -> Result<Reply, CallError>`, `Method {Get, Post, Delete}`, `ENGRAM_ROUTES`.
- [x] 3.7 Gate.

## PR4 / M3a: Call builders, parsers, port (`src/core/ai/memory/{route,reply,port}.rs`)

- [x] 4.1 RED: builders produce exact targets and bodies: percent-encoded `cwd` (`/tmp/a b&c\r\nX: y` gives one query value, no injected header), `project`; context query string; session body with directory; save body fields only (`session_id`, `type`, `title`, `content`, `project`, `scope=project`, `topic_key`); `expected_project` on delete; content cut at 50,000 bytes on a char boundary (16,000 three-byte chars).
- [x] 4.2 GREEN: `route.rs` with `Call { method, target, body, auth }` and the builders.
- [x] 4.3 RED: parsers over every M0 fixture: project classified by `project_source` (`git_remote` resolves; `dir_basename`, `ambiguous`, empty project or unknown source give `NotGit`; never by HTTP status), context 404 `unknown_project` as no memory yet, session created, saved 201 (id, revision), upsert same id, 400 `session_project_mismatch` short sanitized text (at most 120 chars, no escape, no echo of content), context `{context}`, delete 2xx/404 as deleted, 401/403 as `engram refused the delete (check ENGRAM_HTTP_TOKEN)`.
- [x] 4.4 GREEN: `reply.rs`.
- [x] 4.5 RED then GREEN: `MemCmd {Resolve, Context, Save, End, Delete, StartServer, Quit}` and `MemReply {Health, Project, Context, Saved, Deleted, Server, Unavailable}` compile as plain values; reasons enum renders the exact `memory unavailable:` texts of chat-memory "Degradation".
- [x] 4.6 Gate.

## PR5 / M3b: Worker and app wiring (`src/runtime/engram/{mod,worker}.rs`, `fake_engram.rs`, `src/app.rs`, `src/runtime.rs`)

Fallback split if over 400: 5a worker with `Transport` fake, 5b app/runtime wiring and `fake_engram`.

- [x] 5.1 RED: worker with a scripted `Transport` fake on `start_paused`: FIFO; register-once then save; a second save does not register again; `instance_id` change clears the project cache and re-registers; transport failure sets Down, commands answered `Unavailable` at once, a new probe only after 10 s (injected clock); queue of 64 answers `busy`; stale pane reply dropped.
- [x] 5.2 RED: session id format `lazarobox-[0-9a-f]{32}` from 16 `getrandom` bytes (libc, no new crate), unique across calls.
- [x] 5.3 RED: `Quit` flushes pending `End` within a 1 s bound against a server that never answers; a silent server does not stall key handling or a running chat stream (2 s write limit).
- [x] 5.4 GREEN: `engram/{mod,worker}.rs` (one tokio task, bounded mpsc 64, health cache, project cache per cwd, registered-session set); token read from env in the runtime only, `token_present` flag exposed.
- [x] 5.5 RED: `fake_engram` TcpListener replays fixtures by route and asserts requests: project always present, session before save, `Authorization` only on DELETE, `ENGRAM_HTTP_TOKEN` never in an error text, no content in errors or logs (`SECRET-NOTE-456`).
- [x] 5.6 GREEN: `Effect::Memory(MemCmd)`, `AppEvent::Memory(MemReply)` routed through `app.rs` and `runtime.rs`; worker started lazily and only when memory is enabled (default off: no worker, no connection; test with a recording fake).
- [x] 5.7 Gate.
- [x] 5.8 Review fixes (M3b): `MemCmd::Save` carries the chat `cwd` for the session `directory` (engram 3.4.0 accepts an empty or missing one, see FINDINGS); a save that gets 404 `session not found` registers again and retries once; a non-200 `/context` (other than 404 `unknown_project`) is `Unavailable` and degrades health; `registered` survives a panic; the memory flush runs alongside the chat shutdown; `ENGRAM_HTTP_TOKEN` is read only when the worker starts; the session id is Linux-gated; a lost `Busy` reply is covered by the 5 s wait in 10.9.

## PR6 / M4: Config, persistence, menu row, privacy warning (`src/core/ai/{config,menu}.rs`, `src/core/menu.rs`, persistence module)

- [x] 6.1 RED: config parsing: absent table gives enabled false, acknowledged false, port 7437; `ENGRAM_PORT` valid sets the default port; `enabled=true` without `acknowledged=true` is inactive with the notice `memory: turn it on from the menu to review the privacy note`; invalid types and `port = 0`/`70000`/`"7437"` fall back with a notice.
- [x] 6.2 GREEN: `[ai.memory]` parsing in `config.rs`.
- [x] 6.3 RED: persistence owns `ai.memory.enabled/port/acknowledged`: appends `[ai.memory]` preserving original bytes and comments, changes only `enabled`, off keeps `acknowledged`, no `port` key added, a scalar `memory` or table-valued owned key is handled as the specs say.
- [x] 6.4 GREEN: owned keys in the persistence writer.
- [x] 6.5 RED: menu descriptor has `Memory` (`ai.memory.enabled`: off, on) after `Profile`; navigation order and wrap visit `Profile` then `Memory`; `h/l/Left/Right/Space/Enter` toggle, Repeat ignored on Enter; footer `j/k move · h/l change · s save · Esc cancel`; `r` does nothing; toggling makes no request.
- [x] 6.6 RED: privacy warning follows the draft flow (ruling 3): turning on with `acknowledged=false` opens the warning `Turn memory on? (y/n)`; unmodified lowercase `y` sets drafts (`enabled` on, `acknowledged` true) and writes nothing; `n`, Esc, Enter, `Y` decline and keep `off`; Repeat ignored; `s` then persists both keys; Esc at the top level restores the drafts including the acknowledgement; second enable after acknowledgement shows no warning.
- [x] 6.7 GREEN: menu rows, warning view state, draft capture/restore in `menu.rs` and `core/menu.rs`. Note: ai-chat (`Profile`, `Context`) is merged into this tracker: `Memory` is the last AI row, after `Profile` and `Context`. `AI_BODY` (popup body rows) is derived from the descriptor tables (14 with `Profile`, `Context` and `Memory`).
- [x] 6.8 RED then GREEN: insta snapshots: Settings/AI rows with `Memory`, warning view on a normal terminal, one-line fallback on a tiny terminal (no panic); popup size follows the new row.
- [ ] 6.9 [manual, Kitty] The warning is readable without clipping; `y`, then `s`, then restart shows memory on without a second warning.
- [x] 6.10 Gate.

## PR7 / M5: `engram serve` offer (`src/runtime/engram/serve.rs`, `src/core/ai/memory/port.rs`)

The chat-view hook that shows the prompt lands in M6 (and the memory keys in M7b/M8); this PR delivers the worker side and the pure offer state so it is testable now.

- [x] 7.1 RED: pure `ServerOffer` decision: Down + enabled + acknowledged + not used this run + absolute PATH binary with `version` major >= 3 gives an offer with the prompt `engram is not running. Start "<absolute path> serve <port>"? y start · n not now` (ruling 4: the absolute path, never the bare word `engram`); 1.x binary gives `engram on PATH is 1.x; upgrade it`; no binary gives `engram not found on PATH`; Old/Foreign/unknown/ambiguous give no offer; only unmodified lowercase `y` confirms; any other key declines and no second offer in the run.
- [x] 7.2 GREEN: offer state in core, `find_on_path` and the existing 2 s group-killed `version()` helper in runtime.
- [x] 7.3 RED: spawn with fake `engram` scripts on a temp PATH: argv exactly `["serve","<port>"]`, no shell, `ENGRAM_PORT` removed, `ENGRAM_DATA_DIR` untouched, stdin `/dev/null`, `/proc/<pid>/stat` shows sid == pid, log dir 0700 and file 0600 under a permissive umask, `O_NOFOLLOW` (symlink refused), truncated when over 1 MiB, child survives the handle drop (test kills it).
- [x] 7.4 RED: re-probe before spawn adopts a ready server with zero spawns; bind-failure exit plus ready server is adopted with one spawn only; spawn that exits 1 with nothing answering gives `could not start engram` plus the sanitized last log line (at most 120 chars) and no further spawn; success shows `started engram serve (pid N); it keeps running after lazarobox quits`; no signal is ever sent on pane close or quit.
- [x] 7.5 GREEN: `MemCmd::StartServer`, `serve.rs` (`pre_exec` calls only `setsid()`, 100 ms poll for 3 s, reaper task, `kill_on_drop(false)`).
- [ ] 7.6 [manual] Real start: in a shell with `ENGRAM_DATA_DIR=<temp dir>` and the config `port` set to a spare port (never 7437), confirm the offer; quit lazarobox; `engram serve` still answers `/health`; kill it and remove the dir afterwards.
- [x] 7.7 Gate.
- [x] 7.8 Review fixes (judgment day): spawn only the offered path and only while it is still the same file; no offer means no spawn; log directory and file hardened like the chat store (no symlink, no FIFO block, owner, 0700/0600); OS-error hint on a failed spawn; non-UTF-8 or unprintable binary paths never offered; confirm on key press only; launcher injected in runtime tests; umask restored by a drop guard. 7.6 (real start) stays open.

## PR8 / M7a: chat.db v2 and eviction count (`src/runtime/chat_store.rs`, `chat_store/worker.rs`, `src/core/ai/chat/store.rs`)

Starts after ai-chat PR9 is in the ai-chat tracker. Note: ai-chat has no store delete until PR10, so the delete-returns-rows change is in M9 (PR12).

- [ ] 8.1 RED: v1 database with conversations and messages opens as v2 with every row intact and `memory_sessions`/`memory_notes` empty; a v1-only opener refuses a v2 file and leaves its bytes unchanged; file modes 0600 and `secure_delete` cover the new tables.
- [ ] 8.2 GREEN: additive v2 migration in one transaction (`memory_sessions(conversation_id PK FK CASCADE, session_id UNIQUE, project)`, `memory_notes(project, topic_key, observation_id, conversation_id FK CASCADE, title, PK(project, topic_key)) WITHOUT ROWID`).
- [ ] 8.3 RED: `StoreCmd::LinkSession` and `LinkNote`: the same session id is returned on resume; a repeated note save leaves one row; a second conversation saving the same `(project, topic_key)` re-points the row; no note content in the db or side files (`SECRET-BODY` search); a failed link write reports without undoing anything.
- [ ] 8.4 GREEN: link commands on the store thread and `StoreReply` variants.
- [ ] 8.5 RED: retention counts `memory_notes` of the evicted conversations in the same transaction; `StoreReply::Created` carries `evicted_notes: u32` (replacing nothing else); rows go with an evicted conversation.
- [ ] 8.6 GREEN: eviction count; the chat transcript adds, after `EVICTED_LINE`, `N notes of removed conversations remain in engram` (`1 note of a removed conversation remains in engram`), no line for zero, no prompt, no engram request, also with memory off (ruling 1).
- [ ] 8.7 Gate.

## PR9 / M7b: Save as note and sessions (`src/core/ai/chat/{mod,composer}.rs`, `src/ui/components/chat.rs`, `src/app.rs`)

Blocked by task 1.8 if the E-3 fallback is needed (user acceptance first).

- [ ] 9.1 RED: `Composer::with_cap(16_000)` rejects input past the cap; paste clips at the cap.
- [ ] 9.2 GREEN: `with_cap` in `composer.rs`.
- [ ] 9.3 RED: Ctrl+N opens the `NoteEditor`: seeded with the last answer (never reasoning) already redacted, kind `decision`, empty title, hint `N secrets redacted`; `Cut to 16000 characters` for a long source; `Nothing to save as a note` with no answer; `Memory is off; enable it in the menu (Ctrl+Space m)` with memory off; `Memory is not ready: <reason>` plus a health re-probe request when unavailable.
- [ ] 9.4 RED: editor keys (modal-input table): Left/Right cycle the kind, Tab/Shift+Tab move fields, Enter in the title saves, Enter in content is a newline, Ctrl+S saves, Esc discards with no request, everything else swallowed (no `WritePty`), Ctrl+C never quits, Repeat ignored on Ctrl+S and Esc.
- [ ] 9.5 RED: save flow: empty title or empty content refused naming the field; a second redaction pass that changes the text keeps the editor open with `Redacted again; review and save`, and only an unchanged text is sent; success closes with the system line `saved note <kind>: <title>` and costs no model call; failure keeps the edited text and shows the failure; same kind and title upsert silently with the same line; scope is always `project`; `StoreCmd::LinkNote` follows `Saved`; link failure shows `note saved but not linked` once.
- [ ] 9.6 RED: session lifecycle in App: first write emits register data via `Save{session: None}`, a `LinkSession` follows; a resume reuses the stored id (and, per M0, re-registers or falls back); a conversation that never wrote sends no End; pane close, new conversation and quit emit `End` once with `Notes: A; B` (at most 1,000 chars); reasoning (`secret plan`) never appears in any recorded traffic.
- [ ] 9.7 GREEN: `NoteEditor` view, `Entry` system lines, `Effect::Memory(Save/End)`, `StoreCmd::{LinkSession,LinkNote}` wiring, ui draw in `chat.rs`.
- [ ] 9.8 RED then GREEN: insta snapshots for the editor (normal, redacted hint, tiny pane no panic).
- [ ] 9.9 [manual, Kitty] Ctrl+N, Ctrl+S and Esc reach the app in Kitty (including inside the editor); if Ctrl+N or Ctrl+S is swallowed, stop and propose an alternative key to the user.
- [ ] 9.10 Gate.

## PR10 / M6: Context at open/resume (`src/core/ai/chat/{mod,engine,transcript}.rs`, `src/runtime/chat.rs` + adapters, `src/ui/components/{chat,statusline}.rs`)

Starts after ai-chat PR9 and PR13 are in the ai-chat tracker.

- [ ] 10.1 RED: `ChatRequest.memory: Option<String>`; server adapters (Ollama, OpenAI-compatible) append it to the system message after the profile prompt; CLI adapters write it first on stdin and never in argv; history cap becomes `cap - memory_chars` (10 messages of 1,200 chars, a 1,500-char block, 12,000 total).
- [ ] 10.2 GREEN: `ChatRequest` field, `build_context(history, cap - memory_chars)`, adapter changes in `src/runtime/chat.rs`.
- [ ] 10.3 RED: on open and on resume with memory ready the App emits `Resolve` then `Context`; exactly one context request per view, no search; a send never waits (turns before the reply carry no block, later turns do); failed or empty fetch adds no block; the block is never stored nor sent as history.
- [ ] 10.4 RED: `Entry::Memory`: folded `▸ memory (N notes)` / `▸ memory (1 note)` first in the transcript; Ctrl+O expands to `▾ memory (N notes)` plus the injected text behind `┊ ` in `text_muted`; second press folds; Ctrl+O with no memory line does nothing and emits no effect; not counted toward 400 messages, not yanked, not stored; resumed twice shows 2 then 4 notes.
- [ ] 10.5 RED: degradation lines `memory unavailable: <reason>` once per view for each reason (down, old, foreign, timeout, missing binary) and the line `memory needs a git project` for a non-git project; chat streams normally; no memory line and no traffic with memory off.
- [ ] 10.6 RED: offer hook: a Down state on the first conversation view of the run shows the M5 prompt once; `y` emits `StartServer`; other keys decline; memory keys while Down re-show it only if not yet used.
- [ ] 10.7 GREEN: transcript entry, Ctrl+O routing, context flow, offer view, statusline memory segment/hint (spec `statusline`), insta snapshots (folded, expanded, singular, none, tiny pane).
- [ ] 10.8 [manual, Kitty] Ctrl+O toggles the memory line in Kitty without clashing with Ctrl+R.
- [ ] 10.9 RED/GREEN: a pane waiting for its `Resolve`/`Context` reply gives up after 5 s on tokio's paused clock (the worker's `Busy` reply can be dropped when the reply channel is full, review W3 of M3b): the view stops waiting, shows `memory unavailable: engram did not answer` once, the chat streams, and a late reply is ignored; no wait without a bound.
- [ ] 10.10 Gate.

## PR11 / M8: Summary draft and approval (`src/core/ai/chat/{mod,engine}.rs`, `prompts/summary.md`, `src/ui/components/chat.rs`)

Fallback split if over 400 without exception: 11a request and draft view, 11b approval kept and sent at session end.

- [ ] 11.1 RED: Ctrl+E with memory ready and at least one answer issues one `ChatSend` for the `chat` task whose system is `prompts/summary.md` and whose history is message texts only (no reasoning, no memory block); refused while a stream runs; `Cannot summarize: <reason>` when the engine is not ready; cancel emits `ChatCancel` and leaves the pane idle; `ThinkingDelta`s are dropped.
- [ ] 11.2 RED: deltas fill a `SummaryDraft` view, not the transcript; result sanitized, cut to 4,000 chars, redacted (`password=hunter2` becomes `password=[REDACTED]`); title shown `Summary (not saved until you approve)`.
- [ ] 11.3 RED: approval (ruling 2): Ctrl+S runs the second redaction pass, and on success the summary is KEPT in chat state and nothing is sent; approving again replaces it; Esc discards and keeps nothing; the session End sent once at close carries the latest approved summary (else the note titles, else nothing without notes); no engram request occurs at approval time.
- [ ] 11.4 GREEN: `prompts/summary.md`, `SummaryDraft` view, kept-summary slot, End composition at close, draw code, insta snapshots.
- [ ] 11.5 [manual, Kitty] Ctrl+E and Ctrl+S in the summary view reach the app.
- [ ] 11.6 Gate.

## PR12 / M9: Delete offer and docs (`src/runtime/chat_store*`, `src/core/ai/chat/{store,mod}.rs`, `README.md`)

Starts after ai-chat PR10 (list/delete, `Effect::Store` wiring) is in the tracker.

- [ ] 12.1 RED: the store delete transaction returns the `(project, observation_id)` note rows before the cascade; rows go with the conversation; a store delete failure makes no note request.
- [ ] 12.2 GREEN: extend the C9 delete command/reply with the note rows and the note count for the prompt.
- [ ] 12.3 RED: `d` prompt: plain `Delete conversation? (y/n)` without notes; with notes, memory on and a token present `Delete conversation? y · n · a also delete its N notes` (`its 1 note`); `n`, Esc, `x`, `Y`, `A` cancel with no request; `y` deletes the conversation only; without a token or with memory off, the plain prompt then the line `its N notes stay in engram (set ENGRAM_HTTP_TOKEN to delete them)` or `(memory is off)`.
- [ ] 12.4 RED: `a`: `MemCmd::Delete` sends `DELETE /observations/{id}?expected_project=P` with the bearer token per notes; results `deleted N notes from engram`, `deleted K of N notes from engram` (204/404/403 mix gives 2 of 3 plus the refusal text), `could not delete notes: <reason>` when unreachable; the conversation stays deleted; eviction never sends a `DELETE`.
- [ ] 12.5 GREEN: App prompt state, `token_present`, `MemCmd::Delete`/`MemReply::Deleted` wiring.
- [ ] 12.6 README: memory privacy (open HTTP routes, shared db, sync), `ENGRAM_HTTP_TOKEN`, spawned server and its log, `chat/` topic-key prefix to find notes, chat.db v2 rollback note, keys Ctrl+N/Ctrl+O/Ctrl+E.
- [ ] 12.7 Gate.

## PR13 / V: Final manual verify (no code)

- [ ] 13.1 [manual] Live engram 3.4.0 end to end on a throwaway instance (temp `ENGRAM_DATA_DIR`, spare port set in config, never the user's db or `:7437`): enable memory and accept the warning, open a chat, save a note with a typed secret (redacted twice), save the same title again (revision 2), resume the conversation (memory line shows N), approve a summary, close, and find the session end and `chat/` note with `engram search`.
- [ ] 13.2 [manual] Offer path: stop the throwaway server, open a chat, confirm the prompt shows the absolute binary path, accept, quit lazarobox, and confirm the server still runs.
- [ ] 13.3 [manual] Delete path: with `ENGRAM_HTTP_TOKEN` set on the throwaway server, `d` then `a` removes the notes; without the token the stay-in-engram line shows.
- [ ] 13.4 [manual, Kitty] Ctrl+N, Ctrl+O, Ctrl+E and Ctrl+S (editor) all reach the app; an old 1.20 server on a spare port gives the restart notice and no offer.
- [ ] 13.5 Final Gate on the tracker `feat/engram-memory`: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`; kill any throwaway process.
