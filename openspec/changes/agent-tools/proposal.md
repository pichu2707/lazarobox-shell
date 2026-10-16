# Proposal: Agent Tools (the chat pane acts, in the user's pane, with approvals)

## Intent

On 2026-10-15 the user asked the 0.14.0 chat (ollama › qwen3:14b) to "find curso-rust and take me there". The chat printed a `find` command and then asked for the path again. Two gaps cause this:

1. The chat cannot run anything. Every adapter is text-only on purpose.
2. A child process cannot change the cwd of the pane's shell.

lazarobox owns the panes' PTYs, so it can do what no standalone CLI agent can: write an approved command, such as `cd ~/x/curso-rust`, into the user's own pane. This change makes the chat act through:

- read-only tools,
- visible, approved commands in the user's pane,
- one tool registry and one approval UX shared by every engine.

The jail is not weakened, and the engine and model are never chosen by lazarobox (`config.resolve(Task)` is untouched).

## Decisions (user, 2026-10-15, authoritative; engram `sdd/agent-tools/product-decisions`)

- **T1 Approval.**
  - Read-only tools (`find_dir`, `list_dir`, `read_file`) run without asking.
  - Every pane write or command needs an explicit `y`, every time.
  - There is no "always allow" in v1.
- **T2 Target pane.**
  - The target is the last focused shell (PTY) pane.
  - The prompt names it, for example `run in pane 2 (~/dev)`.
  - A key cycles to another PTY pane before accepting.
- **T3 Visibility.**
  - Tool calls and results show inline in the transcript, as rows.
  - The approval prompt sits at the composer.
  - There is no agent pane.
- **T4 User MCP servers.**
  - There is a global list in `~/.config/lazarobox` plus per-project extras.
  - They are configured once and re-exposed to every engine.
- **T5 Paths.**
  - Tools may return absolute or `~`-relative paths within `$HOME`.
  - Excluded: `~/.ssh`, `~/.gnupg`, every entry of `CREDENTIAL_DIRS` (`.claude`, `.gemini`, `.codex`, `.aws`, `.kube`, `.docker`, `.password-store`), `~/.config/gh`, `~/.config/gcloud`, `~/.local/share/keyrings`, `~/.local/share/lazarobox`, and, at any depth, `.env`/`.env.*`/`.envrc`, `.netrc`, `.git-credentials`, `.pgpass`, `.npmrc` and shell or REPL history files.
- **T6 Execution.**
  - State changes run ONLY in the user's own pane, visible and approved.
  - The AI's own tools are read-only.
  - There are no hidden or background commands, and the jail is not weakened.
- **T7 Agents.** One agent per chat pane.
- **T8 Engine and model.** These come from the user's menu assignment, always. Tool support never switches engine or model.

## Scope

### In Scope (four slices, feature-branch chain on `feat/agent-tools`)

| # | Slice | Rough size |
|---|---|---|
| PR1 | **Run in pane.** A key on a bash fenced block in any engine's answer opens the target-pane confirm. On `y`: sanitize the command, write it as one bracketed paste when the shell has bracketed paste on. lazarobox never presses Enter: the user reviews the line in their own shell and runs it. | ~500 (may split 1a core / 1b UI) |
| PR2 | **lazarobox MCP server.** A per-chat stdio shim (`lazarobox __mcp-serve`) relays to the app over a 0600 unix socket with a session token. It serves the read-only tools and `run_in_pane`. `StreamEvent::ToolCall`/`ToolResult` drive the transcript rows. Adapters inject the server into claude, codex, gemini and opencode, and the CLIs' own tools stay off. | ~1,400 (split 2a registry, tools, path policy / 2b shim and socket / 2c events, rows, approval / 2d adapter injection) |
| PR3 | **Native Ollama tools.** The `/api/chat` `tools` field on the same registry, with a bounded tool loop per turn. A model without tool support degrades to text plus PR1. | ~450 |
| PR4 | **lazarobox as an MCP client.** Global and per-project user servers, spawned by lazarobox and re-exposed through the lazarobox server to every engine. Each call to one of their tools needs `y`. | ~900 (split config / client and proxy) |

Total is about 3,250 lines at roughly 400 lines per PR.

### Out of Scope (non-goals)

- Hidden or background command execution, and the AI running commands in its own process.
- Weakening the jail, or enabling the CLIs' own shell or edit tools.
- "Always allow", allowlists or per-session trust in v1.
- Multi-agent orchestration and worktrees, and an agent pane.
- Automatic selection of engine or model.
- Capturing a pane's command output back to the model (v1 returns sent or denied only).
- Native tools for openai-compat servers (later, on the same registry).

## Capabilities

### New Capabilities
- `pane-commands`: run-in-pane from bash blocks, the target-pane rule and its cycling, command sanitization, bracketed-paste writes, and the accept-time rechecks.
- `agent-tools`: the tool registry, the read-only tools, the path policy, the approval model, the tool events and their transcript rows, and untrusted tool output.
- `lazarobox-mcp-server`: the stdio shim, the socket and token, the tool schemas, the approval round trip, and the per-engine injection contract.
- `mcp-servers`: the user server config (global and project), the client lifecycle, re-exposure and the approval of their tools.

### Modified Capabilities
- `chat-pane`: the run-in-pane action on code blocks, tool call and result rows, and the approval prompt at the composer.
- `chat-engines`: `StreamEvent` gains tool events, the CLI adapters gain MCP injection, and the Ollama adapter gains `tools` and the tool loop.
- `modal-input`: the confirm keys `y`, `n` and cycle-pane, and the run-in-pane binding.
- `terminal-session`: approved writes to a PTY respect the bracketed-paste mode.
- `configuration`, `config-persistence`, `config-menu`: an `[ai.tools]` toggle and the `[[ai.mcp.servers]]` lists (PR4).

## Approach

- Everything goes through one pure registry in `core::ai::tools`: schemas, path policy, sanitizer and approval state machine.
- The runtime executes read-only tools off the render thread.
- `run_in_pane` always becomes `InputMode::Confirm`, then `Effect::WritePty`.
- PR1 is engine-agnostic and needs no model tools.
- PR2 to PR4 reuse the PR1 confirm path, so there is a single approval UX.

## Affected Areas

| Area | Impact |
|---|---|
| `src/core/ai/tools/` (registry, path policy, sanitizer) | New |
| `src/runtime/mcp/` (server shim, socket, client) | New |
| `src/core/ai/chat/{engine,adapters,transcript,yank}.rs` | Modified |
| `src/runtime/chat/cli.rs`, `src/core/ai/jail.rs` (MCP config files and shim exec; no new rights) | Modified |
| `src/app.rs`, `src/runtime.rs` (Confirm, WritePty, events) | Modified |
| `src/ui/components/chat.rs` | Modified |
| `src/core/ai/{config,menu}.rs` | Modified |

## Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| Prompt injection through file content or tool results | High | Tool output is wrapped as untrusted data. The prompt always shows the exact command and the target pane. Every write needs `y`, and nothing auto-approves |
| Escape or control characters in a command (for example an embedded `\x1b[201~` that ends the paste early) | Med | Reject ESC, C0 and C1 controls other than `\n` and `\t`. Render the command visibly before approval |
| A multi-line command runs line by line | Med | Use one bracketed paste and no Enter. When bracketed paste is off, multi-line commands are refused |
| Races with the user's typing or a full-screen app in the target pane | Med | Recheck the pane at accept time. Refuse when the pane is on the alternate screen or is gone. A half-typed line is a design question |
| Secrets leak through `read_file` | Med | Use the path policy (T5), resolve symlinks before checking, and reuse the memory redaction pass |
| Unverified MCP injection: codex `-c mcp_servers.*`, gemini `mcpServers` settings plus a policy allow, opencode config and permission routes per version | High | Verify in design against the pinned versions. An engine that fails verification ships without tools and falls back to PR1 |
| The shim may be unable to run or connect inside Landlock (exec of the shim, pathname socket connect) | Med | Verify in design. No rights are added beyond the shim binary and the socket path |
| A CLI's MCP timeout expires while the user decides | Med | Measure the defaults in design. Return `pending/denied` on timeout, never auto-allow |
| A user MCP server tool has side effects | Med | Every such call needs `y`, because lazarobox cannot verify that a tool is read-only |

## Dependencies

- The opencode adapter (`feat/opencode-adapter`, being built in parallel) lands before PR2.
- Resolved outside this change: the Ollama history does reach `/api/chat` (verified in adapters.rs and engine.rs). The second turn ignored "curso-rust" because the `find` never ran, so the model never saw a path. A separate concern is that the Ollama `num_ctx` is unset.

## Rollback Plan

Revert the merge of the tracker `feat/agent-tools`. There is no data migration. Older builds ignore the `[ai.tools]` and `[ai.mcp]` keys. The adapters return to text-only argv, and stale MCP sockets live in the private runtime dir, which is removed on exit.

## Success Criteria

- [ ] `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check` pass on every slice.
- [ ] PR1: on a `cd` block, the run-in-pane key shows `run in pane N (<cwd>)`. On `y`, the pane's cwd changes (OSC 7). On `n`, nothing is written.
- [ ] With PR2 or PR3: "find curso-rust and take me there" shows a `find_dir` row, then a `run_in_pane` confirm, and on `y` the pane is in that directory. The configured engine and model are unchanged.
- [ ] No command reaches any PTY without a `y` in that prompt. Read-only tools never return paths outside `$HOME` or excluded paths.
- [ ] Commands containing control characters are refused. A multi-line command is one paste, and no Enter is sent.
- [ ] The jail rx/ro/rw lists gain nothing beyond the shim and the socket path.

## Proposal question round

Resolved: two rounds (engram `sdd/agent-tools/product-decisions`), recorded as T1 to T8.

Assumptions confirmed by the user on 2026-10-15:
- (a) `y` ONLY types the command into the pane; lazarobox never sends Enter. The user runs it.
- (b) `run_in_pane` returns only "sent" or "denied", and the model does not see the output.
- (c) Tools are on by default (gated by approvals), with an `[ai.tools]` off switch.
- (d) Every tool of a user MCP server needs `y`.
- (e) A target pane running a full-screen app is refused rather than written to.
