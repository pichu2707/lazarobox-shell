# Archive Report: ai-engines

**Date**: 2026-10-11
**Artifact store**: hybrid (openspec files + engram `sdd/ai-engines/archive-report`)
**Base branch**: `feat/ai-engines` at `bf163d3` (PR #90, `feat/ai-engines-verify`)
**Archive branch**: `feat/ai-engines-archive`
**Status**: Archived with an intentional gate override (user-approved, see below)

## Change Summary

Phase 1 of AI engines in lazarobox-shell: detection and selection. The app detects four local model servers and four agent CLIs, the config menu gains an "AI" section (default engine and model), and `config.toml` gains an `[ai]` table with `engine` and `model`.

PRs in this change: #70 through #90 (AE1 to AE4 slices, verify, and archive-prep PRs).

## Specs Synced

| Capability | Action | Requirements (before -> after) | Scenarios (before -> after) |
|------------|--------|--------------------------------|-----------------------------|
| `ai-engine-detection` | Created (new capability, delta copied as full spec) | 0 -> 10 | 0 -> 39 |
| `config-menu` | Updated: 5 ADDED, 4 MODIFIED, Purpose replaced | 9 -> 14 | 29 -> 49 |
| `config-persistence` | Updated: 1 ADDED, 1 MODIFIED, no Purpose change | 6 -> 7 | 20 -> 30 |
| `configuration` | Updated: 2 MODIFIED, Purpose replaced, stray heading removed | 3 -> 3 | 11 -> 16 |

Counts for the three merged specs are computed by script from the requirement and scenario headings, before and after. `ai-engine-detection` counts are from `grep` on the new file, which is the delta file copied unchanged (it already had the main-spec Purpose and Requirements headings).

### Merge details

- **config-menu**: ADDED `AI section layout`, `Engine and Model choices`, `Detection results update the open menu`, `Refresh key`, `AI draft, revert and save`. MODIFIED `Sections and items are data`, `Navigation` (+1 scenario), `Input is swallowed`, `Footer` (+1 scenario). Untouched requirements kept as-is.
- **config-persistence**: ADDED `AI keys collisions`. MODIFIED `Owned keys only` (4 -> 10 scenarios). Purpose unchanged, as the delta states.
- **configuration**: MODIFIED `Schema and defaults` (3 -> 6 scenarios) and `Fallback and startup notice` (6 -> 8 scenarios). `File location` untouched. The stray `## ADDED Requirements` heading left by an earlier archive was removed; no requirement content was touched. Note: this spec now has no `## Requirements` heading (the other main specs have one). Not added, since the instruction was to remove only the stray heading.
- All `(Previously: ...)` lines from the deltas were dropped.

### Purpose lines written

- `openspec/specs/config-menu/spec.md`:
  > A general in-app menu opened with `Ctrl+Space m`. It has two sections, "Settings" (rows that map to `config.toml` keys, with static values) and "AI" (the default engine and model, with choices built from detection, plus a read-only status row for the selected engine, see ai-engine-detection). Changes preview live; Enter saves (see config-persistence), Esc reverts. Tags: [auto] = `cargo test`; [manual] = real Kitty.
- `openspec/specs/configuration/spec.md`:
  > A user config file read once at startup. It places the statusline and the tab bar, and records the default AI engine and model. Tags: [auto] = `cargo test`; [manual] = real Kitty.

### Integrity checks

- No requirement or scenario from the pre-archive main specs was lost. Requirements replaced by a MODIFIED block keep their name and their scenarios (MODIFIED blocks are supersets, no scenario dropped).
- Every delta ADDED or MODIFIED requirement appears exactly once in its main spec. Every delta scenario appears exactly once.
- No duplicate requirement or scenario names in any touched main spec.
- No `(Previously:`, `## ADDED` or `## MODIFIED` text left in the main specs.

## Tasks

`openspec/changes/archive/2026-10-11-ai-engines/tasks.md`: 81/81 complete. VP.6 (Purpose lines, hand-edited at archive) ticked as part of this archive. VP.1 to VP.4 were already ticked.

## Verification

`verify-report.md`: "## Verdict: PASS WITH WARNINGS", "0 CRITICAL, 5 WARNING, 4 SUGGESTION. Nothing blocks archive." `cargo test` exit 0 (931 unit tests plus 1 integration test). `cargo clippy --all-targets -- -D warnings` exit 0. All 85 scenarios covered (81 automated, 4 manual PASS by the user).

## Archive Gate Override (user-approved)

The user approved the archive with the verbatim words "sí, archívalo", covering the case where the gentle-ai gate blocks only because its keyword scan flags the verify report.

Gate output before the move (`gentle-ai sdd-status ai-engines --cwd <worktree> --json`, relevant fields):

```
"taskProgress": { "total": 81, "completed": 81, "pending": 0, "allComplete": true },
"nextRecommended": "verify",
"blockedReasons": [
    "verify-report.md is not clearly passing."
]
```

The only blocker is that keyword scan. The verify report is not edited. Its verdict line is "PASS WITH WARNINGS" and it contains "0 CRITICAL, 5 WARNING", which is the text the scan matches (gentle-ai 1.36.5 keyword-scan bug). No other blocker was reported: no unchecked tasks, no missing artifacts, no CRITICAL findings.

## Deferred Follow-ups

- **W2**: silent Enter before the first detection result. Possible AI selector change. Not in this archive.
- **S1**: app-level test for "Undetected saved engine survives" (currently covered at the persistence level only).
- **Roadmap P2 to P6**: later phases of AI engines, tracked in the roadmap, not in this change.

## Relevant Files

- `openspec/specs/ai-engine-detection/spec.md` (new)
- `openspec/specs/config-menu/spec.md`
- `openspec/specs/config-persistence/spec.md`
- `openspec/specs/configuration/spec.md`
- `openspec/changes/archive/2026-10-11-ai-engines/` (moved change folder, tasks.md with VP.6 ticked)
