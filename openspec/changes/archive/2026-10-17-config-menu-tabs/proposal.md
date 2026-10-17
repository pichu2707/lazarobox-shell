# Proposal: Config Menu Tabs (Settings | AI, MCP later)

## Intent

The config popup stacks every section: 2 titles + 19 items, `AI_BODY` 18, `POPUP_H` 22. At 80x24 that leaves 2 rows. agent-tools 2e-3 (D18 Approval row) and 4a (MCP presets) need more. Horizontal tabs bound the height by the tallest view, not by the sum of sections. The main `config-menu` spec has also drifted from the code (exploration §4) and is reconciled here.

## Decisions (user, 2026-10-17, settled)

1. Tabs come from a `TABS` table: **Settings | AI**. MCP stays hidden until agent-tools 4a adds its entry.
2. Tab / Shift+Tab switch tabs (BackTab and Tab+SHIFT). `h`/`l`/Left/Right cycle values, `j`/`k` move rows. No new Ctrl keys.
3. The cursor is kept per tab. Opening always lands on Settings, row 0. `j`/`k` wrap within the tab.
4. A `⇥ Tab` hint sits on the tab bar and is dropped at narrow widths. Footers are unchanged.
5. While an update waits, the label reads `Settings*`. The Updates row and its note stay on Settings.
6. Notify and Sound stay on Settings.
7. In sub-levels (pickers, privacy warning, custom field) the bar is drawn but inert, and Tab/BackTab are swallowed. The warning still declines on any non-`y` key.
8. Height is fixed at the max over all views (~16 rows, was 22) and does not jump. With one tab the bar is hidden (no-AI test paths only).
9. The stale main spec is reconciled: 6 Settings rows, Notify as a 4-mode Settings row, real sizes, `t`/Enter on Updates.
10. This lands before agent-tools 2e-3.

## Scope

### In Scope
- Tab state, per-tab cursor, Tab/BackTab handling, wrap within a tab.
- Tab bar widget: active highlight, `⇥ Tab` hint, `‹ AI ›` fallback, `Settings*` cue.
- `rows()` filtered by the active tab. Height constants, `fits_size` and `fits_warning` move to a max-over-views body.
- Update note drawn on the Settings tab only.
- Delta to the `config-menu` spec, including the drift fixes.

### Out of Scope
- MCP tab content and its entry (agent-tools 4a).
- Refactoring the repeated `rows()` match arms.
- Footer text changes.
- The 2e-3 Approval row itself.

## Capabilities

### New Capabilities
None

### Modified Capabilities
- `config-menu`: tabs as data, navigation (Tab/BackTab, per-tab wrap and cursor), input swallowing (Tab no longer unlisted at the top level), open/close, tiny terminals, AI task list and picker sizes, plus the drift fixes from Decision 9.

## Approach

- Add `tab` and `cursor: [usize; N]` over the existing `Section` tables. `ItemKind` dispatch (`on_ai_row`, `on_notify_row`, ...) is reused unchanged.
- `TABS` maps a label to its sections. The body height is a const max over the tabs, `picker::VIEW + 1` and the warning rows.
- The bar is one line at the top of the body, clipped with `tab_bar::{cells, clip}`.

## Affected Areas

| Area | Impact |
|---|---|
| `src/core/menu.rs` | Modified: tabs, cursor, keys, sizes |
| `src/ui/components/menu.rs` | Modified: bar, body height |
| `src/core/menu/*_tests.rs`, `src/app.rs` tests, snapshots | Modified |
| `openspec/specs/config-menu/spec.md` (via delta) | Modified |

## Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| ~22 of 26 popup snapshots change, which can hide regressions | High | Review every changed snapshot by hand. The 4 no-AI snapshots must stay byte-identical |
| The bar has little room at the 24-col minimum | Med | `‹ AI ›` fallback, hint dropped first. Tests at 24 and 22 cols |
| BackTab arrives as `BackTab` or as `Tab`+SHIFT | Med | Handle both and test both |
| Spec drift is carried into the delta | Med | Write the delta from the code (exploration §4), not from the old spec |
| Tabs are hard to discover | Low | `⇥ Tab` hint on the bar |

## Rollback Plan

Revert the single PR. No config keys, no file format and no persisted state change.

## Dependencies / Follow-ups

- After merge: rebase the agent-tools `specs/config-menu` deltas (Tools, Approval) and D18's size paragraph onto tabs. The 2e-3 Approval row becomes the last AI-tab row.
- agent-tools 4a adds the MCP `TABS` entry.

## Success Criteria

- [ ] `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check` pass.
- [ ] The popup is ≤ 16 rows and keeps the same height across tabs, pickers and the warning.
- [ ] Tab and Shift+Tab (both encodings) cycle tabs, and are inert in every sub-level.
- [ ] Reopening lands on Settings row 0. The cursor survives tab switches.
- [ ] No-AI snapshots are unchanged. No panic from 0x0 up.
- [ ] The main spec matches the code after archive.

## Size Forecast

About 200-280 non-test lines, one PR: state and keys 70-90, `rows()` per tab 0-40, constants and fit ~20, bar widget 60-80, update note 15-25. Test churn is large (~30-60 tests, ~22 snapshots) but mechanical.

## Proposal question round

Resolved: the user confirmed Decisions 1-10 on 2026-10-17.
