# Exploration: AI menu tabs (#429)

## Problem

The AI tab of the config menu holds 19 flat rows. The popup is 25 rows high (`MENU_BODY` 21 plus `CHROME` 4). MCP connectors (agent-tools 4a) and the ai-cost `Usage (alpha)` row will add more. The issue asks to split the AI tab into Settings │ Models │ Tools │ Connectors │ Chat and to declare each row once.

## Current state (integrated tree: main + agent-orchestrator + model-marks + terminal-agnostic)

| Area | Location | Notes |
|------|----------|-------|
| Row kinds | `src/core/menu.rs` `ItemKind` (~:39) | One variant per behavioural row |
| Sections | `src/core/menu.rs` `SECTIONS` (~:136), `AI_SECTION` (~:197) | Static descriptor tables |
| Tabs | `src/core/menu.rs` `TABS` (~:302) | `[Tab; TAB_COUNT]`, per-tab cursor, Tab/BackTab, height derived by `const fn` |
| Body and rows | `view_body` (~:366), row builders (~:1411-1638) | |
| Keys | `on_key` (~:1643), `cycle` (~:1977) | |
| Edits | `edits` (~:2017) | Ordered `ConfigEdit` list |
| Footer | (~:2176) | |
| Per-row AI logic | `src/core/ai/menu.rs` | toggle / cycle / changed triple per row |
| Widget | `src/ui/components/menu.rs` | Generic; `tab_line` (~:225) |
| Edit constructors | `src/core/config.rs` (~:76-122) | `ConfigEdit` ctors |

## Findings

- Tab infrastructure exists and is table-driven. The split is mostly new `TABS` entries and a `TAB_COUNT` bump.
- The real cost of each row: about 8 exhaustive `ItemKind` match sites plus 3 `AiMenu` methods.
- About 11 rows are plain draft values (on/off or a short list over `AiConfig`). They differ only in getter, setter and edit key.
- Behavioural rows need their own code: Assign, Memory (privacy warning), Updates, HfAccount, Feedback, Usage.
- About 40 insta snapshots will churn (tab bar line and popup height).
- `openspec/specs/config-menu/spec.md` trails the code. It still describes 10 AI rows. The in-flight deltas (agent-tools, coding-tools, agent-orchestrator, auto-execute, hf-download) are not archived.
- Manual test T02 (agent-orchestrator) and `docs/assets/tapes/ai-menu.tape` say "Tab to AI".

## Approaches

| Approach | Description | Verdict |
|----------|-------------|---------|
| A. Generalized `Choice` descriptor | `AiChoice` with `get`/`set` fn pointers over `AiConfig` and a derived edit; draft rows collapse into one variant; a small enum stays for behavioural rows | Recommended |
| B. Trait object per row | One `impl Row` per setting | More boilerplate, dynamic dispatch, no gain over A |
| C. Split tabs only | Keep the per-row match sites | Leaves the "declare in 8 places" debt that MCP and Usage will hit |

## Expected result

Popup height drops from 25 rows to about 17. Models is one Tab away from Settings.
