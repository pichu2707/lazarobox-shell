# Tasks: Agent Tools (the chat acts in the user's pane, with approvals)

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | ~5,930 total (tests and snapshots included), 17 child PRs of 250-450 lines each |
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
| 2e-3 | Chat approval mode (D18): config key, route, auto-type, foreground check, toggle `a`, badge, menu row | PR7b | ~380 | PR7 | 2e |
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

- [ ] 2d.1 RED: session socket `<runtime>/lazarobox/s-<pid>-<pane>.sock`, dir 0700 (fallback `<data>/run`), socket 0600; wrong/missing token closed unserved; constant-time compare; `peer_cred` UID check; token bound to its chat (A's token on B rejected); token never in argv or logs.
- [ ] 2d.2 RED: cleanup on chat close, stale sockets swept at start (alive_by_comm), removal at exit; session calls a `Dispatch` trait (fake in tests).
- [ ] 2d.3 RED: shim relay generic over `Read`/`Write` via `UnixStream::pair`: handshake `{"lazarobox":1,"token"}` then line copy, 1 MiB cap, malformed lines survive, shim opens no user file, starts no process; `LAZAROBOX` set does not block it.
- [ ] 2d.4 RED: `main.rs` dispatches `__mcp-serve` right after `__jail-exec`, before install-desktop and the nesting guard.
- [ ] 2d.5 RED: `tests/jail.rs` (skipped when `probe()` is Unavailable): jailed helper with the shim granted executes the shim and connects; `/usr/bin/curl` execve and `~/.ssh` read still EACCES.
- [ ] 2d.6 GREEN: implement listener, hub, shim, dispatch.
- [ ] 2d.7 Gate.
- [ ] 2d.8 Docs: doc comments on the transport and trust model.
- [ ] 2d.9 [manual] Landlock on the real kernel: a pathname-socket connect works under the granted rights and `add_rule` accepts the socket inode. If it fails, record it; jailed engines stay text-only (do not widen the jail).
- [ ] 2d.10 [manual] M-base check: start a session by hand, drive the shim with a scripted client for `initialize`, `tools/list`, `find_dir`; confirm the 0700/0600 modes.

## PR7 / 2e: Events, rows, ToolHost, decisions (`src/core/ai/chat/{engine,mod}.rs`, `src/runtime/tools/mod.rs`, `src/runtime/chat.rs`, `src/app.rs`, `src/app/run.rs`, `src/ui/components/chat.rs`)

Fallback split if over 400: 2e-1 (events, rows, ToolHost) and 2e-2 (approval wiring and timers).

- [ ] 2e.1 RED: `StreamEvent::ToolCall{id,name,summary,approval}` and `ToolResult{id,status,summary}` paired exactly once; produced by the host, never parsed from text (imitation line is plain text); rows are `Entry::Tool`, shown, never stored or sent; summaries capped and sanitized.
- [ ] 2e.2 RED: `EngineKind::locality()`: loopback HTTP is Local, every CLI Cloud, whatever the model name.
- [ ] 2e.3 RED: `needs_approval`: `run_in_pane` and External always; `read_file` only Cloud; `find_dir`/`list_dir` never. Cloud `read_file` shows `read <path>`, file not opened until `y`; `n`, Esc, timeout, cancel, close give `denied`; local `read_file` runs free.
- [ ] 2e.4 RED: `Decision{Sent,Denied}` via `Effect::ToolDecision{pane,turn,call,decision}` and `Chats::decide`; `Effect::EndToolSession(pane)` with ChatCancel; one pending per chat, second is denied at once; free tools run around a pending one; stale decision withdraws the prompt.
- [ ] 2e.5 RED: tokio paused time: first-byte and idle timers suspended while `pending > 0`, 15-minute total keeps running, a timer never approves; two chats: closing one ends only its calls.
- [ ] 2e.6 RED: injection in file text wrapped; model `run_in_pane rm -rf ~` still shows the exact command and needs `y`; tools off leaves every request byte-identical; `ChatRequest.tools: Tools{Off,On}`.
- [ ] 2e.7 GREEN: implement `ToolHost`, runner `Option<Arc<ToolHost>>`, rows, decisions; connect session `Dispatch` to the host.
- [ ] 2e.8 Gate.
- [ ] 2e.9 Docs: doc comments; design D17 cross-reference.

### 2e-3: Chat approval mode (design D18; own PR after 2e, base = the 2e branch; 2f then targets the 2e-3 branch)

Files: `src/core/ai/{config,menu}.rs`, `src/core/menu.rs`, `src/core/config_edit.rs`, `src/core/ai/chat/{approval,mod}.rs`, `src/core/prefix.rs`, `src/app.rs`, `src/app/chat.rs`, `src/runtime.rs`, `src/ui/components/chat.rs`. Fallback split if over 400: 2e-3a (config key, route, auto-type, foreground check, toggle, badge) and 2e-3b (menu row and persistence).

- [ ] 2e-3.1 RED: `[ai.tools] approval`: default `ask` (no key, no table); `"auto"` parses; `"AUTO"`, `"yes"`, `true` fall back to `ask` with `ai.tools.approval: expected "ask" or "auto"`; notice order after `ai.tools.enabled`; non-table `ai.tools` reports only `ai.tools: expected a table`.
- [ ] 2e-3.2 RED: pure `approval::route`: `AutoType` only for `(Run, Origin::Tool, Auto)` with `typed_this_turn == 0`, target not focused, tab shown; `Read`, `External`, `Origin::Block` and `Ask` always `Prompt`; a second call in the turn is `Prompt`.
- [ ] 2e-3.3 RED: `ChatState.approval_mode` set from config at open; toggle flips only that chat; config change reaches only chats opened later; `auto_typed` reset at turn start; mode never in store commands or requests; requests byte-identical in `ask` and `auto`; tools off byte-identical.
- [ ] 2e-3.4 RED: auto-type in the App harness: no confirm, one `Effect::AutoType{pane,bytes,turn,call}` to `last_shell`, bytes from the shared `typed_bytes` (same as `y`), no `\r`; rechecks (pane gone, other tab, alt screen `\e[?1049h`, multi-line with paste off, escape in command) give `ToolDecision Denied` with reason and a result row, nothing written; pending approval gives `another approval is pending`; toggling to `auto` leaves a pending confirm pending; focused target and hidden tab open the prompt; two calls in one batch: one typed, one prompt.
- [ ] 2e-3.5 RED: runtime foreground check: `tcgetpgrp` on the master equal to the shell's group writes and decides `Sent`; a child in the foreground (fake PTY running `sleep`), an error or a non-unix target decide `Denied("pane N is running a program")` and write nothing.
- [ ] 2e-3.6 RED: prefix leaf `a` → `PrefixAction::ToggleApproval`: root only, unhinted, non-empty description, root hint unchanged (79 columns), action count 34; on a chat pane flips the mode and adds the system line, also with a pending approval; shell pane and tools off only notice; inside a group, COPY or RESIZE nothing.
- [ ] 2e-3.7 RED: TestBackend + insta: ` AUTO ` badge (`bg_base` on `error_red`, bold) top-right in conversation, list and editor views, focused or not; `A` below 6 columns; no marker in `ask` (existing snapshots unchanged); no marker with tools off; 1x1 to 10x2 do not panic.
- [ ] 2e-3.8 RED: menu `Approval` row after `Tools` (`ask`/`auto`, own `ItemKind`), cycle keys, Enter repeat ignored, footer, Esc reverts, no live effect on open chats or a pending approval; navigation wraps from `Approval`; body 19 rows, full popup at 24x22 and 80x24, fallback at 23x22 and 24x21; descriptor integrity.
- [ ] 2e-3.9 RED: persistence of `ai.tools.approval`: created next to `enabled`, updated in place with comment, written explicitly when equal to the default, untouched when unchanged, `[ai.tools.approval]` collision refused with the footer, saved file parses back.
- [ ] 2e-3.10 GREEN: implement `ApprovalMode`, `route`, `typed_bytes` (extracted from the 1b accept path, no behaviour change for `y`), `Effect::AutoType`, foreground check, toggle, badge, menu row, owned key.
- [ ] 2e-3.11 Gate.
- [ ] 2e-3.12 Docs: README "Approval mode" (`Ctrl+Space a`, `[ai.tools] approval`, typed never run, one per turn, the residual risks of design D18); doc comments.
- [ ] 2e-3.13 [manual] Kitty: `auto` types `cd` unexecuted in bash, zsh and fish; refused while `python3` runs in the target; `AUTO` visible on an unfocused chat; `Ctrl+Space a` reaches the app.

## PR8 / 2f: claude wiring (`src/core/ai/tools/wiring.rs`, `src/core/ai/chat/{claude,adapters,engine}.rs`, `src/runtime/chat/cli.rs`, `src/core/ai/jail.rs`)

- [ ] 2f.1 RED: `Job.tmp_files: Vec` generalizes `tmp_file`; files 0600 in the per-turn tmp dir (0700), wiped after the turn.
- [ ] 2f.2 RED: insta snapshot of claude argv/env/mcp.json with tools on: `--mcp-config`, `--strict-mcp-config`, `--tools ""`, `--allowedTools mcp__lazarobox`, `MCP_TOOL_TIMEOUT=900000`; token only in the config env entry, never argv.
- [ ] 2f.3 RED: engine not in `MCP_VERIFIED`: argv, stdin and env byte-identical to tools off.
- [ ] 2f.4 RED: `ShimGrant`: jail differs by the shim binary (rx), the socket file rule, and the loader only when it differs from the CLI's; canonical exe, not `/proc/self/exe`.
- [ ] 2f.5 RED: decoder allow-lists only `mcp__lazarobox__*` tool events; any other tool event still fails closed.
- [ ] 2f.6 GREEN: implement; fallback to text-only on any wiring failure.
- [ ] 2f.7 Gate.
- [ ] 2f.8 Docs: README "Tools with claude".
- [ ] 2f.9 [manual] (user) `claude --help` and `claude mcp --help`: confirm `--mcp-config`, `--strict-mcp-config`, `--allowedTools` and the server-level `mcp__lazarobox` syntax; whether `--tools ""` hides MCP tools; the default and effect of `MCP_TOOL_TIMEOUT`.
- [ ] 2f.10 [manual] (user) one real call: ask claude to find `curso-rust`; expect a `find_dir` row, then `run_in_pane` prompt, `y` types `cd` with no Enter; ask it to create a file and run `ls`: neither happens; `read_file` asks first. Record the stream-json `tool_use` shape.
- [ ] 2f.11 GREEN after 2f.9 and 2f.10 pass: add claude to `MCP_VERIFIED`. If either fails, claude stays outside and the failure is recorded in the PR.

## PR9 / 2g: codex wiring (`wiring.rs`, `src/core/ai/chat/codex.rs`, `adapters.rs`)

- [ ] 2g.1 RED: insta snapshot of codex `-c mcp_servers.lazarobox.{command,args,env_vars,tool_timeout_sec=900,startup_timeout_sec=20}`; token in the CLI env, not argv; unverified means byte-identical.
- [ ] 2g.2 RED: decoder accepts `mcp_tool_call` items with `server=="lazarobox"` only; anything else fails closed.
- [ ] 2g.3 GREEN: implement.
- [ ] 2g.4 Gate.
- [ ] 2g.5 Docs: README "Tools with codex".
- [ ] 2g.6 [manual] (user) `codex --help` and `codex mcp --help`: do `-c mcp_servers.*` apply under `--ignore-user-config`; do `env_vars`, `tool_timeout_sec`, `startup_timeout_sec` exist; how MCP approval behaves in `exec`; codex shell and patch stay disabled.
- [ ] 2g.7 [manual] (user) one real call (`find_dir` then the `cd` prompt); record the `mcp_tool_call` item shape.
- [ ] 2g.8 GREEN after 2g.6 and 2g.7 pass: add codex to `MCP_VERIFIED`; else it stays text-only.

## PR10 / 2h: gemini wiring (`wiring.rs`, `src/core/ai/chat/gemini.rs`, `adapters.rs`)

- [ ] 2h.1 RED: snapshot of `<turn tmp>/work/.gemini/settings.json` (`mcpServers.lazarobox` with env, `timeout: 900000`, `trust: true`), `--allowed-mcp-server-names lazarobox`, policy allow for `mcpName=lazarobox` above the deny-all; per-turn cwd; unverified means byte-identical.
- [ ] 2h.2 RED: decoder allow-lists only lazarobox tool events; name collision with built-in `read_file` handled by the qualified name.
- [ ] 2h.3 GREEN: implement.
- [ ] 2h.4 Gate.
- [ ] 2h.5 Docs: README "Tools with gemini".
- [ ] 2h.6 [manual] (user) `gemini --help`: workspace settings honoured with `--skip-trust` in a per-turn cwd; policy `mcpName` matching and priority against the 999 deny-all; `trust` and `timeout`; built-in tools stay off.
- [ ] 2h.7 [manual] (user) one real call; check tool naming when `read_file` collides.
- [ ] 2h.8 GREEN after 2h.6 and 2h.7 pass: add gemini to `MCP_VERIFIED`; else it stays text-only.

## 2i: opencode wiring (DEFERRED)

Conditional on the opencode adapter, which is parked. Not part of this change's apply run; no task is scheduled.

- [ ] 2i.1 [deferred] When the adapter lands, plan RED snapshot, config and permission routes, then `opencode --help` [manual] and one real call before `MCP_VERIFIED`.

## PR11 / 3a: Ollama tools body and decoder (`src/core/ai/chat/{adapters,stream}.rs`)

- [ ] 3a.1 RED: `ollama_body` adds the registry's `tools` only with tools on; off is byte-identical; follow-up body carries the tool messages with `tool_name`.
- [ ] 3a.2 RED: `OllamaToolDecoder` extracts `tool_calls` from NDJSON lines (split across chunks, several calls, bad args become an error result); no other behavior change in `ndjson_line`.
- [ ] 3a.3 GREEN: implement.
- [ ] 3a.4 Gate.
- [ ] 3a.5 Docs: doc comments.

## PR12 / 3b: Ollama loop and fallback (`src/runtime/chat.rs`, `src/core/ai/chat/adapters.rs`)

- [ ] 3b.1 RED: fake Ollama: one round (`find_dir` then text) gives one ToolCall/ToolResult pair; round cap 8 ends with a system line and idle; cancel in the loop closes the socket and sends no further request.
- [ ] 3b.2 RED: model rejecting `tools` (any 400 on a request with tools) retries once without `tools`, streams text, assignment unchanged; local `read_file` runs free; `run_in_pane` waits then continues with `sent` or `denied`.
- [ ] 3b.3 RED: tool messages not kept: next turn's history holds only user and assistant texts; timeouts, sanitizer and answer cap apply.
- [ ] 3b.4 GREEN: implement the bounded loop.
- [ ] 3b.5 Gate.
- [ ] 3b.6 Docs: README "Tools with Ollama".
- [ ] 3b.7 [manual] (user) live Ollama with a tool-capable model: find `curso-rust`, confirm, `cd` typed; record the error text for a model without tools and the `tool_name` field behavior; engine and model unchanged.

## PR13 / 4a: User MCP server config (`src/core/ai/{config,tools/servers}.rs`, `src/core/config_edit.rs`)

- [ ] 4a.1 RED: `[[ai.mcp.servers]]` parse: name, command, args, env as variable NAMES only, optional `project`; per-entry fallback and startup notices after `ai.tools`; a secret value in env is refused.
- [ ] 4a.2 RED (also: a colliding external tool is exposed under its server-qualified name and same-named externals are deduplicated here; 2a only drops them): resolution: global plus project servers for the chat's origin git root, other project excluded, project wins on a name clash, a repo `.lazarobox/mcp.toml` is never read, qualified name `<server>__<tool>` matching `[a-z0-9_-]{1,24}`.
- [ ] 4a.3 RED: persistence: save never adds, changes, reorders or removes `ai.mcp.*` bytes (comments, invalid entries, arrays of tables kept).
- [ ] 4a.4 GREEN: implement.
- [ ] 4a.5 Gate.
- [ ] 4a.6 Docs: README config example (user config only).

## PR14 / 4b: MCP client (`src/runtime/mcp/client.rs`)

- [ ] 4b.1 RED: client handshake, `tools/list` (max 64 tools, qualified names, built-ins never shadowed), `tools/call`, timeouts and cancel against a fake server process.
- [ ] 4b.2 RED: spawn direct, not jailed, scrubbed env (named variables only), `cwd=$HOME`; nothing spawns on config load or repo open; a dying server gives an error result.
- [ ] 4b.3 GREEN: implement reusing the 2c codec.
- [ ] 4b.4 Gate.
- [ ] 4b.5 Docs: doc comments on the trust model.

## PR15 / 4c: Re-exposure and approval (`src/core/ai/tools/mod.rs`, `src/runtime/tools/mod.rs`, `src/ui/components/chat.rs`, `README.md`)

- [ ] 4c.1 RED: user tools are always write tools; every call needs `y` (prompt shows server, tool, arguments); the server is not contacted before `y`; results wrapped untrusted; declined/timeout/cancel give `denied`.
- [ ] 4c.2 RED: user tools appear in the registry for MCP and Ollama only when resolved; tools off hides them.
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
| 2f | PR7 | claude wiring, gated | gate; 2f.9 to 2f.11 manual | drop the `MCP_VERIFIED` entry: claude is text-only |
| 2g | PR8 | codex wiring, gated | gate; 2g.6 to 2g.8 manual | drop the entry |
| 2h | PR9 | gemini wiring, gated | gate; 2h.6 to 2h.8 manual | drop the entry |
| 3a | PR10 | tools body and decoder | gate | revert; loop not wired |
| 3b | PR11 | Ollama loop live | gate; 3b.7 manual | `tools: Off` for loopback; revert |
| 4a | PR12 | server config and resolution | gate | revert; keys ignored by older builds |
| 4b | PR13 | client, unused by the host | gate | revert |
| 4c | PR14 | user tools live with approval | gate; 4c.6, 4c.7 manual | remove `[[ai.mcp.servers]]` or revert |

## Coverage: spec capability to slice

- pane-commands: 1a, 1b, 2e-3 (auto-type exception). agent-tools: 2a, 2b, 2e, 2e-3 (Chat approval mode), 3b. lazarobox-mcp-server: 2c, 2d, 2e, 2e-3 (auto round trip), 2f to 2h. chat-engines: 2e to 2h, 3a, 3b. chat-pane: 1b, 2e (the `AUTO` badge is specified in agent-tools, built in 2e-3). modal-input: 1b, 2e-3 (leaf `a`). terminal-session: 1b (pane state queries), 4b (shutdown with MCP processes). configuration, config-persistence, config-menu: 1b (gate), 2a, 2e-3 (`ai.tools.approval`, `Approval` row), 4a. mcp-servers: 4a to 4c.
