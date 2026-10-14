# Archive Report: ai-chat

**Date**: 2026-10-14
**Artifact store**: hybrid (openspec files + engram `sdd/ai-chat/archive-report`)
**Branch**: `chore/openspec-archive` (from origin/main `fd8e2dc`)
**Status**: Archived. Verdict PASS WITH WARNINGS, 0 CRITICAL. No gate override. PR14 is NOT part of this archive (split out, see below). 13 manual tasks are accepted open.

## Change Summary

The chat pane (P2): a second kind of pane in the tile tree, opened with `Ctrl+Space c`, that talks to the engine assigned to the `chat` task. It ships streaming chat with `ollama` (server) and with the `claude`, `codex` and `gemini` CLIs (text only, run under a Landlock jail), a SQLite conversation store with project tags, resume and delete, the `analyst` / `programmer` prompt profiles with file overrides, a workspace context block, a statusline thinking bar with folded reasoning, and the `Profile` row in the AI menu.

Adapters shipped: `ollama`, `claude`, `codex`, `gemini` (confirmed in code: `adapter_for` in `src/core/ai/chat/adapters.rs:80` returns an adapter for exactly these four ids and `None` for everything else; `has_adapter` in `src/core/ai/chat/engine.rs:187` is `adapter_for(id).is_some()`). `lmstudio`, `llamacpp`, `vllm` and `opencode` resolve to the unsupported state, guidance `<engine>: not supported in chat yet`.

## What Shipped (PRs and releases)

Tracked as PR-per-slice into the tracker, all merged to `main`:

- 0.7.0: #123, #124 (pane-kind seam)
- 0.8.0: #127 to #132, #134, #135, #137 to #143 (availability, history, send, stream, sanitizer, splitters, loopback, runtime task, close lifecycle, SQLite open, store thread, composer, live pane, Ollama)
- 0.9.0: #153 (profile), #154 (prompts)
- 0.11.0: #165 (history), #166 (tags)
- 0.12.0: #169 (CLI jail), #172 (claude), #174 (codex), #175 (gemini)

## Split: PR14 moved to `ai-chat-openai-compat`

PR14 (OpenAI-compatible adapter for `lmstudio`, `llamacpp`, `vllm`; tasks 14.1 to 14.5) was deferred by the user and moved to the new ACTIVE change `openspec/changes/ai-chat-openai-compat/` (proposal, tasks, a chat-engines delta). Before merging, the ai-chat `chat-engines` delta was amended so the archived spec matches the code: four adapters, the SSE decoder requirement and its scenarios removed, the OpenAI reasoning and real-engines scenarios reduced to Ollama only, a pointer to the follow-up added. Tasks 14.1 to 14.5 in `tasks.md` are marked MOVED and are not open work here. Design history is not rewritten: `design.md` got a one-line archive note under the title for the OpenAI-compatible references (CHAT-2 `Adapter::OpenAi`, C12, M7 SSE `[DONE]`).

Known residue, left as written: task 20.3 still says "Ollama and an OpenAI-compatible server stream and cancel" (accepted-open, to be read as Ollama only), and `ai-task-models` still lists `lmstudio` in picker examples (the picker lists detected models of any server engine; this is detection data, not chat support).

## Observation IDs (traceability)

| Artifact | Topic key | Engram ID |
|----------|-----------|-----------|
| Verify report | `sdd/ai-chat/verify-report` | #3117 |
| Archive report (partial, superseded) | `sdd/ai-chat/archive-report` | #3124 |
| Proposal, spec, design, tasks | `sdd/ai-chat/{proposal,spec,design,tasks}` | not re-fetched in this run; the files in this folder are canonical |

## Specs Synced

| Capability | Action | Requirements (before -> after) | Scenarios (before -> after) |
|------------|--------|--------------------------------|-----------------------------|
| `ai-engine-detection` | 3 MODIFIED | 10 -> 10 | 39 -> 44 |
| `ai-task-models` | 1 ADDED, 1 MODIFIED | 10 -> 11 | 24 -> 32 |
| `chat-engines` | Created (new, four adapters) | 0 -> 13 | 0 -> 94 |
| `chat-history` | Created (new) | 0 -> 11 | 0 -> 63 |
| `chat-pane` | Created (new) | 0 -> 14 | 0 -> 78 |
| `config-menu` | 1 ADDED, 3 MODIFIED | 17 -> 18 | 84 -> 95 |
| `config-persistence` | 1 ADDED, 1 MODIFIED | 8 -> 9 | 48 -> 57 |
| `configuration` | 1 ADDED, 1 MODIFIED | 3 -> 4 | 24 -> 31 |
| `copy-mode-scrollback` | 2 ADDED | 9 -> 11 | 33 -> 38 |
| `modal-input` | 1 ADDED, 4 MODIFIED | 11 -> 12 | 55 -> 74 |
| `pane-layout` | 2 ADDED | 8 -> 10 | 29 -> 41 |
| `prompt-profiles` | Created (new) | 0 -> 9 | 0 -> 47 |
| `statusline` | 2 ADDED, 2 MODIFIED | 10 -> 12 | 38 -> 62 |
| `tabs` | 2 ADDED | 6 -> 8 | 31 -> 37 |
| `terminal-session` | 3 ADDED, 3 MODIFIED | 10 -> 13 | 29 -> 46 |

Counts are from `### Requirement:` and `#### Scenario:` headings before and after the merge. Every MODIFIED requirement replaced the same-named requirement in place; ADDED were appended; every MODIFIED target existed by name. New capabilities became full specs (title, Purpose with its Terms paragraph, `## Requirements`). Delta preambles (tag legends and "unchanged" notes) and all `(Previously: ...)` lines were dropped, as in earlier archives. No delta heading is left in any main spec.

## Tasks

`tasks.md`: 122 checked, 13 unchecked, plus 5 MOVED (PR14) that are not counted as open. All 13 unchecked are `[manual]` or final-verification items, all accepted open by the user:

- 9.10 Kitty keys (Ctrl+J, Alt+Enter, Ctrl+Y, Ctrl+B, Ctrl+R)
- 9.11 real look of the thinking bar and spinner in Kitty
- 10.7 `Ctrl+Space T` reaches the app in Kitty
- 13.5 M7 live Ollama (404, cold load, reasoning model)
- 15.6 M12 strace baseline of a jailed node script (strace not installed)
- 16.8 M11b `CLAUDE_CONFIG_DIR` follow-up
- 20.1 to 20.6 and 20.3a: final manual walkthrough, README facts, M1 to M14 drift re-check

### Count reconciliation

Verify #3117 said "28 unchecked, 122 checked". The file at both `4af264a` and `fd8e2dc` has 18 unchecked and 122 checked (140 total). The 18 are the 13 above plus the 5 PR14 tasks (14.1 to 14.5). The verify list itself names exactly those 18 items, so "28" was a counting slip (10 too high) and not a real discrepancy. After the split, 13 remain in ai-chat and 5 live in `ai-chat-openai-compat`.

## Verification

`verify-report` (#3117) at `4af264a` (0.12.0): 0 CRITICAL, 2 WARNING, 2 SUGGESTION. fmt, clippy `-D warnings` clean; `cargo test` 3399 lib + 13 jail + 1 integration passed, 4 live tests ignored. The latest gate on `main` `fd8e2dc` (from the engram-memory verify): 3446 lib + 13 + 1 passed.

- WARNING 1 (PR14 not implemented while the spec described its adapters): resolved by the split and the spec amendment above.
- WARNING 2 (manual scenarios unverified): recorded as accepted-open above. Not code defects.
- SUGGESTION 1 (split PR14 and amend the spec): done. SUGGESTION 2 (install strace for M12): not done; 15.6 stays open.

## Relevant Files

- `openspec/specs/{ai-engine-detection,ai-task-models,chat-engines,chat-history,chat-pane,config-menu,config-persistence,configuration,copy-mode-scrollback,modal-input,pane-layout,prompt-profiles,statusline,tabs,terminal-session}/spec.md`
- `openspec/changes/ai-chat-openai-compat/` (active follow-up change)
- `openspec/changes/archive/2026-10-14-ai-chat/` (this folder)
