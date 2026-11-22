# Tasks: usage-dashboard (/cost overlay redesign)

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated production lines | S1a ~190, S1b ~230, S1c ~160, S2 ~200, S3 ~240, S4 ~300, S5 ~240, S6 ~150 (total ~1,710) |
| 400-line budget risk | Medium (S1b+S1c forecast ~390 if joined, so split; S4 ~300 is the next closest) |
| Chained PRs recommended | Yes |
| Suggested split | S1a -> S1b -> S1c -> S2 -> S3 -> S4 -> S5 -> S6 |
| Delivery strategy | auto-chain |
| Chain strategy | stacked-to-main |

Decision needed before apply: No
Chained PRs recommended: Yes
Chain strategy: stacked-to-main
400-line budget risk: Medium

Count = added non-test `.rs` lines, minus blank lines and comments. Tests, fixtures, snapshots, docs are excluded. If a slice's running count passes ~380, stop and split it.

### Suggested Work Units

| Unit | Goal | PR | Notes |
|------|------|----|-------|
| S1a | Formatter, provider map, `started_at`/`wall_ms` columns filled | PR 1 | base main |
| S1b | `UsageData` queries + pure `Dashboard` model (KPIs) | PR 2 | base main after PR 1 merges; /cost output unchanged |
| S1c | Renderer: range selector, cards, tiers, fallback | PR 3 | base main after PR 2; changes /cost snapshots |
| S2 | Tokens by day | PR 4 | base main after PR 3 |
| S3 | Heatmap | PR 5 | base main after PR 4 |
| S4 | Models table + provider bar | PR 6 | base main after PR 5 |
| S5 | Agents table, peak concurrency | PR 7 | base main after PR 6 |
| S6 | Formatter everywhere, drop charts, READMEs | PR 8 | base main after PR 7 |

## Resolved open points

- R1: Output card sub-line = output tokens / count of `role='chat'` rows that have an output (R6), not stored records. This overrides design "output / records". `Dashboard` therefore needs a chat-only output sum (S1b). `avg` is `n/a` when turns is 0.
- R2: S6 drops `chart_lines` from the under-40 text fallback (spec removes Charts).
- R3: Spec assumptions stand: `1000` -> `1.0k`; cache share = cache_read/(input+cache_read) as whole %; leading `~` for approximate values; range keys `1`-`4` and left/right/`h`/`l`.
- R4: Cache share = `cache_hit / cache_base`, the existing text-report definition, not R3's cache_read/(input+cache_read). It includes cache_write for claude and skips records with no cache figure, so the dashboard and the `/cost` text report agree. The sub-line is omitted when there is no cache data.
- R5: Agent time = sum of `wall_ms` over chat AND worker rows (agent-hours: parallel agents add up; S5's `up to N at once` shows the overlap). A chat turn's `wall_ms` is the model working from send to end. Completeness (`~`) counts both roles; compaction and summary runs are not timed. The Agent time delta is an arrow only when both periods are complete, else `n/a`.
- R6: The missing-value text is `n/a` (never 0). Output average = output of chat rows / count of chat rows that have an output.
- R7: Tasks are chats AND orchestrator runs: every worker row with a run yields `r<run>`, linked to a conversation or not, plus `c<id>` per conversation (a chat with 2 runs is 3 tasks).

## Spec vs design conflicts (the design decides)

- C1: Range change. Spec: re-query off the UI thread, show `reading usage…`. Design: one fetch at open, rebuild in core, no round-trip. Follow design; no loading state on range change. Spec scenario "Switching range" is tested as "model rebuilt for 30d, `30d` marked, no store command sent".
- C2: Delta text. Spec: `+12%`, `-8%`, `0%`. Design: `▲ 12%`, `▼ 3%`, `= 0%`, muted. Follow design; `n/a` unchanged. Spec scenario "+100%" is asserted as `▲ 100%`.
- C3: `compact` units. Spec: `k`, `M`, `B`. Design adds `T` and u128 math. Follow design.
- C4: Heatmap placement. Spec: `started_at`, else `ts`. Design hours query: `ts` only. Follow design (`ts`); the fallback is moot.
- C5: Agent-time delta. Spec: deltas on cards 1-3. Design: `Kpi.delta` on all four, with an agent-time `n/a` rule. Follow design.
- C6: Peak concurrency. Spec: worker rows only. Design spans query selects chat and worker rows. Query both, tagged by role; the peak and the Agent card use worker spans only (agrees with the spec; design is silent on the peak scope).
- C7: Text tier range key. Not in the spec. Design: note "widen the pane to change the range". Follow design.
- C8: OpenAI glyph. Spec: unspecified (not `◎`). Design: `❂` U+2742.

## Per-slice protocol (applies to every slice)

- Worktree: `git worktree add ../lazarobox-shell-<slice> -b feat/usage-dashboard-<slice> origin/main` (next to the repo, never `/tmp`).
- Order: RED test (public interface) -> GREEN code -> REFACTOR. MSRV 1.88: no `if let` guards in match arms.
- Close: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, then `rm -rf target` in the worktree.
- CHANGELOG: entries under `[Unreleased]` only; never edit a released section.

## S1a: formatter, provider, timing columns (~190)

- [x] 1a.1 RED: `usage_fmt.rs` tests: `compact` pinned values (999, 1_000->`1.0k`, 1_049, 1_050, 999_949, 999_950->`1.0M`, 123_456, 11_550_000_000, u64::MAX), `tok`, `grouped`, `duration`, `pct` (d=0 -> None). Result: 8 tests in usage_fmt/tests.rs; seen red (compile) then green.
- [x] 1a.2 GREEN: create `src/core/ai/chat/usage_fmt.rs` (u128 tenths, units k/M/B/T); register in `chat/mod.rs`. Result: usage_fmt.rs, registered in chat/mod.rs.
- [x] 1a.3 RED: `provider.rs` tests: `classify` table (claude, gemini, codex, ollama, foo/bar), `every_provider_glyph_is_one_column_everywhere` (width 1, width_cjk 1, not VS16, not `◎`). Result: provider/tests.rs: classify table, labels, glyph width test; red then green.
- [x] 1a.4 GREEN: create `src/core/ai/chat/provider.rs` (`Provider`, `ALL`, `glyph()`, `label()`, `classify`). Result: provider.rs: Provider, ALL, glyph, label, classify.
- [x] 1a.5 RED: `usage.rs` test: `wall_ms` merge keeps max; update `TurnUsage` literals and equality asserts. Result: usage/tests.rs merge max; history_tests wall at finish; red (compile) then green. Existing TurnUsage literals use ..default, none needed edits.
- [x] 1a.6 GREEN: add `wall_ms: Option<u64>` to `TurnUsage` in `src/core/ai/chat/usage.rs`; `finish()` in `chat/mod.rs` sets `now - started`. Result: wall_ms in TurnUsage + merge max; finish() sets now - started.
- [x] 1a.7 RED: `Held` test: `send` stamps `wall_ms` from `born`; a retry gets its own `born`. Result: orchestra/tests.rs held_stamps_wall_time_from_its_own_birth; seen failing on missing stamp.
- [x] 1a.8 GREEN: `born: tokio::time::Instant` in `Held::new`, stamp in `send` (`src/runtime/orchestra.rs`); compaction and summary rows stay NULL. Result: Held.born (tokio Instant), send stamps wall_ms.
- [x] 1a.9 RED: store tests on a fixture DB: old file gains nullable `started_at`/`wall_ms` with rows untouched; `started_at = ts - ceil(wall_ms/1000)`; NULL `wall_ms` -> NULL both; marker test passes. Result: worker/tests.rs: wall/started_at math, old table gains columns; red then green; marker tests pass in the full run.
- [x] 1a.10 GREEN: `ensure_usage` columns (`src/runtime/chat_store.rs`); `record_usage` computes `started_at` (`chat_store/worker.rs`). Result: ensure_usage adds started_at, wall_ms; record_usage computes started_at.
- [x] 1a.11 CHANGELOG `[Unreleased]` entry. No README or snapshot changes. Result: CHANGELOG [Unreleased] entry.
- [x] 1a.12 Close: protocol checks. Result: cargo test ok (11890 passed), clippy -D warnings ok, fmt --check ok. `the_usage_table_holds_only_counts_ids_and_timestamps` column list gained the two new columns.

## S1b: UsageData + Dashboard model (~230)

- [x] 1b.1 RED: `dashboard/` tests: `Range` default Week; task counting (A x5 + B x2 + 1 run x3 -> tasks 3, turns 7); deleted-chat rows add tokens but no task; cache share; R1 average (output / chat turns, not records); delta rules (7d `▲ 100%`, 90d/All `n/a`, previous 0 -> `n/a`); old worker rows -> `n/a`; mixed -> `~`; `bucket`. Result: dashboard/tests.rs, 14 tests; seen failing at runtime (todo! stubs) then green.
- [x] 1b.2 GREEN: create `src/core/ai/chat/dashboard/{mod,kpi}.rs` (`Range`, `UsageData`, `UsageRow`, `Kpi`, `Delta`, `Dashboard::build`, `bucket`); register in `chat/mod.rs`. Result: dashboard/{mod,kpi}.rs (Range, UsageData, Kpi, Delta, Dashboard::build, bucket); UsageRow gained chat_output, worker_wall_ms, worker_timed. Agent card sub-line is `<n> agent turns` until S5 adds `up to N at once`.
- [x] 1b.3 RED: `usage_data.rs` tests on a fixture DB: rows with `SUM`/`COUNT(wall_ms)`, distinct task keys (`c<id>`, `r<run>`), missing column -> NULLs. Result: usage_data/tests.rs, 4 tests on an in-memory store; seen failing at runtime, then green.
- [x] 1b.4 GREEN: create `src/runtime/chat_store/usage_data.rs` (rows + tasks, read-only); `StoreReply::UsageReport{data: Option<Box<UsageData>>}` in `store.rs`; dispatch in `worker.rs`. Result: chat_store/usage_data.rs (usage_data, wall_sql); usage_report adds chat output and worker wall columns; StoreReply::UsageReport{data}; worker dispatch.
- [x] 1b.5 GREEN: `app/cost.rs` and `app/history.rs` consume the new reply and still build the existing text report; /cost output unchanged. Result: app/cost.rs and history.rs read `data.rows`; /cost text unchanged, all existing tests untouched.
- [x] 1b.6 Close: protocol checks. No snapshot changes expected; if one changes, stop and review it. Result: cargo test ok (11925 passed), clippy -D warnings ok, fmt --check ok; no snapshot changed; 311 production lines. No CHANGELOG entry: nothing user-visible changes.

## S1c: renderer, range selector, tiers (~160)

- [x] 1c.1 RED: tests: tier boundaries (80, 79, 60, 59, 40, 39); keys `1`-`4`, left/right/`h`/`l` rebuild the model with no store command (C1); text-tier key sets the note (C7); active range bracketed without colour. Result: dashboard/tests.rs (tier boundaries, range cycle, height) and cost_view/tests.rs (keys 1-4, arrows, h/l rebuild with no command; text tier note; text view inert); app test asserts no effects. Seen failing at runtime (todo! stubs, missing key handling).
- [x] 1c.2 RED: insta `TestBackend` snapshots at 100/72/50/36 columns plus empty state; no-panic loop for widths 0..=120 x heights 0..=40. Result: cost_tests.rs: insta snapshots at 100/72/50 plus empty and 36 (text); bracket test; scroll test; no-panic loop over widths 0..=120 x heights 0..=40 with data and empty. Red before the renderer.
- [x] 1c.3 GREEN: create `src/ui/components/chat/cost/{layout,cards}.rs` (offscreen `Buffer` of `Dashboard::height(width)` rows, scroll window, 4-row cards, `Sparkline`, delta line, `alpha` title). Result: cost/{layout,cards}.rs; the model has `Tier`, `CARD_ROWS`, `TEXT_BELOW` and `Dashboard::height(&self, width)` (a method: the empty state is shorter). `alpha` title stays the existing header row.
- [x] 1c.4 GREEN: `cost_view.rs` (`Body` enum, `with_data`, range keys, last draw width), `ui/components/chat/cost.rs` wiring; hint `Esc close · 1-4 range · ↑↓ scroll`. Result: cost_view.rs `Body` enum, `with_data`, `set_data`, `dashboard`, `set_width`, `WIDEN`, range keys; cost.rs wiring; app/cost.rs builds the model on data arrival. Hint `Esc close · 1-4 range · ↑↓ scroll` once data arrived.
- [x] 1c.5 Review each of the 8 existing /cost snapshots on its own. Those at 40 columns or more change to the dashboard; tests about text wrapping move to 36 columns. List which changed in the PR. Result: 3 snapshots changed (the data-bearing ones moved to 36 columns), 5 unchanged; listed in the PR.
- [x] 1c.6 CHANGELOG `[Unreleased]`. READMEs are untouched until S6. Result: CHANGELOG `[Unreleased]` entry; READMEs (both languages, same position) updated in this slice on the orchestrator's instruction because the change is user-visible. S6 still owns the rest of the README rewrite.
- [x] 1c.8 Review fixes (#829): deltas never clipped (padding, then cap `▲ 999%+`); range keys with no model say `no usage data to change the range of`; width 0 = unknown; keys line advertises `1-4` only from 40 columns; scroll-clamp and `~`/`M` tests. In the wide tiers (>=40) the text-report lines (today, last 7 days, per engine/model, this chat) are drawn under the cards so no information is lost until S4 (Models) and S5 (Agents) replace them. **S6 removes these lines** together with `chart_lines`. Result: done.
- [x] 1c.7 Close: protocol checks. Result: cargo test, clippy -D warnings, fmt --check ok; see PR.

## S2: tokens by day (~200)

- [x] 2.1 RED: model tests: days 400/100/0 in, 100/200/0 out; the 1-vs-100000 day keeps one cell; empty day is not zero; 90d/All bucket size in the legend.
- [x] 2.2 GREEN: `dashboard/daily.rs`, `cost/daily.rs` (mirrored cells, `▁…█` up, `▀/█` down, legend `in`/`out`); wire into the layout. Result: `Dashboard.daily`, `daily.rs` (`DayTokens`, `fit`, `level`, `peak`, `legend`), `cost/daily.rs`; 3 up rows in eighths, 3 down rows in halves, one scale for both; the legend degrades by width and always says the bar size.
- [x] 2.3 RED/GREEN: snapshots at 100/72/50 (new); existing snapshots changed by the new section are reviewed one by one. Result: wide, medium and narrow gain the chart (pane heights raised so the report stays visible); new 40-column 90d bucket snapshot.
- [x] 2.4 CHANGELOG `[Unreleased]`. Close: protocol checks.

## S3: heatmap (~240)

- [x] 3.1 RED: tests: Tuesday 14:00 peak is the darkest glyph; empty cells blank, non-empty never blank; 59 columns -> 8 columns per weekday; Monday first via `(w+6)%7`; level `ceil(4v/max)`.
- [x] 3.2 GREEN: hours query in `usage_data.rs` (placement by `ts`, C4); `dashboard/heat.rs`; `cost/heat.rs`. Result: `UsageData.hours` (`HourCell`, grouped by day, weekday and hour of `ts`, Monday first in SQL), `Dashboard.heat` (`Heat`, `buckets`, `shade`, `heat_columns`), `HEAT_ROWS` = 10 (gap, legend, axis, 7 weekdays); cells 2 columns wide for 24 hours, 3 for 8 buckets under 60; `·` for a known zero; legend says `no hourly usage in this range` when empty.
- [x] 3.3 RED/GREEN: snapshots at 100 and 50 columns. Close: protocol checks. CHANGELOG `[Unreleased]`.

## S4: models table + provider bar (~300)

- [x] 4.1 RED: tests: order by total tokens (ollama first, no `$`; claude `$0.5000 list-price equivalent`); `unknown`/`mixed` labels; largest remainder 33.4/33.3/33.3 -> 34/33/33; each known provider gets at least one cell; single engine = 100%.
- [x] 4.2 RED: theme tests: `neutral` (0xCDD6F4) outside `palette()`; `palette_lists_eight_entries` unchanged; local glyph `⬡` uses `ai_purple`.
- [x] 4.3 GREEN: `dashboard/tables.rs`; `cost/tables.rs` (`Table`, glyph-repeated bar, legend, drops turns and `$` under 60 columns); `ui/theme.rs` (`neutral`, `provider_color`). Result: `Dashboard.models`/`providers`, `Billed` parameter (`UsageConfig::billed`) kept in `CostView`, columns by width tier (60: `$` with `*` note, 80: turns and full label), `+N more models` over 8, `apportion` (largest remainder, at least one cell), legend packed by width.
- [x] 4.4 RED/GREEN: snapshots at 100/72/50. Close: protocol checks. CHANGELOG `[Unreleased]`. Result: 3 new snapshots (100/72/50), wide/medium/narrow dashboard gain the block (pane heights 43/48/58); both READMEs updated.

## S5: agents table + peak (~240)

- [ ] 5.1 RED: tests: sweep peak (ends before starts on ties; worker spans only, C6); Agents columns `input`/`output`/`time` with no `$`; old rows -> `n/a`; mixed -> `~`; no workers -> `no agent runs in this range`; Agent time card `up to N at once`.
- [ ] 5.2 GREEN: spans query in `usage_data.rs`; `dashboard/agents.rs` (sweep, `AgentRow`); Agents table in `cost/tables.rs`.
- [ ] 5.3 RED/GREEN: snapshots. Close: protocol checks. CHANGELOG `[Unreleased]`.

## S6: formatter everywhere, drop charts, READMEs (~150)

- [ ] 6.1 RED: tests: statusline `today: 1.2k tok` (950 -> `950 tok`, 1_234_567 -> `1.2M tok`), with the `mem: ready` ordering and per-token dollars; usage line `in 1.2M · out 123.5k`; limit notes `turn used 123,456 tokens (limit 100,000)` and `today's usage reached 5,200 tokens (limit 5,000)`; the fallback has no `per day` or `per engine` line (R2).
- [ ] 6.2 GREEN: `usage_view.rs` (`today_segment`, `usage_line`, `pair_lines` compact; remove `chart_lines`); `usage.rs` notes use `grouped`.
- [ ] 6.3 Docs: `README.md` and `README.es.md` in the same PR, same sections in the same order (usage kept, 90 days, `n/a` deltas, timing columns, `/cost` ranges, tiers, text fallback under 40 columns; phrases the code matches stay in English).
- [ ] 6.4 Snapshots: statusline snapshots change to compact format; of the 8 /cost snapshots, the ones that held `per day`/`per engine` lines change. Review each; list them in the PR.
- [ ] 6.5 CHANGELOG `[Unreleased]`. Close: protocol checks.
