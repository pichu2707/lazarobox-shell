# Archive Report: config-unknown-keys

**Date**: 2026-11-07
**Artifact store**: hybrid (openspec files + engram `sdd/config-unknown-keys/archive-report`)
**Branch**: `docs/openspec-closeouts` (no git operation run by this phase; nothing committed)
**Status**: Archived. Clean close, no gate override. Verdict PASS WITH WARNINGS, 0 CRITICAL.

## Change Summary

Unknown keys in the tables lazarobox owns (`[statusline]`, `[tabbar]`, `[updates]`, `[workspace]`, `[ai]` with every table under it, and each `[[ai.mcp.servers]]` entry) are now reported as `unknown key: <full path>` through the existing startup notice. The file still loads, the key is ignored, and the save keeps unknown keys byte for byte. The code shipped in v1.11.0 (commit `4cd64b02`); this archive closes the SDD trail that was never written at the time.

## Observation IDs (traceability)

| Artifact | Topic key | Engram ID |
|----------|-----------|-----------|
| Explore | `sdd/config-unknown-keys/explore` | not found in engram |
| Proposal | `sdd/config-unknown-keys/proposal` | not found in engram (file copy archived) |
| Spec | `sdd/config-unknown-keys/spec` | not found in engram (file copy archived) |
| Design | `sdd/config-unknown-keys/design` | #3760 |
| Tasks | `sdd/config-unknown-keys/tasks` | not found in engram (file copy archived) |
| Verify report | `sdd/config-unknown-keys/verify-report` | #3761 |
| Archive report | `sdd/config-unknown-keys/archive-report` | #3764 |

Engram holds only the design and the verify report for this change. The file copies in this folder were the source of truth (hybrid mode), so the proposal, spec and tasks were taken from the files. The files are the canonical trail for those three.

## Specs Synced

| Capability | Action | Requirements (before -> after) | Scenarios |
|------------|--------|--------------------------------|-----------|
| `configuration` | Updated: 1 ADDED, 7 MODIFIED (text and scenarios), 2 main-spec sentences amended by the delta prose, 1 further sentence amended (see Merge details) | 20 -> 21 | 6 added, 8 modified scenarios |

Untouched requirements: File location, Fallback and startup notice (except one sentence), Profile key validation, The notify flag, The tool mark key, Managed llama-server (one sentence), Extra llama-server arguments, The idle time of the managed llama-server, Prompt-cache options of the local servers, Approval mode key, Orchestrate slot [S1], and the Purpose section.

### Merge details

- **configuration**. Targeted edits, one per sentence or scenario that differs between the delta's MODIFIED block and main. Each replaced text is identical to the delta's version. The rest of each MODIFIED block already matched main.
  - MODIFIED `Schema and defaults`: body sentence (reporting of unknown tables inside owned tables); scenario `General task table and unknown tasks ignored` (notice is `config (2 problems): unknown key: ai.tasks.general`); scenario `Unknown keys ignored` (only unknown keys in owned tables are reported).
  - MODIFIED `Memory keys`: body (reported as `unknown key: ai.memory.<key>`); scenario `Unknown keys ignored` (notice is `config: unknown key: ai.memory.endpoint`).
  - MODIFIED `Server ports`: body (reported by full path); scenario `Unknown servers and keys are ignored` (three problems, notice `config (3 problems): ...`).
  - MODIFIED `Tools key`: body; scenario `Unknown keys ignored`.
  - MODIFIED `MCP server lists`: body (entry kept, reported); scenario `Unknown keys ignored`.
  - MODIFIED `Workspace key`: body; scenario `Unknown keys ignored`.
  - MODIFIED `Usage keys`: body (reported by full path); scenario `Local engines ignored` (the only problem is `unknown key: ai.usage.billing.ollama`).
  - ADDED `Unknown keys in owned tables` (6 scenarios), appended after `Usage keys`.
- **Sentences amended beyond the delta's MODIFIED blocks.** The delta's own ADDED requirement says it amends these sentences, so they were updated to match:
  - `Fallback and startup notice`: "Problems under `ai.tasks.general` and under unknown task names are not reported" now adds "except the unknown keys themselves, which are reported as `unknown key: <path>`".
  - `Managed llama-server`: `managed` and `model` under another server id are now reported (`unknown key: ai.servers.<id>.managed`).
  - `Orchestrator keys`: unknown keys are reported as `unknown key: ai.orchestrator.<key>`.
- **Sentence amended without an explicit delta instruction (judgment call, see Deferred follow-ups).** `Family toggle keys` said "Unknown keys MUST be ignored, so a build without `github` reads a file that has it." It now also says the keys are reported as `unknown key: ai.tools.<key>`, because the ADDED requirement's general rule covers every owned table, including `[ai.tools]`.
- The delta's MODIFIED blocks were written against an older baseline. Requirements that main gained later (Approval mode key, Family toggle keys, Orchestrate slot, Orchestrator keys, Managed llama-server, Prompt-cache options, and others) are not in the delta and were kept verbatim.
- `## ADDED`, `## MODIFIED` headings and no delta-only headings were copied into main. Requirement names are unique in the merged file.

## Tasks

`openspec/changes/archive/2026-11-07-config-unknown-keys/tasks.md`: all 9 tasks (1.1 to 3.1) are ticked `[x]`. Zero unchecked. No checkbox was changed at archive time, so no stale-checkbox reconciliation was needed.

## Verification

`verify-report.md`: "PASS WITH WARNINGS (0 CRITICAL, 1 WARNING, 2 SUGGESTION)". `cargo test config`: lib 409 passed, 0 failed. Every `[auto]` scenario of the delta maps to a passing test. Clippy not run (optional).

WARNINGS (none blocks archive):
1. The invariant "the menu writes only keys the walk knows" is verified by inspection only. No test enforces it. A new menu row whose key is not added to `note_unknown_ai_keys` would make every menu save raise a notice.

SUGGESTIONS: a menu round-trip test (apply every menu edit to an empty file, assert no problems); move the literal known-key slices into named consts (design D5, separate refactor).

## Archive Gate

- No CRITICAL issues. No override requested or used.
- All tasks complete.
- Delta merged into `openspec/specs/configuration/spec.md`.
- No `Purpose` change (the delta does not touch it).

## Deferred Follow-ups

- The verify WARNING (menu-writes-only-known-keys test).
- The two SUGGESTIONS above.
- Owner review of the `Family toggle keys` sentence amendment (see Merge details). If the owner prefers the sentence unchanged, revert that one line in `openspec/specs/configuration/spec.md`.

## Git Operations Left To The User

No git operation was run by this phase, and this session had no shell or delete tool. The change folder was moved by writing each file to the archive path. The active folder was NOT removed. Nothing has been committed.

To finish the move, the user should run:
- `git rm openspec/changes/config-unknown-keys/proposal.md openspec/changes/config-unknown-keys/specs/configuration/spec.md` (tracked files)
- `rm -r openspec/changes/config-unknown-keys` (remaining untracked files: design.md, tasks.md, verify-report.md)
- `git add openspec/changes/archive/2026-11-07-config-unknown-keys openspec/specs/configuration/spec.md`

## Relevant Files

- `openspec/specs/configuration/spec.md` (merged)
- `openspec/changes/archive/2026-11-07-config-unknown-keys/` (moved change folder: proposal.md, design.md, tasks.md, verify-report.md, specs/configuration/spec.md (the delta, unmodified), and this archive-report.md)
- `openspec/changes/config-unknown-keys/` (original folder, to be removed by the user, see above)
