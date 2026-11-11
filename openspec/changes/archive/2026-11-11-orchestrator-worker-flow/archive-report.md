# Archive Report: orchestrator-worker-flow

**Date**: 2026-11-11
**Artifact store**: openspec (files) + engram `sdd/orchestrator-worker-flow/archive-report`
**Status**: ARCHIVED (specs merged, folder moved; the PR is pending merge)
**Location of this file**: `openspec/changes/archive/2026-11-11-orchestrator-worker-flow/archive-report.md`.

## Change Summary

Orchestrator worker flow, milestone 1 (W1a, W1b, W2a, W2b), milestone 2 (W10a, W10b, W3, W4) and milestone 3 (W11, W5, W6, W6c, W7, W8a, W8b, W8c, W9a, W9b). Workers do real work: the disclosure of silent outcomes, `Write` and `Exec` ceilings with `/write` and `/exec`, a worker prompt that asks for tools, parallel writers with one batch per worker, step gates with an `Enter gates off` override, a handoff check against the ledger, a `simple` task with a brief, clarification rounds, and a 400-line limit ask. ORCH is off by default.

## Observation IDs (traceability)

| Artifact | Topic key | Engram ID |
|----------|-----------|-----------|
| Proposal | `sdd/orchestrator-worker-flow/proposal` | #3684 |
| Owner's second question round (decision) | `sdd/orchestrator-worker-flow/proposal-review` | #3685 |
| Spec (delta, 12 domains) | `sdd/orchestrator-worker-flow/spec` | #3687 |
| Design | `sdd/orchestrator-worker-flow/design` | #3690 |
| Tasks, milestone 1 | `sdd/orchestrator-worker-flow/tasks` | #3697 |
| Tasks, milestone 2 | `sdd/orchestrator-worker-flow/tasks-milestone-2` | #3725 |
| Tasks, milestone 3 | not found in engram by search; the file is authoritative | n/a |
| Verify report | `sdd/orchestrator-worker-flow/verify-report` | #3829 |
| Archive report | `sdd/orchestrator-worker-flow/archive-report` | this report (saved with mem_save) |

The files in the change folder are the canonical content. Engram copies were not compared line by line.

## Verification

`verify-report.md`: "PASS WITH WARNINGS". CRITICAL 0, WARNING 2, SUGGESTION 3. The two WARNINGs and SUGGESTION 1 were resolved by owner decision on 2026-11-11 (spec notes and D30 in the design, and the stale tasks note fixed). Known gaps carried forward (from the verify report): D2 (whether each CLI engine enforces `additionalProperties`), M8 (live Ollama behaviour for D12), W1 (whether the disclosure identifies the original trigger). SUGGESTION 2 (no name-based scenario-to-test map) and SUGGESTION 3 (tests must not run from a worktree under `/tmp`) carry forward as notes.

## Archive Gate

- CRITICAL issues: none. No override used.
- Implementation tasks: every checkbox in `tasks.md` is `[x]`; no `- [ ]` line exists in the file (checked on 2026-11-11). No stale-checkbox reconciliation was needed.
- Manual acceptance (M11-M17): recorded in `tasks.md` under "Manual acceptance results, milestone 3". Results:
  - M11: a PASS, b PARTIAL (120 s not reachable with fast workers), c PASS (found an app bug, fixed in PR #698), CLI 15-minute wait NOT RUN.
  - M12: a PASS, b PASS, c PARTIAL (wrapped `collect` output not visible in the UI), d PARTIAL (simple worker tool list only), e PASS, f PASS.
  - M13: a, b, c PASS; d NOT RUN (a review could not be made to exceed `worker_timeout` safely); e PASS (notice flood fixed in PR #700).
  - M14: a PASS, b PASS (`(default)` row marker added in PR #701).
  - M15: a PASS (Ollama), b PASS, c PASS, d recorded (qwen3:14b fills flat briefs with thinking 1/3; others did not).
  - M16 PASS (claude sonnet coordinator, haiku workers; round 1 of 2, answers, round 2 of 2, limit reached, run stopped).
  - M17 PASS (limit ask at 450 lines, keep, split, later batch asks again, outside-brief ask, `.env` refused, test files not counted).
  - Partial and not-run items are plain bullets, not checkboxes, so they are not read as stale tasks.

## Specs Synced

Twelve delta specs merged into `openspec/specs/{domain}/spec.md`. ADDED requirements were appended at the end of the Requirements section; a MODIFIED requirement replaced the whole block of the requirement with the same name (matched ignoring the slice tags in brackets, and the delta's header and `(Previously: ...)` line were kept). No REMOVED or RENAMED requirement existed in any delta. The delta preambles and the "Open points" and "Stale statements" sections are not merged.

| Capability | Added | Modified | Removed |
|------------|-------|----------|---------|
| `agent-orchestrator` | 16 | 6 (Worker contract, Worker toolsets, ODD flow, ODD test-first evidence, Receipt-driven review, Final documentation) | 0 |
| `agent-tools` | 0 | 2 (Toolsets by session role, read_pane tool) | 0 |
| `ai-task-models` | 2 | 1 (Fixed task set) | 0 |
| `chat-pane` | 3 | 0 | 0 |
| `code-write` | 1 | 1 (Worker batches) | 0 |
| `command-runner` | 1 | 1 (Approval on every command) | 0 |
| `config-menu` | 1 (Simple row and picker; merged as is) | 0 | 0 |
| `configuration` | 1 | 0 | 0 |
| `lazarobox-mcp-server` | 0 | 1 (Tools served) | 0 |
| `modal-input` | 4 | 0 | 0 |
| `orchestrator-handoffs` | 2 | 0 | 0 |
| `prompt-profiles` | 4 | 2 (Prompt per task for workers, ODD playbooks) | 0 |

Checks: no `## ADDED` / `## MODIFIED` heading remains in `openspec/specs/`; every requirement header of every delta appears exactly once in its main spec.

### Stale statements left for a follow-up

The deltas of `config-menu`, `ai-task-models` and `agent-orchestrator` list existing statements that the new behaviour makes stale (Models rows scenarios without `simple`, `Runtime use of every task assignment` worker kinds, the S10 scenarios "Small task from the chat" and "Missing dependency"). They were NOT rewritten in this merge; a small follow-up edit of the main specs should reword them.

### Coordination note (for mcp-connectors)

Per the design's coordination rule: `orchestrator-worker-flow` is archived FIRST, so its `config-menu` delta is merged as is. The `mcp-connectors` change is archived SECOND and MUST rebase its config-menu "Sections and items are data" block onto this merged text before it archives. Do not edit that block here.

## Git Operations

Branch `docs/archive-orchestrator-worker-flow` from `origin/main`, commit `docs(openspec): archive orchestrator-worker-flow`, PR opened and not merged (number in the engram record).

## Deferred Follow-ups (not in this change)

- Coordinator without tools: PR #699.
- Typed but not run: PR #702.
- Wording nits (WRITE_OFF text for simple briefs; brief number in the disclosure line): a separate PR.
- Owner to run M11b (120 s), M11 CLI (15-minute wait), M13d when a safe setup exists, and M12c/M12d in a richer setting.
- Verify known gaps D2, M8, W1 (see above).

## Relevant Files

- `openspec/changes/archive/2026-11-11-orchestrator-worker-flow/tasks.md` (manual results recorded)
- `openspec/changes/archive/2026-11-11-orchestrator-worker-flow/archive-report.md` (this report)
- `openspec/changes/archive/2026-11-11-orchestrator-worker-flow/verify-report.md` (unchanged)
- `openspec/specs/*/spec.md` (12 merge targets, changed)
- `openspec/changes/archive/2026-11-11-orchestrator-worker-flow/` (archive location)
