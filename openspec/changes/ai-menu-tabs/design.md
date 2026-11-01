# Design: AI menu tabs (#429)

## Technical Approach

Exploration approach A on the integrated tree. `TABS` grows to five entries. Rows move out of `AI_SECTION` into four sections. Draft rows become `ItemKind::Draft(AiChoice)` descriptors. One `SubView` enum replaces the chained `if` checks. Keys, write order and widths do not change.

## Architecture Decisions

| # | Topic | Options | Decision and why |
|---|-------|---------|------------------|
| D1 | Tab visibility | (a) `Tab.visible: fn(&MenuState)`; (b) hide a tab with no rows | **(b)**: a tab is visible when `needs_ai` allows it AND `tab_items()` is not empty. It needs no new field, and Connectors (`sections: &[]`) appears on its own once MCP adds rows. `view_body()` still sums every tab, hidden or not, so the `const fn` and the fixed height stay. (a) adds a pointer the `const fn` cannot call and duplicates the row count. |
| D2 | Tab index | visible position vs `TABS` position | **`TABS` position**. Today `tab` and `tab_cursor` use the visible position. With Connectors hidden in the middle, that breaks fixed digits and per-tab cursors. `active_tab()` maps the stored position to the visible one for `tab_line`. |
| D3 | Draft descriptor | twin type; `Choice` with a runtime `Live\|Draft` target; generic `Choice<T = Config>` | **Generic**. `Choice<T = Config>` keeps every live call site. Draft rows wrap `Choice<AiConfig>`. A runtime target would put a `match` in every get/set, and live and draft already behave differently (`Changed` relayout vs none). A twin type would duplicate the cycling code. |
| D4 | Values that are not in the list (idle 45) | index get/set; a dedicated `Minutes` shape | **`Minutes { get, set, step: cycled_idle }`**. An index cannot step from 45 to 30 backwards. The row shows `never` / `N min`. The edit is the integer, and `is_integer` is unchanged. |
| D5 | Rows kept as variants | — | `Choice`, `Toggle` (live), `Draft`, `Assign`, `Updates`, `HfAccount`, `Feedback`, later `Usage`. Memory and Orchestrator stay variants until PR 3 (D6). `needs_ai()` becomes true for every AI-only kind (`Draft`, `Assign`, `HfAccount`). |
| D6 | Second key (`acknowledged`) | keep 2 variants; `second: Option<Second>` | **`Second { key, get, set, warn: Option<&Warning> }`**. A row with a `warn` shows `on` only when both keys are true. Turning it on without the second key opens the warning, and accepting sets both keys. Memory has `warn = Some`. Orchestrator has `warn = None`: the chat notice already sets its `acknowledged` through `accept_notice`, and the menu only carries the change into the edits. Edits emit `key` and then `second.key`, each only when it changed. |
| D7 | Write order | row order; `EDIT_ORDER` list; `rank: u8` | **`rank`**. The edit order (profile, context, memory, notify, sound, tools, families, approval, orchestrator, idle) is not the row order. `rank` keeps one declaration per row. A test checks that ranks are unique. Tasks and `managed` stay explicit. |
| D8 | Optional fields | — | `caution: bool` (Write, Exec, Git write). `badge: Option<Badge>` (`NeedsSandbox` on Exec). `test: bool` (Notify, Sound: the `t` key and `HINT_NOTIFY`). `needs_ai` is derived from the kind, not stored. |
| D9 | Sub-views | trait object; `SubView` enum | **Enum** `{ Warning, Picker, AccountField, Feedback }`, chosen by one `sub_view()` with the priority Warning, Picker, AccountField, Feedback. The views are not always exclusive: an async Hugging Face `REJECTED` reply waits (with a footer error, then a notice) until the open view closes before it opens the token field. `rows`, `on_key`, `footer`, `on_paste` and `in_sub_level` each `match` it exhaustively. Usage and MCP each add one variant, and the compiler lists every site to update. The account `y/n` prompt stays a pre-check because it is a footer over the normal rows. A trait would need `&mut MenuState` anyway, because the state is spread across `ai`, `account` and `feedback`. |
| D10 | Digits | — | `1`–`5` map to `TABS[0..5]` at the top level only (sub-views get digits first, e.g. the Hub search). A hidden target sets `notice` (muted footer, cleared on the next press, like `copied`). Two cases: `4` while Connectors is hidden, and `2`–`5` without an AI section. An unmodified digit is never ignored silently: `0` and `6`–`9` set `Tabs are 1-5`; Tab/Shift+Tab with one visible tab sets `No other tabs`. Ctrl/Alt+digit stay swallowed. |
| D11 | Hint | — | `TAB_HINT = "⇥ Tab · 1-5"` (11 cells). The 5-tab bar is 47 cells: 47 + 13 = 60, which is exactly the inner width at 62. With 4 tabs the bar is 34 cells. Below 49 columns `tab_line` falls back to `‹ label ›`, and that fallback already exists. |

Height: `VIEW_BODY` = max(picker 11, warning 10, Settings 8 + 3 notes, Tools 8 + 1 caution) = 11, so the popup is **16 rows** (was 25). It becomes 17 when ai-cost 2b adds Usage to Settings.

## Data Flow

    key ─→ on_key ─→ sub_view()? ─→ SubView arm
                 └─→ digit / Tab ─→ tab (TABS idx) ─→ tab_items()
                 └─→ h/l ─→ Draft(AiChoice) ─→ AiMenu::step(choice) ─→ draft
    save ─→ edits(): live choices ─→ tasks ─→ drafts sorted by rank ─→ managed

## File Changes

| PR | File | Change | Prod lines |
|----|------|--------|-----------|
| 1 | `src/core/menu.rs` | 4 sections, 5 `TABS`, `TAB_COUNT = 5`, visibility rule, absolute `tab`, digits, `notice`, `needs_ai` | ~230 |
| 1 | `src/ui/components/menu.rs` | `TAB_HINT` | ~3 |
| 1 | README, CHANGELOG, `manual-tests.md` T02, `ai-menu.tape` | "Tab to AI" → tabs and digits | ~60 |
| 2.1 | `menu.rs`, `ai/menu.rs` | `Choice<T>`, `AiChoice`, flag rows (Context, Sound, Tools, 5 families) | ~370 |
| 2.2 | same | List rows (Profile, Notify, Approval) and Minutes (idle) | ~270 |
| 3 | `menu.rs`, `ai/menu.rs` | `Second` hook (Memory, Orchestrator → `Draft`), `SubView` | ~300 |

A single PR 2 would be about 610 lines, mostly deletions (8 `rows()` arms, 16 `AiMenu` methods, 8 `edits` blocks). It is therefore split into 2.1 and 2.2.

## Interfaces

```rust
pub struct Choice<T = Config> { table, key, values, get: fn(&T) -> usize, set: fn(&mut T, usize) }
pub enum DraftValue { Flag(Choice<AiConfig>), List(Choice<AiConfig>),
    Minutes { table, key, get: fn(&AiConfig) -> u32, set: fn(&mut AiConfig, u32), step: fn(u32, bool) -> u32 } }
pub struct AiChoice { value: DraftValue, rank: u8, second: Option<Second>,
    caution: bool, badge: Option<Badge>, test: bool }
```

## Testing Strategy

| Layer | What | How |
|-------|------|-----|
| Characterization | Every draft row changed, plus a live row, tasks, managed, memory with `enabled = true` and `acknowledged = false`, and an off-list idle value | Last commit of PR 1, test-only. It asserts the exact `Vec<ConfigEdit>` and the `apply_edit` TOML from an empty file. PR 2.x and PR 3 must not touch it. |
| Unit | Visibility, digits and notices, absolute cursor, rank uniqueness, `get(set(i)) == i` for every descriptor, `values[i] == ALL[i].id()` | `tabs_tests.rs`, `edits_tests.rs` |
| Snapshot | About 40 churn in PR 1 only | A separate `test(menu): …` commit that holds only `.snap` files. PR 2.x and PR 3 must show no pending snapshots. |
| Size | 0x0, 48 and 49 columns, `fits_size` | widget tests |

## Migration / Rollout

No migration: keys and write order are unchanged. Merge order: PR 1 → 2.1 → 2.2 → ai-cost 2b → PR 3. If 2b brings a sub-view, land PR 3 before 2b. Archive ai-menu-tabs after the in-flight deltas.

## Open Questions

- [ ] The owner should confirm the PR 2 split (2.1 / 2.2) or accept `size:exception` for about 610 lines.
- [ ] The premises place the digits in PR 3, but the proposal places them in PR 1. This design follows the proposal, because the hint ships with the bar.
- [ ] MCP rows are dynamic, so `view_body()` cannot count them. Agent-tools 4a must cap or reserve its rows.
