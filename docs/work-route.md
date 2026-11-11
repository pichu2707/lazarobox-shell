# Work route

Status: 2026-11-11. Current release: 1.18.0 (orchestrator milestone 3).

This file is the short-term plan: what is in flight, what we found while testing, and
what comes next, in order. `roadmap-1.0.md` is the closed plan for 1.0.

## 1. In flight

| PR | What | State |
|---|---|---|
| #697 | Connectors area: entry screen, `y` prompts for install, login and probe | Reviewed, CI |
| #699 | Stop a run whose coordinator cannot get its tools | Reviewed, CI |
| #702 | Tell the model a typed command has not run (`NOT RUN YET`) | Reviewed, last nit |
| #700 | Collapse a repeated review notice into one line with a count | Draft, needs review |
| #701 | Show `(default)` on a worker row whose task row is unassigned | Draft, needs review |
| — | Flaky reap tests in `runtime::mcp` (cause first, deadline last) | In progress |
| — | `mcp-connectors` S5: Search Console with a service account | In progress |
| — | Manual steps M15b, M15c, M16, M17 with a cloud coordinator | In progress |

Then: release **1.19.0** (Connectors area, the orchestrator fixes above).

## 2. Found while testing (2026-11-11)

Fixed or in a PR:

| Problem | Where | State |
|---|---|---|
| A batch review stayed open after its run ended, and a late `y` wrote the files | M11c | Fixed, #698 |
| Secret values in MCP failure text, tool results, tool names and `[table]` headers | Review of #695/#696 | Fixed, #695, #696 |
| A Connectors prompt confirmed on `Ctrl+y`/`Alt+y`, and showed a clipped command on a narrow terminal | Review of #697 | Fixed in #697 |
| A coordinator on a CLI engine ran as plain text when the jail refused its tools | M14b | #699 |
| The model said a typed (not run) command had created a file | Owner, chat | #702 |
| A review notice repeated 22 times while a run was paused | M13e | #700 |
| The worker row did not say `(default)` | M14b | #701 |
| `esc_stops_the_probe...` and `shutdown_leaves_no_orphan_grandchild` fail under load | CI, review | In progress |

Open, not started:

| Problem | Notes | Proposed next step |
|---|---|---|
| The jail refuses an MCP shim in a group-writable path | Linux with `umask 002` (`target/release`), macOS Intel Homebrew and `/usr/local/bin` at 775 | Owner decision: keep strict (recommended) or accept a private group; document `chmod g-w` and `~/.local/bin` |
| A worker's `run_command` first answered `refused: path not available`, then ran | Recording of the orchestrator video | Investigate: reproduce and find which path check fails |
| `spawn gate: step authorize is not closed` lines with a cloud coordinator | Recording; local coordinators never close `authorize` | Review the gate wording and whether the first step can close itself |
| A coordinator answers with code instead of delegating unless told to use `spawn_agent` | Recording, haiku and sonnet | Coordinator prompt: say delegation is the default in ORCH |
| `gemini` and `codex` are found in `/usr/local/bin` even with a stripped `PATH` | Manual tests sent synthetic diffs to Google by accident | Document engine discovery; tests must pin every task row |
| Closing the menu during a probe loses its Esc | Review of #696 | Accepted: the probe ends within 20 s. Optional: cancel on close |
| Clarity "delete token" wording | README "Desconectar" | Check the control in the Clarity UI |

## 3. Next, in order

1. Merge the PRs in section 1, then release 1.19.0.
2. Manual steps that need the owner:
   - 4.6: Clarity with a real token, GA4 with a real property.
   - 5.3: a Search Console query on a property the service account was added to.
3. `mcp-connectors` S6: Google Workspace (Gmail drafts only, Docs, Sheets) and Apps Script,
   including 6.4 (verify the installed binary names `analytics-mcp`, `clarity-mcp-server`
   and the workspace pre-authorization).
4. Archive `orchestrator-worker-flow` once M15–M17 pass, then `mcp-connectors` after S6
   and its live steps. The second one archived rebases the config-menu "Sections and items
   are data" block.
5. Later items: L1 WSL (`BROWSER`), L2 hot reload of MCP servers, S7 profile default
   entries, S8 Gmail send with an explicit permission (undecided).
6. Proposals waiting for a decision: #664, #666, #668, #671, #672.
