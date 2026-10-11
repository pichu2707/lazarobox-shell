# Archive Report: copy-selection

**Change**: copy-selection
**Archived**: 2026-10-11
**Archived to**: `openspec/changes/archive/2026-10-11-copy-selection/`
**Artifact store**: hybrid (engram + openspec)
**Status**: intentional archive with warnings (user-approved gate override, see below)

## Delivered PRs

#55, #60, #62, #61, #63, #64, #65, #66, #67

## Specs Synced

| Capability | Action | Details |
|---|---|---|
| copy-selection | Created (new capability) | Delta copied to `openspec/specs/copy-selection/spec.md`; no ADDED/MODIFIED headings. |
| statusline | Updated | ADDED `Selection marker`, `Copy notices`. |
| copy-mode-scrollback | Updated | MODIFIED `Navigation`, `Cursor in COPY`, `Exit COPY` replaced in place. Purpose line hand-edited (VP.8). |
| modal-input | Updated | MODIFIED `TERMINAL mode` replaced in place. |

`(Previously: ...)` lines from the delta were dropped from the main specs. Other `(Previously: ...)` lines already present in `modal-input` and `statusline` come from earlier archived changes and were left untouched.

### Requirement and scenario counts (before -> after)

| Capability | Requirements | Scenarios |
|---|---|---|
| copy-selection | 0 -> 6 | 0 -> 33 |
| statusline | 8 -> 10 | 28 -> 38 |
| copy-mode-scrollback | 9 -> 9 | 25 -> 33 |
| modal-input | 11 -> 11 | 46 -> 55 |

Integrity check: no requirement or scenario from the previous main specs was lost, and no delta requirement or scenario is duplicated. Every delta requirement appears exactly once in its main spec. The replaced requirements kept all their prior scenario names.

### Purpose line written (copy-mode-scrollback)

> Vim-style navigation of ~10k lines of scrollback with character/line selection and yank (see copy-selection). All scenarios [auto] unless tagged.

## Gate Override (user-approved)

The gentle-ai archive gate blocks archiving while any implementation task is unchecked. The user explicitly approved archiving ("sí, archívalo") with three tasks still open:

- **VP.3 [manual] Yank over ssh**: DEFERRED by the user. No remote install yet; to be run later on a remote host. Still unchecked.
- **VP.6 [manual] tmux `set-clipboard`**: DEFERRED / not applicable. The user does not use tmux; the README documents the setting. Still unchecked.
- **VP.8 Archive prep** (Purpose line edit): performed during archive and ticked.

`verify-report.md` was not edited. Its verdict (PASS WITH WARNINGS, CRITICAL 0) and its "Update 2026-10-11" ledger are kept as written. Tasks VP.3 and VP.6 were not ticked.

Verification verdict at archive time: CRITICAL 0 (archive is not blocked by the CRITICAL rule). WARNINGS: W1 resolved by PR #67 (design sync), W2 resolved except VP.3 and VP.6, W3 unchanged (TDD evidence is prose only).

## Deferred Follow-ups

1. **VP.3 ssh yank**: run lazarobox inside an ssh session on a remote host; confirm `y` reaches the local clipboard via OSC 52.
2. **VP.6 tmux**: if tmux support is needed, confirm OSC 52 requires `set -g set-clipboard on` (or `external`) and record it in user docs.
3. **W3**: TDD evidence is prose only. No per-task RED/GREEN/REFACTOR table exists. Not re-verified at archive.
4. **Documented tradeoff** (verify S3): a deliberate retype of the exit key within 500 ms of leaving COPY is dropped.

## Source Artifacts

- proposal.md, design.md, tasks.md, verify-report.md, specs/ (copy-selection, copy-mode-scrollback, modal-input, statusline)
