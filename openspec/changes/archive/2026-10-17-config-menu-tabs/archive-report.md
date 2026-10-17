# Archive Report: config-menu-tabs

**Date**: 2026-10-17
**Artifact store**: hybrid (openspec files + engram `sdd/config-menu-tabs/archive-report`)
**Branch**: `docs/archive-config-menu-tabs` (from the tracker after the merge of PR #215)
**Status**: Archived with warnings (intentional). Verdict PASS from verify, 0 CRITICAL. Manual tasks 5.1 and 5.2 are accepted open, owned by the user.

## Change Summary

Config menu tabs: the popup rows are grouped into two menu tabs, `Settings` and `AI`, from a static `TABS` table. Tab and Shift+Tab (both BackTab encodings) switch tabs at the top level, the cursor is remembered per tab, the popup is fixed at 16 rows (was 22), a one-line tab bar with a `⇥ Tab` hint sits on body row 0, `Settings*` marks a pending update, and the main spec is reconciled with the shipped code (six Settings rows, the four-mode Notify row, `t` and Enter on Updates).

## Observation IDs (traceability)

| Artifact | Topic key | Engram ID |
|----------|-----------|-----------|
| Proposal | `sdd/config-menu-tabs/proposal` | #3193 |
| Spec (delta) | `sdd/config-menu-tabs/spec` | #3194 |
| Design | `sdd/config-menu-tabs/design` | #3195 |
| Tasks | `sdd/config-menu-tabs/tasks` | #3196 |
| Verify report | `sdd/config-menu-tabs/verify-report` | #3200 |
| Exploration | `sdd/config-menu-tabs/explore` | not re-fetched in this run; the file in this folder is canonical |
| Archive report | `sdd/config-menu-tabs/archive-report` | written by this run (see Persistence) |

Engram content was read through `mem_get_observation` for proposal, spec, design, tasks and verify-report. The archived files are copies of the change folder files, which are the same content the orchestrator persisted.

## Specs Synced

| Capability | Action | Requirements (before -> after) | Scenarios (before -> after) |
|------------|--------|--------------------------------|-----------------------------|
| `config-menu` | 6 ADDED, 8 MODIFIED | 22 -> 28 | 117 -> 148 |

Counts are from `### Requirement:` and `#### Scenario:` headings, counted by hand from the main spec before and after the merge.

ADDED (appended, in delta order): Menu tabs, Menu tab bar, Sub-levels keep the bar inert, Popup height is fixed, Update note is Settings-only, Notify, Sound and Updates keys (drift fix).

MODIFIED (replaced in place by the delta block, Previously lines dropped): Open and close, Sections and items are data, Navigation, Input is swallowed, Tiny terminals, AI task list, Picker viewport, Notify row. All 8 targets existed by name.

Merge notes:
- The delta preamble ("Note: this delta is written from the code...") was not copied into the main spec.
- The delta's Terms paragraph was merged into the main spec's Purpose Terms paragraph, because the new requirements use "menu tab" and "body" and the Live preview requirement needs the pane-tab disambiguation.
- No other main-spec requirement was touched. `openspec/changes/agent-tools/` was not touched.

## Tasks

`tasks.md` (archived copy): implementation units 1 to 4 and task 5.3 are checked. Open at archive:

- 5.1 [manual] Open the menu in a real terminal: `Ctrl+Space m`, Tab and Shift+Tab, per-tab cursor, `j`/`k` wrap.
- 5.2 [manual] Tab and Shift+Tab inside tmux and zellij (BackTab encodings), narrow widths (24 and 22 columns), `Settings*` with an update pending.

These are not implementation tasks and are owned by the user. The Task Completion Gate was passed for every implementation task.

### Intentional archive with warnings

The archive proceeds with 5.1 and 5.2 open, on explicit user instruction (orchestrator message, 2026-10-17: keep them listed as open, do not block). The reason is recorded here. The result of 5.1 and 5.2 must go into the PR description once the user has run them. If either check fails, open a new change or a fix, not a reopening of this archive.

## Verification

`verify-report` (#3200): PASS, 0 CRITICAL, 0 WARNING, 2 SUGGESTION. fmt clean, clippy `-D warnings` clean, `cargo test` 4420 passed, 0 failed. PR #215 is merged.

## Persistence

- openspec: this report at `openspec/changes/archive/2026-10-17-config-menu-tabs/archive-report.md`.
- engram: `sdd/config-menu-tabs/archive-report` (type architecture, project lazarobox-shell, capture_prompt false).

## Relevant Files

- `openspec/specs/config-menu/spec.md` (merged main spec, 28 requirements)
- `openspec/changes/archive/2026-10-17-config-menu-tabs/` (this folder: proposal, exploration, design, tasks, specs/config-menu/spec.md, archive-report)
- `openspec/changes/config-menu-tabs/` (old active folder, to be removed with `git rm` by the orchestrator)
- `openspec/changes/agent-tools/` (untouched; its config-menu deltas must be rebased onto tabs in a follow-up, see the design Follow-ups)
