# Tasks: Config Menu Tabs (Settings | AI, MCP later)

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | ~170 non-test, ~300-400 test code, ~700 snapshot diff lines (23 `.snap` files). Total about 1,200-1,300 changed lines |
| 400-line budget risk | High (total). Non-test code alone is Low (~170). Almost all of the excess is test and snapshot churn |
| Chained PRs recommended | Optional. A split exists and stays green, but it does not remove the need for `size:exception` (see below) |
| Suggested split | Option A (recommended): one PR, 5 work-unit commits, `size:exception`. Option B: PR1 = test-helper refactor (no behaviour change), PR2 = tabs + snapshots (`size:exception` still needed) |
| Delivery strategy | ask-on-risk |
| Chain strategy | not needed for Option A. Option B would use stacked-to-main (PR1 merges first, PR2 rebases on main) |

Decision needed before apply: Yes
Chained PRs recommended: Optional (no, for the recommended option)
Chain strategy: none for Option A; stacked-to-main for Option B
400-line budget risk: High

### Decision needed before apply

- **Option A (recommended): single PR with `size:exception`.** Five work-unit commits, each green. The behaviour commit (unit 3) must contain the code, the tab-aware tests and the regenerated snapshots together, because the snapshots are the output of that code. Review effort is concentrated in ~170 non-test lines plus a checklist-driven snapshot review (unit 4).
- **Option B (viable alternative): two PRs.** PR1 = units 1-2: the test-helper refactor (`goto(label)`, hand-rewritten fixed `j` counts rewritten to label-based `goto`) and the pure `tab_line` function. PR1 is real and green: `goto(label)` is label-driven and bounded, so it works on today's single stacked list and keeps working once rows move to tabs. It changes no behaviour and no snapshot, and is roughly 200-250 lines (test code and ~30 lines of non-test `tab_line` under a temporary `#[allow(dead_code)]`). PR2 = units 3-5: tabs, render integration, 23 snapshots. PR2 is still ~170 non-test + ~150 new tests + ~700 snapshot lines, so it STILL exceeds 400 and still needs `size:exception`. Option B therefore lowers the review load of PR2 by moving the mechanical test churn out, but it does not remove the exception. The cost is one more PR cycle.
- **Option C (rejected): splitting the snapshots from the code.** Leaves the first PR red (snapshots out of date) or the second PR without its output. Not viable.

If the user picks Option B, the "Per-PR task counts" below apply; if A, all tasks ship in one PR.

Rules for every unit:
- Strict TDD: RED first (compiling stubs, exact assertions), then GREEN, then REFACTOR. Tests are never trimmed.
- Gate = `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
- One work-unit commit per unit (conventional commits, no AI attribution). Nothing is committed by the planning phase. Each commit builds and passes the gate on its own.
- Snapshots are reviewed one file at a time with `cargo insta review`; never `cargo insta accept` in bulk.
- Do not refactor the ten `self.selected == Some(index)` arms in `rows()` and `step_selection` (design T2).

### Per-PR task counts

| Scope | Tasks |
|-------|-------|
| Unit 1 (test helpers) | 7 |
| Unit 2 (`tab_line`) | 6 |
| Unit 3 (tabs, render, snapshots regen) | 17 |
| Unit 4 (snapshot review) | 4 |
| Unit 5 (manual, docs) | 3 |
| **Total** | **37** |
| Option A: single PR | 37 |
| Option B: PR1 (units 1-2) / PR2 (units 3-5) | 13 / 24 |

## Unit 1: Tab-aware test helpers, no behaviour change (`src/core/menu.rs`, `src/core/menu/*_tests.rs`, `src/ui/components/menu.rs` tests)

Goal: after this unit, no test depends on the AI rows being below the Settings rows by position or count. Behaviour and snapshots are untouched. Commit: `test(menu): drive menu tests through a label-based goto helper`.

- [x] 1.1 RED: tests for `#[cfg(test)] MenuState::goto(&mut self, label)`: reaches every row label of the current stacked layout from any start; panics with `no row {label}` for an unknown label (`#[should_panic]`); the loop is bounded (it never spins more than the selectable count, so a missing label fails instead of hanging). Stub `goto` with `todo!()` so it compiles and fails.
- [x] 1.2 GREEN: implement `goto(label)` by pressing `j` through the rows, bounded by `selectable()`. It does not press Tab yet (Tab is still swallowed). Put it next to the other `#[cfg(test)]` helpers in `src/core/menu.rs`.
- [x] 1.3 REFACTOR: replace the bodies of the seven local helpers with `goto`: ui `goto`, `footer_tests::goto`, `memory_tests::goto_memory`, `save_tests::on_task_row`, `picker_tests::open`, `profile_tests::on_profile`, `tools_tests::on_tools`. Remove the `while selected_label != X { j }` loops that target AI rows.
- [x] 1.4 REFACTOR: rewrite the fixed-count `j` loops to `goto(label)` in `ai_tests` (~198, 208, 232, 386), `context_tests` (~76), `profile_tests` (~72), `memory_tests` (~447), `reanchor_tests` (~64) and `custom_flow_tests`. Keep each assertion exactly as strong as before.
- [x] 1.5 REFACTOR: do the same for the menu-driving tests in `src/app.rs` and `src/app/app_tests/menu_copy.rs` (any fixed `j` count that lands on an AI row). Tests only; no non-test change in `app.rs`.
- [x] 1.6 Gate. Confirm `git diff --stat` shows no `.snap` file touched.
- [x] 1.7 Docs: doc comment on `goto` stating it is tab-aware from unit 3 and bounded.

## Unit 2: Pure `tab_line` (`src/ui/components/menu.rs`)

Goal: the bar-degradation rule exists as a pure, fully tested function before anything uses it. Commit: `feat(menu): add the pure tab bar line builder`. It needs no `#[allow(dead_code)]`: the function is `pub` in a library crate, so clippy stays green without it. Unlike the planned signature it also takes the theme (the styles come from it).

- [x] 2.1 RED: `tab_line(labels, active, width, hint) -> Vec<(String, Style)>` tests with a stub that returns an empty vec. Cases: 2 labels `Settings`,`AI` give ` Settings │ AI ` (15 cells) plus the hint right-aligned ending one column before the inner right edge when W >= 22; with `Settings*` BAR is 16 and the hint shows only when W >= 23; bar-only when BAR <= W < BAR + 7; active-only ` ‹ AI › ` clipped to W when W < BAR; a 3-label table (BAR 21/22; hint at 28/29 W); width 0 and 1 give no panic.
- [x] 2.2 RED: style tests: active segment is `fg bg_base, bg primary_cyan, BOLD`; idle labels, `│` and hint are `text_muted`; the active style differs from the selected-row style (`info_blue` REVERSED).
- [x] 2.3 RED: the hint is absent when the caller passes `None` (used over sub-levels, design T9).
- [x] 2.4 GREEN: implement `tab_line` with the step table from the design (bar + hint, bar only, active only, using the existing `clip`).
- [x] 2.5 Gate.
- [x] 2.6 Docs: doc comment on `tab_line` with the step thresholds.

## Unit 3: Tabs in the core and the widget, snapshots regenerated (`src/core/menu.rs`, `src/ui/components/menu.rs`, tests, snapshots)

Goal: the behaviour change. This is the atomic unit: code, tests and regenerated snapshots must land in one commit to stay green. Commit: `feat(menu): group the config menu rows into Settings and AI tabs`.

- [ ] 3.1 RED (descriptors): `TABS` well-formed: Settings first, AI second, every `Section` in exactly one tab, unique labels, MCP absent; `tabs()` lists only `Settings` when the AI section is not attached; the AI-tab visibility follows `needs_ai`. Stub `Tab`, `TABS`, `TAB_COUNT`, `tabs()` so it compiles and fails.
- [ ] 3.2 RED (keys): Tab / Tab+SHIFT / BackTab / BackTab+SHIFT cycle with wrap; Ctrl+Tab and Alt+Tab swallowed; Repeat of Tab and BackTab switches like Press; single tab is a no-op; switching emits no `ResizePty`, `WritePty`, Refresh or Save and leaves the draft unchanged; Tab with the key list `Tab is listed` in the swallow tests moves out of the ignored lists.
- [ ] 3.3 RED (cursor and navigation): `j`/`k`/Up/Down wrap inside Settings (6 rows) and inside AI (10 rows) and never cross tabs; cursor kept per tab; each tab starts on its first row; reopening lands on Settings row 0 with the memory cleared (core and `app.rs` test using `open_menu`).
- [ ] 3.4 RED (sub-levels): Tab, BackTab and Tab+SHIFT are inert in the engines list, the models list and the custom field (buffer stays `ab`, field focused); in the privacy warning Tab declines, the draft stays `off`, nothing acknowledged, and the active tab stays AI.
- [ ] 3.5 RED (labels and note gate): `tab_labels()` reads `Settings*` iff `update.is_some() && installed.is_none()` and `Settings` after install; `update_note()` is `Some` only on the note tab (Settings) and `None` on AI and over sub-levels; `in_sub_level()` true for warning, picker and custom field.
- [ ] 3.6 RED (sizing): `VIEW_BODY` equals the tallest view and no view exceeds it; `MENU_BODY == 12`; `warning_rows().len() == WARNING_ROWS`; `fits_warning` true at 24x15, false at 24x14 and 23x15; no-AI path keeps its previous size. Remove the old `AI_BODY` / `VIEW < AI_BODY` assert tests.
- [ ] 3.7 RED (render): TestBackend asserts: bar on body row 0 with the hint on a normal terminal; hint absent over every sub-level; no section title rows when tabbed; titles still drawn for a single tab; popup is 16 rows on Settings, AI, picker L1/L2, custom field and warning; note in the last 2 body rows on Settings only; fallback at 22 columns and at 62x14; `Settings*` at 24 columns drops the hint; 24x15 draws the full popup. Sweeps: every view (both tabs, Settings with update, pickers, custom, warning, error) at w 0..=70 x h 0..=20 with no panic.
- [ ] 3.8 RED (snapshots): add the three new insta cases (AI tab, Settings with the note, 24 columns with `Settings*`); let the existing menu snapshots fail against the old files.
- [ ] 3.9 GREEN (core state): add `Tab { label, sections, needs_ai, note }`, `static TABS`, `TAB_COUNT`, `tab` and `tab_cursor: [usize; TAB_COUNT]`, `tabs()`, `tab_items()`, `step_tab` (save to `tab_cursor[tab]`, move, load `selected`), `selected_kind()` and `selectable()` on `tab_items()`, `in_sub_level()`, `tab_labels()`, `active_tab()`. If `std::slice::from_ref(&AI_SECTION)` is rejected in a `static` at MSRV 1.88, use the `&'static [&'static Section]` fallback (design T1).
- [ ] 3.10 GREEN (keys): Tab / BackTab arms in `on_key` after the modifier guard, top level only (T5). No code for sub-levels (T6).
- [ ] 3.11 GREEN (sizes): `TAB_BAR`, `NOTE_ROWS`, `WARNING_ROWS`, const fn `view_body()`, `VIEW_BODY`, `MENU_BODY`; remove `AI_BODY`, `ai_body()` and the const assert; `fits_warning()` becomes `fits_size(cols, rows, MENU_BODY)`; `update_note()` returns `Option<[String; NOTE_ROWS]>` gated by tab and sub-level; `rows()` emits `Row::Title` only when one tab.
- [ ] 3.12 GREEN (widget): remove the unit-2 `#[allow(dead_code)]`; draw `tab_line` at body row 0 when tabbed (hint only when not `in_sub_level()`); `body = if tabbed { MENU_BODY.max(TAB_BAR + rows.len()) } else { rows.len() }`; list rows start at `inner.y + bar`; note drawn in the last 2 body rows; `POPUP_H` from `MENU_BODY`; single-tab path keeps its old code.
- [ ] 3.13 GREEN (test helper): extend `goto(label)` to press `j` through the current tab, then `Tab`, across every visible tab, still bounded and panicking with `no row {label}`. Fix any test that still passes by accident of order.
- [ ] 3.14 REFACTOR: delete the dead subtraction logic in `selected_kind()` and the unused `settings()` helper; keep the `selected == Some(index)` arms as they are.
- [ ] 3.15 Regenerate snapshots: run `cargo insta test`, then review each file one by one in `cargo insta review` using the unit-4 checklist (do not accept in bulk). Accept only after the unit-4 review of that file passes.
- [ ] 3.16 Gate (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`).
- [ ] 3.17 Docs: doc comments on `Tab`, `TABS`, `step_tab`, the size consts (`VIEW_BODY` is the max over views, the picker is the binding term); README / user docs if the menu is described there (tabs, `Tab` / `Shift+Tab`).

## Unit 4: Snapshot review (blocking before the PR is opened)

Goal: the 700 lines of snapshot diff are reviewed by hand, not trusted.

- [ ] 4.1 Review the 21 changed menu snapshots and the 2 `ui` snapshots, one file at a time, against this checklist: (a) 16 rows border to border; (b) the bar text is right for the state, with the hint present at the right width and absent over sub-levels; (c) the row values equal the old snapshot's values for the same tab; (d) no section title under the bar; (e) the footer is unchanged; (f) the update note is on Settings only and sits in the last 2 body rows.
- [ ] 4.2 Confirm with `git diff --stat` that the 4 no-AI `.snap` files and the tiny warning fallback `.snap` are untouched (byte-identical). Any change there is a bug in the single-tab path.
- [ ] 4.3 Confirm no `.snap.new` / `.pending-snap` files are left in the tree.
- [ ] 4.4 Re-run the full gate once more after the last accepted snapshot.

## Unit 5: Manual check and docs

- [ ] 5.1 [manual] Open the menu in a real terminal (Kitty or similar): `Ctrl+Space m` opens on Settings row 0; Tab and Shift+Tab switch tabs, the cursor is kept per tab, and `j`/`k` wrap inside a tab.
- [ ] 5.2 [manual] Repeat the Tab / Shift+Tab check inside tmux and inside zellij; they may encode BackTab differently (`ESC [ Z` versus CSI-u). Record which encodings arrive and confirm both `BackTab` and `Tab+SHIFT` work. Also check narrow widths (shrink to 24 and 22 columns: bar, then hint dropped, then fallback) and `Settings*` with an update pending (bar label and the note on Settings only).
- [ ] 5.3 Docs: update the `config-menu` main spec only at archive time, not here. Note the result of 5.2 in the PR description.

## Follow-ups (outside this change)

- After merge, rebase the agent-tools `specs/config-menu` deltas (Tools, Approval) and the D18 size paragraph onto tabs. New text: the popup stays 16 rows and the AI tab goes to 11 rows, within `VIEW_BODY`.
- Agent-tools 2e-3 then adds one `AI_SECTION` entry (`Approval`, after `Tools`) and no layout change.
- Agent-tools 4a adds the `MCP` `TABS` entry, bumps `TAB_COUNT` and adds its section.
- Update the main `config-menu` spec at archive (this delta is written from the code; it includes drift fixes).

## Spec-to-task coverage

| Requirement | Tasks |
|-------------|-------|
| Menu tabs (cycle, Shift+Tab encodings, per-tab cursor, modifiers and repeat, no side effects, MCP not listed) | 3.1, 3.2, 3.3, 3.9, 3.10, 5.1, 5.2 |
| Menu tab bar (layout, styles, hint steps, `Settings*`, active-only guard, single tab hides bar) | 2.1-2.4, 3.5, 3.7, 3.12, 4.1, 4.2 |
| Sub-levels keep the bar inert (picker, custom field, warning) | 3.4, 3.7, 3.12 |
| Popup height is fixed (16 rows, body is the max over views) | 3.6, 3.7, 3.11, 3.12 |
| Update note is Settings-only | 3.5, 3.7, 3.11, 3.12, 4.1 |
| Notify, Sound and Updates keys (drift fix) | 1.4, 1.5 (tests keep passing through `goto`), 3.2, 3.3 (rows reachable on Settings), 4.1 |
| MODIFIED Open and close (reopen resets tab and cursor) | 3.3, 3.9 |
| MODIFIED Sections and items are data (descriptor integrity, Settings and AI rows) | 3.1, 3.7, 3.8 |
| MODIFIED Navigation (tab-local wrap) | 3.3, 3.9 |
| MODIFIED Input is swallowed (Tab listed at top level only) | 3.2, 3.4, 3.10 |
| MODIFIED Tiny terminals (24x15, fallback, no panic) | 3.6, 3.7 |
| MODIFIED AI task list (rows, marks, stable size) | 3.7, 3.8, 4.1 |
| MODIFIED Picker viewport (fixed height) | 3.6, 3.7 |
| MODIFIED Notify row (Settings tab, navigation order) | 3.3, 3.7, 4.1 |
