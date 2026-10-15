# Exploration: Agent Tools (the chat pane acts, with approvals)

> Source note: this exploration was read from the **0.13.0 crate source** (in `~/.cargo/registry`), because the explorer had no git/Bash access. The v0.14.0 changes did not touch the CLI adapters or the jail, so the findings still apply. The proposal phase re-checked `CREDENTIAL_DIRS` (`src/core/ai/jail.rs`) and the pane `bracketed_paste` mode (`src/core/pane.rs`) on the v0.14.0 tracker worktree. Engram: `sdd/agent-tools/explore` (obs 3139).

## Goal

Let the lazarobox chat pane act: run commands, read files and `cd` in the user's pane, with approvals, while keeping the engine and model the user assigned (`config.resolve(Task)`).

## Why

- Product vision: the chat must DO work the way Claude Code does, not only explain.
- lazarobox owns the panes' PTYs, so it can write `cd <dir>` into the user's shell. No child CLI can change its parent shell's cwd.
- Hard constraint: lazarobox never chooses the engine or model.

## Current state (code read)

| Area | Finding |
|---|---|
| `src/core/ai/chat/adapters.rs` | Pinned, text-only argv for each CLI (`Wire`/`Job`/`Invocation`). Claude already passes `--permission-prompts none` and `--tools ""`. Gemini uses a deny-all policy. Codex has shell and patch turned off. |
| `src/core/ai/chat/engine.rs` | `StreamEvent` is `Slow`/`Delta`/`Notice`/`ThinkingDelta`/`Done`/`Error`. There is no tool event. |
| `src/core/ai/jail.rs`, `src/runtime/jail.rs` | Landlock helper `__jail-exec` with rx/ro/rw lists. Exec is allowed only on rx paths, and `/usr/bin` is unreadable. `BindTcp` is handled with no rule. Outgoing connect is OPEN. |
| `src/runtime/chat/cli.rs` | Spawn uses a scrubbed env, a private cwd and tmp, `process_group` and `kill_on_drop`. |
| `src/runtime.rs` | `Effect::WritePty(PaneId, bytes)` goes to `PtyHandle::write` and never blocks. |
| `src/app.rs` | `PaneContent::{Pty,Chat}`. `InputMode::Confirm(Confirm::...)` is the y/n seam. `effective_cwd` comes from OSC 7 or `/proc`. |
| `src/core/ai/chat/yank.rs`, `transcript.rs` | Seams for the last code block and fence parsing. |
| `workspace.rs` | Never exposes absolute paths. |

## Options compared

| Option | Summary | Verdict |
|---|---|---|
| A. The CLIs' own tools | Turn on each CLI's tools inside the jail | Rejected as primary: four different approval models, the jail would need exec of `/usr/bin` and a writable project dir, and cd-into-pane is impossible |
| B. Native tools (Ollama `tools` field) | lazarobox runs the tool loop for servers | Later, on the same registry and approval UX |
| C. lazarobox MCP server | Unix socket or stdio, tools `run_in_pane` plus read-only fs tools, consumed by claude/codex/gemini/opencode | Recommended for CLIs. The CLIs' own tools stay off |
| Run in pane | A user-initiated action on a bash fenced block: approval, then `Effect::WritePty` to a target PTY pane | Recommended first slice: works with any engine, needs no model tools and no jail change |

Recommendation: layered. Slice 1 is run-in-pane, slice 2 is Option C, and Option B follows on the same registry.

## Engine notes (from docs/source; items marked UNVERIFIED need checking in design)

- **Claude**: `--permission-mode` (default/acceptEdits/dontAsk/plan/auto/bypass), `--allowedTools`, `--disallowedTools`, `--permission-prompt-tool mcp__srv__tool` (it receives `tool_name` and `input` and returns allow/deny plus `updatedInput`), `--permission-prompts none`, `--mcp-config`, `--strict-mcp-config`.
- **Codex**: `exec` takes `-s read-only|workspace-write|danger-full-access`. The approval policy is untrusted/on-request/never. Network is off by default in workspace-write. `exec` cannot prompt. The app-server JSON-RPC does prompt (UNVERIFIED).
- **Gemini**: a TOML policy engine with allow/deny/ask_user rules, where ask_user means deny in non-interactive mode. Approval modes are default/auto_edit/yolo/plan. The adapter already writes a deny-all policy to a tmp file passed with `--policy`.
- **opencode**: permissions are allow/ask/deny per tool, plus `opencode run --auto`. Server mode emits permission requests over SSE `/event` and takes replies by POST. The route differs by version (`/session/:id/permissions/:pid` or `/permission/:id/reply`), so pin a version (UNVERIFIED).
- MCP is the only approval seam the engines share. `permission-prompt-tool` exists only in claude.

## Jail notes

- Today the jail gives no exec rights outside the CLI package, and the cwd is a private empty dir.
- A write-enabled agent would need an rw project dir and exec of `/usr/bin` tools. This change avoids that.
- Landlock does not gate network connect today. ABI 4 has `AccessNet::ConnectTcp`, but a "no network" rule would need an allowlist of API ports, and allowlisting by host is hard.

## Open product questions (resolved later in `sdd/agent-tools/product-decisions`, obs 3141)

Approval granularity, network, where actions render (inline or an agent pane), one agent or several, target pane selection, and the config scope of user MCP servers.
