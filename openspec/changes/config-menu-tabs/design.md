# Design: Config Menu Tabs (Settings | AI, MCP later)

> Size note: this design goes over the 800-word budget on purpose. The orchestrator asked for exact thresholds, arithmetic and a test plan, which take more room. Tables keep it scannable.

## Technical Approach

Tabs are data. A `TABS` table groups the existing `Section` descriptors. `MenuState` gains an active tab and one remembered cursor per tab. `rows()`, `selected_kind()` and `step_selection()` become tab-local because they iterate only the active tab's items. All `ItemKind`-keyed behaviour (`on_ai_row`, `on_draft_row`, `on_notify_row`, `on_updates_row`, `cycle`, the footer) is reused as it is.

The widget draws a one-line tab bar as body row 0. The bar is not a `Row`, so `rows()` and its test helpers stay bar-free. The body height is a const max over every view, so the popup is 16 rows on every tab and in every sub-level. With a single tab (the no-AI paths) the bar and the new layout are skipped, so the 4 no-AI snapshots stay byte-identical. `app.rs` non-test code does not change: `open_menu` already builds a fresh `MenuState`, so each reopen starts on Settings, row 0.

## Architecture Decisions

| # | Decision | Rejected alternatives | Rationale |
|---|---|---|---|
| T1 | **`Tab { label, sections: &'static [Section], needs_ai, note }`** in `static TABS: [Tab; TAB_COUNT]`. Settings is `sections: SECTIONS, needs_ai: false, note: true`. AI is `sections: std::slice::from_ref(&AI_SECTION), needs_ai: true, note: false`. 4a adds one entry and bumps `TAB_COUNT`; the compiler enforces the pair. | `[usize; TABS.len()]` (an array length that reads a static is risky at MSRV 1.88); `Vec<usize>` cursors (they would need resizing in `with_ai`); `&[&Section]` | The iteration types stay `&Section` as today. Reading immutable statics in const context already compiles here (`ai_body()` does it). Fallback if the compiler rejects `from_ref` in a static: `sections: &'static [&'static Section]`. |
| T2 | **Cursor**: keep `selected: Option<usize>` as *the active tab's* cursor and add `tab: usize` plus `tab_cursor: [usize; TAB_COUNT]`. `step_tab` saves `selected` into `tab_cursor[tab]`, moves `tab`, then loads `selected`. `tab_cursor[tab]` is stale while that tab is active; only `step_tab` reads it. | Replace `selected` with `cursor[tab]` everywhere | This is the smallest diff: the 10 `self.selected == Some(index)` arms in `rows()` and `step_selection` stay untouched (do NOT refactor the arms). The invariant lives in one function. |
| T3 | **Tab-local helpers**: `settings()` becomes `tab_items()`, which yields the active tab's items with the existing `needs_ai` filter. `selected_kind()` becomes `tab_items().nth(selected?)`, and the subtraction logic goes away. `selectable()` becomes `tab_items().count()`. `tabs()` yields the visible `TABS` (`!needs_ai \|\| ai.is_some()`), and `tab` indexes that list. | An index offset per tab | It deletes code, and `ItemKind` dispatch is unchanged. |
| T4 | **Titles**: `rows()` emits `Row::Title(section.title)` only when the bar is hidden (one tab). When tabbed, the bar names the tab. | Keep titles under the bar | Keeping them would cost 1 row per tab and push 2e-3 to a bigger popup. Keeping them in the single-tab path keeps the no-AI snapshots identical. |
| T5 | **Keys at the top level only**: `Tab` with SHIFT → previous, `Tab` → next, `BackTab` (any SHIFT) → previous. They are added to the `on_key` match after the existing modifier guard, so Ctrl/Alt+Tab stay swallowed. A repeat switches like a press (as `j`/`k` do). With one tab they do nothing. | Ignoring repeats | This matches how movement treats repeats; holding Tab is harmless. |
| T6 | **Sub-levels need no code**: `on_warning_key` runs first, and its non-`y` decline covers Tab. `on_picker_key` falls through to `_ => {}` for Tab and BackTab, in the list and in the custom field. Only tests are added. | Explicit swallow arms | The behaviour already holds; tests pin it. |
| T7 | **Bar position**: it is part of the **body** (`MENU_BODY = TAB_BAR + VIEW_BODY`), and `CHROME` stays 4. | A tabbed `CHROME` of 5 | `CHROME` and `fits_size` are shared with the no-AI path. One constant keeps one formula. |
| T8 | **Update note**: it is drawn inside the fixed body on the tab with `note: true`, in the last 2 body rows (just above the blank row and the footer). `update_note()` returns `None` on other tabs and over sub-levels. On the no-AI path the note keeps today's code (extra rows under the list). | Directly under the list | The note keeps the same position relative to the footer as today, and that position does not depend on the row count. |
| T9 | **Hint over sub-levels**: the bar is drawn with the same styles but **without** `⇥ Tab`. | A hint that stays | A hint saying Tab works while Tab is swallowed would be a false affordance. |

## Tab bar layout

Segments, left to right from `inner.x`: each label is padded as ` {label} `, and labels are joined by `│`. Two tabs give ` Settings │ AI `, which is **15** cells; ` Settings* │ AI ` is **16**. With MCP it becomes 21, or 22 with `*`.

- Active segment: `fg bg_base, bg primary_cyan, BOLD`, the same as the main tab bar's active style. It must not look like the `info_blue` REVERSED of the selected row.
- Idle segments and `│`: `fg text_muted`.
- Hint `⇥ Tab` (U+21E5 is 1 cell wide, so the hint is 5 cells): `fg text_muted`. It is right-aligned and ends one column before the inner right edge, like row values.
- `*` appears when the Updates row shows `* Updates`, that is `update.is_some() && installed.is_none()`.

A pure function `tab_line(labels, active, width, hint) -> Vec<(String, Style)>` picks the first step that fits. Here W is the inner width (popup width − 2) and BAR is the full bar width:

| Step | Condition | 2 tabs | 2 tabs, `Settings*` | 3 tabs (4a) |
|---|---|---|---|---|
| Bar + hint | W ≥ BAR + 7 | W ≥ 22 (popup ≥ 24, always) | W ≥ 23 (popup ≥ 25) | popup ≥ 30 / 31 |
| Bar only | BAR ≤ W | — | popup 24 | popup ≥ 23 / 24 |
| Active only ` ‹ AI › ` | W < BAR, clipped with `clip` | never in the full popup | never | never |
| One-line fallback | popup < `MIN_WIDTH` (24) | unchanged | unchanged | unchanged |

The "active only" step cannot be reached in the full popup with three or fewer tabs. It is kept as a guard for narrow widths and is covered by unit tests on `tab_line`. With one tab, no bar is drawn at all.

## Sizing (arithmetic)

| View (rows under the bar) | Rows |
|---|---|
| Picker: title + `picker::VIEW` | 1 + 10 = **11** |
| Privacy warning: title + 7 facts + blank + prompt | **10** (`WARNING_ROWS = WARNING_TEXT.len() + 3`) |
| AI tab: 10 items, no title | 10 (11 with 2e-3's Approval row) |
| Settings tab: 6 items + 1 gap + note | 6 + 1 + `NOTE_ROWS` 2 = 9 |
| Custom entry: title + field | 2 |

- `VIEW_BODY = 11`, `MENU_BODY = TAB_BAR (1) + 11 = 12`.
- Popup height = 12 + `CHROME` 4 = **16** (was 22).
- The smallest full popup is **24×15**; the blank row is dropped first (`fits_size`).
- The fallback appears below 24 columns or below 15 rows. At 80x24 the headroom grows from 2 rows to 8.
- 2e-3 makes the AI tab 11 rows, which is still ≤ 11, so the size does not change. The picker is the binding term. The next row added to any tab grows the popup automatically.

```rust
pub const TAB_BAR: usize = 1;
const NOTE_ROWS: usize = 2;                         // update_note() -> Option<[String; NOTE_ROWS]>
const WARNING_ROWS: usize = WARNING_TEXT.len() + 3;
pub const VIEW_BODY: usize = view_body();           // max(1 + VIEW, WARNING_ROWS, 2, per tab: items + if note {1 + NOTE_ROWS})
pub const MENU_BODY: usize = TAB_BAR + VIEW_BODY;
```

- `AI_BODY`, `ai_body()` and the `VIEW < AI_BODY` const assert are removed. The picker is now an input of the max, so the assert would always hold. A test pins `warning_rows().len() == WARNING_ROWS`.
- `fits_warning()` becomes `fits_size(cols, rows, MENU_BODY)`.
- Widget: `body = if tabbed { MENU_BODY.max(TAB_BAR + rows.len()) } else { rows.len() }`; list rows start at `inner.y + bar`.

## Data Flow

    key ─▶ on_key ─┬─ warning?  ─▶ on_warning_key (Tab declines)
                   ├─ picker?   ─▶ on_picker_key  (Tab swallowed)
                   └─ top level ─▶ Tab/BackTab ─▶ step_tab (save/load cursor)
                                   j/k ─▶ step_selection (wraps in tab_items)
    MenuPopup ─▶ tab_labels()/active_tab()/in_sub_level() ─▶ tab_line ─▶ row 0
             └─▶ rows() (active tab only) ─▶ rows 1..; update_note() ─▶ last 2 body rows

## File Changes

| File | Action | Description |
|---|---|---|
| `src/core/menu.rs` | Modify | `Tab`, `TABS`, `TAB_COUNT`, `tab`/`tab_cursor`, `tabs()`, `tab_items()`, `step_tab`, Tab/BackTab arms, `in_sub_level()`, `tab_labels()`, `active_tab()`, the size consts, `fits_warning`, the note gate. Plus a `#[cfg(test)] goto(label)` driven by keys |
| `src/ui/components/menu.rs` | Modify | `tab_line`, bar drawn at row 0, body/offset, note inside the body, `POPUP_H` from `MENU_BODY` |
| `src/core/menu/*_tests.rs` (17 files) | Modify | Helpers made tab-aware, new tab tests, Tab removed from the ignored lists |
| `src/app.rs`, `src/app/app_tests/menu_copy.rs` (+ other menu-driving app tests) | Modify (tests only) | Fixed `j` counts to AI rows become `Tab` + (n − 6); the Tab-ignored case moves to a new test |
| `src/ui/components/snapshots/*menu*` | Modify | 21 of 26 change; the 4 no-AI ones and the tiny warning fallback stay identical. Plus the 2 `ui` snapshots |

## Testing Strategy

**Helpers (one place).** A test-only `MenuState::goto(&mut self, label)` presses `j` through the current tab, then `Tab`, across every visible tab, and panics with `no row {label}`. It replaces the bodies of: ui `goto`, `footer_tests::goto`, `memory_tests::goto_memory`, `save_tests::on_task_row`, `picker_tests::open`, `profile_tests::on_profile` and `tools_tests::on_tools`. The `while selected_label != X { j }` loops that target AI rows would **hang**, not fail, so all of them must change. Loops that target Settings rows (Notify, Sound, Mouse, Updates) keep working. Fixed-count loops (`ai_tests` 198/208/232/386, `context_tests:76`, `profile_tests:72`, `memory_tests:447`, `reanchor_tests:64`, `custom_flow_tests`) are rewritten by hand.

**New tests:**
- `TABS` well-formed: Settings first, every section in exactly one tab, unique labels.
- Tab, Tab+SHIFT, BackTab and BackTab+SHIFT cycle tabs and wrap.
- Ctrl+Tab and Alt+Tab are swallowed.
- A single tab is a no-op.
- `j`/`k` wrap inside Settings (6 rows) and AI (10 rows).
- The cursor is kept per tab.
- Reopening lands on Settings, row 0 (core and app).
- Tab is inert in the engines list, the models list and the custom field. In the warning it declines, and the menu stays on the AI tab.
- `Settings*` appears with an update and goes away once installed.
- The note shows on Settings only.
- `VIEW_BODY` equals the tallest view.
- `fits_warning` at 24x15 (true), 24x14 and 23x15 (false).
- `tab_line` at each step, including a 3-label table and `‹ … ›` clipping.
- Bar styles. The hint is absent over sub-levels.
- The popup is 16 rows across tabs, the picker and the warning. The fallback is drawn at 62x14.
- New snapshots: the AI tab, Settings with the note, and 24 columns with `Settings*` (bar only).

**Sweeps:** every view (both tabs, Settings with the update, pickers, custom, warning, error) is drawn at w 0..=70 × h 0..=20 with no panic.

**Snapshot discipline:** run `cargo insta review` one file at a time. For each file, check:
- 16 rows border to border;
- the bar text and the hint step;
- the same row values as the old snapshot for that tab;
- no section title;
- an unchanged footer.

Confirm with `git diff --stat` that the 4 no-AI `.snap` files and the tiny warning file are untouched.

## Size Forecast

| Part | Non-test lines changed |
|---|---|
| `menu.rs` state, keys, helpers | ~70 |
| `menu.rs` consts, fit, note gate | ~30 |
| ui `tab_line` + render integration | ~70 |
| `app.rs` | 0 |
| **Non-test total** | **~170 (one PR)** |

Test code is about 300–400 lines. Snapshots are about 23 files, roughly 700 diff lines, regenerated and reviewed by hand. The total diff is over 400, almost all of it test and snapshot churn. `sdd-tasks` must raise this with the delivery strategy; the expected answer is a single PR with `size:exception`, because splitting the snapshots from the code that changes them would leave a red PR.

## Migration / Rollout

No migration required: there are no config keys, no file format and no persisted state. Rollback is a revert of the single PR.

## Follow-ups

- After merge, rebase the agent-tools `specs/config-menu` deltas (Tools, Approval) and D18's size paragraph onto tabs. The new text: the popup stays 16 rows, and the AI tab goes to 11 rows, within `VIEW_BODY`.
- 2e-3 adds one `AI_SECTION` item (`Approval`, after `Tools`) and nothing else in the layout.
- 4a adds the `MCP` `TABS` entry, bumps `TAB_COUNT` and adds its section.

## Open Questions

- [ ] If `std::slice::from_ref(&AI_SECTION)` is rejected in a `static` at 1.88, use the `&[&Section]` fallback in T1. This does not affect behaviour.
- [ ] The spec is written in parallel: align its scenarios with the exact thresholds and with the "hint hidden over sub-levels" choice (T9).
