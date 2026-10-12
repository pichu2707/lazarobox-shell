# Archive Report: ai-selector

**Date**: 2026-10-12
**Artifact store**: hybrid (openspec files + engram `sdd/ai-selector/archive-report`)
**Tracker branch**: `feat/ai-selector` (PR #120 merged at `bab5c39`)
**Archive branch**: `feat/ai-selector-archive` (created from tracker at `bab5c39`; not committed by this phase)
**Status**: Archived. Clean close, no gate override. Verdict PASS WITH WARNINGS, 0 CRITICAL.

## Change Summary

Per-task AI engine and model in lazarobox-shell. The AI section stops cycling a single `Engine` and `Model` with `h`/`l`. It shows a fixed list of five tasks (`general`, `code`, `chat`, `review`, `summarize`) plus a `Set all tasks` row. Each row is `engine › model`, chosen from a two-level picker in the same popup. Local servers list detected models (`✓`) and a curated, non-selectable list of not-downloaded Ollama models (`download required`). CLI engines offer a static catalog and `custom…`. `s` saves, Enter no longer saves, and `[ai.tasks.<task>]` tables are written with nested TOML edits. W2 (silent Enter before the first detection) and S1 (undetected saved engine survives) are closed.

## Observation IDs (traceability)

| Artifact | Topic key | Engram ID |
|----------|-----------|-----------|
| Proposal | `sdd/ai-selector/proposal` | #2960 |
| Spec (reconciled) | `sdd/ai-selector/spec` | #2962 |
| Design | `sdd/ai-selector/design` | #2963 |
| Tasks | `sdd/ai-selector/tasks` | #2965 |
| Verify report | `sdd/ai-selector/verify-report` | #2981 |
| Archive report | `sdd/ai-selector/archive-report` | #2982 |

The file copies in this folder are the canonical content for the files-side trail. Engram observations are the source the copies were taken from.

## Specs Synced

| Capability | Action | Requirements (before -> after) | Scenarios (before -> after) |
|------------|--------|--------------------------------|-----------------------------|
| `ai-task-models` | Created (new capability, delta copied as full spec) | 0 -> 10 | 0 -> 24 |
| `config-menu` | Updated: 5 ADDED, 11 MODIFIED, 2 REMOVED, Purpose replaced | 14 -> 17 | 49 -> 84 |
| `config-persistence` | Updated: 1 RENAMED (+1 MODIFIED), 5 MODIFIED, 1 ADDED, Purpose unchanged | 7 -> 8 | 30 -> 48 |
| `configuration` | Updated: 2 MODIFIED, Purpose replaced | 3 -> 3 | 16 -> 24 |

Counts are by hand from the requirement and scenario headings of the main specs before the merge and of the merged files after it. Config-menu: 34 scenarios in MODIFIED requirements before, 51 after; 5 unchanged in `Live preview and revert`; 10 removed with `AI section layout` and `Engine and Model choices`; 28 added in the five new requirements.

### Merge details

- **ai-task-models** (new): the delta was already a full spec with the main-spec Purpose and Requirements headings. Copied as-is to `openspec/specs/ai-task-models/spec.md`, with no delta-only headings.
- **config-menu**:
  - ADDED: `AI task list`, `Task picker`, `Custom model entry`, `Picker viewport`, `Detection cues`.
  - MODIFIED: `Open and close`, `Sections and items are data`, `Navigation`, `Value cycling`, `Input is swallowed`, `Footer`, `Tiny terminals`, `Events while open`, `Detection results update the open menu`, `Refresh key`, `AI draft, revert and save`. Each replaces the main requirement of the same name.
  - REMOVED: `AI section layout`, `Engine and Model choices`.
  - Untouched: `Live preview and revert`.
  - Purpose replaced. The delta's "Terms:" paragraph (definitions of task list, picker, custom field, disabled row) was placed as a second paragraph of the Purpose section, because the main spec has no other place for non-requirement prose.
- **config-persistence**:
  - RENAMED `Save on Enter` to `Save on s` (kept at the same position). Its body was then replaced by the MODIFIED text.
  - MODIFIED: `Owned keys only` (10 -> 16 scenarios), `Create file and directories`, `Refuse on errors`, `Failure feedback` (+1 scenario), `AI keys collisions` (4 -> 8 scenarios).
  - ADDED: `Nested table edits` (4 scenarios).
  - `Atomic write` untouched. Purpose unchanged, as the delta states.
- **configuration**: MODIFIED `Schema and defaults` (6 -> 10 scenarios) and `Fallback and startup notice` (8 -> 12 scenarios). `File location` untouched. Purpose replaced.
- All `(Previously: ...)` lines from the deltas were dropped, as in the ai-engines archive. No `## ADDED`, `## MODIFIED`, `## REMOVED` or `## RENAMED` heading is left in the main specs.

### Purpose lines written

- `openspec/specs/config-menu/spec.md`:
  > A general in-app menu opened with `Ctrl+Space m`. It has two sections, "Settings" (rows that map to `config.toml` keys, with static values) and "AI" (a fixed list of tasks, each with an `engine › model` assignment chosen from a picker that shows what is detected and downloaded, see ai-task-models and ai-engine-detection). Changes preview live; `s` saves (see config-persistence), Esc goes back or reverts. Tags: [auto] = `cargo test`; [manual] = real Kitty.
- `openspec/specs/configuration/spec.md`:
  > A user config file read once at startup. It places the statusline and the tab bar, and records the AI engine and model of the general task and of each AI task. Tags: [auto] = `cargo test`; [manual] = real Kitty.

### Integrity checks

- Every delta ADDED, MODIFIED and RENAMED requirement appears exactly once in its main spec. Every delta scenario appears exactly once.
- No requirement from the pre-archive main specs was lost except the two REMOVED ones. `Live preview and revert`, `Atomic write`, `File location`, `Create file and directories` (scenarios unchanged) and the other untouched requirements keep their text.
- Requirement names are unique in each touched main spec.

## Tasks

`openspec/changes/archive/2026-10-12-ai-selector/tasks.md`: all 53 implementation and manual tasks (PR1 to PR12, 1.1 to 12.5) are ticked `[x]`. Zero unchecked. No checkbox was changed at archive time, so no stale-checkbox reconciliation was needed.

## Verification

`verify-report.md`: "Verdict: PASS WITH WARNINGS (0 CRITICAL, 3 WARNING, 2 SUGGESTION)". `cargo test` 1095 unit plus 1 integration passed. `cargo clippy --all-targets -- -D warnings` clean. `cargo fmt --check` clean. 169 scenarios: 163 automated, 6 manual ticked 12.1 to 12.5 by the user in Kitty.

WARNINGS (none blocks archive; the verify report recommends follow-ups):
1. `clip()` in `src/ui/components/tab_bar.rs:80` measures a VS16 emoji with the wrong width, so the ` · download required` mark can be pushed out of the line.
2. The engine label shrinks to `…` at 24 columns, the minimum full popup.
3. `model_rows` and `disabled_reason` show `engine not detected` for an empty detection (`src/core/ai/picker.rs:245`, `src/core/menu.rs:159`), which is imprecise when an engine is detected with no models.

## Archive Gate

- No CRITICAL issues in the verify report. No override needed or requested.
- All tasks complete.
- All four delta specs and the three delta-only-requirement sets merged.
- Purpose lines for `config-menu` and `configuration` updated as the verify report and delta specs required.

## Deferred Follow-ups

- The three verify WARNINGS above (tab-bar width for VS16, 24-column engine label, `engine not detected` wording on empty detection).
- The two SUGGESTIONS from the verify report: `footer()` rebuilds `pick_rows()` on every draw (`src/core/menu.rs:268`), and no scenario-to-test trace exists.
- Roadmap P2 to P6 (chat, summarize and the orchestrator) use these assignments. Not in this change.

## Git Operations Left To The User

No git operation was run by this phase. The change folder move is performed by writing each file to the archive path. The old files must be removed with git (see the list delivered to the orchestrator). Nothing has been committed.

## Relevant Files

- `openspec/specs/ai-task-models/spec.md` (new)
- `openspec/specs/config-menu/spec.md`
- `openspec/specs/config-persistence/spec.md`
- `openspec/specs/configuration/spec.md`
- `openspec/changes/archive/2026-10-12-ai-selector/` (moved change folder: proposal.md, design.md, tasks.md, verify-report.md, specs/ with the four deltas, and this archive-report.md)
