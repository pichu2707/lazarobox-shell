# Exploration: usage-dashboard (/cost overlay redesign)

The owner asked for this on 2026-11-22: a dashboard modelled on a reference screenshot, with a glyph and a colour per provider instead of image logos. This text mirrors engram `sdd/usage-dashboard/explore` (obs 4090).

## Current state

- **Store.** The usage table is `turn_usage` in `src/runtime/chat_store.rs` (SCHEMA_USAGE).
  - `ts` is in unix seconds and is written at the end of the turn.
  - Other columns: `conversation_id` (ON DELETE SET NULL), `engine`, `model` (NULL when unreported, "mixed" when several models answered), `role` (chat|worker|compaction|summary), `worker` ("run.worker"), the token columns, `cost_micros`, `duration_ms`, `partial`, `prefill_ms`, `load_ms`.
  - Columns are added through `ensure_usage`, not through a schema version, so every change must stay additive.
  - Retention is `USAGE_DAYS = 90` (worker.rs).
- **Durations.**
  - `duration_ms` is the API time the engine reports, not wall time, and codex reports none.
  - Wall time is not stored. `ChatState.started` (an `Instant`) exists, so `finish()` can measure it.
  - It is not yet verified that workers have a start instant.
- **Report query.** `usage_report` (worker.rs) groups by day x engine x model x mine. It has no hour or weekday dimension and no wall time.
- **The /cost overlay** is plain text:
  - `report_lines` (usage_view.rs) produces the lines;
  - `CostView` (cost_view.rs) holds them;
  - `render_cost` (`ui/components/chat/cost.rs`) draws them;
  - 8 snapshots cover it.
- **Number formatting.** There is no compact formatter. Raw numbers appear in `today_segment`, `usage_line`, `pair_lines` and the limit notes.
- **Theme.** `ui/theme.rs` has fixed Rgb tokens and no neutral one. The TUI does not honour NO_COLOR.
- **Engines.** claude, codex and gemini are billed. ollama, llamacpp, lmstudio and vllm are local. Model names are free strings.
- **Layout.** `MIN_PANE` is (2,10). The project uses unicode-width 0.2 and ratatui 0.30.

## Findings

1. **Time data.**
   - The hour and weekday come from `ts` in local time. Engine and model are already stored.
   - Agent time and "N at once" need two new nullable columns, `started_at` and `wall_ms`, filled in `finish()`.
   - Old rows can only be approximated, as `ts - duration_ms`, or shown as n/a.
   - The heatmap and tokens by day work on old rows.
   - With 90-day retention, a delta for the 90 days and All ranges is not possible.
2. **Task.** A task is a distinct non-null conversation plus a distinct orchestrator run. Turns are the rows with `role = 'chat'`.
3. **Widgets.**
   - Each KPI card uses a `Sparkline`.
   - Tokens by day uses custom mirrored cells, because `BarChart` cannot draw negative bars.
   - The heatmap is a custom 7x24 grid. Under about 60 columns it uses 3-hour buckets, and it uses shade glyphs when there is no colour.
   - The Models and Agents tables use `Table`.
   - The provider stacked bar uses custom spans with largest-remainder rounding.
   - The range selector is a custom `Line`.
   - Layout by width:
     - 80 columns or more: four cards in a row;
     - 60 to 79: a 2x2 grid;
     - 40 to 59: one column;
     - under 40: the current text report.
4. **Formatter.** Add a pure `src/core/ai/chat/usage_fmt.rs` with `compact(u64)` and `tok(u64)`. Pin the unit boundaries in tests, for example 999_950 -> 1.0M.
5. **Provider map.**
   - A pure core function `classify(engine, model)` returns Anthropic|Google|OpenAI|Local|Other.
   - Colours live only in the UI. OpenAI needs a new neutral token.
   - Glyph candidates are ✻ (Anthropic), ✦ (Google) and ◎ (OpenAI; East Asian Width "ambiguous"). Glyphs for the local engines are not chosen yet.
   - A unit test must assert width 1 for every glyph.
6. **No colour.** Meaning must never rely on colour alone. Every element carries a glyph plus a text label.

## Approaches

- **A (recommended).** A structured Dashboard model in `CostView`, drawn as sections, with the text report kept as the fallback under 40 columns.
- **B.** Text charts inside `Vec<String>`. This is a dead end for colour.

## Slice plan (production lines)

| Slice | Lines | Contents |
|---|---|---|
| S1a | ~250 | Formatter, provider map with glyph test, `started_at`/`wall_ms`, retention |
| S1b | ~350 | Dashboard model, KPI cards, range selector, fallback |
| S2 | ~250 | Tokens by day |
| S3 | ~250 | Heatmap |
| S4 | ~300 | Models table and stacked bar |
| S5 | ~250 | Agents table, wall time and concurrency |
| S6 | ~150 | Formatter everywhere, remove the old report, READMEs, snapshots |

## Decisions for the owner

1. The task definition.
2. Retention: 90 days or about 190.
3. The final glyph set.
4. Whether the limit notes switch to compact numbers.
