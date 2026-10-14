# Archive Report: pane-scroll

**Date**: 2026-10-14
**Artifact store**: hybrid (openspec files + engram `sdd/pane-scroll/archive-report`)
**Branch**: `chore/openspec-archive` (from origin/main `fd8e2dc`)
**Status**: Archived, after ai-chat and engram-memory. Verdict ready with conditions, 0 CRITICAL; W1 fixed in the spec at archive time. 4 manual tasks accepted open.

## Change Summary

Mouse capture and the vertical wheel for every pane, plus COPY on chat panes. The wheel scrolls the pane under the pointer (shell scrollback or chat transcript), is forwarded to children that asked for mouse events, or becomes arrow keys on the alternate screen without a mouse mode. Capture is on by default (`\x1b[?1000h\x1b[?1006h`), controlled by the `mouse` key and a live `Mouse` row in the config menu (replacing the disabled "coming soon" row). COPY now also works on chat panes with line scrolling only. New capability `mouse-input`; `terminal-session` "No mouse capture" removed.

## What Shipped (PRs and releases)

- #170 `feat/pane-scroll-01-copy` (chat COPY and scrolling)
- #171 `feat/pane-scroll-02-wheel` (wheel routing)
- #173 `feat/pane-scroll-03-capture` (capture lifecycle, config and menu)
- All in release 0.12.0 (#176).

## Observation IDs (traceability)

| Artifact | Topic key | Engram ID |
|----------|-----------|-----------|
| Verify report | `sdd/pane-scroll/verify-report` | #3116 |
| Proposal, spec, design, tasks | `sdd/pane-scroll/{proposal,spec,design,tasks}` | not re-fetched in this run; the files in this folder are canonical |

## Verify W1 fixed before merging

Verify W1: `restore_to` (`src/runtime.rs`, around line 880) ALWAYS writes the mouse-disable sequence, even with `mouse = false`, while the delta said the app must never write any mouse sequence with `mouse = false` and that the panic hook writes it only when capture was on. The code is the intended behavior (harmless: disabling a mode that is off is a no-op; pinned by the test `restore_always_disables_the_mouse_capture`). The delta `specs/mouse-input/spec.md` was amended BEFORE the merge:

- Requirement "Capture lifecycle": with `mouse = false` the app never writes an ENABLE sequence; the disable sequence `\x1b[?1006l\x1b[?1000l` is written on every exit path (quit and panic), unconditionally; the panic hook always emits it.
- Scenario "Disabled by config" now forbids only ENABLE sequences and allows the restore's disable bytes on quit.
- Scenario "Panic with capture off writes no mouse bytes" became "Panic with capture off still restores".

The archived delta in this folder holds the amended text, and the main `mouse-input` spec matches the code.

## Specs Synced

Merged last of the three; every MODIFIED target existed by name in the post-ai-chat and post-engram-memory main specs.

| Capability | Action | Requirements (before -> after) | Scenarios (before -> after) |
|------------|--------|--------------------------------|-----------------------------|
| `chat-pane` | 2 MODIFIED | 16 -> 16 | 86 -> 98 |
| `config-menu` | 1 ADDED, 3 MODIFIED (+ reconciliation below) | 20 -> 21 | 107 -> 112 |
| `config-persistence` | 1 MODIFIED (+ scenarios restored) | 10 -> 10 | 66 -> 73 |
| `configuration` | 2 MODIFIED (+ reconciliation below) | 5 -> 5 | 39 -> 42 |
| `copy-mode-scrollback` | 2 ADDED, 1 MODIFIED | 11 -> 13 | 38 -> 58 |
| `modal-input` | 2 ADDED, 1 MODIFIED | 15 -> 17 | 93 -> 106 |
| `mouse-input` | Created (new) | 0 -> 9 | 0 -> 70 |
| `statusline` | 2 ADDED | 13 -> 15 | 67 -> 84 |
| `terminal-session` | 1 MODIFIED, 1 REMOVED (`No mouse capture`, reason and migration are in the delta) | 13 -> 12 | 46 -> 46 |

### Reconciliation of base drift (done at archive time)

The pane-scroll deltas were written before engram-memory and, for two capabilities, before ai-chat reached the main specs. A plain replace of the MODIFIED requirements would have silently dropped later content. These were repaired in the main specs; the delta files are unchanged:

- `config-menu`: the replaced requirements "Sections and items are data" (and its integrity scenario), "Navigation" (and its wrap and order scenarios) and the "Task rows rendered" scenario had lost the `Context` and `Memory` rows of engram-memory. Restored them, keeping the new `Mouse` row. Also fixed the Terms sentence "The Settings Mouse row is the only row the cursor skips" (the delta states it no longer holds): it now reads "The cursor skips no row of the task list."
- `configuration` "Schema and defaults": the replacement text omitted `[ai] profile` (ai-chat). Restored the key, its default `programmer`, the empty-string rule and the pointer to "Profile key validation", plus the profile mention in the defaults scenario. Nine unchanged scenarios (AI keys, profile, task tables, unknown engine) and "Positions in Kitty [manual]" had been left out of the replacement and were restored.
- `config-persistence` "Owned keys only": 13 unchanged scenarios (engine saved as empty, escaping, undetected engine survives, task tables, set all, profile creation and update) were restored.
- Scenarios intentionally replaced and NOT restored: "Mouse untouched", "Mouse reserved", "Disabled row is skipped", and the two "Refused on a chat pane" scenarios.

## Tasks

42 tasks: 38 checked, 4 accepted open.

- 3.17 (tracker PR and final gate) ticked: shipped in release 0.12.0 (#176).
- 1.12, 2.13, 3.12: the stale "PR is NOT opened yet" text replaced by "Merged as PR #170", "#171" and "#173".

Accepted open (all `[manual, Kitty]`):

- 3.13 wheel in a shell pane (scrollback, `[+N]` badge, back to live) and in less/man
- 3.14 wheel over a chat answer; chat COPY keys; stream continues
- 3.15 Shift-drag native selection and a clean terminal after quit (the panic leg is not testable by hand)
- 3.16 menu `Mouse` toggle live, Esc reverts, `s` persists `mouse = false`

## Verification

Verify #3116 at `4af264a` (0.12.0): fmt OK, clippy `-D warnings` clean, `cargo test` 3399 lib + 13 + 1 passed. 0 CRITICAL, 2 WARNING, 3 SUGGESTION. Latest gate on `main` `fd8e2dc`: 3446 lib + 13 + 1 passed.

- W1: fixed in the spec (above).
- W2 (manual checks 3.13 to 3.16 open): accepted open.
- S1 (3.17 stale) and S2 (stale "PR not opened" texts): fixed in `tasks.md`.
- S3 (deltas modify unarchived ai-chat and engram-memory deltas): handled by the archive order and the reconciliation above.

## Relevant Files

- `openspec/specs/{chat-pane,config-menu,config-persistence,configuration,copy-mode-scrollback,modal-input,mouse-input,statusline,terminal-session}/spec.md`
- `openspec/changes/archive/2026-10-14-pane-scroll/` (this folder)
