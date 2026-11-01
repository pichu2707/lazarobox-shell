# Tasks: AI menu tabs (#429)

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines (production) | PR 1 ~290, PR 2.1 ~370, PR 2.2 ~270, PR 3 ~300 (total ~1230) |
| 400-line budget risk | Medium (PR 2.1 near the limit) |
| Chained PRs recommended | Yes |
| Suggested split | PR 1 → PR 2.1 → PR 2.2 → (ai-cost 2b) → PR 3 |
| Delivery strategy | ask-on-risk |
| Chain strategy | feature-branch-chain |

Decision needed before apply: Yes
Chained PRs recommended: Yes
Chain strategy: feature-branch-chain
400-line budget risk: Medium

Tests and snapshots are excluded from the budget. Strict TDD: every task is RED (failing test) then GREEN.

### Suggested Work Units

| Unit | Goal | Likely PR | Notes |
|------|------|-----------|-------|
| 1 | Tabs, `1`-`5`, notices, hint, docs | PR 1 | base = tracker `feat/ai-menu-tabs` |
| 2 | Draft descriptor, on/off rows | PR 2.1 | base = PR 1 branch |
| 3 | Profile, Notify, Approval, idle | PR 2.2 | base = PR 2.1 branch |
| 4 | Second key + `SubView` | PR 3 | base = PR 2.2 branch (or after ai-cost 2b) |

Open: owner confirms the 2.1/2.2 split. Merge order 1 → 2.1 → 2.2 → ai-cost 2b → 3; if 2b adds a sub-view, land PR 3 before 2b.

## PR 1: Tabs, jump keys, notices, docs (~290)

- [x] 1.1 RED: `src/core/menu/tabs_tests.rs` visibility, wrap, digits by fixed position, absolute cursor memory (Connectors hidden/appears, spec "Menu tabs", "Jump keys").
- [x] 1.2 GREEN: `src/core/menu.rs`: 4 sections, 5 `TABS`, `TAB_COUNT = 5`, visibility = `needs_ai` AND non-empty `tab_items()`, `tab`/`tab_cursor` keyed by `TABS` position (D1, D2), `active_tab()` mapping.
- [x] 1.3 RED: notice tests: `4` hidden, `2`-`5` without AI, `0`/`6`-`9` show `Tabs are 1-5`, Tab/Shift+Tab with one tab shows `No other tabs`, Ctrl/Alt+digit swallowed, cleared by next key, `s`/Esc not blocked.
- [x] 1.4 GREEN: `notice` field and digit handling at top level only (D10); sub-views keep digits and Tab inert; footer shows notice with error precedence.
- [x] 1.5 RED: `src/ui/components/menu.rs` bar thresholds (49/48/62 five tabs; 49/48/36/35 four tabs), `Settings*`, 16-row height, 24x15 minimum, 0x0 no panic.
- [x] 1.6 GREEN: `TAB_HINT = "⇥ Tab · 1-5"`, `VIEW_BODY` = 11, hint hidden in sub-levels, Update note Settings-only.
- [x] 1.7 Docs: orchestrator `manual-tests.md` T02 and every "Tab to AI" step; `docs/assets/tapes/ai-menu.tape` (reach Models with `2`); README menu section (Spanish prose: five tabs, Tab/Shift+Tab, `1`-`5`); CHANGELOG (AI tab split, jump keys).
- [x] 1.8 Commit: `test(menu): update snapshots` holding only `.snap` files (~40).
- [x] 1.9 LAST commit, test-only: characterization test `src/core/menu/edits_tests.rs` asserting exact `Vec<ConfigEdit>` and `apply_edit` TOML from an empty file (one changed row of every kind, live row, tasks, managed, memory `enabled = true` with `acknowledged = false`, off-list idle 45). Visit-order and unchanged-tabs scenarios. Must pass untouched in later PRs.

## PR 2.1: Descriptor and on/off rows (~370)

- [ ] 2.1.1 RED: `get(set(i)) == i` and `values[i] == ALL[i].id()` for every descriptor; rank uniqueness.
- [ ] 2.1.2 GREEN: `Choice<T = Config>`, `AiChoice`, `DraftValue::Flag`, `rank`, `caution`, `badge`, `test` (D3, D7, D8) in `src/core/menu.rs`, `src/core/ai/menu.rs`.
- [ ] 2.1.3 Migrate Context, Sound, Tools, 5 families; delete their `rows()` arms, `AiMenu` methods and `edits` blocks; `needs_ai()` derived from kind (D5).
- [ ] 2.1.4 Gate: 1.9 test and snapshots unchanged (no pending `.snap`).

## PR 2.2: List rows and Minutes (~270)

- [ ] 2.2.1 RED: Profile, Notify, Approval cycling; idle 45 steps both ways, `never`/`N min`.
- [ ] 2.2.2 GREEN: `DraftValue::List` and `Minutes { step: cycled_idle }` (D4); migrate the four rows.
- [ ] 2.2.3 Gate: 1.9 test unchanged, no pending snapshots.

## PR 3: Second key and SubView (~300)

- [ ] 3.1 RED: Memory warning (both keys on; accept sets both; `on` only when both true), Orchestrator without warning; edits emit `key` then `second.key` only when changed.
- [ ] 3.2 GREEN: `Second { key, get, set, warn }` (D6); Memory and Orchestrator become `Draft`.
- [ ] 3.3 RED/GREEN: `SubView { Warning, Picker, AccountField, Feedback }` via one `sub_view()`; exhaustive `match` in `rows`, `on_key`, `footer`, `on_paste`, `in_sub_level` (D9). Account `y/n` stays a pre-check.
- [ ] 3.4 Gate: 1.9 test unchanged, no pending snapshots.

## Manual checks (Kitty and tmux)

- [ ] M.1 Kitty, standard width: Tab, Shift+Tab, `1`-`5`; `4` shows the notice; nvim gets no keys.
- [ ] M.2 Kitty at 49 and 48 columns: whole bar, then only ` ‹ active › `; no panic.
- [ ] M.3 tmux: Tab, Shift+Tab (BackTab) and digits work; sub-levels inert.
- [ ] M.4 `0`, `6`-`9` show `Tabs are 1-5`; no-AI menu: Tab shows `No other tabs`.

## Risks

- Once Connectors is visible and an update waits, BAR = 48 and the `⇥ Tab · 1-5` hint is hidden at the standard width (spec scenario "Five tabs and an update drop the hint"). Future, when MCP rows land.
- MCP rows are dynamic, so `view_body()` cannot count them; agent-tools 4a must cap or reserve rows.
- Archive after all in-flight config-menu deltas.
