# Proposal: usage-dashboard (/cost overlay redesign)

## Intent

`/cost` is a plain text report: raw unseparated numbers, one fixed 7-day window, no time-of-day view, no wall time, no provider identity. Users cannot answer "how much did I use this month, when, with which provider, and is it going up?" at a glance. The owner (2026-11-22) asked for a dashboard modelled on a reference screenshot, with glyph+colour provider identity instead of image logos.

## Users and situations

- A developer checking spend/volume after a work session or before a billing cycle (7/30 days).
- A user comparing billed vs local engines and models.
- An orchestrator user who wants agent wall time and peak concurrency.
- Users on narrow splits, CJK terminals or no-colour terminals: the view must stay readable.

## Scope per slice (each under 400 production lines)

| Slice | Contents |
|---|---|
| S1a | Pure `usage_fmt.rs` (`compact`, `tok`, `grouped`); pure `classify(engine, model)` provider map with glyph width-1 test; nullable `started_at`/`wall_ms` columns via `ensure_usage`, filled in `finish()`, so data accumulates |
| S1b | Structured Dashboard model in `CostView`; range selector 7/30/90/All; four KPI cards with sparkline and delta; width layout; text-report fallback under 40 columns |
| S2 | Tokens by day, input above the axis, output below |
| S3 | Weekday x hour heatmap (3-hour buckets under ~60 columns) |
| S4 | Models table + provider stacked bar (largest-remainder rounding) |
| S5 | Agents/engines table; agent time and "up to N at once" |
| S6 | Formatter everywhere (status segment, usage line, limit notes); remove superseded text sections; README.md + README.es.md; snapshots |

## Non-goals

- No image logos.
- No cost-per-day chart unless trivial on existing data.
- No change to how usage is collected beyond the two columns.
- No retention change (stays 90 days); no backfill of old rows.
- No per-worker or per-run drill-down.

## Business rules

- **KPI cards**: input tokens (+ cache share); output tokens (+ average per turn); tasks (+ turns); agent time (+ "up to N at once").
- **Task** = distinct non-null `conversation_id` or distinct orchestrator run within the range. **Turns** = rows with `role = 'chat'`.
- **Delta** vs the previous equal period for 7 and 30 days; `n/a` for 90 days and All.
- **Numbers**: compact everywhere (`1.2M`, `123.5k`, `11.6B`), one unit `tok`, `.` decimal; KPI counts grouped (`1,110`); limit notes exact and grouped (`turn used 123,456 tokens (limit 100,000)`).
- **Providers**: ✻ Anthropic, ✦ Google, ◎ OpenAI, a purple glyph for local engines; every glyph has a text label; meaning never by colour alone.
- **Dollars** only for billed engines, under existing billing-mode rules.
- **Layout**: >=80 cols four cards in a row; 60-79 2x2; 40-59 one column; <40 old text report.
- **Security unchanged**: usage is display-only, counts/ids/timestamps only, never reaches a model or a log.

## Edge cases

| Case | Expected |
|---|---|
| No data | Empty state message, no charts, no panic |
| Single engine | Stacked bar is one full segment; tables have one row |
| Unknown model / NULL / `mixed` | Provider `Other` (or by engine), label `unknown` / `mixed` |
| Local engines | Tokens shown, never dollars |
| Deleted chats | Tokens still counted; task count undercounts (NULL link), a known caveat |
| First 90 days after columns ship | Agent time/concurrency approximate or `n/a` for old rows; delta `n/a` if the previous period lacks wall data |
| CJK terminal, ◎ renders 2 columns | Layout must not break; design decides fallback glyph or tolerance |
| No colour | Glyph + label carry meaning; heatmap uses shade glyphs |
| Store unavailable / loading | Existing `usage history is not available` / `reading usage…` |

## Capabilities

### New Capabilities
- `usage-dashboard`: the `/cost` dashboard: range selector, KPI cards, deltas, tokens-by-day, heatmap, Models and Agents tables, provider glyph map, width layout and narrow fallback.

### Modified Capabilities
- `ai-usage`: `TurnUsage`/store gains nullable `started_at`/`wall_ms`; "Usage text formats" switches to compact/grouped numbers; "Thresholds" notes use grouped exact numbers; "The `/cost` view" and "Charts" are superseded by `usage-dashboard` (text report kept only under 40 columns); "Usage privacy and README" covers the new columns.
- `statusline`: the `today: <N> tok` segment uses compact numbers.

## Approach

Exploration approach A: a structured Dashboard model built from store queries off the UI thread, rendered as ratatui sections (`Sparkline`, `Table`, custom mirrored cells, custom grid, custom spans). Pure core for formatting, classification and aggregation; colours live only in `ui/theme.rs` (new neutral token for OpenAI).

## Affected Areas

| Area | Impact | Description |
|---|---|---|
| `src/core/ai/chat/usage_fmt.rs` | New | Formatter |
| `src/runtime/chat_store.rs` | Modified | Two nullable columns |
| `src/runtime/worker.rs` | Modified | Range/hour/weekday/wall queries |
| `usage_view.rs`, `cost_view.rs` | Modified | Dashboard model, fallback |
| `src/ui/components/chat/cost.rs` | Modified | Section rendering |
| `src/ui/theme.rs` | Modified | Provider colours, neutral token |
| `README.md`, `README.es.md` | Modified | Same PR, same sections |

## Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| The 8 `/cost` snapshots change | High | Review each diff deliberately in the slice that changes it |
| README.md / README.es.md drift | Med | Change both in the same PR (AGENTS.md rule) |
| ◎ ambiguous width on CJK terminals | Med | unicode-width says 1; design picks fallback or tolerant layout |
| Local engine glyph not chosen | Med | Width test decides; settle in design |
| Worker start instant unverified | Med | Verify in S1a; else worker wall time is `n/a` |
| Slice over 400 lines (S1b ~350) | Med | Split S1b if forecast exceeds budget |
| Heavier queries over 90 days | Low | Off-thread, indexed by `ts` |

## Rollback Plan

Each slice is an independent PR; revert it. The new columns are nullable and additive, so old binaries ignore them; no data migration to undo. The text report remains as the fallback path until S6.

## Dependencies

- None external (ratatui 0.30, unicode-width 0.2 already present).

## Success Criteria

- [ ] `/cost` shows range selector, four KPI cards with sparkline and delta (`n/a` for 90/All).
- [ ] All provider glyphs pass the width-1 test and always show a text label.
- [ ] Dashboard renders without panic at every width from 0 up; <40 columns shows the text report.
- [ ] Numbers follow the compact/grouped rules everywhere, including limit notes and status segment.
- [ ] New rows store `started_at`/`wall_ms`; old rows show approximate or `n/a`.
- [ ] Privacy scenarios still pass; READMEs updated together.

## Proposal question round

Owner decisions are binding; remaining items for user review:
1. Which glyph for local engines (purple)?
2. On CJK terminals, swap ◎ for a width-1 alternative, or accept and pad?
3. Agents/engines table columns: tokens, turns, wall time, dollars?
