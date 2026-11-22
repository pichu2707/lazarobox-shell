# Design: usage-dashboard (/cost overlay redesign)

## Technical Approach

This follows exploration approach A. The store thread reads the usage data once, when `/cost` opens: the existing `usage_report` rows plus three new queries. It replies with a `UsageData` value.

A pure `Dashboard` model is built in core from `UsageData` and the selected range. Changing the range rebuilds the model from the data the view already holds, with no new store round-trip. The UI draws the model's sections into an offscreen `Buffer` and copies the scroll window into the frame.

Under 40 columns the view shows the existing `report_lines` text. Formatting (`usage_fmt`), provider classification (`provider`) and aggregation (`dashboard/*`) are pure core code. Colours exist only in `ui/`.

Code constraint: the MSRV is 1.88, so match arms cannot use `if let` guards. Use let-chains in `if` or nested matches instead.

## Architecture Decisions

| # | Decision | Rejected | Rationale |
|---|---|---|---|
| D1 | Fetch everything once (≤90 days of folded rows, task keys, hour cells and spans) and slice ranges in core | One query per range key | Retention caps the data at 90 days, so it is small. Range keys update instantly and need no async state. The model stays pure and unit-testable |
| D2 | Add `wall_ms: Option<u64>` to `TurnUsage`. It is measured by lazarobox, not reported by the engine (documented as an exception), and `merge` keeps the max | New fields on `Pending`, `StoreCmd::RecordUsage` and `OrchestraEvent::Usage` | It already travels inside `Box<TurnUsage>` through both paths: one field instead of three variant changes |
| D3 | `started_at` (unix s) = `ts - ceil(wall_ms/1000)`, computed in `record_usage`, where `ts` is set | Read a wall clock in core | Core takes no clocks (it uses explicit `Instant`s). The interval `[started_at, ts)` has the same store lag as `ts` |
| D4 | Old rows get no approximation: `ts - duration_ms` is not used | Use `duration_ms` as wall time | `duration_ms` is API time and codex reports none, so an estimate would mislead. Without data the value shows `n/a`; with some rows missing it is marked as a lower bound |
| D5 | The provider glyph lives in core (`Provider::glyph()`) and its colour in UI (`provider_color`) | Both in UI | A glyph is meaning, and the width test sits in pure code. A colour is presentation |
| D6 | Each segment of the stacked bar is its provider glyph repeated, drawn in that provider's colour | Coloured `█` segments | The bar stays readable without colour |
| D7 | New queries go in `src/runtime/chat_store/usage_data.rs` | Growing `worker.rs` | `worker.rs` is already over 1,300 lines |
| D8 | The core model is the `dashboard/` module; UI sections are `ui/components/chat/cost/*.rs` | One file each | Each slice adds files, so diffs stay small and reviewable |
| D9 | The offscreen buffer is `Dashboard::height(width)` rows tall and the view scrolls over it | Clipping sections to the area | This keeps the current `j/k/PgUp/PgDn` scroll and `set_layout` contract |

### Glyph set

Verified data:
- `emoji-data.txt` and `emoji-variation-sequences.txt` (termwiz 0.23.3 copy in the cargo registry). None of the chosen code points is `Emoji` or `Extended_Pictographic`, and none has a VS16 sequence.
- East Asian Width follows UAX #11 (EastAsianWidth.txt). The ambiguous (A) entries in these blocks are:
  - Dingbats: 273D and 2776..277F.
  - Geometric Shapes: 25C6..25C8, 25CB and 25CE..25D1, among others.
  - Misc Symbols and Arrows: 2B56..2B59.

No EastAsianWidth.txt file is on disk, so the test below is the executable gate: `width_cjk` counts A as 2 columns.

| Provider | Glyph | EAW | Colour token |
|---|---|---|---|
| Anthropic | ✻ U+273B | N | `warning_orange` |
| Google | ✦ U+2726 | N | `info_blue` |
| OpenAI | **❂ U+2742** | N | new `neutral` (0xCDD6F4), **not** in `palette()` |
| Local | ⬡ U+2B21 | N | `ai_purple` |
| Other | ⬦ U+2B26 | N | `text_muted` |

Why ❂: it is in the same Dingbats block as ✻ (same font fallback), it reads as a rosette like the OpenAI mark, and it is neither an emoji nor ambiguous. Rejected candidates:
- ◎ U+25CE: A.
- ◉ U+25C9: next to the A ranges, so too risky.
- ⊛: reads as a math operator.
- ✺: too close to ✻.
- ⬢: too close to ⬡.

Keeping `neutral` out of `palette()` leaves the settings preview and the `palette_lists_eight_entries` test unchanged.

The test lives in `provider.rs`:

```rust
#[test]
fn every_provider_glyph_is_one_column_everywhere() {
    for p in Provider::ALL {
        let g = p.glyph();
        assert_eq!(g.width(), Some(1), "{p:?}");
        assert_eq!(g.width_cjk(), Some(1), "{p:?} is East Asian Ambiguous"); // A -> 2
        assert_ne!(g, '\u{FE0F}');
        assert_eq!(format!("{g}").chars().count(), 1); // no VS16 sequence
    }
}
```

Block elements (`▁…█`, `░▒▓`) are A, but the project already uses them (`LEVELS`, ratatui `Sparkline`). This is an accepted, pre-existing risk. Provider identity never relies on them.

### Formatter (`src/core/ai/chat/usage_fmt.rs`)

- `compact(n)`:
  - `n < 1000` is written as is (`"999"`).
  - Otherwise, unit `u` ∈ {k=1e3, M=1e6, B=1e9, T=1e12}, computed in u128 as `tenths = (n*10 + u/2) / u` (round half up on the tenth). If `tenths >= 10_000`, the next unit is used. The output always has one decimal and a `.` separator.
  - Pinned values: 999→`999`, 1_000→`1.0k`, 1_049→`1.0k`, 1_050→`1.1k`, 999_949→`999.9k`, 999_950→`1.0M`, 123_456→`123.5k`, 11_550_000_000→`11.6B`, u64::MAX→`18446744.1T`.
- `tok(n)` = `"{compact} tok"`.
- `grouped(n)`: a comma every three digits (0→`0`, 1110→`1,110`, u64::MAX fully grouped).
- `duration(ms)`: under 60 s `38s`, under 1 h `12m`, from 1 h `3h 07m`, from 100 h `128h`.
- `pct(n, d)`: rounded half up; `d == 0` gives `None`.

### Provider classification (`src/core/ai/chat/provider.rs`)

`classify(engine, model) -> Provider` decides by engine first, then by model:

| Engine | Provider |
|---|---|
| `claude` | Anthropic |
| `gemini` | Google |
| `codex` | OpenAI |
| `ollama`, `llamacpp`, `lmstudio`, `vllm` | Local, whatever the model |
| Any other engine | Decided by model prefix |

Model prefixes: `claude` → Anthropic; `gemini` → Google; `gpt-`, `o1`, `o3`, `o4` or `codex` → OpenAI; anything else → Other. The labels shown in the UI are `unknown` for a NULL model and `mixed` for `mixed`.

## Data Flow

```
finish()/Held::send ─ wall_ms in TurnUsage ─> RecordUsage ─> record_usage: ts, started_at, wall_ms
/cost ─> StoreCmd::UsageReport ─> usage_data.rs ─> StoreReply::UsageReport{data: Option<Box<UsageData>>}
  app/cost.rs: CostView::with_data(data, billed_engines, fallback_lines)
  key 1-4/←→ ─> CostView rebuilds Dashboard::build(&data, range, &billed)   (pure)
  render_cost: width<40 → text lines | else sections → offscreen Buffer → scroll window
```

**Wall start instants.** For a chat turn, `ChatState.started` (set at send) is still `Some` inside `finish()` at the point where `Pending` is pushed, so `wall_ms = now - started`.

Workers have **no** start instant on the usage path. `Slot.started` exists in core `Run` but `Held` cannot reach it. S1a adds `born: tokio::time::Instant` in `Held::new`, and `send` stamps `wall_ms`. A retry is a new `Held`, so it becomes its own row. Without this, worker rows store NULL and count as `n/a`. Compaction and summary rows stay NULL and are not counted as agent time.

## Queries (`usage_data.rs`, read-only, no DDL; the columns are probed as in `prefill_sql`)

- **Day series**: the existing `usage_report` rows (day × engine × model), plus `SUM(wall_ms)` and `COUNT(wall_ms)` per row.
- **Tasks**: `SELECT DISTINCT day, key` where `key` is `'c'||conversation_id` (non-null) or, for workers, `'r'||substr(worker,1,instr(worker,'.')-1)`. A range counts the distinct keys whose day falls in it.
- **Hours**: `day`, `strftime('%w',ts,'unixepoch','localtime')`, `strftime('%H',…)` and `SUM(known input+output)`. The weekday row is `(w+6)%7`, so Monday comes first.
- **Spans**: `day`, `started_at`, `ts`, `engine` for chat and worker rows where `started_at IS NOT NULL`.
- **Concurrency**: a sweep-line over `[start, end)`, sorted by time with ends before starts on ties. The result is the peak count.
- **Delta**: compares `days_ago < N` with `N <= days_ago < 2N` for N = 7 and 30. It is `n/a` for 90 days, All, a previous total of 0, or (agent time only) any previous-period row without `wall_ms`. It is shown as `▲ 12%`, `▼ 3%` or `= 0%` in the muted colour: the arrow carries the meaning.

## Dashboard model (`src/core/ai/chat/dashboard/`)

```rust
pub enum Range { Week, Month, Quarter, All }          // 7/30/90/All, default Week
pub struct UsageData { rows: Vec<UsageRow>, tasks: Vec<(u32, String)>,
                       hours: Vec<HourCell>, spans: Vec<Span> }
pub struct Dashboard { range: Range, empty: bool, kpis: [Kpi; 4], daily: Vec<DayTokens>,
                       heat: [[u64; 24]; 7], models: Vec<ModelRow>,
                       providers: Vec<Share>, agents: Vec<AgentRow>, peak: Option<u32> }
pub struct Kpi { label: &'static str, value: String, sub: String, delta: Delta, spark: Vec<u64> }
```

The text is formatted in core with `usage_fmt`, so the UI only lays it out and colours it. The KPI cards are:

| Card | Value | Sub-line |
|---|---|---|
| Input | input tokens | `cache 63%` |
| Output | output tokens | `avg 1.2k/turn` (output / records) |
| Tasks | `grouped` count | `2,674 turns` |
| Agent time | `duration` | `up to N at once` |

`bucket(values, cols)` sums adjacent days, so 90 points fit in narrow widths. It serves both the sparklines and tokens by day.

| Section | Widget |
|---|---|
| Header + range selector | custom `Line`: `[7d] 30d 90d All`, with the selected range in brackets and bold |
| KPI cards | borderless `bg_panel`, 4 rows: label + delta / value / sub / `Sparkline` |
| Tokens by day | custom mirrored cells: input grows up from the axis (`▁…█`), output grows down (`▀`/`█`, half-cell resolution) |
| Heatmap | custom 7×24 grid, or 7×8 (3-hour buckets) under 60 columns. Shades ` ░▒▓█`, level `ceil(4v/max)` |
| Models | `Table`: glyph + model, in, out, turns, `$` (billed engines only). Under 60 columns it drops turns and `$` |
| Provider bar | glyph-repeated segments, largest-remainder rounding to the bar width, with a legend `✻ Anthropic 62%` |
| Agents | `Table`: engine, in, out, time (the owner's columns) |

| Width | Layout |
|---|---|
| ≥80 | 4 cards in a row; heatmap 24 columns |
| 60–79 | 2×2 cards |
| 40–59 | cards stacked; heatmap uses 3-hour buckets |
| <40 | `report_lines` text (precomputed when the reply arrives) |

The range keys are `1` `2` `3` `4` and `←`/`→`/`h`/`l` (cycle). In the text tier a range key sets the note `widen the pane to change the range`, so a key is never ignored silently. `CostView.layout` also records the last draw width. The hint reads `Esc close · 1-4 range · ↑↓ scroll`.

**No colour**: every provider shows its glyph and label. The heatmap uses shades, the delta uses arrows, and the selected range is bracketed.

**Security**: `UsageData` holds counts, ids, timestamps and sanitized model ids. It is only displayed: it never enters a `ChatRequest`, a prompt or a log.

## File Changes by slice (production lines, tests excluded)

| Slice | Files | Est. |
|---|---|---|
| S1a | new `core/ai/chat/usage_fmt.rs` and `provider.rs`; `chat/mod.rs` (mods, `finish` wall); `usage.rs` (`wall_ms`, merge); `runtime/orchestra.rs` (`Held.born`); `runtime/chat_store.rs` (`ensure_usage` columns); `chat_store/worker.rs` (`record_usage`) | ~190 |
| S1b | new `dashboard/{mod,kpi}.rs`, `chat_store/usage_data.rs` (rows + tasks), `ui/components/chat/cost/{layout,cards}.rs`; modified `store.rs` (reply type), `worker.rs` (dispatch), `app/cost.rs`, `app/history.rs`, `cost_view.rs` (Body enum, range keys), `ui/.../cost.rs` | ~370 |
| S2 | `dashboard/daily.rs`, `cost/daily.rs` | ~200 |
| S3 | `usage_data.rs` (hours), `dashboard/heat.rs`, `cost/heat.rs` | ~240 |
| S4 | `dashboard/tables.rs` (models, shares), `cost/tables.rs`, `ui/theme.rs` (`neutral` + `provider_color`) | ~300 |
| S5 | `usage_data.rs` (spans), `dashboard/agents.rs` (sweep, agents), `cost/tables.rs` | ~240 |
| S6 | `usage_view.rs` (`today_segment`/`usage_line`/`pair_lines` compact; drop `chart_lines`), `usage.rs` notes (`grouped`), `README.md` + `README.es.md` | ~150 |

## Testing Strategy (strict TDD)

| Layer | What | How |
|---|---|---|
| Unit | `compact`/`grouped`/`duration` boundaries, `classify`, glyph width, `bucket`, largest remainder, sweep peak, delta rules, task distinctness | pure tests next to each module |
| Store | new columns are additive on an old file; `started_at` math; queries on a fixture DB; a missing column gives NULLs | `chat_store/worker/tests.rs` pattern |
| UI | insta `TestBackend` snapshots at 100/72/50/36 columns, plus the empty state. The buffer symbols *are* the monochrome view | one per tier per slice that adds a section |
| Robustness | renders at every width 0..=120 × height 0..=40 without panic | loop test in `cost_tests.rs` |

Each of the 8 existing snapshots is reviewed on its own in S1b. Tests whose intent is text wrapping move under 40 columns. Struct literals and equality asserts of `TurnUsage` gain `wall_ms` in S1a.

## Migration / Rollout

The `started_at` and `wall_ms` columns are nullable, added by `ensure_usage`, and old binaries ignore them. There is no backfill. Every slice is a PR that can be reverted.

## Open Questions

- [ ] Output card average: per record (chosen) or per chat turn (needs a chat-only output sum)?
- [ ] Should S6 drop the `chart_lines` from the <40 text fallback, as proposed here?
