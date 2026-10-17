# Exploration: config-menu-tabs

User decisions (2026-10-17): the config popup becomes horizontal tabs **Settings | AI | MCP**. Tab / Shift+Tab switch tabs. `h`/`l`/Left/Right keep cycling the selected row's value and `j`/`k` move between rows. The MCP tab will host presets for standard MCP servers once agent-tools 4a-4c exist. This change lands before agent-tools 2e-3, whose `Approval` row goes into the AI tab.

## 1. Current state

### Menu model (`src/core/menu.rs`)
- There are two `Section` descriptors:
  - `SECTIONS` holds "Settings", with 6 items: Statusline position, Tab bar position, Mouse, Notify (4 modes), Sound and Updates.
  - `AI_SECTION` holds 10 items: Set all tasks, general, code, chat, review, summarize, Profile, Context, Memory and Tools.
- Notify and Sound live in Settings but edit the AI draft (`ItemKind::needs_ai`).
- The cursor is one global `selected: Option<usize>` that runs over the Settings rows and then the AI rows.
  - `selected_kind()` resolves the index by subtracting the Settings count.
  - `step_selection` wraps over both sections, so `j`/`k` cross from one section to the other.
  - `rows()` emits a `Row::Title` and then the items of each section: 21 rows in total, 2 titles plus 19 items.
- `AI_BODY` is a const fn of the tables, currently 18, so `POPUP_H` is 22 (body plus `CHROME` 4). A const assert requires `picker::VIEW` (10) to be smaller than `AI_BODY`, and `fits_warning` uses `AI_BODY.max(warning rows)`.
- Sub-levels replace the rows in place, checked in this order: the privacy warning, then the pickers, then the task list. `on_key` routes to `on_warning_key` or `on_picker_key` before the top-level match. The update note (2 lines) is drawn under the list and hidden behind sub-levels.
- Footer and behaviour are keyed by `ItemKind` (`on_ai_row`, `on_notify_row`, `on_updates_row`, `on_draft_row`), so a tab filter reuses them well.

### Rendering (`src/ui/components/menu.rs`)
- The renderer draws `rows()` verbatim. The body is `AI_BODY.max(rows.len())`, so the popup keeps a stable height.
- Widths: `WANT_WIDTH` is 51 without AI and `WANT_WIDTH_AI` is 62 with AI. `MIN_WIDTH` is 24.
- The update note adds height but never triggers the fallback. `tab_bar::{cells, clip}` are available for width-aware clipping.

## 2. Key conflicts
- **Top level**: Tab and BackTab are unhandled and fall through to `_ => None`. Crossterm sends BackTab with SHIFT; some terminals send `Tab` plus SHIFT instead, so handle both.
- **Picker**: Tab is swallowed. That is correct, since tabs must not switch in the middle of a pick.
- **Custom model field**: Tab does nothing and should stay inert.
- **Privacy warning**: any key other than `y` declines, Tab included. This is existing behaviour and stays.
- **App routing**: `InputMode::Menu` delegates straight to `MenuState::on_key`, with no prefix conflict.
- **Tests that list Tab as ignored**:
  - `src/core/menu.rs:1340`
  - `src/core/menu/save_tests.rs:101`
  - `src/app.rs:5855`
  - the spec scenario "Unknown key: `x`, Tab or Ctrl+Space"
- **Hint width**: the inner width is 60 and `HINT_TASK` is already 55 characters, so "Tab sections" does not fit in the footer.

## 3. Design options
- **Tab bar**: one line at the top of the body, such as `Settings │ AI │ MCP` (about 21 cells), with the active tab highlighted. When the full bar does not fit, show only the active tab as `‹ AI ›`. Below `MIN_WIDTH` the existing fallback applies. With a single tab (no AI, reachable only in tests) the bar is hidden.
- **Cursor per tab**: either remember it (`tab` plus `cursor: [usize; N]`) or reset to the first row. Recommendation: remember it. Reopening the menu always starts fresh on Settings, row 0.
- **`j`/`k` wrap**: wrap within the tab (recommended). Tab and Shift+Tab are the only way to move across tabs.
- **Opening tab**: always Settings (recommended); "last used" would need app-level state.
- **Sub-levels**: pickers open only from AI rows. While a sub-level is open, Tab and BackTab are swallowed. The bar stays drawn and inert, which keeps the layout stable.
- **Popup height**: the maximum over all views (recommended). The body is max(AI tab 10, or 11 with Approval; picker 11; warning 10) = 11, so the popup is 1 + 11 + 4 = 16 rows, down from 22 now and 23 with Approval. Headroom at 80x24 goes from 2 rows to 8. A per-tab height was rejected because it jumps.
- **MCP tab until 4a**:
  - A: hidden, using a data-driven `TABS` table, with 4a adding the entry. This is the recommendation.
  - B: a muted "coming soon" placeholder.
  - C: a disabled list of planned presets. Rejected: it promises something that cannot be delivered yet, and the preset schema belongs to 4a.

## 4. Impact on specs
- `openspec/specs/config-menu/spec.md` needs deltas for: Sections and items are data (tabs), Navigation (per-tab wrap, Tab and Shift+Tab), Input is swallowed (Tab listed at the top level only), Open and close, Tiny terminals, and the AI task list and picker viewport sizes.
- The main spec has drifted from the code:
  - it says Settings has 3 rows, while the code has 6;
  - it describes Notify as an AI on/off row, while the code makes it a 4-mode Settings row;
  - it gives the popup as 62x19 with a minimum of 18, while the code uses 62x22;
  - it does not list the `t` and Enter keys of the Updates row;
  - the Notify wrap scenario is stale.
- The agent-tools config-menu deltas (Tools, Approval) and D18's size paragraph assume the stacked layout. They become obsolete and must be rebased after this change; 2e-3 then only adds one descriptor entry.

## 5. Test and snapshot impact
- 26 popup snapshots: about 22 change, and the 4 without AI stay the same if the bar is hidden for a single tab. `ui` also has 2 snapshots (`menu_popup_is_drawn_over_the_panes_after_the_bars` and `update_menu_frame`).
- About 30 to 60 tests change, mostly through the shared helpers (`goto`, `selected_label`, `press(j)`) used in 17 files under `src/core/menu/`, plus the height assertions and the Tab-ignored tests.
- New tests:
  - Tab and BackTab cycling, with both encodings;
  - wrap within a tab;
  - the per-tab cursor;
  - Tab is inert in the picker, the custom field and the warning;
  - the bar degrades at 24 and 22 columns;
  - the hidden-tab table;
  - size sweeps.

## 6. Size estimate
| Part | Non-test lines |
|---|---|
| State and keys | 70 to 90 |
| `rows()` per tab | 0 to 40 |
| Constants and fit | about 20 |
| Tab bar widget | 60 to 80 |
| Update note | 15 to 25 |
| Placeholder (option B only) | about 30 |
| **Total** | **200 to 280** |

This fits in one PR. Do not refactor the repeated `rows()` match arms in this change.

## 7. Risks
- The snapshot churn can hide real regressions, so review the changed snapshots by hand.
- The spec drift has to be reconciled explicitly.
- The 24-column minimum leaves little slack for the bar, so the degradation path needs a test.
- The agent-tools 2e-3 deltas and D18 sizes become obsolete.
- Tab and Shift+Tab are hard to discover.
- Terminals encode BackTab differently.
- A visible placeholder MCP tab would be a dead tab.
- `fits_warning` and the one-line fallback depend on `AI_BODY`.

## 8. Recommendation
- Add a tab state with a per-tab cursor on top of the global-index model, driven by a `TABS` table.
- Wrap within the tab and always open on Settings.
- Hide the bar when there is only one tab. Keep it drawn but inert in sub-levels.
- Set the popup height to the maximum over all views.
- Keep the MCP tab hidden until 4a.
- Leave the footers untouched.
- Land this before 2e-3, then rebase the agent-tools menu deltas and D18.
