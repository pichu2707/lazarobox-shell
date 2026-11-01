# Design: AI usage and cost visibility (#440)

## Technical Approach

Approach A from the proposal, bound by `sdd/ai-cost/premises`. Each decoder reads the usage its engine already prints and emits one new `StreamEvent::Usage(TurnUsage)` **before** its final event (`emit()` in `stream.rs` stops at the first `Done`/`Error`, so a later event would be lost). The runtime folds the requests of one turn into one value. `ChatState` keeps it for the turn, and the app turns it into a store write and the UI. Rows are stored in `chat.db` table `turn_usage` by the existing store thread. Every aggregate is read by the store thread and answered through `StoreReply`, so the UI thread never queries SQLite.

## Architecture Decisions

| # | Topic | Options | Tradeoff | Decision |
|---|---|---|---|---|
| D1 | Event shape | `Done(Finish, Option<TurnUsage>)` / new `Usage` variant | Changing `Done` touches about 150 match and test sites, and usage then rides only on clean ends. A variant needs about 3 exhaustive matches (`Pipe::event`, `ChatState::apply_at`, `ollama::round` uses `other =>`), and it also works before an `Error` | **Variant `Usage(TurnUsage)`**, emitted before `Done`/`Error` |
| D2 | Token semantics | Keep each engine's semantics / normalize | Mixed semantics make sums meaningless | **Disjoint buckets**: `input` = fresh prompt tokens, plus `cache_read`, `cache_write` and `output`. `reasoning` is part of `output` (shown as informational only). For engines that count cache inside input (codex `cached_input_tokens`, gemini `cached` inside `input_tokens`), `input = input − cached` with a saturating subtraction |
| D3 | Dollars | f64 / integer | `StreamEvent` and `StoreCmd` derive `Eq`, which f64 breaks | **`cost_micros: Option<u64>`** (rounded) plus `cost_source: Option<CostSource>` (`EngineList` today, `Gateway` reserved for OxideGate) |
| D4 | Claude totals | top-level `usage` / sum of `modelUsage[*]` | `usage` leaves out sub-agents and side models, but `total_cost_usd` includes them | **Sum `modelUsage`** (fallback `usage`). Cost = `total_cost_usd`. `model` = the single key, or the request model |
| D5 | Multi-request turns | Sum in the app / fold in the runtime | The app cannot see a round that sent no usage | **Runtime fold** in `ollama.rs::round` (stores `Reply.usage`). One `Usage` goes out before the turn's final event, including on error or at the cap. `partial = true` when a round sent nothing, or when a field is `Some` on one side and `None` on the other. The app also sums whatever arrives, as a defensive measure |
| D6 | Missing data | 0 / `None` | A 0 would be a false statement | **`None` per field**. If no `Usage` arrives, the row still gets written with every field `None` ("no data") |
| D7 | Persistence | bump `user_version` to 4 / additive `ensure_usage` | **`refuse_newer` locks a v3 build out of a v4 file**, so a bump would break the rollback claim in the proposal | **No version bump**: `CREATE TABLE IF NOT EXISTS` runs on every open after the v3 step. This follows the `ensure_folder` precedent (chat-folder D8). Later changes to this table must stay additive |
| D8 | Link on delete | cascade / SET NULL | The premise requires rows to survive deletion | **`ON DELETE SET NULL`**. SQLite enforces it from the schema even in older builds. Indexed so the delete stays cheap |
| D9 | Retention | timer / on open | The store opens lazily | **Prune `ts < now − 90 d`** when the worker opens the store and after each insert (an indexed range delete) |
| D10 | History off | record anyway / follow the switch | The switch is the user's privacy decision about chat.db | **Follow `history`**: no rows are written. The answer line and status use session memory, and `/cost` shows only the current session, read from memory, and says that history is off (owner-confirmed) |
| D11 | Day boundary | Rust timezone code / SQLite `'localtime'` | No chrono crate. SQLite already uses localtime_r | **SQLite** `date(ts,'unixepoch','localtime')`. Replies carry the day key, so the app holds no timezone logic |
| D12 | Interpretation | store billing mode per row / current config | Rows hold facts and config interprets them, so fixing a wrong mode also fixes past rows | **Current config**. `per-token` shows "spent", `subscription` shows "list-price equivalent". Local engines never show dollars |
| D13 | Status data | poll / reply-driven | Polling wastes work | After each insert the worker answers `UsageRecorded { day, today }`. The status segment and the day threshold read that reply |

## Data Flow

    decoder ─Usage→ Pipe(pass) ─→ [ollama round fold] ─→ AppEvent::Chat
       ChatState.apply: merge → finish(): usage_outbox + answer line
    app.store_effects ─Store(RecordUsage)→ worker: insert, prune,
       today sum ─UsageRecorded{day,today}→ app: status cache, day note
    "/cost" ─Store(UsageReport{conversation})→ worker: GROUP BY day,
       engine, model (7 d) ─UsageReport{rows}→ core::usage::view → overlay

## Interfaces / Contracts

```rust
pub struct TurnUsage { pub input: Option<u64>, pub output: Option<u64>,
  pub cache_read: Option<u64>, pub cache_write: Option<u64>,
  pub reasoning: Option<u64>, pub cost_micros: Option<u64>,
  pub cost_source: Option<CostSource>, pub duration_ms: Option<u64>,
  pub model: Option<String> /* sanitized, ≤80 */, pub partial: bool }
```

```sql
CREATE TABLE IF NOT EXISTS turn_usage (id INTEGER PRIMARY KEY,
 ts INTEGER NOT NULL, conversation_id INTEGER REFERENCES conversations(id) ON DELETE SET NULL,
 engine TEXT NOT NULL, model TEXT, role TEXT NOT NULL DEFAULT 'chat', worker TEXT,
 input INTEGER, output INTEGER, cache_read INTEGER, cache_write INTEGER, reasoning INTEGER,
 cost_micros INTEGER, cost_source TEXT, duration_ms INTEGER,
 partial INTEGER NOT NULL DEFAULT 0 CHECK (partial IN (0,1)));
-- indices: (ts), (engine, ts), (conversation_id)
```

`role` takes `chat`, `compaction` or `summary`. `worker` is reserved for #435. The table holds no message text. Engine and model ids are identifiers, not message content.

Config (`[ai.usage]`): `answer_line = true` and `status = true`. Thresholds are `turn_tokens` (int), `day_tokens` (int) and `day_usd` (float, compared with **spend** only). An absent threshold means off. `[ai.usage.billing]` defaults `claude`, `codex` and `gemini` to `"subscription"`. A key for a local engine is reported as a config problem. The menu owns the toggles and billing. Thresholds are set in the config file (a menu entry is a follow-up).

`/cost`: in `folder::parse_command`, the token `/cost` is `Command::Cost` and `//cost` is `Escaped`. Trailing text gives a visible notice `usage: /cost`. The overlay sits in `render_view` next to the list. It shows today, 7 days (by local day), per engine/model and this conversation, and Esc closes it.

Per-answer line (`Entry::Usage`, shown, never stored or sent): `1.2k in · 4 out · cache 606w · $0.0049 list-price equivalent`, or `usage: no data`, with `(partial)` appended when it applies. Status segment: ` · today 61k tok`, plus a `$` figure only for spend. It is the first segment dropped when the line is narrow.

## File Changes and Slices (production lines; tests excluded)

| Slice | Files | Est. |
|---|---|---|
| 1a capture | `core/ai/chat/usage.rs` (new: TurnUsage, merge, micros), `engine.rs`, `claude.rs`, `codex.rs`, `gemini.rs`, `stream.rs`, `runtime/chat.rs`, `runtime/chat/ollama.rs`, `core/ai/chat/mod.rs` | ~310 |
| 1b persist | `runtime/chat_store.rs` (ensure_usage), `chat_store/worker.rs` (insert, prune, today), `core/ai/chat/store.rs` (RecordUsage, UsageRecorded), `app/history.rs`, `app.rs` (compaction role) | ~240 |
| 1c `/cost` text | `folder.rs`, `core/ai/chat/usage_view.rs` (new, pure fold), `worker.rs` (report query), `store.rs`, `ui/components/chat/cost.rs` (new), `ui/components/chat.rs`, app composer | ~260 |
| 2a display | `core/ai/config.rs` (`[ai.usage]` parse), `mod.rs` (`Entry::Usage`), chat render, `statusline.rs`, app wiring | ~280 |
| 2b menu | `core/ai/menu.rs`, `core/menu.rs`, `core/config.rs` (owned keys), README | ~200 |
| 3 thresholds | `usage.rs` (checks), app notes (once per day key) | ~150 |
| 4 charts | `ui/components/chat/cost.rs` (ratatui `Sparkline`/`BarChart`) | ~180 |

## Testing Strategy (strict TDD)

| Layer | What | How |
|---|---|---|
| Unit | Usage from each decoder; lenient parsing (bad `stats` gives `None`, never an error); emitted before Done; normalization | Existing fixtures under `tests/fixtures/*` |
| Unit | Fold/partial, micros rounding, view fold, `/cost`/`//cost` parse, thresholds | Pure functions |
| Integration | ensure_usage on v3 files, survival under a v3 build (`open_with(…,3)`), SET NULL, prune, `localtime` grouping | Temp DB in `chat_store/tests` |
| App | RecordUsage only when history is on, UsageRecorded drives the status, notes never block a send | `app_tests` |

## Migration / Rollout

Additive table, no version bump. Older builds keep working and still null the link on delete. A revert leaves an inert table behind.

## Risks

- Gemini's decoder fails closed on unknown events, but usage is read from inside `result.stats`. A parse failure there gives `None` and never an `Error`. `total_tokens` is higher than input + output (thoughts or tool tokens?), so it is not used.
- Claude's `costUSD` has `costBasis: "list"`: it is a list price, not a bill, so it gets the "list-price equivalent" label under subscription. Summing sub-agent usage through `modelUsage` needs one fixture with a sub-agent in it.
- Ollama leaves out `prompt_eval_count` when the prompt was cached, so input is `None` and never 0. With llama.cpp, a `usage` chunk that comes after `finish_reason` is lost, so `timings` are read from the finish chunk.
- Codex: the assumption that `reasoning_output_tokens` is part of `output_tokens` comes from the fixtures (30 ≥ 23). Check it.
- Usage from a cancelled turn is lost (the task is dropped), and the row says "no data".

## Open Questions

- [x] D10 (history off means no usage rows): confirmed. `/cost` shows only the current session, from memory.
- [x] `day_usd` counts actual spend only (engines in per-token mode); list-price equivalent does not count. Confirmed.
- [x] Usage lines under old answers in resumed conversations: follow-up, not in this change. Confirmed.
