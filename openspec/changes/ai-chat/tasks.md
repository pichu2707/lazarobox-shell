# Tasks: AI Chat (P2 chat pane, all engines, topic memory)

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | ~6,000 total, 19 code PRs of 150-450 lines each, plus 1 no-code manual PR |
| 400-line budget risk | High (total); Medium per PR; High for PR8 (C7), PR10 (C9), PR15 (C13a) |
| Chained PRs recommended | Yes |
| Suggested split | PR1 → PR2 → ... → PR19 → PR20 (manual) (see work units) |
| Delivery strategy | auto-chain |
| Chain strategy | feature-branch-chain (tracker `feat/ai-chat`, from main d27daea) |

Decision needed before apply: No
Chained PRs recommended: Yes
Chain strategy: feature-branch-chain
400-line budget risk: High

Rules for every PR:
- Strict TDD: RED with compiling stubs and exact assertions, then GREEN, then refactor. Tests are never trimmed.
- Gate = `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.
- One work-unit commit per task group (conventional commits, no AI attribution).
- Re-measure with `git diff --shortstat` before opening each PR; if above 400 (additions + deletions) use the fallback split noted.
- PR1 base = `feat/ai-chat`; PR(n) base = PR(n-1) branch. Only the tracker merges to main.
- Pre-planned splits: C6 (6a/6b) and C10 (10a/10b) are already split because their design estimates are over 400.

### Suggested Work Units

| Unit | Goal | PR | Est. | Depends on | Order |
|------|------|----|------|-----------|-------|
| C1 | Pane-kind seam (no behaviour change) | PR1 | ~350 | - | fixed |
| C2 | Chat core: state, contract, availability, history cap | PR2 | ~400 | C1 | fixed |
| C3 | `Utf8Stream`, sanitizer, NDJSON/SSE splitters | PR3 | ~400 | - | reorderable with C4 |
| C4 | `parse_head`, `Dechunker`, `post_stream`, timeouts | PR4 | ~400 | - | reorderable with C3 |
| C5 | Runtime chat task, Effects/Events, cancel, `GroupKill` move | PR5 | ~350 | C2, C3, C4 | fixed |
| C6a | SQLite open: path, modes, pragmas, schema | PR6 | ~250 | C2 | reorderable after C2 |
| C6b | Store thread, retention, degrade | PR7 | ~250 | C6a | reorderable after C6a |
| C7 | UI: composer, transcript, code blocks, widget | PR8 | ~400 | C1, C2 | reorderable after C2 |
| C8 | Key routing, guidance, yank, `c` binding, statusline (variant goes live) | PR9 | ~350 | C5, C7 | fixed |
| C9 | List, tags, resume, delete, editor (fallback C9a list/resume/delete, C9b tags/editor) | PR10 | ~400 | C6b, C8 | fixed |
| C10a | `Profile` config, menu row, persistence | PR11 | ~300 | C2 | reorderable after C2 |
| C10b | 10 built-in prompts, overrides, `resolve_prompt` | PR12 | ~250 | C10a | reorderable after C10a |
| C11 | Ollama adapter + live wiring | PR13 | ~300 | C5, C8, C10b | fixed |
| C12 | OpenAI-compatible adapter | PR14 | ~300 | C11 | fixed |
| C13a | Landlock jail: spec, probe, `__jail-exec`, `main.rs`, `tests/jail.rs` (fallback C13a-1 pure spec/paths, C13a-2 helper + test) | PR15 | ~350 | C5 | fixed |
| C13 | claude adapter + CLI process runtime (Layered) | PR16 | ~350 | C13a, C8 | fixed |
| C14 | codex adapter (Required, Linux-only) | PR17 | ~250 | C13 | fixed |
| C15 | gemini adapter (Required, Linux-only) | PR18 | ~250 | C13 | fixed (C14/C15 reorderable) |
| C16 | README / docs | PR19 | ~150 | C15 | fixed |
| V | Final manual verify, real machine | PR20 | 0 | PR19 | no code |

Reconciliation edits are applied by this phase, not by an apply PR (see section below).

## Reconciliation applied

User rulings R1-R29 (design.md "Spec reconciliation") were applied in place during the tasks phase.

Files changed:
- `openspec/changes/ai-chat/design.md`: header (rev 3), CHAT-5 (journal_mode DELETE, side files), CHAT-11 (execute per file, R29), testing strategy (jail test 5, store modes), M14, table rows R14 (overridden) and R29, C13a/C14/C15 notes.
- `specs/chat-engines/spec.md`: R2 per-engine caps, R4 2 MiB CLI line cap, R5 `Slow` at 3 s, R23 no-jail state, R24 stdin channel, R25 CLI and total timeouts, R26 "never opened for reading", new CLI jail requirement with R29 scenarios (execve EACCES [auto, tests/jail.rs]; ssh/curl [manual]), M14 manual scenario.
- `specs/chat-history/spec.md`: R14 DELETE (side files 0600, no WAL, no checkpoint), R13 worktree scenario, R19 open conversation skipped by retention, R21 status enum.
- `specs/chat-pane/spec.md`: R7 Ctrl+B yank, R23 no-jail and gated-off guidance.
- `specs/modal-input/spec.md`: R7 Ctrl+B in the key table, Repeat rules, scenario.
- `specs/prompt-profiles/spec.md`, `specs/configuration/spec.md`, `specs/config-menu/spec.md`, `specs/config-persistence/spec.md`: R11 default `programmer`, R27 `cannot be read`.

| Ruling | Result |
|--------|--------|
| R1, R5, R6, R8, R9, R10, R12, R15-R18, R20, R22 | Spec already matched the ruling; no spec edit (design text was the outlier) |
| R2, R4, R25 | Spec edited (chat-engines) |
| R3 | Spec matched (1 MiB, `truncated` status now explicit in chat-history) |
| R7 | Spec edited (chat-pane, modal-input) |
| R11 | Spec edited (prompt-profiles, configuration, config-menu, config-persistence) |
| R13, R19, R21 | Spec edited (chat-history) |
| R14 | OVERRIDDEN by the user: journal_mode DELETE, not WAL; spec and CHAT-5 edited; no `wal_checkpoint` task exists |
| R23, R24, R26, R28 | Spec edited (chat-engines, chat-pane) |
| R27 | Spec edited (prompt-profiles) |
| R29 | NEW: CHAT-11 and the allow-list amended; chat-engines scenarios added; manual M14 added |

## PR1 / C1: Pane-kind seam (`src/app.rs`)

- [x] 1.1 RED: seam tests with a `#[cfg(test)]` chat-pane constructor: chat gets no `SpawnPane`, `ResizePty` or `ClosePane`; `Pty`/`Cwd` events for a chat id are dropped; cwd poll skips chats; existing tests and snapshots unchanged.
- [x] 1.2 RED: `Prefix [` on a chat is refused with the exact notice (R6), vt100 untouched; second Ctrl+Space swallowed; chat keys and paste never emit `WritePty`; Repeat rules for a chat.
- [x] 1.3 RED: tab label and inherited cwd (split, new tab) come from the chat's `origin_cwd`; vanished origin cwd falls back; ids shared and never reused; last pane being a chat.
- [x] 1.4 GREEN: `PaneState { content: PaneContent::{Pty(PtyPane), Chat(Box<ChatState>)}, size }`, accessors, `NewPane::{Shell, Chat}`, `relayout`, `remove_pane`, `drop_tab`, `App::pane_view`; `Chat` variant carries `cfg_attr(not(test), expect(dead_code))`.
- [x] 1.5 Gate.

## PR2 / C2: Chat core (`src/core/ai/chat/{mod,engine}.rs`, `src/core/ai/jail.rs` stub)

- [x] 2.1 RED: `ChatState` lifecycle: send appends user message and emits `ChatSend`; blank, whitespace and send-while-streaming refused with composer kept; waiting text `waiting for the model…` then `loading model…` on `Slow`.
- [x] 2.2 RED: stream events: deltas in order, final, error keeps partial, error before text leaves no empty message, cut stream `stream interrupted`, late events dropped by turn, cancel keeps partial as `cancelled`, cancel idle is a no-op, 1 MiB answer cap stores `truncated`.
- [x] 2.3 RED: `Msg` status enum (`complete|cancelled|interrupted|truncated|error`, partial means not `complete`); system lines never in history; 400 messages per conversation.
- [x] 2.4 RED: history cap (R2): 40 messages, 12,000 chars servers and 100,000 chars CLIs, newest always whole, system prompt pinned, omitted flag and the marker line once.
- [x] 2.5 RED: `availability(&AiConfig, Option<&Detection>, JailSupport)`: ready, unassigned, `checking engines…`, undetected, unsupported (opencode), NoJail, GatedOff, NoModel, ModelMissing, `:latest` equivalence, fallback to general, never switches.
- [x] 2.6 GREEN: implement `ChatState`, `StreamEvent`, `ChatError`, `ChatRequest`, `Effect::ChatSend/ChatCancel` variants, `JailSupport` type.
- [x] 2.7 Gate.

## PR3 / C3: Sanitizer and splitters (`src/core/ai/sanitize.rs`, `src/core/ai/chat/stream.rs`)

- [x] 3.1 RED: `StreamSanitizer` tables: control characters, CSI/OSC/DCS/APC/PM/SOS, bidi and format characters, unterminated at finish, 4,096-char cap, every split point.
- [x] 3.2 RED: `Utf8Stream`: 3- and 4-byte splits, invalid bytes to U+FFFD, cut tail at finish; fuzz invariant (random bytes, random cuts, never ESC or C0/C1).
- [x] 3.3 RED: `NdjsonSplitter` and `SseParser`: Ollama NDJSON, OpenAI SSE with `[DONE]`, comments, CRLF, every split point, malformed line, server `error` object sanitized and capped, 1 MiB line cap (2 MiB for CLI decoders).
- [x] 3.4 GREEN: implement; move `is_format` into `sanitize.rs`; `parse.rs` uses `clean()` with identical output (existing parse tests unchanged).
- [x] 3.5 Gate.

## PR4 / C4: Loopback HTTP client (`src/core/ai/http.rs`, `src/runtime/loopback.rs`)

- [x] 4.1 RED: `parse_head` keeps detection's strict framing (200 only, conflicting or signed Content-Length, CL with TE, malformed chunking); incremental `Dechunker` at every split; head cap 16 KiB.
- [x] 4.2 RED: `post_stream` against `fake_server`: POST HTTP/1.1 headers (no `Authorization`/`Cookie`), foreign host refused with no connection, redirect not followed, non-200 gives status plus a message of at most 200 chars, streaming is incremental.
- [x] 4.3 RED: timeouts with injected time: connect refused at once, first byte slower than idle is fine, idle 60 s stalled, 15-min total, `Slow` at 3 s (none at 2.9 s); cancel before first byte closes the socket.
- [x] 4.4 RED: detection stays GET-only and never runs during a chat; a long chat stream is not capped by the detection 1 MiB framing cap.
- [x] 4.5 GREEN: implement; detection call sites reuse `parse_head`/`Dechunker`.
- [x] 4.6 Gate.

## PR5 / C5: Runtime chat task (`src/runtime/{chat,proc}.rs`, `src/runtime.rs`, `src/app.rs`, `src/runtime/ai_detect.rs`)

- [ ] 5.1 RED: `AppEvent::Chat(pane, turn, ev)` routing with a fake adapter: events in order, error terminal, cancel stops events, two chats stream independently, stream continues in a background tab, during COPY, across zoom and focus.
- [ ] 5.2 RED: closing a chat pane or tab emits `ChatCancel` (never `ClosePane`) and persists nothing yet; stale events after close or tab close are dropped; quit closes sockets and kills CLI children; closing does not block.
- [ ] 5.3 RED: runtime tests: cancel closes the socket within 1 s; a fake process group that ignores SIGTERM is killed and reaped within 1 s; answer cap stops at 1 MiB.
- [ ] 5.4 GREEN: `Effect::ChatSend/ChatCancel` handling, one tokio task per pane, `GroupKill` moved to `runtime/proc.rs`, `ai_detect.rs` updated.
- [ ] 5.5 Gate.

## PR6 / C6a: SQLite open (`Cargo.toml`, `src/runtime/chat_store.rs`)

- [ ] 6.1 Pin `rusqlite` with `bundled`; record the clean-build cost in the commit body.
- [ ] 6.2 RED: path resolution (XDG absolute, empty or relative falls back to `$HOME/.local/share`, neither means no store); directory 0700; db pre-created 0600 with O_NOFOLLOW under umask 0022; any side file that exists (`-journal`, `-wal`, `-shm`) is 0600; wide mode corrected; symlink or foreign owner is unavailable.
- [ ] 6.3 RED: pragmas `secure_delete` on, `foreign_keys`, `trusted_schema` off, `busy_timeout=2000`, `journal_mode` reads `delete`; no `wal_checkpoint` anywhere; marker gone from the db and any side file after delete.
- [ ] 6.4 RED: schema v1 (`conversations`, `messages` with status enum and `UNIQUE(conv,seq)`, `tags`, indexes), `user_version` migration, newer version means unavailable.
- [ ] 6.5 GREEN: implement `open`, mode checks, migrations.
- [ ] 6.6 Gate.

## PR7 / C6b: Store thread (`src/runtime/chat_store.rs`, `src/runtime.rs`, `src/app.rs`)

- [ ] 7.1 RED: `StoreCmd`/`AppEvent::Store` FIFO over one thread: create on first send, append user, append assistant with status (complete, cancelled, interrupted, truncated), errors not stored, stored text sanitized.
- [ ] 7.2 RED: retention at 200 in the create transaction: oldest by last activity evicted, activity keeps alive, tagged still evicted, over-limit trimmed, open conversations skipped, system line `oldest conversation removed (limit 200)` only on eviction.
- [ ] 7.3 RED: degrade: corrupt file bytes unchanged, cannot open, no path, mid-session write failure shows `could not save conversation` once, error text sanitized and at most 120 chars; slow store (2 s fake) does not delay keys or frames; quit flushes Cancel and Append within the 2 s join.
- [ ] 7.4 GREEN: implement thread, commands, pane-side queue for appends that precede `Created`.
- [ ] 7.5 Gate.

## PR8 / C7: Composer, transcript, widget (`src/core/ai/chat/{composer,transcript}.rs`, `src/ui/components/chat.rs`, `src/ui/mod.rs`, `Cargo.toml`)

- [ ] 8.0 Carry-over from PR1 review (W2): the composer must own the cursor of a focused chat (position and shape); until C7 a focused chat leaves the cursor at the default.
- [ ] 8.1 RED: composer: type, Backspace, Delete, Left/Right, Home/End, Ctrl+U, newline, paste with CRLF and control stripping, 8,000-char cap, 5-row height cap.
- [ ] 8.2 RED: transcript wrap with `unicode-width` cached by (width, generation); fenced code styled, unclosed fence is code, no other markdown; follow at bottom, pinned when scrolled, send returns to bottom, clamp.
- [ ] 8.3 RED: insta snapshots: transcript above composer, composer growth capped, code block, literal markdown, long line wrap; no panic from 1x1 to 10x2 with and without a stream.
- [ ] 8.4 GREEN: implement; add `unicode-width`; `PaneView` dispatch in `ui/mod.rs`; spans only, never raw bytes.
- [ ] 8.5 Gate. Re-measure: if above 400, split composer (8a) from transcript and widget (8b).

## PR9 / C8: Live chat pane (`src/app.rs`, `src/core/prefix.rs`, `src/ui/components/{chat,statusline}.rs`)

- [ ] 9.1 RED: `c` leaf after `m` and before `q` (`chat`), root hint `… m menu · c chat · q quit`; `c` opens a right split like `w v` (refusal when too small), mode TERMINAL, no `SpawnPane`; `c` inside a group or other modes does nothing; several chats; open requests exactly one detection run (none if in flight).
- [ ] 9.2 RED: chat key table: Enter, Alt+Enter, Ctrl+J, Ctrl+U, Esc/Ctrl+C cancel or nothing (never Quit, never `WritePty`), Ctrl+Y/Ctrl+B yank (notices `Copied N characters`, `Nothing to copy`, `Selection too large to copy`), PageUp/PageDown, unlisted keys swallowed, Repeat rules, paste in MENU and mode changes keep the composer.
- [ ] 9.3 RED: guidance states and exact texts (unassigned, undetected, unsupported, no jail `<engine>: not available on this system yet`, gated off, model not downloaded, `checking engines…`), fix without reopening, re-resolved on every send.
- [ ] 9.4 RED: statusline: `CHAT` label, origin cwd, `engine › model` or `no engine`, falls back to general, narrow width, other modes keep their label.
- [ ] 9.5 GREEN: implement; remove the `dead_code` expectation on `Chat`; `Prefix c` creates `NewPane::Chat`.
- [ ] 9.6 [manual] M8: Ctrl+J, Alt+Enter, Ctrl+Y, Ctrl+B reach the app in Kitty (Ctrl+T re-checked in PR10).
- [ ] 9.7 Gate. Re-measure: if above 400, split keys (9a) from guidance/statusline (9b).

## PR10 / C9: List, tags, resume, delete (`src/core/ai/chat/{list,tags}.rs`, `src/runtime/project.rs`, `src/app.rs`)

- [ ] 10.1 RED: repo-root walk (injected `repo_root`): `.git` dir, worktree file without gitdir gives `wt`, worktree whose gitdir contains `/.git/worktrees/` gives the main repo (R13), 64 levels, not a repo, unknown cwd, cleaning (trim, control and format, 64 chars), no git process.
- [ ] 10.2 RED: list view: filtered by project, newest first, rows `YYYY-MM-DD · profile · first message…`, `a` toggle, empty texts, no project lists all, long message cut with `…`, cursor clamps, `n`/Esc new, insta snapshots.
- [ ] 10.3 RED: resume: full history with partial marks, appends to the same conversation, current engine and stored profile used, already open elsewhere refused with a system line.
- [ ] 10.4 RED: delete: `d` then only lowercase `y` confirms, other keys cancel, deleted while open elsewhere stops persisting with its system line, delete failure line.
- [ ] 10.5 RED: tag editor (`t` on a row, Ctrl+T in a conversation): comma line, project tag first, 8 extras, duplicates case-insensitive, discard on Esc, saved at once with last activity and list filter.
- [ ] 10.6 GREEN: implement views in `ChatState`, `Effect::Store` wiring.
- [ ] 10.7 [manual] M8 tail: Ctrl+T reaches the app in Kitty.
- [ ] 10.8 Gate. Fallback if above 400: PR10a (10.2-10.4) then PR10b (10.1, 10.5).

## PR11 / C10a: Profile config and menu (`src/core/ai/{config,menu}.rs`, `src/core/{menu,config_edit}.rs`)

- [ ] 11.1 RED: `Profile { Analyst, Programmer }`, default `programmer` (absent, empty, unknown), exact notices for non-string and unknown values, counted with other problems, `ai = 3`.
- [ ] 11.2 RED: `Profile` row after the six task rows: descriptor integrity, navigation wrap (`Profile` last), cycle with `h/l`/Space/Enter, Repeat of Enter ignored, footer, `r` does nothing, no side effects, Esc reverts, `s` saves.
- [ ] 11.3 RED: persistence: profile created, updated in place, written explicitly even when default, untouched when unchanged, no engine keys written, `[ai.profile]` table refused with exact footer, scalar replaced, collision ignored when unchanged, `ai` scalar.
- [ ] 11.4 GREEN: implement parse, menu row, `edits()`, `config_edit` owned key `ai.profile`; Profile row render snapshots.
- [ ] 11.5 Gate.

## PR12 / C10b: Prompts (`src/core/ai/chat/prompts/**`, `src/core/ai/chat/prompts.rs`, `src/runtime/chat.rs`)

- [ ] 12.1 RED: built-in table integrity (10 prompts, non-empty, at most 4 KiB, profiles differ per task), reply-in-user-language text, chat prompt states no files, shell or terminal.
- [ ] 12.2 RED: pure `resolve_prompt`: task override, then general override, then built-in; other profile ignored; exact content after trim; resolved once per conversation; profile switch applies to new conversations only; resumed uses the stored profile.
- [ ] 12.3 RED: bounded override read (at most 16,385 bytes, regular files only, no FIFO block, `/dev/zero` symlink): notices `not valid UTF-8`, `larger than 16 KiB`, `cannot be read`, `not a regular file`; at-cap file used; whitespace-only is missing; sanitized path.
- [ ] 12.4 GREEN: write the 10 English `.md` files via `include_str!`, `resolve_prompt`, runtime reader.
- [ ] 12.5 Gate. Prompt text counts toward the 400 lines; re-measure.

## PR13 / C11: Ollama (`src/core/ai/chat/adapters.rs`, `src/runtime/chat.rs`)

- [ ] 13.1 RED: `Adapter::Ollama` builds the exact JSON body (model, system prompt, history, stream flag, nothing else) for `/api/chat`; NDJSON decoder events; traffic audit on the recording fake server (POST on `127.0.0.1`, no credential headers).
- [ ] 13.2 RED: end-to-end turns with a fake server: ready server, `latest` equivalence, saved change applies to the next message, error object, malformed line, oversized line, non-200 short error, answer cap, ambiguous framing, redirect, chat never pulls a model.
- [ ] 13.3 GREEN: `adapter_for`, `Invocation::Http` execution through `post_stream`, system prompt from `resolve_prompt`.
- [ ] 13.4 [manual] M7: live Ollama gives a 404 for an unknown model and shows a cold load as `waiting for the model…` then `loading model…` with no timeout; reply streams and cancel stops it.
- [ ] 13.5 Gate.

## PR14 / C12: OpenAI-compatible (`adapters.rs`, `runtime/chat.rs`)

- [ ] 14.1 RED: lmstudio, llamacpp and vllm bodies and paths (`/v1/chat/completions`), SSE decoder deltas, finish marker and `[DONE]` exactly one final, traffic audit per catalog port.
- [ ] 14.2 GREEN: implement; `opencode` still unsupported (adapter table scenario: seven adapters, none for opencode).
- [ ] 14.3 [manual] M7: a real lmstudio, llama.cpp or vLLM streams, the `[DONE]` ends the turn, cancel stops it.
- [ ] 14.4 Gate.

## PR15 / C13a: Landlock jail (`src/core/ai/jail.rs`, `src/runtime/jail.rs`, `src/main.rs`, `tests/jail.rs`, `Cargo.toml`)

- [ ] 15.1 [manual] M13: check the kernel ABI (expect 6 or more on 6.19) and the `landlock` crate API (`RulesetStatus`, `Scope`); pin the exact crate version in `Cargo.toml` (Linux target only).
- [ ] 15.2 RED: pure `jail_spec` for an nvm node layout, a native claude layout and a `CODEX_HOME` override; package root and shebang parsing (256 bytes, `env node` resolved); `/usr`, `/bin`, `/lib`, `/lib64` read-only and never in `rx` (R29); `rx` is exactly the interpreter file, the package root and proven helpers; no path under `$HOME` except the CLI's own package and state; never `~/.ssh`, `$HOME` or `/tmp`.
- [ ] 15.3 RED: probe mapping (ABI 2 gives Unavailable, 6 gives Available); policy `Required`/`Layered`; exit 125 becomes `ChatError::Jail`; codex without a jail spawns nothing; claude runs bare when Unavailable.
- [ ] 15.4 RED: `tests/jail.rs` (skipped with a message when the probe is Unavailable): (1) outside read fails with EACCES, (2) allowed read prints, (3) `--ro` write fails and `--rw` succeeds, (4) `--min-abi 99` exits 125 and never execs, (5) execve of `/usr/bin/ssh`, `/usr/bin/curl` and `/bin/sh` fails with EACCES while one exact granted helper runs and its shared libraries load.
- [ ] 15.5 GREEN: sync `main` dispatching `__jail-exec` before the tokio runtime; helper builds the ruleset (all FS rights of the probed ABI as hard requirements, `Scope` on ABI 6 or more), requires `FullyEnforced` and `no_new_privs`, then `exec`s.
- [ ] 15.6 [manual] M12 baseline: strace a jailed node script to confirm the common allow-list (`/proc`, `/sys`, certs, locale); per-CLI repeats are in PR16-PR18.
- [ ] 15.7 Gate. Fallback if above 400: PR15a (15.2, 15.3 pure spec and probe) then PR15b (15.4, 15.5, 15.6 helper and test).

## PR16 / C13: claude (`adapters.rs`, `src/runtime/{chat,proc}.rs`)

- [ ] 16.1 [manual] M1-M4 (claude): `claude -p` reads stdin, `stream-json` needs or not `--verbose`, capture event fixtures (M2), flags keep the OAuth login and a "write /tmp/x" prompt creates nothing and emits no tool events (M3), `--model=<m>` accepted (M4). Do this before any code; record results in the PR body.
- [ ] 16.2 RED: exact claude argv (design list), `default` omits `--model`, a model starting with `-` is rejected, a message `--dangerous-flag` stays data, system prompt in argv and history only on stdin, tools disabled in argv.
- [ ] 16.3 RED: stream-json decoder from the M2 fixtures; 2 MiB line cap; long `result` event accepted.
- [ ] 16.4 RED: process runtime with fake CLIs: no shell (`; touch /tmp/x $(id) | cat` stays data), stdin written then closed, neutral cwd 0700, env scrubbed (`SSH_AUTH_SOCK`, `GPG_AGENT_INFO`) plus `NO_COLOR` and private `TMPDIR`, stderr 16 KiB sanitized last line, non-zero exit and empty output are errors, decoy credential files never opened, CLI timeouts 120 s and 90 s, killed group on cancel within 1 s, no content in logs or errors.
- [ ] 16.5 GREEN: `Adapter::Claude`, `Invocation::Process`, spawn through the jail wrapper (Layered) when Available.
- [ ] 16.6 [manual] M11: a jailed claude starts, answers and refreshes its token; check the atomic rewrite of `~/.claude.json`. If the jail breaks claude it runs Layered-bare and the README says so. Also M12 for claude (strace) and M3 re-run under the jail.
- [ ] 16.7 Gate.

## PR17 / C14: codex (`adapters.rs`, `runtime/chat.rs`)

- [ ] 17.1 [manual] M4 (codex), M2 (codex JSONL fixtures), M5: `codex exec … -` reads stdin, `--ignore-user-config` keeps auth, `--ephemeral` writes nothing under `~/.codex/sessions`.
- [ ] 17.2 [manual] M14 (codex): `strace -f -e trace=execve` on a jailed codex turn lists every program it runs; add each helper by exact path, never a directory. If codex cannot work without a general shell this PR is GATED: ship the adapter `gated off`, tell the user. Also M12 (codex).
- [ ] 17.3 RED: exact codex argv, `Instructions:` block on stdin, JSONL decoder from fixtures, policy `Required`: NoJail spawns nothing and shows `codex: not available on this system yet`, helper allow-list gains the M14 helpers by exact path.
- [ ] 17.4 GREEN: implement.
- [ ] 17.5 [manual] M9: jailed codex asked to "read ~/.ssh/config", "list my home" and "ssh to X" or "curl Y" fails or refuses; no file content appears and no connection is made by those programs.
- [ ] 17.6 Gate.

## PR18 / C15: gemini (`adapters.rs`, `runtime/chat.rs`)

- [ ] 18.1 [manual] M4 (gemini), M2 (gemini stream-json fixtures), M6: transcript on stdin with the fixed `-p` text appended; note where session files live.
- [ ] 18.2 [manual] M14 (gemini) and M12 (gemini), same rules as 17.2; GATED if a general shell is needed.
- [ ] 18.3 RED: exact gemini argv (never `--raw-output`, never `-y`), prompt block on stdin, decoder from fixtures, policy `Required`, allow-list includes `~/.gemini` rw and M14 helpers by exact path.
- [ ] 18.4 GREEN: implement.
- [ ] 18.5 [manual] M10: jailed gemini asked to read `~/.ssh/config`, list the home, "ssh to X" or "curl Y" fails or refuses.
- [ ] 18.6 Gate.

## PR19 / C16: Docs (`README.md`)

- [ ] 19.1 README: db is unencrypted plain text, path and how to delete it; CLI conversations go to the vendor; gemini keeps its sessions in `~/.gemini` outside `chat.db`; system prompt visible in `ps`; per-OS availability (Linux jail; codex and gemini unavailable elsewhere); CLI versions tested; execute-denied jail policy.
- [ ] 19.2 README: profiles, `prompts/<profile>/<task>.md`, the 16 KiB cap, order task then general then built-in, profile switch applies to new conversations only, whether claude runs jailed (M11 result).
- [ ] 19.3 Gate (docs only; cargo fmt/clippy unaffected).

## PR20 (no code): Final manual verify, real machine [manual]

- [ ] 20.1 `Ctrl+Space c` beside the shell in Kitty; typing, Enter, Alt+Enter, Esc and Ctrl+C when idle never quit; Ctrl+Y and Ctrl+B copy.
- [ ] 20.2 A long answer survives tab switches and COPY elsewhere, complete and not duplicated.
- [ ] 20.3 Ollama and an OpenAI-compatible server stream and cancel; cold model load text; engine switch in the menu applies to the next message.
- [ ] 20.4 claude, codex and gemini each stream a text reply; "create a file" produces no file; "ssh to X" and "curl Y" do nothing; no credential file touched by lazarobox.
- [ ] 20.5 Resume a conversation, tag it, delete it, hit the 200 limit notice; db files are 0600 and `journal_mode` is delete.
- [ ] 20.6 README states the facts (db, `ps`, gemini sessions, overrides). Re-run M1-M14 and record any drift before archive.

## Coverage: spec scenario → PR

chat-pane: Open from a shell pane, Refused when too small, Several chats, Opening requests detection P9; Chat in a real session 20.1; Transcript and composer, Composer growth, Tiny pane P8; Type and send, Newline, Paste, Blank, Send while streaming, Composer cap P8 (state) + P9 (routing); First-byte wait, Deltas, Error keeps partial, Error before any text, Cut stream, Error lines not context, Two chats P2 + P5; Cancel (all five) P2 + P9; Ctrl+C idle never quits P1 + P9; Code block, No other markdown, Unclosed fence, Long line wraps P8; Follow, Pinned, Send returns to bottom P8; Unassigned, Undetected, Unsupported, CLI gated off, No jail, Model not downloaded, Checking engines, Fix without reopening P2 (logic) + P9 (render); Stream across tab, during COPY, Composer survives MENU, Keys in MENU, Zoom and focus P5 + P9; Stream survives long run 20.2; Yank (last answer, nothing, clean text, code block) P9; Close while streaming, Stale events P5; Marker shown, Short conversation P2.

chat-history: Path resolution, Directory mode, Fresh files 0600, Wide mode corrected, Journal mode DELETE, Symlink or foreign owner, secure_delete, Deleted text gone P6; README states the facts 19.1 + 20.6; Created on first send, Assistant saved on finish, Partial saved on cancel, Cut and over-long statuses, Error lines not stored, Stored text sanitized, Slow store P7; Retention (201st, activity, user told, tagged, open skipped, over limit) P7; Delete (confirmed, declined, deleted elsewhere, failure) P10; Resume (all four) P10; Project tag (git, worktree file, worktree main repo, not a repository, unavailable, cleaning, no git process) P10; Edit tags (all five) P10; Conversation list (all eight) P10; Unavailable store (all five) P7.

chat-engines: Events in order, Error terminal, Cancel stops events P5; opencode no adapter P14; Engine resolution (ready, general, each guidance, never switches, latest, re-resolved) P2 + P13; Loopback client (traffic audit, body, foreign host, redirect, non-200, ambiguous, answer cap, incremental) P4 (client) + P13 (adapter); Stream parsers (NDJSON P3 + P13, SSE P3 + P14, split points, UTF-8, comments, malformed, error object, oversized, CLI long result) P3; Timeouts (connect, first byte, stalled, CLI timeouts, total, Slow) P4 + P16; Cancel releases the engine (server, CLI, before first byte) P4 + P5; CLI adapters (argv tools, no shell, option injection, default model, credentials, failure, gated off) P16 + P17 + P18; write-request manual 20.4; CLI jail (probe, nvm spec, no ssh keys, outside read, write rules, execve EACCES, shared libraries, partial enforcement, no jail, claude bare, exit 125) P15 + P16 + P17; ssh/curl manual 17.5 + 18.5 + 20.4; strace manual 17.2 + 18.2; jailed claude manual 16.6; Context window (all five) P2; Output sanitizer (all ten) P3; No content in logs P16; Real engines (Ollama, OpenAI, cold load) 13.4 + 14.3 + 20.3; CLI engines 20.4.

prompt-profiles: Profile set integrity, Default, Unknown value P11; Profile changes only prompts P12; Built-in prompts (three) P12; Resolution (six) P12; Override files untrusted (ten) P12; Profile in the menu (three) P11; Switch applies to new conversations only (three) P12; README documents overrides 19.2.

configuration: Defaults, Profile parsed, Empty profile, Unknown profile value, Case and spaces, Non-string, Counted, Non-table ai P11; all existing and task-table scenarios unchanged (regression in gate of P11).

config-persistence: Profile created, updated, default written explicitly, untouched, no engine keys, Profile key collisions (four) P11; existing owned-keys scenarios regression P11.

config-menu: Descriptor integrity, Settings rows, Navigation (wrap, task rows order), AI task list rendering, Profile row (all nine) P11.

modal-input: TERMINAL mode chat rows (chat keys never reach PTY, Repeat on a chat) P1 + P9; `c` in table, `c` opens a chat, inside group P9; Enter COPY refused P1; Paste into composer, Resize with a chat pane P1 + P9; Chat pane input (typing, Enter, Ctrl+B, Esc/Ctrl+C, unlisted, prefix, list keys) P9 + P10; Real keys in Kitty 9.6 + 10.7 + 20.1.

statusline: Root hint with chat, hint table-generated P9; Chat pane segments, CHAT label, Other modes keep label P9.

copy-mode-scrollback: Refused on a chat pane, Notice clears, COPY on a shell pane beside a chat P1; Stream while in COPY, Focus moves to the chat P5 + P9.

pane-layout: Pane kinds, Chat panes in split, focus, zoom and close, Last pane is a chat P1; Resize with a chat neighbour P1.

tabs: State preserved, Stream continues in a background tab, Closing a tab cancels, Events of a closed tab dropped P5; Label from origin cwd, Label without origin cwd P1.

terminal-session: Chat pane spawns nothing, No PTY effect, Chat not polled, Inherited cwd from a chat P1; Quit kills CLI children, Quit closes sockets, Closing a pane does not block P5.

ai-task-models: Effective assignment used, Fallback to general, Server model must be detected, Empty server model, CLI model not validated, Assignment never changed, Saved change applies to next message P2 + P13; Chat never pulls a model P13; Catalog integrity, No download path, Docs say stale already shipped by ai-selector (regression in each gate).

ai-engine-detection: Chat replies follow the same framing rules, Long chat stream not capped, Detection stays GET-only, Chat never runs during detection, Shared client loopback-only P4; Real machine manual 20.3.
