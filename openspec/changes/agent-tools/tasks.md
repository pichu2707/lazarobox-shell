# Tasks: Agent Tools (the chat acts in the user's pane, with approvals)

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | ~5,970 total (tests and snapshots included), 17 child PRs of 250-450 lines each |
| 400-line budget risk | High (total); Medium per PR; High for 1b, 2d, 2e, 2e-3 (fallback splits noted) |
| Chained PRs recommended | Yes |
| Suggested split | 1a → 1b → 2a → 2b → 2c → 2d → 2e → 2e-3 → 2f → 2g → 2h → 2i (deferred) → 3a → 3b → 4a → 4b → 4c |
| Delivery strategy | auto-chain |
| Chain strategy | feature-branch-chain (tracker `feat/agent-tools`, from v0.14.0) |

Decision needed before apply: No
Chained PRs recommended: Yes
Chain strategy: feature-branch-chain
400-line budget risk: High

Rules for every PR:
- Strict TDD: RED with compiling stubs and exact assertions, then GREEN, then refactor. Tests are never trimmed.
- Gate = `ENGRAM_PORT=8123 cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.
- One work-unit commit per task group (conventional commits, no AI attribution). Nothing is committed by the planning phase.
- Re-measure with `git diff --shortstat` before opening each PR; above 400 (additions + deletions) use the noted fallback split.
- PR 1a base = `feat/agent-tools`; each later PR base = the previous PR branch (`feat/agent-tools-<slice>`). Only the tracker merges to main.
- An engine joins `MCP_VERIFIED` only in the commit that follows its passed [manual] check. Until then it is byte-identical text-only.
- Docs task per PR: module and item doc comments always; README only where user-visible (1b, 2f, 2g, 2h, 3b, 4c).

### Milestones

- **M-run (after 1b)**: run-in-pane from any engine's fenced bash block, with approval. Shippable alone.
- **M-base (after 2d)**: "the MCP base" = registry, read tools, path policy, socket, `__mcp-serve` shim and protocol subset, driven by a fake MCP client. No engine is wired yet.
- **M-host (after 2e)**: events, rows, `ToolHost`, approvals and timers complete; first real engine in 2f.

### Suggested Work Units

| Unit | Goal | PR | Est. | Base | Depends on |
|------|------|----|------|------|-----------|
| 1a | validate, `last_shell_block`, `Approval` and `ApprovalAsk` types | PR1 | ~300 | tracker | - |
| 1b | Prefix `e`, `last_shell`, approval keys, rechecks, WritePty, bar | PR2 | ~400 | PR1 | 1a |
| 2a | Registry, parse_call, path policy, `[ai.tools]` config and menu row | PR3 | ~350 | PR2 | 1b |
| 2b | fs read tools, redaction, untrusted wrapper | PR4 | ~350 | PR3 | 2a |
| 2c | MCP JSON-RPC core (pure) | PR5 | ~350 | PR4 | 2a |
| 2d | Session, token, sweep, shim, main dispatch, `tests/jail.rs` | PR6 | ~400 | PR5 | 2c |
| 2e | Tool events, rows, `ToolHost`, locality routing, decisions, timers | PR7 | ~400 | PR6 | 2b, 2d |
| 2e-3 | Chat approval mode (D18): config key, route, auto-type, foreground check, toggle `a`, badge, menu row | PR7b | ~420 (split 2e-3a/2e-3b likely) | PR7 | 2e |
| 2f | claude wiring, tmp files, `ShimGrant`, decoder allow-list | PR8 | ~380 | PR7 | 2e |
| 2g | codex wiring | PR9 | ~250 | PR8 | 2f |
| 2h | gemini wiring | PR10 | ~300 | PR9 | 2f |
| 2i | opencode wiring (DEFERRED: adapter parked) | - | ~250 | - | opencode adapter |
| 3a | Ollama tools body and tool_calls decoder | PR11 | ~350 | PR10 | 2e |
| 3b | Ollama loop and no-tools fallback | PR12 | ~300 | PR11 | 3a |
| 4a | `[[ai.mcp.servers]]` config and resolution | PR13 | ~350 | PR12 | 2a |
| 4b | MCP client for user servers | PR14 | ~400 | PR13 | 2c, 4a |
| 4c | Re-exposure, approval, docs | PR15 | ~350 | PR14 | 2e, 4b |

Order note: 2g and 2h are independent of each other (both need 2f). 2c may run before 2b. PR numbers skip 2i.

## Reconciliation applied (spec and design edits by this phase)

- `specs/agent-tools/spec.md`: locality terms; "Read tools and their approval" (`read_file` asks on cloud engines and shows the path; `find_dir` and `list_dir` free everywhere); approval model, five new scenarios, pending-approval scenarios.
- `specs/lazarobox-mcp-server/spec.md`: engine-side allow, `read_file asks through MCP` scenario, minimal jail grants, injection files in the per-turn tmp dir (0700, wiped).
- `specs/chat-engines/spec.md`: jail grants are "the minimal grants" (binary rx, socket file rule, shim loader when it differs).
- `specs/pane-commands/spec.md`: only `y` accepts, only `n` and Esc decline, other keys swallowed; `p` cycles; `e` opens; confirm is chat state, not an input mode.
- `specs/modal-input/spec.md`: leaf `e`, cycle `p`, swallow rule, no `InputMode` variant.
- `specs/chat-pane/spec.md`: prompt shows `read <path>`; overlay of chat state.
- `design.md`: new D17 (approval routing by locality), D11 wording, open questions closed, opencode marked conditional. Per-project MCP servers only in user config (spec and D16 already agreed).

## PR1 / 1a: Command validation, shell block, Approval type (`src/core/ai/tools/{mod,command}.rs`, `src/core/ai/chat/{yank,approval,mod}.rs`)

- [x] 1a.1 RED: `validate` table tests (pane-commands "Command validation"): ESC refused, `\x1b[201~` refused and `echo x[201~` valid, `\x07 \x00 \x7f \x9b` lone `\r` U+202E refused, `\n`/`\t` kept, trailing newlines and blanks trimmed, empty/blank/over 4096 bytes refused.
- [x] 1a.2 RED: `yank::last_shell_block`: bash/sh/shell/zsh case-insensitive; rust, no language, unclosed, and streaming message give None; last closed block wins.
- [x] 1a.3 RED: `Approval` state: one pending at most, second request refused with `another approval is pending`, `ApprovalAsk::{Run, Read, External}`, withdraw on cancel/close, repeat/release do not answer (pure transitions).
- [x] 1a.4 GREEN: implement the three units; `Refusal` carries a short content-free reason.
- [x] 1a.5 REFACTOR: share the fence parser with yank.rs; no duplicate.
- [x] 1a.6 Gate.
- [x] 1a.7 Docs: doc comments on `validate` caps and `Approval` invariants.

## PR2 / 1b: Run in pane (`src/core/prefix.rs`, `src/app.rs`, `src/app/{chat,run}.rs`, `src/ui/components/chat.rs`, `src/core/ai/config.rs`)

Fallback split if over 400: 1b-i (state, keys, effects) and 1b-ii (bar and snapshots).

- [x] 1b.1 RED: prefix table has root leaf `e` (non-empty description, no duplicate, hint at most 80 columns); on shell pane, list/editor view and tools off it only notices and emits nothing.
- [x] 1b.2 RED: `Tab.last_shell` set in `set_focus_in`; target is last focused shell, never chat or another tab; no shell pane refuses; `p` cycles with wrap and writes nothing.
- [x] 1b.3 RED: keys routed first in `on_chat_key`: `y` approves; `n`/Esc decline; `Y`, Enter, Space, Ctrl+y, Alt+y, `a`, Tab, F5 swallowed; Repeat/Release and paste do not answer; Ctrl+C cancels and denies, never Quit; Ctrl+Space still works; MENU shields it.
- [x] 1b.4 RED: accept-time rechecks (pane gone, other tab, alt screen `\e[?1049h`), bracketed paste on frames via `encode_paste`, off refuses multi-line/tab and writes single line raw, state read at accept time, one accept is one `Effect::WritePty`, no `\r`.
- [x] 1b.5 RED: lifecycle: survives tab switch and focus change; pane close, cancel, tools off and quit end it with no write; keys elsewhere go to the PTY; target removed refuses `y` but `p` still works.
- [x] 1b.6 RED: `tools_enabled` gate reads `[ai.tools] enabled` (default true; key parse only, menu row is in 2a); off gives the tools-off notice.
- [x] 1b.7 RED: TestBackend + insta: prompt `run in pane 2 (~/dev)` + command + keys, composer kept, clipped 20-line command states the line count, sanitized cwd, hint under the block, prompt visible when scrolled.
- [x] 1b.8 GREEN: implement `PrefixAction::RunInPane`, `Tab.last_shell`, overlay keys, rechecks, bar. No `InputMode` variant.
- [x] 1b.9 Gate.
- [x] 1b.10 Docs: README section "Run a command from the chat" (`Ctrl+Space e`, `y`/`n`/`p`, never Enter).
- [ ] 1b.11 [manual] Kitty: `echo hi` appears on the input line unexecuted in bash, zsh and fish; two-line command is an editable buffer; `Ctrl+Space e`, `p`, `y`, `n` reach the app.

## PR3 / 2a: Registry, path policy, config (`src/core/ai/tools/{mod,paths}.rs`, `src/core/ai/{config,menu}.rs`, `src/core/config_edit.rs`, `src/core/ai/jail.rs`)

- [x] 2a.1 RED: registry lists `find_dir`, `list_dir`, `read_file`, `run_in_pane` with schema and Access; same list for MCP and Ollama builders; built-in names never shadowed.
- [x] 2a.2 RED: `parse_call` refuses unknown tool, missing/mistyped args and extra keys (`mode`).
- [x] 2a.3 RED: `check_path` over a temp `$HOME`: every `CREDENTIAL_DIRS` entry and a child, `.ssh`, `.gnupg`, `.env`/`.env.*` at any depth, outside HOME, traversal, symlink to excluded, symlinked dir out, symlink inside HOME fine, relative input vs origin cwd, one identical refusal text for excluded/outside/missing.
- [x] 2a.4 RED: config `[ai.tools] enabled` default true, bad value falls back with the startup notice order; menu Tools row after Notify; save writes only `ai.tools.enabled`, never `ai.mcp.*`; popup size lists updated.
- [x] 2a.5 GREEN: implement; make `CREDENTIAL_DIRS` `pub(crate)`.
- [x] 2a.6 Gate.
- [x] 2a.8 Review follow-up: deny list widened by user decision (credential files, `.config/gh`, `.config/gcloud`, keyrings, shell/REPL histories, `.envrc`, lazarobox store); real roots canonicalized for every dir root; tests per group.
- [x] 2a.7 Docs: doc comments; README config key `[ai.tools]`.

## PR4 / 2b: Read tools and wrapper (`src/runtime/tools/{mod,fs}.rs`, `src/core/ai/tools/label.rs`)

- [x] 2b.1 RED: `find_dir` (depth 8, 20,000 visited, 2 s, 20 results, no symlink follow, no descent into excluded, truncation said); fake slow fs keeps the loop responsive.
- [x] 2b.2 RED: `list_dir` (200 cap, kinds, excluded names omitted, control/bidi names cleaned); `read_file` (64 KiB, NUL means not text, redaction via `memory::redact`, real path re-checked through `/proc/self/fd` after open, swapped-symlink case).
  - 2b MUST also: check hard links with `st_nlink` (a regular file with more than one link is refused, since a hard link to a secret has an ordinary path); open with `O_NOFOLLOW` / `openat2` with `RESOLVE_BENEATH` and re-check the opened file through `/proc/self/fd`; refuse every call when HOME is unset or not absolute, never falling back to `/`.
  - 2b MUST build `SecretRoots::load(home)` ONCE per tool call (it reads HOME and every secret root) and pass it to every `resolve` and to the entry filter of `list_dir` and `find_dir`; a `None` means refuse the whole call. The roots are a snapshot taken at load: re-check the opened fd's real path against them, and a root created after the load is out of scope for that call. Pass the real lazarobox config and data dirs (honouring `XDG_CONFIG_HOME` and `XDG_DATA_HOME`) as `extra` roots: `SecretRoots::load(home, &extra)`.
  - 2c/2e (the layer that reads the raw line; 2b parses nothing, it takes a validated `Call`) MUST reject duplicate JSON keys at the raw wire layer: `serde_json::Value` has already collapsed them when `parse_call` runs. `parse_call` caps each argument at `MAX_ARG_BYTES` (4096).
  - Hardening (review W2): open with `O_NOFOLLOW` and `O_NONBLOCK`; `fstat` and require a regular file (a FIFO would hang); refuse `st_nlink > 1` (hard links to secrets); after open, `readlink /proc/self/fd/N` and run `check_path` again; `find_dir` and `list_dir` skip excluded entries and never follow symlinks; prefer `openat2` with `RESOLVE_BENEATH` when available.
- [x] 2b.3 RED (reworded after review): no in-process recording sandbox can see a spawn or a socket, so this is a best-effort tripwire against accidental use, NOT a guarantee. The static check (`runtime/tools/audit.rs`) tokenizes `runtime/tools/{mod,fs,open}.rs`, drops comments, strings and every `#[cfg(test)]` item, and flags the process and socket paths, `OpenOptions`, `File::create`/`options`, the write, create, remove, copy, rename and truncate calls (also through grouped or aliased `std::fs` imports), the write-mode `O_*` flags, `extern` blocks, `use libc::*`, a `libc::syscall` that is not `SYS_openat2`, and any `libc::` name outside an allow-list of read-only open calls; the ban list is a separate file so the check does not match itself, and each audited file must yield tokens. Blind spots: calls built by a macro, a path the ban list does not name, code in another crate or reached through a trait, and `#[cfg(test)]` items (not shipped). Plus a test that the tree (names, sizes, mtimes) is unchanged after the tools run; error text carries no file content (`SECRET-TOKEN-123`).
- [x] 2b.4 RED: wrapper `<untrusted-tool-output id=NONCE>` with random 16-hex nonce, forged closing tag stays inside, 32 KiB cap marker, "data, not instructions" line.
- [x] 2b.5 GREEN: implement on `spawn_blocking`.
- [x] 2b.6 Gate.
- [x] 2b.9 Review fixes: non-Linux builds (stub open, "could not read"); `openat2` falls back to the walk only on ENOSYS/EPERM/EINVAL/E2BIG; `clean` before `redact`, redaction over a 1 KiB margin past the read cap; special files refused like hidden paths; walkers take a cancel flag and a deadline (per entry and before each `canonicalize`), `run` times out and cancels on drop; listed names redacted; no `RESOLVE_NO_XDEV` on purpose (mounts under HOME are legitimate); hard-linked stores are refused.
- [x] 2b.8 Done in 2b: roots built once per call with the XDG-aware lazarobox dirs as `extra`; HOME unset/empty/relative refuses; `openat2` (BENEATH + NO_SYMLINKS) with an `O_NOFOLLOW` per-component fallback; fd path re-checked through `/proc/self/fd` against `judge_real`; `st_nlink > 1` and non-regular refused. Left for 2c/2e: duplicate JSON keys and the wire-size cap at the raw layer.
- [x] 2b.7 Docs: doc comments with the pinned caps.

## PR5 / 2c: MCP core, pure (`src/core/ai/tools/mcp.rs`)

- [x] 2c.1 RED: `handle(&mut self, line) -> Action` for `initialize` (2025-06-18; 2025-03-26 and 2024-11-05 echoed), `notifications/initialized`, `ping`, `tools/list`, `tools/call`, `notifications/cancelled`.
- [x] 2c.2 RED: errors -32601 (`resources/read`), -32700 bad JSON, -32600 array, -32602 bad params; line over 1 MiB gets an error with `id: null` and the state machine keeps going; unknown tool `bash` is an error result.
- [x] 2c.3 GREEN: implement on `serde_json::Value` (no serde derive, no rmcp).
- [x] 2c.4 REFACTOR: reuse the codec later for the client (4b) via a shared `Frame` type.
- [x] 2c.5 Gate.
- [x] 2c.6 Docs: protocol subset table in module docs.
  - Done in 2c: duplicate JSON keys are rejected at the raw layer (`Frame::parse`, a hand-written scanner, any depth, after unescaping) with -32600 (echoing the request id when usable, else `id: null`); a line over 1 MiB (`MAX_LINE_BYTES`) gets the same. The wire cap is a module constant, not a config `Limits` type (the design has none). Left for 2d: the shim and session must also cap the bytes they buffer before calling `handle`.

## PR6 / 2d: Session, shim, jail test (`src/runtime/mcp/{mod,session,shim}.rs`, `src/main.rs`, `tests/jail.rs`)

Fallback split if over 400: 2d-1 (session, token, sweep) and 2d-2 (shim, main dispatch, jail test).

- [x] 2d.1 RED: session socket `<runtime>/lazarobox/s-<pid>-<pane>.sock`, dir 0700 (fallback `<data>/run`), socket 0600; wrong/missing token closed unserved; constant-time compare; `peer_cred` UID check; token bound to its chat (A's token on B rejected); token never in argv or logs.
- [x] 2d.2 RED: cleanup on chat close, stale sockets swept at start (alive_by_comm), removal at exit; session calls a `Dispatch` trait (fake in tests).
- [x] 2d.3 RED: shim relay generic over `Read`/`Write` via `UnixStream::pair`: handshake `{"lazarobox":1,"token"}` then line copy, 1 MiB cap, malformed lines survive, shim opens no user file, starts no process; `LAZAROBOX` set does not block it.
- [x] 2d.4 RED: `main.rs` dispatches `__mcp-serve` right after `__jail-exec`, before install-desktop and the nesting guard.
- [x] 2d.5 RED: `tests/jail.rs` (skipped when `probe()` is Unavailable): jailed helper with the shim granted executes the shim and connects; `/usr/bin/curl` execve and `~/.ssh` read still EACCES.
- [x] 2d.6 GREEN: implement listener, hub, shim, dispatch.
- [x] 2d.7 Gate.
- [x] 2d.8 Docs: doc comments on the transport and trust model.
- [ ] 2d.9 [manual] Landlock on the real kernel: a pathname-socket connect works under the granted rights and `add_rule` accepts the socket inode. If it fails, record it; jailed engines stay text-only (do not widen the jail).
- [ ] 2d.10 [manual] M-base check: start a session by hand, drive the shim with a scripted client for `initialize`, `tools/list`, `find_dir`; confirm the 0700/0600 modes.

  - Split chosen: 2d-1 = session side (`src/runtime/mcp/{mod,session}.rs`), 2d-2 = the rest.
    - Done in 2d-1: 2d.1, 2d.2 (listener, hub, `Dispatch` trait with a fake in tests, token, sweep with `alive_by_comm`, `peer_cred` check), the session half of 2d.6, the gate (2d.7) for this slice, and the module docs for the session transport and trust model (the session half of 2d.8). Handshake format the session accepts: first line `{"lazarobox":1,"token":"<32 hex>"}`, answered by `{"ok":true}`; then MCP lines.
    - Policy: a line is read into at most `MAX_LINE_BYTES + 2` bytes before any `&str` exists; an oversize line gets the `id: null` -32600 error, is skipped to its newline, and the connection keeps serving. The handshake line is capped at 512 bytes and has a 2 s timeout; failing either closes silently. Limits per session: 8 unauthenticated connections (extra ones dropped at once), 4 authenticated (extra ones closed after a valid handshake); per connection 32 calls in flight (-32000 echoing the id), a duplicate in-flight id is -32600; a panicking dispatch answers an `isError` result; `close` sets a flag so nothing dispatches after it.
    - Done in 2d-2: 2d.3 to 2d.8 (`src/runtime/mcp/shim.rs`, the `__mcp-serve` arm in `src/main.rs`, `tests/mcp_shim.rs`, `tests/jail.rs`). Left for the user: 2d.9, 2d.10.
    - Shim contract: `lazarobox-shell __mcp-serve <socket>`; the socket path is argv (not a secret), the token is read from the env var `LAZAROBOX_MCP_TOKEN` (the engine's MCP config forwards it by name, D9). The shim is sync (two std threads), generic over `Read`/`Write` for stdin/stdout, and caps each line at 1 MiB before buffering: an oversized client line is answered `id: null` -32600 on stdout, not forwarded; malformed lines pass through untouched. Handshake reply must be exactly an object with `ok: true` within 5 s and at most 512 bytes. Refusal (EOF, other reply, oversize reply) exits 1 with `lazarobox mcp: the lazarobox session refused the connection`; silence exits 1 (`did not answer in time`); unusable command or missing token exits 2; no message carries the token. Either direction ending (stdin EOF, session closed) ends the shim with 0. The token env var is not removed after reading: the shim spawns nothing and opens no file, so nothing inherits it, and it stays in `/proc/<pid>/environ` regardless.
    - Security follow-up on 2d-2 (review): the shim requires an absolute socket path, `lstat`s socket and parent (parent real dir, ours, 0700; socket a real socket, ours, no symlinks) and checks the peer uid after connect, all before the token is sent; the token must be 32 lowercase hex (exit 2, value never echoed). On stdin EOF it half-closes and keeps relaying replies until the session closes or `DRAIN_GRACE` (5 s). The handshake has one overall deadline (reads and a write timeout). Known gap in the 2d-1 session: on read EOF its connection loop breaks and drops calls still in flight, so a slow call is not saved by the half-close; draining in-flight work at EOF is a session change left for 2e (done in 2e-1: the connection stops reading at EOF, lets its calls finish and flushes their replies for up to the same 5 s, then aborts; `Hub::close` and a full hangup end the wait at once). `LAZAROBOX_REQUIRE_JAIL=1` makes `tests/jail.rs` fail instead of skip; both jail tests share `with_shim_grant`. D11 now records the ABI 9 `RESOLVE_UNIX` and inode-binding caveats.
    - Jail test (`tests/jail.rs`, skipped without Landlock): the app binary jailed with rx on itself + its loader and an `rw_file` on the socket only (no folder) runs `__mcp-serve` with `LAZAROBOX=1`, connects to a real `Hub` session and gets an `initialize` result. `curl`/`ssh` execve and a `~/.ssh` listing still get EACCES under the same grant. On the dev machine (Landlock ABI 7) it passes without widening the jail; pathname unix-socket connect is not gated by Landlock before ABI 9, so the socket rule is not what makes it connect (2d.9 stays manual for the real-kernel check).

## PR7 / 2e: Events, rows, ToolHost, decisions (`src/core/ai/chat/{engine,mod}.rs`, `src/runtime/tools/mod.rs`, `src/runtime/chat.rs`, `src/app.rs`, `src/app/run.rs`, `src/ui/components/chat.rs`)

Fallback split if over 400: 2e-1 (events, rows, ToolHost) and 2e-2 (approval wiring and timers).

The split was taken. **2e-1 is built**: 2e.1, 2e.2, the host half of 2e.7 and its gate (2e.8). The seam is `Approver` in `src/runtime/tools/host.rs`; D18 needs no host change (the app applies `approval::route` when a `ToolCall` with an ask arrives, and its auto-type answer reaches the same `Approver`). Review fixes in 2e-1: the result slot is reserved before a call is announced, `Approver::ask` registers synchronously and runs before the announcement, and a cloud read asks with the resolved path or is refused unasked. Also in 2e-1: calls in flight now drain after the client half-closes.

**2e-2 is built** (2e.3 to 2e.7, 2e.9; the gate 2e.8 ran again):
- Runtime. `runtime/tools/approver.rs`: `Waits::approver(pane, turn, pending)` is the real `Approver`; the wait is registered in `ask`, keyed by (pane, turn, call); `Waits::decide` answers it; `end_pane` and `end_turn` deny; dropping the future unregisters. `runtime/tools/pending.rs`: `Pending`, a per-turn count of calls that wait for the user, handed to the adapter's timers as a task-local. `loopback::Wait` pauses the first-byte limit (the time already spent is kept), the idle limit (it starts over when the last wait ends) and the `Slow` hints while the count is above zero; the 15-minute total ignores it. `Chats` gained `with_tools(host, dirs)`, a tool session per turn whose request has `Tools::On` (the hub is made on the first such turn), `decide`, `end_session`, `session`; a finished, failed, timed-out or aborted turn denies its waiting calls (`EndTurn` guard), and `send` / `cancel` do it at once. `Effect::ToolDecision { pane, turn, call, decision }` and `Effect::EndToolSession(pane)` are applied in `runtime.rs`.
- Request. `ChatRequest.tools: Tools { Off, On { origin } }`, set by the app through `tools_for(engine, enabled, origin, verified)`; `MCP_VERIFIED` was empty until the activation of 2026-10-17 (now `["claude", "codex", "gemini", "ollama"]`); with an empty verified set every request carries `Off` (a test pins that On and Off build the same invocation for the four engines with `&[]`).
- App. `app/tool.rs`: `App::on_tool_ask` is the single entry for an ask (2e-3 wraps it with `approval::route`): tools off, a kind of call with no prompt yet (`External`), no shell in the chat's own tab, or another approval pending are denied at once with a reason; otherwise the approval opens in the chat's overlay under (turn, call). `y` on a run types through the 1b path (one `WritePty`, bracketed paste, no `\r`) and answers `Sent` only then; a failed accept-time check answers `Denied(reason)` (full-screen app, no paste mode, refused command, pane gone after the confirm closed). A target that is gone before `y` keeps the prompt pending, as in 1b, so `p` can pick another. `y` on a read answers `Sent` with no write. `n`, Esc, Ctrl+C (which also cancels the turn) and tools turned off answer `Denied`. A `ToolResult` for the call, or the end of its turn, withdraws the prompt; an ask from a turn that is over is denied. `EndToolSession` is emitted next to `ChatCancel` for a chat whose last turn offered tools (cancel, answer cap, pane or tab close). The read prompt shows `read <path>` with its own keys line.
- Review follow-ups. (1) The prompt shows everything `y` would approve or says `pane too small to show it all: enlarge it or press n` and shows none of it; `core::ai::chat::approval_rows::{parts, wanted, fits}` is the one function both the renderer and `y` use (Read, model Run and the `Prefix e` confirm), so a resize re-enables `y`; the old "clipped: N lines" marker is gone. (2) Turning tools off emits `EndToolSession` and clears the flag for every chat with a session, after denying a waiting call. (3) A prompt opened by the model (`Origin::Tool`) ignores `y`, `n` and Esc for `ARM_DELAY` (400 ms, on the app's injected clock); its keys line shows `…` until then; Ctrl+C always works and the user's own `Prefix e` confirm has no delay. (4) `host.rs` documents why `ask` registers (and counts as pending) before the channel slot is reserved.
- Not done here: D18 (2e-3) and `External` asks (4c). A chat on a hidden tab keeps its prompt until it is shown; nothing rings for it yet.

- [x] 2e.1 RED: `StreamEvent::ToolCall{id,name,summary,approval}` and `ToolResult{id,status,summary}` paired exactly once; produced by the host, never parsed from text (imitation line is plain text); rows are `Entry::Tool`, shown, never stored or sent; summaries capped and sanitized.
- [x] 2e.2 RED: `EngineKind::locality()`: loopback HTTP is Local, every CLI Cloud, whatever the model name.
- [x] 2e.3 RED: `needs_approval`: `run_in_pane` and External always; `read_file` only Cloud; `find_dir`/`list_dir` never. Cloud `read_file` shows `read <path>`, file not opened until `y`; `n`, Esc, timeout, cancel, close give `denied`; local `read_file` runs free.
- [x] 2e.4 RED: `Decision{Sent,Denied}` via `Effect::ToolDecision{pane,turn,call,decision}` and `Chats::decide`; `Effect::EndToolSession(pane)` with ChatCancel; one pending per chat, second is denied at once; free tools run around a pending one; stale decision withdraws the prompt.
- [x] 2e.5 RED: tokio paused time: first-byte and idle timers suspended while `pending > 0`, 15-minute total keeps running, a timer never approves; two chats: closing one ends only its calls.
- [x] 2e.6 RED: injection in file text wrapped; model `run_in_pane rm -rf ~` still shows the exact command and needs `y`; tools off leaves every request byte-identical; `ChatRequest.tools: Tools{Off,On}`.
- [x] 2e.7 GREEN: implement `ToolHost`, runner `Option<Arc<ToolHost>>`, rows, decisions; connect session `Dispatch` to the host.
- [x] 2e.8 Gate (2e-1; it runs again with 2e-2).
- [x] 2e.9 Docs: doc comments; design D17 cross-reference.

### 2e-3: Chat approval mode (design D18; own PR after 2e, base = the 2e branch; 2f then targets the 2e-3 branch)

Files: `src/core/ai/{config,menu}.rs`, `src/core/menu.rs`, `src/core/config_edit.rs`, `src/core/ai/chat/{approval,mod}.rs`, `src/core/prefix.rs`, `src/app.rs`, `src/app/chat.rs`, `src/runtime.rs`, `src/ui/components/chat.rs`. Fallback split if over 400: 2e-3a (config key, route, auto-type, foreground check, toggle, badge) and 2e-3b (menu row and persistence).

> 2e-3 split (2e-3a, branch `feat/agent-tools-2e3a`): done are 2e-3.1 to 2e-3.7, the non-menu part of 2e-3.10, 2e-3.11 and 2e-3.12. **2e-3.8 (menu `Approval` row) and 2e-3.9 (persistence of `ai.tools.approval` from the menu) move to 2e-3b**, after the config-menu tabs PR (#215) lands: `src/core/menu.rs` and `src/ui/components/menu.rs` are untouched here. The sizes first written in 2e-3.8 (a taller body and a larger minimum, from before the tabs) are history and no longer apply: with tabs the `Approval` row is the 11th row of the AI tab and the popup stays 16 rows tall. Reading `ai.tools.approval` from the file (2e-3.1) is in 2e-3a; only the menu write path waited.
> As built in 2e-3b: `ItemKind::Approval` (the 11th and last row of `AI_SECTION`), `AiMenu::flip_approval` / `changed_approval`, `ConfigEdit::approval(mode)`; the edit is the string `ask`/`auto`, placed after the `ai.tools.enabled` edit; `apply_edit` needed no change (a new key lands in the existing `[ai.tools]` table, after `enabled`). The saved mode reaches `App::ai`, which the chat opens and conversation switches already read; no chat is touched on save. The 8 AI-tab snapshots gained the `Approval` line in what was a blank row.
> As built in 2e-3a: `Effect::AutoType{chat, target, bytes, turn, call}` carries both panes (the call is answered under the chat, the bytes go to the target); `typed_bytes` is the old `type_command` renamed (it was already the one function behind `y`, no behaviour change); the foreground denial and the result row name the pane: `Effect::AutoType` also carries `number` (the pane's number in its tab) and the runtime answers `Decision::Busy(number)` (host wording `pane N is running a program`) or `Decision::Typed(number)` (summary `typed into pane N — no Enter`), because `Decision::Denied` holds a `&'static str`; the runtime claims the answer (`Waits::take`) before writing, so a call that is over gets no bytes; `PtyHandle::shell_in_foreground()` (default `None` = cannot tell = denied) wraps `tcgetpgrp` through portable-pty's `process_group_leader`; the `AUTO` badge shows while `run_hint` (the tools-on flag of a chat) is set.

- [x] 2e-3.1 RED: `[ai.tools] approval`: default `ask` (no key, no table); `"auto"` parses; `"AUTO"`, `"yes"`, `true` fall back to `ask` with `ai.tools.approval: expected "ask" or "auto"`; notice order after `ai.tools.enabled`; non-table `ai.tools` reports only `ai.tools: expected a table`.
- [x] 2e-3.2 RED: pure `approval::route`: `AutoType` only for `(Run, Origin::Tool, Auto)` with `typed_this_turn < MAX_AUTO_PER_TURN` (5), `!line_pending`, target not focused, tab shown; `Read`, `External`, `Origin::Block` and `Ask` always `Prompt`; the 6th call in a turn is `Prompt`.
- [x] 2e-3.3 RED: `ChatState.approval_mode` set from config at open and reset to the live config value when the user opens another conversation from the list or starts a new one in the pane; toggle flips only that chat; config change reaches only later opens and switches; `auto_typed` reset at turn start; mode never in store commands or requests; requests byte-identical in `ask` and `auto`; tools off byte-identical.
- [x] 2e-3.4 RED: auto-type in the App harness: no confirm, one `Effect::AutoType{pane,bytes,turn,call}` to `last_shell`, bytes from the shared `typed_bytes` (same as `y`), no `\r`; rechecks (pane gone, other tab, alt screen `\e[?1049h`, multi-line with paste off, escape in command) give `ToolDecision Denied` with reason and a result row, nothing written; pending approval gives `another approval is pending`; toggling to `auto` leaves a pending confirm pending; focused target and hidden tab open the prompt; two calls to one pane in one batch: one typed, one prompt.
- [x] 2e-3.4b RED: `line_pending` per PTY pane: set synchronously with `Effect::AutoType`; a later auto call to that pane prompts (same turn, later turn, same batch); cleared by user key events Enter, Ctrl+C, Ctrl+U forwarded to that pane (also with an extended keyboard protocol on), by a paste holding `\r`, by closing the pane, and by a `denied` result of the marked call; Backspace and other keys leave it set; a mark on pane 2 never affects pane 3; Enter after each command lets 5 be typed and the 6th prompts.
- [x] 2e-3.5 RED: runtime foreground check: `tcgetpgrp` on the master equal to the shell's group writes and decides `Sent`; a child in the foreground (fake PTY running `sleep`) or an unknown foreground (the `None` of `shell_in_foreground`, which is also the default of targets without process groups; the non-unix cfg gate is not tested) decide `Busy(N)`, which the host words `pane N is running a program`, and write nothing.
- [x] 2e-3.6 RED: prefix leaf `a` → `PrefixAction::ToggleApproval`: root only, unhinted, non-empty description, root hint unchanged (79 columns), action count 34; on a chat pane flips the mode and adds the system line, also with a pending approval; shell pane and tools off only notice; inside a group, COPY or RESIZE nothing.
- [x] 2e-3.7 RED: TestBackend + insta: ` AUTO ` badge (`bg_base` on `error_red`, bold) top-right in conversation, list and editor views, focused or not; `A` below 6 columns; no marker in `ask` (existing snapshots unchanged); no marker with tools off; 1x1 to 10x2 do not panic.
- [x] 2e-3.8 RED: menu `Approval` row after `Tools` (`ask`/`auto`, own `ItemKind`), cycle keys, Enter repeat ignored, footer, Esc reverts, no live effect on open chats or a pending approval; navigation wraps inside the AI tab from `Approval`; the AI tab holds 11 rows and the popup stays 16 rows (`MENU_BODY` 12, minimum 24x15, per `openspec/specs/config-menu/spec.md`); descriptor integrity.
- [x] 2e-3.9 RED: persistence of `ai.tools.approval`: created next to `enabled`, updated in place with comment, written explicitly when equal to the default, untouched when unchanged, `[ai.tools.approval]` collision refused with the footer, saved file parses back.
- [x] 2e-3.10 GREEN (2e-3a: everything except the menu row and the owned key; 2e-3b: the `Approval` row and `ConfigEdit::approval`): implement `ApprovalMode`, `route`, `MAX_AUTO_PER_TURN`, `line_pending` and its clear points in the key and paste paths, the reset on conversation switch, `typed_bytes` (extracted from the 1b accept path, no behaviour change for `y`), `Effect::AutoType`, foreground check, toggle, badge, menu row, owned key.
- [x] 2e-3.11 Gate (2e-3a: green; it runs again with 2e-3b).
- [x] 2e-3.12 Docs (2e-3a: README "Approval mode" and doc comments; 2e-3b adds the menu row to the README: done): README "Approval mode" (`Ctrl+Space a`, `[ai.tools] approval`, typed never run, a prompt while a typed line is unsent, at most 5 per turn, the mode resets per conversation, the residual risks of design D18); doc comments.
- [ ] 2e-3.13 [manual] Kitty: `auto` types `cd` unexecuted in bash, zsh and fish; a second command prompts until Enter or Ctrl+C in the pane, then types; refused while `python3` runs in the target; `AUTO` visible on an unfocused chat; `Ctrl+Space a` reaches the app.

## PR8 / 2f: claude wiring (`src/core/ai/tools/wiring.rs`, `src/core/ai/chat/{claude,adapters,engine}.rs`, `src/runtime/chat/cli.rs`, `src/core/ai/jail.rs`)

> As built: `Job.tmp_files: Vec<TmpFile>` (static files) plus `Job.mcp: Option<McpWire>` (the session; the runtime renders the config with the token, so the token never sits in a `Debug`-printable `TmpFile`). `Tools::On` gained `mcp: Option<McpWire>`, set by `Chats::send` from the turn's session and read by `Adapter::build_with(request, verified)` (`build` passes `MCP_VERIFIED`; it was empty when 2f landed, and since the activation of 2026-10-17 it lists all four engines). `CliEnv.shim` is the canonical executable resolved once at startup. The tools are wired only for a jailed run; a jail that refuses the shim layout is retried without it, never widened. The claude decoder fails closed in two modes (a deliberate hardening, new for text-only chats): a wired turn (tools on, session present: `fresh_decoder(wire, wired)`, `ClaudeDecoder::wired()`) allows only the content blocks `text`, `thinking`, `redacted_thinking` and a `tool_use` named `mcp__lazarobox__` plus at least one character, and every other block type ends the turn; a text-only turn refuses any type that contains `tool` or ends in `_use` (ASCII case-insensitive), ours included, and tolerates other new block types. `CREDENTIAL_DIRS` did not need to become `pub(crate)`. The jail integration test (`tests/jail.rs`) keeps its own `with_shim_grant` helper. Review fixes (2f review): the shim binary and its loader pass `interpreter_trust` before `ShimGrant` is built (a refusal takes the retry-without-the-shim path); `ClaudeWiring` has a manual `Debug` that hides the config; the turn's token is replaced by `(..)` in the stderr tail; `McpWire.unwire` (an `Unwire` hook that `Hub::open` fills with the session's own end, a no-op once the session was closed or replaced) is called by `process_turn` whenever the engine is not wired after all (a bare run, no shim program, a layout the jail refuses, a refused plan), so the socket and token go at once; `Tools::On.mcp` is boxed to keep `SendOutcome` small. Honest TDD note: tests 2f.1 to 2f.5 were written together with their code in the same commits and were not run red first; the review fixes above were each run red first, except the guard tests that pin behaviour that already existed (`mcp.json` wiped after a wired turn and after the refused-shim retry, the dormant byte-identical plan for every engine), which were checked by mutation instead.

- [x] 2f.1 RED: `Job.tmp_files: Vec` generalizes `tmp_file`; files 0600 in the per-turn tmp dir (0700), wiped after the turn.
- [x] 2f.2 RED: insta snapshot of claude argv/env/mcp.json with tools on: `--mcp-config`, `--strict-mcp-config`, `--tools ""`, `--allowedTools mcp__lazarobox`, `MCP_TOOL_TIMEOUT=900000`; token only in the config env entry, never argv.
- [x] 2f.3 RED: engine not in `MCP_VERIFIED`: argv, stdin and env byte-identical to tools off.
- [x] 2f.4 RED: `ShimGrant`: jail differs by the shim binary (rx), the socket file rule, and the loader only when it differs from the CLI's; canonical exe, not `/proc/self/exe`.
- [x] 2f.5 RED: decoder allow-lists only `mcp__lazarobox__*` tool events in a wired turn (strict block allow-list); any other tool event, and every tool event of a text-only turn, still fails closed.
- [x] 2f.6 GREEN: implement; fallback to text-only on any wiring failure.
- [x] 2f.7 Gate.
- [x] 2f.8 Docs: README "Tools with claude".
- [x] 2f.9 [manual] (user) `claude --help` and `claude mcp --help`: confirm `--mcp-config`, `--strict-mcp-config`, `--allowedTools` and the server-level `mcp__lazarobox` syntax; whether `--tools ""` hides MCP tools; the default and effect of `MCP_TOOL_TIMEOUT`. Verified by the user on 2026-10-17, `MCP_TOOL_TIMEOUT` (900000) included.
  - Partially verified by the orchestrator on claude 2.1.302 (`claude --help`): `--mcp-config <configs...>` ("Load MCP servers from JSON files or strings (space-separated)") and `--strict-mcp-config` ("Only use MCP servers from --mcp-config, ignoring all other MCP configurations") exist; `--tools <tools...>` ("Specify the list of available tools from the built-in set. Use \"\" to disable all tools") names the BUILT-IN set only; `--allowedTools, --allowed-tools <tools...>` is a comma- or space-separated list of tool names, the server-level `mcp__lazarobox` syntax is not shown in the help; `MCP_TOOL_TIMEOUT` is not in the help. Still open for the user: the `mcp__lazarobox` server-level syntax, whether `--tools ""` also hides MCP tools, and `MCP_TOOL_TIMEOUT` (default and effect). All three are checked by the real call of 2f.10 and by `claude mcp --help`.
  - `--mcp-config` and `--allowedTools` are variadic: the wiring passes them as `--flag=value` (one argv element each), so they cannot swallow a following value; `wiring::tests::variadic_flags_use_the_equals_form_so_nothing_after_them_is_swallowed` pins it. The prompt is on stdin, so no positional follows anyway. That claude accepts the `=` form for these two flags is part of the 2f.10 real call.
  - Verified by the orchestrator on claude 2.1.302 with a real claude and a fake MCP stdio server (owner test 2f.10 found the defects below):
    - `--tools ""` does NOT hide MCP tools: with it, the init event lists `mcp__lazarobox__find_dir` and the call works.
    - The server-level `--allowedTools=mcp__lazarobox` works (every tool of the server is allowed).
    - The `tool_use` shape is `assistant.message.content[].type == "tool_use"` with `name` `mcp__lazarobox__<tool>` (what the wired decoder already allow-lists).
    - `--safe-mode` DOES hide MCP servers ("CLAUDE.md, skills, installed plugins, hooks, MCP servers, ..."): with it the init event shows `tools=[] mcp=[]`. So a wired turn replaces it by `--setting-sources=` (empty; `=` form, checked to parse with a real claude) and gets `mcp=[lazarobox: connected]`, `skills=[]`, no slash commands and only builtin plugins. Text-only turns, and the retry without the shim, keep `--safe-mode`. `ClaudeWiring.drops` names the flag the wiring replaces; `Wired` removes its first match before appending the wiring flags.
    - `MCP_TOOL_TIMEOUT` was part of the user's check of 2026-10-17 and passed; 2f.9, 2f.10 and 2f.11 are closed.
  - 2f-fix also changes the system prompt of a turn that RUNS wired (any engine with a session whose `plan()` attempt carries the wiring): the built-in "You are a text assistant. You have no access to files ..." sentence is replaced by the text of `prompts/tools.md` (tools by purpose; a command is only typed, the user presses Enter and the model never sees the output). A user prompt without that sentence gets the block as a last bullet. The adapter builds the job twice: the text-only one (`Job.args`/`Job.stdin`) and `Job.tools` (`ToolsText`: same argv and stdin with the tools prompt); `plan()` swaps the argv in and `process_turn` writes the stdin only on the attempt that carries the wiring, so the text fallbacks (no jail, no shim, `wire_mcp` None, refused layout, untrusted shim) keep the original prompt byte for byte. `find_dir` finds directories only, so the block says "a directory", not "a directory or file".
- [x] 2f.10 [manual] (user) one real call: ask claude to find `curso-rust`; expect a `find_dir` row, then `run_in_pane` prompt, `y` types `cd` with no Enter; ask it to create a file and run `ls`: neither happens; `read_file` asks first. Record the stream-json `tool_use` shape. Verified by the user on 2026-10-17 with a real run.
- [x] 2f.11 GREEN after 2f.9 and 2f.10 pass: add claude to `MCP_VERIFIED`. If either fails, claude stays outside and the failure is recorded in the PR. Done 2026-10-17: claude is in `MCP_VERIFIED`.

## PR9 / 2g: codex wiring (`wiring.rs`, `src/core/ai/chat/codex.rs`, `adapters.rs`)

> As built: `wiring::codex(shim, wire)` renders the five `-c mcp_servers.lazarobox.*` overrides (values are TOML strings escaped by the `toml` crate) and returns the one variable codex's own process env must hold (`LAZAROBOX_MCP_TOKEN`, a `Secret`, so the derived `Debug` hides it). The token is never in argv: `env_vars` only names it, the runtime sets it in the child env and codex forwards it to the shim (UNVERIFIED, see 2g.6). No config file is written. `codex_invocation` carries `Job.mcp` through `wire_for` like claude; `wire_mcp` dispatches on `Job.cli` (before this slice it would have rendered claude's flags for any engine that carried a wire). `fresh_decoder` gives a wired codex turn `CodexDecoder::wired()`: the allow-list (`agent_message`, `reasoning`, `error`, `todo_list`) plus an `mcp_tool_call` whose `server` is exactly `lazarobox` (case-sensitive, string); every other item, a missing or non-string server included, ends the turn. Deviation from the claude split: the text-only codex decoder stays the 4-kind allow-list (it does NOT tolerate unknown benign kinds), because codex already failed closed on every unknown kind and the shell is the tool a missed flag leaves open; loosening it would weaken an existing guarantee. A turn that falls back to text ends its session through the same `Unwire` path as claude. The wired `-c` pairs are appended after the job's argv, i.e. after the final `-`; that clap accepts options after the stdin `-` is part of 2g.7. Honest TDD note: the wiring renderer tests were red on a `todo!()` stub, the wired-decoder acceptance test and the adapter/runtime tests were run red first; the fail-closed decoder cases (other server, other kinds, text-only) pin behaviour that already held and were not red.

- [x] 2g.1 RED: insta snapshot of codex `-c mcp_servers.lazarobox.{command,args,env_vars,tool_timeout_sec=900,startup_timeout_sec=20}`; token in the CLI env, not argv; unverified means byte-identical.
- [x] 2g.2 RED: decoder accepts `mcp_tool_call` items with `server=="lazarobox"` only; anything else fails closed.
- [x] 2g.3 GREEN: implement.
- [x] 2g.4 Gate.
- [x] 2g.5 Docs: README "Tools with codex".
- [x] 2g.6 [manual] (user) `codex --help` and `codex mcp --help`: do `-c mcp_servers.*` apply under `--ignore-user-config`; do `env_vars`, `tool_timeout_sec`, `startup_timeout_sec` exist; how MCP approval behaves in `exec`; codex shell and patch stay disabled. Verified by the user on 2026-10-17 with a real run.
  - Partially verified by the orchestrator on codex-cli 0.162.1: `codex exec -c, --config <key=value>` ("Override a configuration value that would otherwise be loaded from `~/.codex/config.toml`. Use a dotted path (`foo.bar.baz`)... The `value` portion is parsed as TOML") exists, so the `-c mcp_servers.lazarobox.*` form is plausible; `--ignore-user-config` ("Do not load `$CODEX_HOME/config.toml`; auth still uses `CODEX_HOME`") does not say whether `-c` overrides still apply under it; `codex mcp add --env <KEY=VALUE>` ("Environment variables to set when launching the server. Only valid with stdio servers") exists for the config-file route, but the help shows none of the `-c` key names `env_vars`, `env`, `tool_timeout_sec` or `startup_timeout_sec`. Still open for the user: all four keys, `-c` under `--ignore-user-config`, whether `env_vars` forwards a variable from codex's own env to the stdio server (the design relies on it, so the token stays out of argv; the alternative `-c ...env.LAZAROBOX_MCP_TOKEN=...` would put it in argv and is NOT used), MCP approval in `exec`, and that shell and patch stay disabled.
  - Verified by the orchestrator's reviewer on codex-cli 0.162.1 (scratch `CODEX_HOME`, no auth): `-c` placed AFTER the stdin `-` is accepted and applied (a bad `mcp_servers.lazarobox.command=5` gives a config type error whether it is before or after `-`); the full five-key set (`command`, `args`, `env_vars`, `tool_timeout_sec`, `startup_timeout_sec`) loads and reaches the auth stage, so those keys exist. The existing argv (`codex_invocation`) already passes `--ignore-user-config` together with its own `-c` pins (`web_search`, `include_apply_patch_tool`, `tools.view_image`), so `-c` under that flag is already relied on; whether it applies to `mcp_servers.*` specifically is not shown. Still unverified, so 2g.6 and 2g.7 stay unchecked: `-c` overrides under `--ignore-user-config` for `mcp_servers.*`; whether `env_vars` forwards the token to the shim; MCP approval in `exec` with `-s read-only`; whether `--disable apps`/`plugins` gate MCP; the real `mcp_tool_call` shape.
- [x] 2g.7 [manual] (user) one real call (`find_dir` then the `cd` prompt); record the `mcp_tool_call` item shape. Verified by the user on 2026-10-17 with a real run.
- [x] 2g.8 GREEN after 2g.6 and 2g.7 pass: add codex to `MCP_VERIFIED`; else it stays text-only. Done 2026-10-17: codex is in `MCP_VERIFIED`.

## PR10 / 2h: gemini wiring (`wiring.rs`, `src/core/ai/chat/gemini.rs`, `adapters.rs`)

- [x] 2h.1 RED: snapshot of `<turn tmp>/work/.gemini/settings.json` (`mcpServers.lazarobox` with env, `timeout: 900000`, `trust: true`), `--allowed-mcp-server-names lazarobox`, policy allow for `mcpName=lazarobox` above the deny-all; per-turn cwd; unverified means byte-identical.
- [x] 2h.2 RED: decoder allow-lists only lazarobox tool events; name collision with built-in `read_file` handled by the qualified name.
- [x] 2h.3 GREEN: implement.
- [x] 2h.4 Gate.
- [x] 2h.5 Docs: README "Tools with gemini".

> As built: `wiring::gemini(shim, policy, wire)` renders the workspace `settings.json` (`mcpServers.lazarobox`: `command`, `args`, `env` with the token, `timeout: 900000`, `trust: true`), the policy text and two args in the `=` form (`--allowed-mcp-server-names=lazarobox`, `--admin-policy=<file>`; both flags are yargs arrays). `GeminiWiring` has a manual `Debug` that hides the settings. The adapter carries `Job.mcp` through `wire_for`; its argv and its text-only `--policy` file are untouched (so a turn that falls back to text, or an unverified engine, is byte-identical). `wire_mcp` writes `<turn tmp>/work/.gemini/settings.json` (0600, dirs 0700) and `<turn tmp>/gemini-mcp-policy.toml`, and `Plan.cwd` makes the child start in `work` (the jail already grants the turn folder rw, nothing new). `unwrite_mcp` removes `mcp.json`, the policy file and `work` on the retry-without-shim path and when writing fails. `GeminiDecoder::wired()` lets a `tool_use` through only when `tool_name` is a string `mcp_lazarobox_<non-empty>`; gemini's built-in `read_file` has no prefix, so the collision is the qualified name. Text-only stays as it was: any `tool_use` (ours too) ends the turn; unknown event kinds still fail closed in both modes.
> Deviation from the design text: the allow rule is NOT in the same file as the deny-all. Priorities inside one policy file are 0..999 and the deny is already 999, so an allow "above" it cannot exist in the user tier. The installed gemini 0.63.0 bundle (read, not run) puts `--policy` files in the user tier and `--admin-policy` files in the admin tier (above user), so the allow goes through `--admin-policy`, leaving the verified text-only policy file and its argv unchanged. Consequence: gemini ignores `--admin-policy` (with a warning) when the system policies dir already holds a `.toml`; then the allow is not loaded and the server's tools are denied (safe failure, visible in 2h.7).
> Token: the design lists `env` in the settings; the literal token is in that file (0600 in a 0700 per-turn folder, like claude's `mcp.json`). gemini's docs say `env` values may reference `$VAR`/`${VAR}`, which would allow keeping the token out of any file, but whether gemini's sanitisation of the MCP child env keeps such a value is unverified, so it was not used.
> Honest TDD notes: wiring renderer tests were red on a `todo!()` stub (5 failed); the wired decoder positive test was red (1 failed) and the fail-closed cases pin behaviour that already held; the runtime wired test and the stderr redaction test were red (2 failed); the two fallback tests and the text-only-session test pin existing behaviour and were checked by mutation (removing `unwrite_mcp` from the retry makes the refused-shim test fail); the adapter test was red (1 failed).
> 2h review fixes: `wiring::gemini` returns `None` (text-only) when the policy path holds `,` or edge whitespace (gemini splits and trims `--admin-policy`/`--policy` values) or when the shim or socket path holds `$` (gemini expands `$VAR` in settings); `full_args` refuses the turn with a clear error for a job file whose path would be split, since its text-only `--policy` has no fallback. `read_file` already denied the token files under a moved data dir (the tool host passes `<XDG_DATA_HOME>/lazarobox` as an extra secret root); two tests now pin it for the default and a moved dir. The `GEMINI_TOOL_PREFIX` doc says the policy `mcpName` is the real gate and the prefix check is defence in depth.
> 2h manual-test fixes (gemini 0.63.0, fake MCP server): gemini got no tools for two independent reasons. (1) `--skip-trust` sets `GEMINI_CLI_TRUST_WORKSPACE` only after the workspace `.gemini/settings.json` was loaded as untrusted, so `mcpServers` was dropped; `wire_mcp` now returns `GEMINI_CLI_TRUST_WORKSPACE=true` in `Wired.vars` for wired gemini turns only (text turns stay byte-identical; the jail helper `exec`s the CLI without clearing the env, so the var reaches it). The user layer (`~/.gemini/settings.json`) loads whether or not the folder is trusted; `--allowed-mcp-server-names=lazarobox` is what blocks the user's own servers (a test pins it). With the folder trusted the server's `trust: true` now takes effect, so gemini skips its own confirmation for lazarobox tools (intended: lazarobox asks the user itself). Trust also unlocks more than `mcpServers` (gemini 0.63.0 bundle, `chunk-*.js`): it imports every variable of the first `.env` / `.gemini/.env` found walking up from `work/` (`findEnvFile`, `loadEnvironment`), loads the `GEMINI.md` hierarchy upward, project hooks and skills. The generated `settings.json` switches off what a key can: `advanced.ignoreLocalEnv: true` (skips plain `.env` files; schema, `findEnvFile`), `context.memoryBoundaryMarkers: []` (schema: "An empty array disables parent traversal"), `hooksConfig.enabled: false`, `skills.enabled: false`. No setting exists for `.gemini/.env` files above the folder (`findEnvFile` reads them whenever trusted, `ignoreLocalEnv` does not apply to them), so that one stays; the user's own `~/.gemini/.env` is read anyway. Workspace policies need no key: `disableWorkspacePolicies = true` is hard-coded in 0.63.0. `GEMINI_CLI_TRUST_WORKSPACE` is in `SCRUBBED`, so a text turn never inherits it from the user's shell; the wired turn's value is re-applied after the scrub and wins. (2) The admin policy failed schema validation (`rule.0.toolName: Invalid input`), so gemini dropped it and the user-tier deny-all blocked the tool: the rule now has `toolName = "*"` next to `mcpName`.
- [x] 2h.6 [manual] (user) `gemini --help`: workspace settings honoured with `--skip-trust` in a per-turn cwd; policy `mcpName` matching and priority against the 999 deny-all; `trust` and `timeout`; built-in tools stay off. Verified by the user on 2026-10-17 with a real run.
  - Partially verified on gemini 0.63.0 (the orchestrator's `--help`, then a read of the installed docs and bundle, nothing run against the model): `--skip-trust` "Trust the current workspace for this session"; `--policy` "Additional policy files or directories to load (comma-separated or multiple --policy) [array]"; `--admin-policy` exists and is also an array; `--allowed-mcp-server-names` is `[array]`; `--allowed-tools` is deprecated for the Policy Engine; `--approval-mode` takes default|auto_edit|yolo|plan; `-e, --extensions` "if not provided, all extensions are used". The existing argv already passes `-e none` and `--allowed-mcp-server-names none`, so extensions are already off (no risk to flag there). From the docs/bundle: an untrusted workspace ignores workspace settings and MCP servers do not connect (so `--skip-trust` must really trust the per-turn `work` folder: UNVERIFIED); MCP tools are named `mcp_{server}_{tool}` (server names must not hold `_`; `lazarobox` has none); rule matching is by `mcpName`; policy priority in a file is 0..999 and the tier base is added (docs table: Admin above User above Workspace above Default; the bundle loads `--policy` as User and `--admin-policy` as Admin); `env` values may use `$VAR`; `timeout` defaults to 600000 ms; `trust: true` bypasses confirmations for the server.
  - Still unverified, so 2h.6 and 2h.7 stay unchecked: that workspace settings are honoured with `--skip-trust` in `<turn tmp>/work` (and that the server connects); that the admin-tier allow beats the user-tier deny-all 999 in a real run, and that `--admin-policy` is not ignored on the user's machine (a system policy `.toml` disables it); `trust`/`timeout` behaviour; whether `--approval-mode plan` blocks MCP tool discovery or calls; that MCP tools show in `stream-json` as `tool_name: "mcp_lazarobox_<tool>"` (a different name makes the wired decoder refuse our own call: safe but useless); that the token in `env` reaches the shim un-sanitised; that built-in tools stay denied in a wired turn; that a repeated `--allowed-mcp-server-names` (`none` from the base argv, then `lazarobox`) keeps `lazarobox` (yargs array: either concatenated or last wins, both include it).
- [x] 2h.7 [manual] (user) one real call; check tool naming when `read_file` collides. Verified by the user on 2026-10-17 with a real run.
- [x] 2h.8 GREEN after 2h.6 and 2h.7 pass: add gemini to `MCP_VERIFIED`; else it stays text-only. Done 2026-10-17: gemini is in `MCP_VERIFIED`.

## 2i: opencode wiring (DEFERRED)

Conditional on the opencode adapter, which is parked. Not part of this change's apply run; no task is scheduled.

- [ ] 2i.1 [deferred] When the adapter lands, plan RED snapshot, config and permission routes, then `opencode --help` [manual] and one real call before `MCP_VERIFIED`.

## PR11 / 3a: Ollama tools body and decoder (`src/core/ai/chat/{adapters,stream}.rs`)

- [x] 3a.1 RED: `ollama_body` adds the registry's `tools` only with tools on; off is byte-identical; follow-up body carries the tool messages with `tool_name`.
- [x] 3a.2 RED: `OllamaToolDecoder` extracts `tool_calls` from NDJSON lines (split across chunks, several calls, bad args become an error result); no other behavior change in `ndjson_line`.
- [x] 3a.3 GREEN: implement.
- [x] 3a.4 Gate.
- [x] 3a.5 Docs: doc comments.

## PR12 / 3b: Ollama loop and fallback (`src/runtime/chat.rs`, `src/core/ai/chat/adapters.rs`)

- [x] 3b.1 RED: fake Ollama: one round (`find_dir` then text) gives one ToolCall/ToolResult pair; round cap 8 ends with a system line and idle; cancel in the loop closes the socket and sends no further request.
- [x] 3b.2 RED: model rejecting `tools` (any 400 on a request with tools) retries once without `tools`, streams text, assignment unchanged; local `read_file` runs free; `run_in_pane` waits then continues with `sent` or `denied`.
- [x] 3b.3 RED: tool messages not kept: next turn's history holds only user and assistant texts; timeouts, sanitizer and answer cap apply.
- [x] 3b.4 GREEN: implement the bounded loop.
- [x] 3b.5 Gate.
- [x] 3b.6 Docs: README "Tools with Ollama".
- [x] 3b.7 [manual] (user) live Ollama with a tool-capable model: find `curso-rust`, confirm, `cd` typed; record the error text for a model without tools and the `tool_name` field behavior; engine and model unchanged. Verified by the user on 2026-10-17 with a real run. Ollama is in `MCP_VERIFIED` (activation commit, 2026-10-17).

> As built in 3b (`src/runtime/chat/ollama.rs`): `Chats::open_session` builds the turn's `ToolHost` dispatch for Ollama too (same `Waits` approver, `Pending`, ledger and approval mode) but opens no MCP socket; the dispatch rides to the adapter as the task-local `ollama::DISPATCH`, and `runner_with` runs `ollama::tool_turn` when the engine is Ollama, `Tools::On` and a dispatch exists, otherwise the old plain `http_turn` (a turn without tools is byte-identical). A turn is at most `MAX_ROUNDS` = 8 requests; the calls of the 8th reply are not run (no request would follow), and the turn ends with `Notice(CAP_NOTICE)` then `Done(Stop)`. Each round streams through an inner channel that holds back `Done`/`Error`; a text 400 (`HTTP 400...`) on a request with tools and before any round retries once without `tools`, with the original system prompt, and the calls of that reply are ignored (the failed request produced no event); relay buffers are capped at `ANSWER_CAP`/`REASONING_CAP`, the echoed tool results at `RESULTS_CAP` (256 KiB, then `DROPPED`), a panicking call answers `FAILED`, and a newline parts the text of two rounds; the Ollama branch of `open_session` closes the pane's previous hub session. The system prompt of the requests gets `prompts::with_tools`. A call the registry refuses (or the 17th+ of a reply) still gets a tool message (`mcp::refusal_text`, now `pub`). The total limit wraps all rounds. Activation gate: no new constant; `tools_for` already requires the engine in `MCP_VERIFIED`, so the activation PR adds `"ollama"` there after 3b.7. Known limit: `Notice` lines are deduplicated per transcript, so a second cap hit in the same conversation shows no second line.

## PR13 / 4a: User MCP server config (`src/core/ai/{config,tools/servers}.rs`, `src/core/config_edit.rs`)

- [x] 4a.1 RED: `[[ai.mcp.servers]]` parse: name, command, args, env as variable NAMES only, optional `project`; per-entry fallback and startup notices after `ai.tools`; a secret value in env is refused.
- [x] 4a.2 RED (also: a colliding external tool is exposed under its server-qualified name and same-named externals are deduplicated here; 2a only drops them): resolution: global plus project servers for the chat's origin git root, other project excluded, project wins on a name clash, a repo `.lazarobox/mcp.toml` is never read, qualified name `<server>__<tool>` matching `[a-z0-9_-]{1,24}`.
- [x] 4a.3 RED: persistence: save never adds, changes, reorders or removes `ai.mcp.*` bytes (comments, invalid entries, arrays of tables kept).
- [x] 4a.4 GREEN: implement.
- [x] 4a.5 Gate.
- [x] 4a.6 Docs: README config example (user config only).

## PR14 / 4b: MCP client (`src/runtime/mcp/client.rs`)

Split: this PR ships the client over a generic transport (4b.1, client part of 4b.3). Spawn and env (4b.2), the rest of 4b.3, the lifecycle and shutdown, 4b.4 and 4b.5 go to the next PR.

- [x] 4b.1 RED: client handshake (client part done over a generic transport with an in-process fake server; review fixes C1, W1 to W5, S1 to S3 included), `tools/list` (max 64 tools, qualified names, built-ins never shadowed), `tools/call`, timeouts and cancel against a fake server process.
- [x] 4b.2 RED: spawn direct, not jailed, scrubbed env (named variables only), `cwd=$HOME`; nothing spawns on config load or repo open; a dying server gives an error result.
- [x] 4b.3 GREEN: implement reusing the 2c codec. (Client part: codec, handshake, list, call, one deadline for write and wait, cancel. Part 2: `client/spawn.rs` with spawn, env, stderr note, start and shutdown.)
- [x] 4b.4 Gate.
- [x] 4b.5 Docs: doc comments on the trust model.

## PR15 / 4c: Re-exposure and approval (`src/core/ai/tools/mod.rs`, `src/runtime/tools/mod.rs`, `src/ui/components/chat.rs`, `README.md`)

- Note (4a review, carried by 4b): when 4c wires `servers::resolve`, canonicalize BOTH the entry `project` path and the chat's origin root there, not inside the pure `resolve`. A symlinked `project` never matches: that fails closed when adding a server but fails OPEN when a disabled project entry is meant to remove a global one. Also in 4c: route calls with `Qualified::raw_of` and show `Dropped::notice` (4b tests both against fake servers; no production caller yet).
Split (approved, three PRs to main, one after another, each after the previous merge):

- 4c-a pure core: `Call::External`, `parse_call_with` (lookup by `Qualified`, never a split on `__`), `ask_for` External (sanitized, capped), `Tools::On.external`, registry and Ollama body with externals, Ollama decoder and mcp `Server` routing. No live behaviour: the app still denies External (`NO_USER_TOOLS`) and nothing passes externals. Done in this PR; the RED tests of 4c.1 and 4c.2 that are pure are in it.
- 4c-b runtime host: `ExternalSet` (start, list, qualify, call by `raw_of`, shutdown) and the `ToolHost` External branch (only after `y`, D14 wrap and cap, denied/timeout/cancel), against fake servers. Not wired.
- 4c-c live wiring and README: pre-turn start and list with `mcp: started`, `unavailable` and `Dropped::notice` notices, canonicalized `resolve` (symlink test), hub registry with externals, the app prompt (always `y`, even in `auto`), shutdown on close and quit, README "Tus propios servidores MCP".

Interpretation: spawn and `tools/list` happen at the start of a turn with tools on (announced); no `tools/call` before `y`; nothing spawns with tools off, on config load or on repo open.

- [ ] 4c.1 RED: user tools are always write tools; every call needs `y` (prompt shows server, tool, arguments); the server is not contacted before `y`; results wrapped untrusted; declined/timeout/cancel give `denied`. (4c-a: the always-asks and the sanitized, capped prompt text, pure part only; the rest in 4c-b and 4c-c.)
- [ ] 4c.2 RED: user tools appear in the registry for MCP and Ollama only when resolved; tools off hides them. (4c-a: registry, mcp `Server` and Ollama body and decoder take externals; the resolution is in 4c-c.)
- [ ] 4c.3 GREEN: implement.
- [ ] 4c.4 Gate.
- [ ] 4c.5 Docs: README "Your own MCP servers".
- [ ] 4c.6 [manual] (user) declare a real server in the user config; its tool asks every time; a repo-local file is ignored.
- [ ] 4c.7 [manual] Final pass per engine on a real machine: `Whole flow per engine` (find then go there) for each verified engine and Ollama; unverified engines still text-only; record drift before archive.

## PR boundaries (start, finish, verification, rollback)

| PR | Start (base) | Finish | Verification | Rollback |
|----|--------------|--------|--------------|----------|
| 1a | tracker | pure types and tests merged | gate; no app wiring | revert PR; nothing depends on it live |
| 1b | PR1 | run in pane live for all engines | gate; 1b.11 manual | revert; or `[ai.tools] enabled=false` |
| 2a | PR2 | registry, policy, config, menu row | gate | revert; registry unused until 2e |
| 2b | PR3 | read tools callable from tests only | gate | revert; not reachable by engines |
| 2c | PR4 | pure protocol subset | gate | revert; pure code |
| 2d | PR5 | M-base: socket, shim, dispatch, jail test | gate; 2d.9, 2d.10 manual | revert; no engine launches the shim |
| 2e | PR6 | M-host: events, rows, approvals, timers | gate | revert; tools stay Off in requests |
| 2e-3 | PR7 | approval mode live, default `ask` | gate; 2e-3.13 manual | revert; or `approval = "ask"` (the default) |
| 2f | PR7 | claude wiring, gated | gate; 2f.9 to 2f.11 manual | drop the `MCP_VERIFIED` entry: claude is text-only (the same applies to each of the four engines) |
| 2g | PR8 | codex wiring, gated | gate; 2g.6 to 2g.8 manual | drop the entry |
| 2h | PR9 | gemini wiring, gated | gate; 2h.6 to 2h.8 manual | drop the entry |
| 3a | PR10 | tools body and decoder | gate | revert; loop not wired |
| 3b | PR11 | Ollama loop live | gate; 3b.7 manual | `tools: Off` for loopback; revert |
| 4a | PR12 | server config and resolution | gate | revert; keys ignored by older builds |
| 4b | PR13 | client, unused by the host | gate | revert |
| 4c | PR14 | user tools live with approval | gate; 4c.6, 4c.7 manual | remove `[[ai.mcp.servers]]` or revert |

## Coverage: spec capability to slice

- pane-commands: 1a, 1b, 2e-3 (auto-type exception). agent-tools: 2a, 2b, 2e, 2e-3 (Chat approval mode), 3b. lazarobox-mcp-server: 2c, 2d, 2e, 2e-3 (auto round trip), 2f to 2h. chat-engines: 2e to 2h, 3a, 3b. chat-pane: 1b, 2e (the `AUTO` badge is specified in agent-tools, built in 2e-3). modal-input: 1b, 2e-3 (leaf `a`). terminal-session: 1b (pane state queries), 4b (shutdown with MCP processes). configuration, config-persistence, config-menu: 1b (gate), 2a, 2e-3 (`ai.tools.approval`, `Approval` row), 4a. mcp-servers: 4a to 4c.
