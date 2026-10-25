# Roadmap to 1.0.0

Status: draft, 2026-10-25. Current release: 0.23.0.

## What 1.0 promises

**A coding agent in the terminal that is stable and safe.** From a chat pane, a model can
search the project, write files behind a reviewed batch, run allowed commands in a sandbox,
and use git, with the user approving every change that touches disk or history.

1.0 also promises stability:
- no breaking change to `config.toml`, to the chat history database or to the key bindings
  without a migration;
- documented security boundaries.

The agent orchestrator (several coordinated AIs) is **1.1**, built on top of these tools.

## Scope

### 1. coding-tools (blocking)

Change: `openspec/changes/coding-tools`. Chain: `feat/coding-tools-*` on the tracker
`feat/coding-tools`.

| Milestone | Release | Slices | State |
|---|---|---|---|
| P0, T, M1 search | 0.23.0 | P0a, P0b, T, M1-a..c | Published |
| M2 write | 0.24.0 | W-a..W-e4 | Open PRs #310, #312-#317 (W-e4 in review fixes) |
| | | W-f, W-g, W-h, M5-a | To do (W-f..W-h edit `host.rs`) |
| M3 run | 0.25.0 | X-a, X-b, X-c1, X-c2, X-c3, X-d1, X-d2 | To do |
| M4 git | 0.26.0 | G-a, G-b, G-c, G-d, M5-b | To do |
| Close | | Z.1 archive `agent-tools`, then `coding-tools`; Z.2 repoint orchestrator deps; Z.3 | To do |

There is a go/no-go checkpoint on the review UX after W-h, using the app for real.

**Remaining: 16 PRs**, about 6,200 lines by the plan's estimates. Real slices have come in
above estimate, and 2 of the last 4 needed a size exception.

### 2. Open manual checks and bugs (blocking)

| Item | Where | Owner |
|---|---|---|
| Issue #180: AI models not detected correctly | GitHub | User |
| macOS Seatbelt checklist sent to the #180 tester (4.6) | `macos-seatbelt-jail` | User, waiting on tester |
| Real `llama-server` model pick flow (1.6) | `llamacpp-pick-model` | User |
| Kitty, Landlock and per-engine final passes (1b.11, 2d.9, 2d.10, 2e-3.13, 4c.6, 4c.7) | `agent-tools` | User, on a real machine |

`agent-tools` 2i.1 (opencode adapter) is deferred and not part of 1.0.

### 3. Stabilization (blocking)

- **Config freeze:** review every `config.toml` key and default. Remove or rename what is
  odd now, because after 1.0 it needs a migration.
- **Storage:** check that the chat history schema has a versioned migration path.
- **Keys:** audit the key table across chat, review view, menu and panes for conflicts. This
  includes Ctrl+G now and Ctrl+O for the 1.1 orchestrator.
- **Docs:** README and threat model for all four tool families (M5-a, M5-b), an upgrade
  note, and the macOS tester checklist.
- **Release candidate:** ship `1.0.0-rc.1` to crates.io, run it daily for a few days on
  Linux and macOS, then ship `1.0.0`.

### Out of scope (1.x)

- The agent orchestrator, plus worker batches (deferred D20 in coding-tools): 1.1.
- Issue #282, Hugging Face model download.
- Issue #268, the WSL Start-menu app.
- `agent-tools` 2i.1, the opencode adapter.

## Order

1. Finish M2: W-f, W-g, W-h, the UX checkpoint, M5-a. Merge the chain, release 0.24.0.
2. M3 run: X-a..X-d2. Release 0.25.0.
3. M4 git: G-a..G-d, M5-b. Release 0.26.0. Z.1..Z.3.
4. Stabilization, plus the manual checks and #180 in parallel (user).
5. `1.0.0-rc.1`, then `1.0.0`.

## Risks

- **`host.rs` slices** (W-f..W-h, X-d1, X-d2, G-c, G-d): each one needs a seam spike first.
  The orchestrator chain rebases on them later, for 1.1.
- **M3 sandbox:** it touches the jail on both Linux (Landlock) and macOS (Seatbelt). macOS
  coverage depends on CI and the external tester.
- **macOS-only failures** that Linux cannot see, such as case-insensitive file systems or
  `/tmp`. These have already happened twice in M2, so watch the macOS CI on every PR.
