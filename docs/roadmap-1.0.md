# Roadmap to 1.0.0

Status: draft, 2026-10-29. Current release: 0.30.0.

## What 1.0 promises

**A coding agent in the terminal that is stable and safe.** From a chat pane, a model can
search the project, write files behind a reviewed batch, run allowed commands in a sandbox,
and use git, with the user approving every change that touches disk or history. The chat
and its tools work wherever lazarobox is launched.

1.0 also promises stability:
- no breaking change to `config.toml`, to the chat history database, to the key bindings
  or to the CLI flags without a migration;
- documented security boundaries.

The agent orchestrator (several coordinated AIs) is **1.1**, built on top of these tools.

## Scope

### 1. coding-tools: close (blocking)

Change: `openspec/changes/coding-tools`.

| Milestone | Release | State |
|---|---|---|
| M1 search | 0.23.0 | Published |
| M2 write | 0.24.0 | Published |
| M3 run | 0.25.0 | Published |
| M4 git | 0.26.0 | Published |
| Everything else | 0.29.0 | Finished |

Remaining:

| Item | What |
|---|---|
| Z.1 | Archive `agent-tools`, then `coding-tools` |
| Z.2 | Repoint the orchestrator dependencies |
| Z.3 | Decided: `Evidence.digest` is the cleaned output tail |

### 2. chat-folder (blocking)

Change: `openspec/changes/chat-folder`. Closes issue #394 (`/cd`).

**Goal:** the chat and its tools work wherever lazarobox is launched: rofi, desktop
launcher or terminal.

| PR | Content | Lines |
|---|---|---|
| A1 | Validator, probe, `set_folder`, busy state, memory epoch | ~345 |
| A2 | `/cd` + confirm, folder in `chat.db`, resume | ~320 |
| A3 | Start dir: `--dir`, `[workspace] dir`, `install-desktop --dir` (independent) | ~290 |
| B1 | Picker core | ~330 |
| B2 | Picker integration + README | ~285 |

Order: A1 → A2 → B1 → B2. A3 runs in parallel.

### 3. Close open SDD changes (blocking)

Finish or classify the loose tasks, then archive every completed change.

- `llamacpp-tools`
- `feedback`
- `hf-download`
- `llama-server-install`
- `managed-llama-server-options`
- `process-watcher`
- `quit-names-pane-jobs`

### 4. Open manual checks (blocking)

| Item | Where | Owner | State |
|---|---|---|---|
| Issue #180: AI models not detected correctly | GitHub | User | Closed (done) |
| macOS Seatbelt checklist (4.6) | `macos-seatbelt-jail` | User | Open |
| Real `llama-server` model pick flow (1.6) | `llamacpp-pick-model` | User | Open |
| Kitty, Landlock and per-engine final passes (1b.11, 2d.9, 2d.10, 2e-3.13, 4c.6, 4c.7) | `agent-tools` | User, on a real machine | Open |

### 5. Stabilization (blocking)

- **Config freeze:** review every `config.toml` key and default. Remove or rename what is
  odd now, because after 1.0 it needs a migration.
- **Storage:** check that the chat history schema has a versioned migration path.
- **Keys:** audit the key table across chat, review view, menu and panes for conflicts.
  Ctrl+G is in use now. Ctrl+O is reserved for the 1.1 orchestrator.
- **CLI flags:** review every flag and subcommand option, because they are now stable.
- **Docs:** README and threat model, an upgrade note, and the macOS tester checklist.
- **Release candidate:** ship `1.0.0-rc.1` to crates.io, run it daily for a few days on
  Linux and macOS, then ship `1.0.0`.

### Out of scope (1.x)

- The agent orchestrator, plus worker batches: 1.1.
- Issue #366, chat memory sync to Google Drive.
- Issue #345, the OpenCode engine (and `agent-tools` 2i.1).
- Issue #268, the WSL Start-menu app.

Issue #282 (Hugging Face download) shipped, so it is no longer listed.

## Order

1. chat-folder: A1 and A3 in parallel, then A2, B1, B2.
2. Close the SDD changes (including Z.1..Z.3).
3. Stabilization, plus the user manual checks in parallel.
4. `1.0.0-rc.1`, then `1.0.0`.

## Risks

- **chat-folder must not touch `host/*.rs`.** That is the rebase point with the
  agent-orchestrator chain.
- **`chat.db` gets a new nullable `folder` column (A2).** It must land before the storage
  freeze.
- **macOS-only failures** that Linux cannot see, such as case-insensitive file systems or
  `/tmp`. These have already happened twice, so watch the macOS CI on every PR.
