# Archive Report: config-menu

- Change: `config-menu` (in-app config menu, `Ctrl+Space m`), tracker `feat/config-menu`, rebased onto main at 3c683f2 (PR #10 merge), with the deltas completed in PR #52
- Archived: 2026-10-11, to `openspec/changes/archive/2026-10-11-config-menu/`
- Artifact store: hybrid (openspec + engram)
- Archive branch: `feat/config-menu-archive` (base `origin/feat/config-menu`, tip e106c4e at archive start)
- Verdict carried from verify: PASS WITH WARNINGS, 0 CRITICAL

## Engram observation IDs (traceability)

| Artifact | Engram topic key | Observation ID |
|---|---|---|
| explore | `sdd/config-menu/explore` | #2867 |
| proposal | `sdd/config-menu/proposal` | #2869 |
| spec | `sdd/config-menu/spec` | #2871 |
| design | `sdd/config-menu/design` | #2872 |
| tasks | `sdd/config-menu/tasks` | #2873 |
| apply-progress | `sdd/config-menu/apply-progress` | #2875 (no openspec file exists, see Risks) |
| verify-report | `sdd/config-menu/verify-report` | #2886 |

## Task completion gate

- `openspec/changes/config-menu/tasks.md`: `grep -c '^- \[ \]'` = **0**. All 79 tasks checked (`gentle-ai` taskProgress: total 79, completed 79). No stale-checkbox reconciliation was needed.
- `verify-report.md`: present. Verdict PASS WITH WARNINGS, 0 CRITICAL.
- Gate passed before the archive move.

## Specs merged into main

Capabilities touched: 3 existing (MODIFIED/ADDED) and 2 new (created from ADDED-only deltas). No REMOVED or RENAMED requirements in any delta. Every MODIFIED name was found in main before merging (the gate would have stopped otherwise).

| Capability | Action | Requirements before to after | Scenarios before to after | Detail |
|---|---|---|---|---|
| `statusline` | MODIFIED (2 requirements) | 8 to 8 | 25 to 28 | Mode block 3 to 5 (+MENU label and color, +MENU mapping). Group-pending key hint 7 to 8 (`Root hint in PREFIX` replaced by `Root hint with menu`, +`Root hint stays table-generated`) |
| `modal-input` | MODIFIED (3) + ADDED (1) | 10 to 11 | 38 to 46 | ADDED `MENU mode` (+5). MODIFIED `Prefix tree with described key groups` 7 to 8 (+`m is in the table`). MODIFIED `TERMINAL mode` 4 to 5 (+`Repeat in MENU`). MODIFIED `Paste and resize in any mode` 2 to 3 (+`Paste in MENU`) |
| `tabs` | MODIFIED (1) | 6 to 6 | 27 to 31 | `Tab bar` 8 to 12 (+4 forced-bar, close, unaffected and tiny scenarios) |
| `config-menu` | NEW (ADDED only) | 0 to 9 | 0 to 29 | new `openspec/specs/config-menu/spec.md` |
| `config-persistence` | NEW (ADDED only) | 0 to 6 | 0 to 20 | new `openspec/specs/config-persistence/spec.md` |

Totals across the five touched specs: 40 requirements and 154 scenarios after the merge.

Untouched specs (`configuration`, `copy-mode-scrollback`, `key-encoding`, `pane-layout`, `terminal-emulation`, `terminal-session`) are byte-identical to HEAD.

## Self-check (script output)

Script: independent check run against the HEAD (pre-archive) main specs, the post-merge main specs and the delta files. Checks: no duplicate requirement or scenario names; every ADDED and MODIFIED delta requirement present verbatim with all of its scenarios; every pre-archive requirement still present; untouched requirements byte-identical; MODIFIED requirements lose only the explicitly allowed scenario; no unexplained new requirements.

```
  scenario dropped by MODIFIED: [statusline] Group-pending key hint :: Root hint in PREFIX [auto, TestBackend + insta] -> ALLOWED (intended replacement)
statusline: pre req=8 scen=25 | post req=8 scen=28
modal-input: pre req=10 scen=38 | post req=11 scen=46
tabs: pre req=6 scen=27 | post req=6 scen=31
openspec/specs changes:
 M openspec/specs/modal-input/spec.md
 M openspec/specs/statusline/spec.md
 M openspec/specs/tabs/spec.md
?? openspec/specs/config-menu/
?? openspec/specs/config-persistence/

SELF-CHECK PASS
```

The new capabilities were checked as "requirement list equals the delta ADDED list" (9 and 6 requirements respectively).

The only scenario removed from any pre-archive requirement is `Root hint in PREFIX`, which is the intended replacement from PR #52 (`Root hint with menu`).

## Gate output (recorded before the archive move, after the S4 update)

`gentle-ai sdd-status config-menu --cwd . --json` (artifactPaths and contextFiles omitted here for brevity; all other fields verbatim):

```json
{
  "schemaName": "gentle-ai.sdd-status",
  "schemaVersion": 1,
  "changeName": "config-menu",
  "artifactStore": "openspec",
  "planningHome": { "mode": "repo-local", "path": ".../openspec" },
  "changeRoot": ".../openspec/changes/config-menu",
  "artifacts": {
    "applyProgress": "missing",
    "design": "done",
    "proposal": "done",
    "specs": "done",
    "tasks": "done",
    "verifyReport": "done"
  },
  "taskProgress": { "total": 79, "completed": 79, "pending": 0, "allComplete": true },
  "dependencies": {
    "proposal": "all_done", "specs": "all_done", "design": "all_done",
    "tasks": "all_done", "apply": "all_done", "verify": "ready", "archive": "blocked"
  },
  "applyState": "all_done",
  "actionContext": { "mode": "repo-local", "allowedEditRoots": [".../agent-a1f1017a3c2090825"] },
  "nextRecommended": "verify",
  "blockedReasons": ["verify-report.md is not clearly passing."]
}
```

## Gate override (explicit user approval, 2026-10-11)

- The only blocked reason is `verify-report.md is not clearly passing.`
- Cause: the verify-report keyword parser in gentle-ai 1.36.5 matches "0 CRITICAL" and the mandatory `### CRITICAL` heading ("None."). This is upstream Gentleman-Programming/gentle-ai#848, fixed in later versions.
- The user explicitly approved archiving config-menu despite this block, on 2026-10-11.
- The override covers ONLY that keyword block. It does not cover unchecked tasks (none exist) or missing requirements (none missing, see self-check).

## S4 update (verify-report factual update, approved by the user, 2026-10-11)

- `verify-report.md` suggestion S4 said the tracker rebase onto main (CM-16) was still pending. It is resolved. Only the S4 line was edited, and one ledger line was added to the header: "S4 resolved by the CM-16 rebase; deltas completed in #52".
- Evidence: on 2026-10-11 `feat/config-menu` was rebased onto main at 3c683f2 (PR #10 merge) and force-pushed at tip 69f0034. The rebase had one trivial conflict, in `src/core/mod.rs`. 643 tests pass, clippy and fmt are clean. All 35 commits compile individually. No test was lost from either side. A fresh review gave the verdict OK.
- Nothing else in the verify report changed: verdict, counts, CRITICAL heading and test names are untouched.

## Deferred follow-ups (carried from design and verify report, not in scope of this change)

- W1 relative HOME wording: "save failed: no config path (HOME is not set)" is misleading when HOME is set but relative. The spec is met; only the wording is off.
- W3 ownership: ownership is not preserved on the replaced file.
- W3 directory fsync: no directory fsync after rename (atomic, but not power-loss durable).
- W3 dangling parent chain: a dangling parent chain yields "File exists (os error 17)".
- W3 stray directory: a dangling target with a parent-dir component (`a/../c.toml`) creates a stray directory.
- W3 BOM and trailing newline: a leading BOM is dropped (known limit, CM-9); trailing-newline edge cases.
- Mixed line endings: a mixed file is normalised to the style of its first break (CM-9). Only the first-break rule is implemented.

## Risks and notes

- Task count mismatch: verify-report.md states 76 tasks, but tasks.md now has 79 (CM4a.14, CM4a.15 and CM4b.13 were added after the verify snapshot). All 79 are checked, so the gate is satisfied; the verify report was not edited for this (out of scope).
- The apply-progress artifact exists only in engram (#2875). The openspec folder has no `apply-progress.md`, so gentle-ai reports `applyProgress: missing`. It is not a required archive input.
- Pre-existing quirk, not touched by this change: `openspec/specs/configuration/spec.md` contains a stray `## ADDED Requirements` heading under its Purpose section (it was already in main at HEAD). It was left as-is because it is outside config-menu's scope; it should be cleaned up in a separate change.
- After the archive move, `gentle-ai sdd-status config-menu --cwd . --json` reports `Active OpenSpec change not found: config-menu.` and `nextRecommended: sdd-new`, which is the expected sanity result.
