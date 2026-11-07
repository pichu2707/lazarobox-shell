# Archive Report: llamacpp-tools

**Date**: 2026-11-07
**Artifact store**: hybrid (openspec files + engram `sdd/llamacpp-tools/archive-report`)
**Branch**: `docs/openspec-closeouts` (no git operation run by this phase; nothing committed)
**Status**: Archived. Clean close, no gate override. Verdict PASS WITH WARNINGS, 0 CRITICAL.

## Change Summary

llama.cpp (managed or unmanaged) gets the same bounded tool loop as Ollama, over the OpenAI wire (`/v1/chat/completions`). Tool-call fragments are accumulated per `index` and released only after a clean end. A server that rejects `tools` with a `--jinja` message gets one retry as text, and the pane gets one notice. `lmstudio` and `vllm` stay text-only. `llamacpp` joined `MCP_VERIFIED` last, after the manual check on Qwen2.5-7B-Instruct. The code is already on main (v1.14.0).

## Observation IDs (traceability)

| Artifact | Topic key | Engram ID |
|----------|-----------|-----------|
| Explore | `sdd/llamacpp-tools/explore` | #3389 |
| Proposal | `sdd/llamacpp-tools/proposal` | #3390 |
| Spec (delta, pre-rebase) | `sdd/llamacpp-tools/spec` | #3391 |
| Design | `sdd/llamacpp-tools/design` | #3392 |
| Tasks | `sdd/llamacpp-tools/tasks` | #3393 |
| Apply progress | `sdd/llamacpp-tools/apply-progress` | #3394 |
| Verify report | `sdd/llamacpp-tools/verify-report` | #3762 |
| Archive report | `sdd/llamacpp-tools/archive-report` | #3765 |

The file copies in this folder are the canonical content for the files-side trail. For tasks, the file was used (hybrid rule), and it is all `[x]`. The engram copy of the tasks (#3393) was not compared line by line.

## Specs Synced

| Capability | Action | Requirements (before -> after) | Scenarios |
|------------|--------|--------------------------------|-----------|
| `chat-engines` | Updated: 3 ADDED, 1 MODIFIED (`Ollama native tools`, extended, nothing removed), 1 sentence and 1 scenario reworded in `OpenAI-compatible servers` | 44 -> 47 | 30 added in the ADDED requirements, 1 added in `Ollama native tools` (8 scenarios now, 7 before), 1 reworded |

Requirement count is by hand from the `### Requirement:` headings before and after the merge.

### Merge details

- **Rebase done.** The delta's "Rebase needed" header and its "Original baseline note" were removed from the delta before the move, as the verify report (W4) asked. The two normative invariant sentences were kept (tools off byte-identical; Ollama byte-identical). The ordering constraint (archive after `agent-tools` and `coding-tools`) is satisfied: both are archived in `2026-11-02-*`.
- **MODIFIED `Ollama native tools`.** The delta's block is an abbreviated copy of the pending text. Replacing the main block would have dropped the main scenarios (round cap, model without tools, write tool, cancel, tool messages, live tools), so it was NOT replaced. Instead the delta's new normative sentence (per-engine wire selection, no cross-wire use) was appended to the body, and the delta's new scenario `Wires are not mixed` was added before `Live Ollama tools [manual]`. Every existing scenario is kept.
- **Conflict resolved: `OpenAI-compatible servers`.** The main requirement says the body of these servers holds "only the model, the messages ... and the streaming flag". With llamacpp tools on, the body also carries `tools`. The delta does not MODIFY this requirement, so the merge added one exception sentence ("The one exception is `llamacpp` with tools on ... see llama.cpp native tools") and narrowed its scenario `Body content for OpenAI-compatible servers` to "tools off". This is a judgment call not stated by the delta. It is flagged under Deferred follow-ups.
- **ADDED `llama.cpp native tools`** (26 scenarios, including 2 `[manual]`), **ADDED `Verified list gating for llamacpp`** (3 scenarios), **ADDED `llama.cpp tools documented`** (1 scenario). Appended after `A managed llama-server turned on from the menu`.
- No MODIFIED target was missing from main, so no requirement was added as a fallback.
- No `## ADDED`, `## MODIFIED` heading is left in the main spec.

## Tasks

`openspec/changes/archive/2026-11-07-llamacpp-tools/tasks.md`: all 29 tasks (T1.0 to T3.3) are ticked `[x]`. Zero unchecked. No checkbox was changed at archive time, so no stale-checkbox reconciliation was needed.

## Verification

`verify-report.md` (#3762): "PASS WITH WARNINGS (0 CRITICAL, 4 WARNING, 2 SUGGESTION)". `cargo test` lib 9885 passed (one flaky runner test failed once, passed on rerun); `tests/jail.rs` 2 environmental failures; clippy and fmt clean. 24 `[auto]` scenarios covered by passing tests. Manual legs: Qwen2.5-7B passed (T3.0); Qwen3.8-27B and the `--no-jinja` live leg were not recorded.

WARNINGS (none blocks archive):
1. Two `[manual]` legs not run: Qwen3.8-27B IQ4_XS, and the live `--no-jinja` leg (fallback covered by auto tests only).
2. Flaky test `runtime::tools::runner::tests::a_stdout_flood_stops_at_the_cap_and_kills_the_group` (once in the full run).
3. `tests/jail.rs` environmental failures ("jail not enforced"), unrelated.
4. Spec delta needed a rebase onto the main spec. Done in this archive (see Merge details).

## Archive Gate

- No CRITICAL issues. No override requested or used.
- All tasks complete.
- Delta merged into `openspec/specs/chat-engines/spec.md`, as described above.

## Deferred Follow-ups

- The manual legs (Qwen3.8-27B; `--no-jinja` live) should be run and recorded by the owner.
- Owner review of the `OpenAI-compatible servers` exception sentence and scenario change (see Merge details). If the owner prefers a different wording, edit `openspec/specs/chat-engines/spec.md` only.
- Verify SUGGESTIONS: per-pane memory of the rejection (design D9 follow-up), stabilise the stdout-flood test.

## Git Operations Left To The User

No git operation was run by this phase, and this session had no shell or delete tool. The change folder was moved by writing each file to the archive path. The active folder was NOT removed. Nothing has been committed.

To finish the move, the user should run:
- `git rm openspec/changes/llamacpp-tools/{proposal.md,design.md,exploration.md,apply-progress.md,tasks.md,specs/chat-engines/spec.md}` (tracked files; check with `git ls-files`)
- `rm -r openspec/changes/llamacpp-tools` (untracked: verify-report.md)
- `git add openspec/changes/archive/2026-11-07-llamacpp-tools openspec/specs/chat-engines/spec.md`

## Relevant Files

- `openspec/specs/chat-engines/spec.md` (merged)
- `openspec/changes/archive/2026-11-07-llamacpp-tools/` (moved change folder: proposal.md, design.md, exploration.md, apply-progress.md, tasks.md, verify-report.md, specs/chat-engines/spec.md (the delta, header removed), and this archive-report.md)
- `openspec/changes/llamacpp-tools/` (original folder, to be removed by the user, see above)
