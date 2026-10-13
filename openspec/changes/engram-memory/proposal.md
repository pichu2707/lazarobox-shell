# Proposal: Engram Memory (the chat as an engram client)

## Intent

A lazarobox chat forgets everything when it ends. To continue a project, the user has to resume and reread a whole conversation, and nothing they learn in lazarobox reaches Claude Code or opencode. Those agents already share one project memory: the user's engram. This change makes lazarobox an engram **client**. The user saves the important parts of a chat (decisions, fixes, conventions) as categorized notes. The engine summarizes a conversation when it ends, and the user approves the summary. A new chat starts with a compact view of what the project already knows. Raw reasoning is never stored.

This change supersedes ai-chat **D15** ("Shared project memory is out of scope").

## Decisions (user, 2026-10-12, authoritative)

- **E1 Engram client over HTTP.**
  - lazarobox talks to `engram serve` (v3.4.0) on `127.0.0.1:7437`. The port can be changed in config.
  - It does not use MCP or the CLI (see the exploration's comparison).
  - lazarobox never re-implements engram's model in its own SQLite.
- **E2 Pure port, runtime worker.**
  - `core::ai::memory` holds the commands, replies, types, formatting and redaction, with no I/O.
  - A runtime engram worker talks to it over command and reply channels, like the chat store thread.
  - Every call has a bounded timeout. The render thread is never touched.
  - The port is shaped so that P3 tools (`mem_save`/`mem_search` called by the model) map onto it with no rework.
- **E3 Loopback client.** The hand-written loopback client gains `GET` and plain (non-streaming) JSON request and response bodies. It stays bound to 127.0.0.1.
- **E4 Opt-in.**
  - `[ai.memory] enabled = false` by default.
  - The first time it is enabled, a one-time privacy warning says that notes leave the 0600 chat store and become readable by every local process and agent that uses engram, and by engram sync if that is on.
  - Declining the warning leaves memory disabled. Accepting it is recorded, so the warning is not shown again.
- **E5 Project identity.**
  - The engram project comes from `GET /project/current?cwd=<pane cwd>`, and every request passes `project` explicitly.
  - The ai-chat project tag (D13) stays as the list and display tag, so memory never splits from it.
  - Memory works only for git projects: a `dir_basename` or `ambiguous` project (engram answers 200 with `project_source`, never 404/409) makes memory unavailable for that chat, with the line `memory needs a git project`. lazarobox never guesses a project.
- **E6 One engram session per conversation.**
  - The session id is `lazarobox-<32 lowercase hex>` (random, not the conversation row id). It is created and registered on the first write, because engram needs a session for every save, and it is stored in chat.db (E14) so a resumed conversation reuses it.
  - The session ends with a summary when the conversation closes. That summary is the approved summary (E9) when there is one, or otherwise `Notes: <titles>`.
- **E7 Closed category enum.**
  - A note's kind is one of decision, architecture, bugfix, discovery, pattern, config, preference. Engram does not validate types, so lazarobox enforces the list.
  - The topic_key is `chat/<kind>/<slug(title)>` (slug of at most 60 characters, `untitled` when empty) and the scope is always `project`.
  - Engram has no tags field, so the `chat/` prefix marks the notes that came from lazarobox.
- **E8 Slice 1: "save as note".** (amended by round 2, R1)
  - A chat key opens an editor. A chat pane has no selection, so the editor is prefilled with the redacted last answer and stands in for it. The user picks a kind, enters a title and edits the text.
  - It costs no model call.
  - The content never includes reasoning blocks.
- **E9 Slice 2: end-of-conversation summary.** (amended by ME4)
  - One extra engine call with a fixed prompt: "decisions and facts, no reasoning".
  - The proposal is shown in the editor, where the user can edit and approve it, or discard it. It is NEVER saved automatically.
  - The approved summary is saved only as the engram session end summary, not as an extra note.
- **E10 Context at chat open.** (amended by round 2, R3, and ME5)
  - `GET /context?project=P&scope=project&compact=true&observations=8&sessions=2&prompts=0&max_bytes=2000` (about 500 tokens).
  - The result is sanitized, wrapped in a `<project-memory>` block that marks it as untrusted data, capped, and placed in the system context (servers) or on stdin (CLIs, never argv). Its characters are subtracted from the history cap.
  - It is shown as a folded `▸ memory (N notes)` line and fetched again when a conversation is resumed.
  - Details are fetched on demand only. There is no automatic search per turn.
- **E11 Redaction.** Engram's own `<private>` stripping still applies. On top of it, lazarobox runs a secret-redaction pass (key and token prefixes, bearer headers, PEM blocks, `password=`/`token=` assignments) before anything is sent. The user sees the redacted text before saving.
- **E12 When engram is down, lazarobox OFFERS to start `engram serve` (the user's choice).**
  - **Confirmation.** The user must confirm before anything is spawned. The offer is made at most once per session.
  - **Spawn.** The binary is resolved on PATH to an absolute path and run with no shell, detached (`setsid`), with `ENGRAM_DATA_DIR` left unset so it uses the user's `~/.engram`, and with output going to a log under the XDG state dir.
  - **Port race.** lazarobox probes `/health` again right before spawning. If the spawn fails to bind, it probes again and adopts a server that answers. It never spawns twice.
  - **Version check.**
    - A `/health` reply without `instance_id`/`capabilities` means an old 1.x server. lazarobox reports "old engram server, restart it with 3.x" and makes no offer, because the port is taken.
    - A non-engram reply means memory is unavailable.
  - **Lifecycle.** lazarobox never kills a server it did not start. It also leaves the server it did start running when it quits, because other agents may now depend on that server. It shows the PID so the user can stop it.
- **E13 Degradation.** When memory is unavailable, a statusline or chat hint says so and the chat works normally. Memory errors never block a reply.
- **E14 chat.db v2 link tables (ME1).** `memory_sessions` and `memory_notes` in chat.db link a conversation to its engram session and notes (engram has no route that lists observations by session). The migration is additive; a pre-v2 build reports history unavailable. This supersedes the earlier "chat.db is untouched".
- **E15 Deleting notes (ME2, round 2 R2).** A USER delete of a conversation offers to delete its notes too, only when memory is on, notes are linked and `ENGRAM_HTTP_TOKEN` is set; otherwise one line says why the notes stay. Retention eviction never prompts and never deletes: it reports how many notes remain.

## Round 2 decisions (user, 2026-10-12, binding; engram #3015)

| # | Decision | Where it lands |
|---|---|---|
| R1 | The note content is editable in a small editor before saving. It opens with the text already redacted, and the redaction pass runs again on save (a changed result keeps the editor open for review). | E8, E11; chat-memory "Save as note" |
| R2 | Deleting a conversation offers to delete its notes (needs `ENGRAM_HTTP_TOKEN`; without it no offer and a one-line reason). Eviction notifies only. Notes are linked to conversations through a local mapping in chat.db (conversation to engram observation ids), not through engram session ids, because engram cannot list by session. | E14, E15; chat-history |
| R3 | Injected memory is a folded `▸ memory (N notes)` transcript line, expanded with Ctrl+O (a separate toggle in the style of Ctrl+R, which stays on the reasoning block), and injected again on resume. | E10; chat-memory, modal-input |
| R4 | The same kind and title upsert SILENTLY through `chat/<kind>/<slug(title)>`. Slug: lowercase, Unicode letters and digits kept, every other run becomes one `-`, trimmed, at most 60 characters, `untitled` when empty. | E7; chat-memory |
| R5 | Notes are always `project` scope. | E7 |

## Design rulings folded in (design #3016)

ME1 chat.db v2 is in scope. ME2 note deletion is in scope for a user delete with a token. ME3 there is no selection in a chat pane; the redacted, editable last answer stands in. ME4 the approved summary is the session end summary only. ME5 CLI engines get the memory block on stdin, never in argv. Keys (all to be checked in Kitty): Ctrl+Space n save note (Ctrl+N is captured outside the terminal), Ctrl+O toggle memory block, Ctrl+E summary draft. Config: `[ai.memory] enabled`, `port` (default `ENGRAM_PORT` if valid, else 7437), `acknowledged`.

## Scope

### In Scope
- The memory port, types, closed enum, slug, redaction and context formatter (pure).
- GET and JSON support in the loopback client.
- The engram adapter and worker: health and version, project resolution, sessions, saving observations, context.
- The `[ai.memory]` config keys, a menu row, and the one-time privacy warning.
- The offer to start `engram serve` (E12).
- Context injection at chat open.
- The "save as note" key and modal.
- The end-of-conversation summary with approval (slice 2).
- A README section on memory privacy.
- chat.db v2: the link tables for sessions and notes (E14), and the eviction note count.
- Deleting a conversation's notes on a user delete when `ENGRAM_HTTP_TOKEN` is set (E15).
- The memory fold line in the transcript and the editors (note, summary).

### Out of Scope
- P3 tools: the model calling memory itself. Only the port is made ready for them.
- Automatic search per turn, and automatic saving of any kind.
- Editing or browsing notes from lazarobox, and deleting notes any other way than a user delete of their conversation (use engram's own tools).
- MCP or CLI transports, and unix-socket transport.
- Encrypting or authenticating engram's HTTP API (engram's responsibility, documented).
- Storing note text in chat.db: the link tables hold ids, topic keys and titles only. It stays the raw 0600 conversation store.

## Capabilities

The ai-chat capabilities are still in flight (`openspec/changes/ai-chat/specs/`). The deltas below apply on top of them.

### New Capabilities
- `engram-client`: the loopback HTTP adapter and worker, timeouts, health and version detection, project resolution, the session lifecycle, saving observations, fetching context, and the start-server offer with its port race and detached lifecycle.
- `chat-memory`: opt-in and the privacy warning, the category enum and the topic_key scheme, the save-as-note flow, the redaction pass, context injection (untrusted block, cap), summary approval, and the degradation hints.

### Modified Capabilities
- `chat-pane`: the save-note key and modal, the summary approval view, and the memory-unavailable hint.
- `chat-engines`: the system context gains the memory block, there is a summary request, and the loopback client gains GET and JSON bodies.
- `ai-engine-detection`: the shared loopback client rule now covers GET, POST and DELETE JSON exchanges with the engram port, still on 127.0.0.1 only, with the bearer token on a confirmed DELETE as the single credential exception.
- `chat-history`: schema v2 link tables, the eviction note count and second line, and the three-way delete prompt (`y`, `n`, `a` also delete its notes).
- `modal-input`: the bindings for save-note and summary.
- `statusline`: the memory state segment or hint.
- `configuration`, `config-persistence`, `config-menu`: the `[ai.memory]` keys (`enabled`, `port`, the acknowledgement flag), the owned keys, and the Memory row.

## Approach

The chat state machine sends `Effect::Memory(cmd)`. The engram worker answers with `AppEvent::Memory(reply)`, the same template as the store thread. The health result is cached per session and probed again on failure. All formatting and redaction stay pure and are tested in core. The adapter is thin and is tested against recorded HTTP fixtures.

## Affected Areas

| Area | Impact |
|---|---|
| `src/core/ai/memory/` (port, enum, slug, redaction, context formatter) | New |
| `src/core/ai/http.rs`, `src/runtime/loopback.rs` (GET, JSON) | Modified |
| `src/runtime/engram/` (worker, adapter, serve offer) | New |
| `src/core/ai/chat/*`, `src/runtime/chat.rs`, `src/ui/components/chat.rs` | Modified |
| `src/app.rs`, `src/runtime.rs` (Effects, AppEvents, worker wiring) | Modified |
| `src/core/ai/{config,menu}.rs`, `src/core/menu.rs` | Modified |
| `src/runtime/chat_store*`, `src/core/ai/chat/store.rs` (schema v2, link tables, eviction count, delete rows) | Modified |
| `README.md` | Modified |

## Dependencies and Ordering

- PR4 (loopback client) has landed. M1 to M5 can start now.
- M6 and M7 need **ai-chat PR9** (the live chat pane).
- M6 and M8 need **ai-chat PR13** (the Ollama adapter), so that the injected context and the summary call have a real engine behind them.
- The user should restart engram with 3.4.0. The running server on :7437 is still 1.20, and E12 reports this but does not fix it.

## Delivery Slices

Feature-branch chain on `feat/engram-memory`, auto-chain, strict TDD, about 400 lines per PR, about 3,000 lines in total.

| # | Content | Size |
|---|---|---|
| M1 | Core port: enum, slug/topic_key, redaction, context formatter | ~400 |
| M2 | Loopback GET and JSON bodies | ~250 |
| M3 | Engram adapter and worker: health/version, project, sessions, save, context | ~400 |
| M4 | Config keys, menu row, one-time privacy warning | ~350 |
| M5 | `engram serve` offer: confirm, detached spawn, port race | ~350 |
| M6 | Context injection at chat open (after PR9 and PR13) | ~300 |
| M7 | Save-as-note key, modal, session register/end (after PR9) | ~400 |
| M8 | **Slice 2:** end-of-conversation summary and approval (after PR13) | ~400 |
| M9 | README privacy section, docs | ~150 |

## Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| A poisoned note steers the model (prompt injection) | Med | Sanitize the context, wrap it in an untrusted-data block, cap its bytes, and never treat it as instructions |
| Secrets leak into the shared engram db | Med | The redaction pass plus `<private>`, a preview before saving, and the opt-in warning |
| Any local process can read memory (open HTTP routes) | High (by design) | Documented in the warning and the README. lazarobox cannot fix it |
| An old 1.20 server, or one that is not engram, is on the port | High (today) | The `/health` capability check, a clear notice, and no spawn |
| A spawned server is orphaned or races another server | Med | Re-probe before spawning, adopt on bind failure, detached and never killed, and the PID is shown |
| The engram HTTP API drifts (it has no version number) | Med | Gate on `capabilities`, use recorded fixtures, and degrade instead of failing |
| A project mismatch splits memory | Low | `/project/current` is the single source, and a non-git `project_source` makes memory unavailable |
| The chat.db v2 migration makes an older build report history unavailable | Med | Additive migration, file untouched by the old build, documented in the README and the rollback plan |
| The ai-chat state shapes change underneath this change (indicator work happening at the same time) | Med | UI hooks (M6 to M8) land only after PR9 and PR13 have merged |

## Rollback Plan

Revert the merge of the tracker `feat/engram-memory`. The previous version ignores the `[ai.memory]` keys. Notes that were already saved stay in engram, and the user can find and delete them by the `chat/` topic_key prefix with engram's tools. A server that lazarobox started keeps running until the user stops it. `chat.db` has been migrated to v2 (additive): a build without this change treats a v2 database as newer and reports history unavailable, leaving the file untouched, until the user upgrades again or restores a v1 backup.

## Success Criteria

- [ ] `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check` pass on every slice.
- [ ] With memory disabled (the default), the app makes no request to engram.
- [ ] Enabling memory shows the privacy warning once. Declining it keeps memory off.
- [ ] Saving a note sends `type` from the enum, `topic_key` `chat/<kind>/<slug>`, the project from `/project/current`, and redacted content. Saving the same kind and title again upserts the note.
- [ ] A new chat's system context holds at most about 2,000 bytes of delimited, sanitized memory.
- [ ] A 1.x server is reported and never replaced. When engram is down, a server starts only after confirmation, and it survives lazarobox quitting.
- [ ] A summary is saved only after the user approves it. Reasoning text never reaches engram.
- [ ] When engram is unreachable, the chat still streams replies and a hint is shown.
- [ ] The editor opens redacted and redaction runs again on save. The same kind and title upsert with no warning.
- [ ] A user delete offers to delete the conversation's notes only with a token. Eviction never prompts and reports how many notes remain.
- [ ] The injected memory shows as a folded line, expands with Ctrl+O and is fetched again on resume.

## Proposal question round

Resolved: round 1 (engram `sdd/engram-memory/question-round-1`) is recorded above as E4, E7, E8, E9 and E12. The user chose E12 (the offer to start the server) over the exploration's recommendation to only show guidance. Round 2 (engram `sdd/engram-memory/question-round-2`) is recorded in "Round 2 decisions": the user chose the editable note, the delete offer with a token, the visible fold line and the silent upsert. The scope question (`project` only) was assumed, not asked.

Assumptions that need user review: there is no project override in the first slice; the session is registered on the first write, not at chat open; and the default `~/.engram` data dir is shared with the other agents.
