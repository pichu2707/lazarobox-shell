# Tasks: AI usage and cost visibility (#440)

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated production lines | 1a ~310, 1b ~240, 1c ~260, 2a ~280, 2b ~200, 3 ~150, 4 ~180 (~1620 total) |
| 400-line budget risk | Low per slice (1a Medium) |
| Chained PRs recommended | Yes |
| Suggested split | 7 PRs: 1a, 1b, 1c, 2a, 2b, 3, 4 |
| Delivery strategy | ask-on-risk |
| Chain strategy | feature-branch-chain |

Decision needed before apply: Yes
Chained PRs recommended: Yes
Chain strategy: feature-branch-chain
400-line budget risk: Medium

Decision: the split is already chosen (chained, tracker `feat/ai-cost`); confirm and apply slice 1a. Tests, fixtures and snapshots do not count.

### Suggested Work Units

| Unit | Goal | PR base |
|------|------|---------|
| 1a | Capture usage from 5 engines | `feat/ai-cost` |
| 1b | Persist in chat.db | PR 1a |
| 1c | `/cost` text view | PR 1b |
| 2a | Answer line + status segment | PR 1c |
| 2b | AI menu toggles + billing + README | PR 2a |
| 3 | Thresholds | PR 2b |
| 4 | Charts | PR 3 |

Every task is RED (failing test) then GREEN (code); refactor at slice end.

## Slice 1a: Capture (PR 1a)

No README or CHANGELOG entry: this slice adds nothing the user can see.

- [x] 1a.1 RED: `usage.rs` tests for TurnUsage merge, partial, micros rounding, None-not-0 (ai-usage: summing).
- [x] 1a.2 GREEN: create `core/ai/chat/usage.rs` (TurnUsage, CostSource, merge); add `StreamEvent::Usage` in `engine.rs`/`mod.rs`.
- [x] 1a.3 RED+GREEN `claude.rs`: modelUsage sum incl. sub-agent fixture, total_cost_usd, fallback; Usage before Done.
- [x] 1a.4 RED+GREEN `codex.rs` and `gemini.rs`: cached subtracted saturating; lenient stats (bad stats = None, no Error).
- [x] 1a.5 RED+GREEN `stream.rs` Pipe pass-through and `ChatState::apply_at` merge; Usage emitted before Error too.
- [x] 1a.6 RED+GREEN `runtime/chat/ollama.rs` round fold (ollama, llama.cpp finish-chunk timings, missing prompt_eval_count = None, cap/error).

## Slice 1b: Persist (PR 1b)

No README or CHANGELOG entry: this slice adds nothing the user can see (the privacy text lands in 2b.2).

- [x] 1b.1 RED: store tests on a v3 file: ensure_usage, opens under `open_with(..,3)`, SET NULL on delete, 90-day prune, no text.
- [x] 1b.2 GREEN: `ensure_usage` in `runtime/chat_store.rs`; worker insert, prune, today sum in `chat_store/worker.rs`.
- [x] 1b.3 RED+GREEN `core/ai/chat/store.rs` RecordUsage/UsageRecorded; `app/history.rs` writes only when history is on (no rows when off).
- [x] 1b.4 RED+GREEN compaction/summary rows carry their role (`compaction`, `summary`); a no-data turn writes no row (ai-usage: No data, no row).
- [x] 1b.5 RED+GREEN failed write shows `Usage not saved: <reason>` once per run, never blocks the turn.

## Slice 1c: /cost text (PR 1c)

- [ ] 1c.1 RED+GREEN `parse_command`: `/cost`, `//cost` escaped, trailing text gives `usage: /cost`.
- [x] 1c.2 RED+GREEN `core/ai/chat/usage_view.rs` pure fold (today, 7d, per engine/model, this chat, partial).
- [x] 1c.3 RED+GREEN worker report query (localtime day grouping) and UsageReport reply; `reading usage…` state.
- [ ] 1c.4 RED+GREEN `ui/components/chat/cost.rs` overlay, header `AI usage (alpha)`, Esc closes; history off shows session-only view from memory with the history-off line.
- [ ] 1c.5 Docs: README documents `/cost` (and `//cost`), what it shows, `no data` and the history-off line; an "alpha" note in the `/cost` section (figures may be missing or not match the provider's bill depending on engine and account; how to report it via the report skill / GitHub issues); CHANGELOG entry.

## Slice 2a: Display (PR 2a)

- [ ] 2a.1 RED+GREEN `core/ai/config.rs` parse `[ai.usage]`, `[ai.usage.billing]`; local-engine key = config problem.
- [ ] 2a.2 RED+GREEN `Entry::Usage` render (not stored, not sent); `usage: no data`, `(partial)`, billing labels.
- [ ] 2a.3 RED+GREEN `statusline.rs` ` · today N tok` from UsageRecorded, first segment dropped.
- [ ] 2a.4 Docs: README documents the per-answer usage line, the status-bar segment (on by default) and the `[ai.usage]` / `[ai.usage.billing]` keys; CHANGELOG entry.

## Slice 2b: Menu (PR 2b)

- [ ] 2b.1 RED+GREEN `core/ai/menu.rs`/`core/menu.rs` `Usage (alpha)` row and sub-view (toggles, billing); confirm AI-tab row capacity.
- [ ] 2b.2 RED+GREEN owned keys in `core/config.rs`; README privacy and usage section.
- [ ] 2b.3 Docs: CHANGELOG entry for the AI menu usage rows (README is covered by 2b.2).

## Slice 3: Thresholds (PR 3)

- [ ] 3.1 RED+GREEN checks in `usage.rs`: turn_tokens, day_tokens, day_usd (per-token engines only; list-price never counts).
- [ ] 3.2 RED+GREEN app notes once per day key per run; never block a send.
- [ ] 3.3 Docs: README documents the thresholds (`turn_tokens`, `day_tokens`, `day_usd`; spend-only; passive notes); CHANGELOG entry.

## Slice 4: Charts (PR 4)

- [ ] 4.1 RED+GREEN `Sparkline`/`BarChart` in `cost.rs` with snapshot tests; narrow-terminal fallback.
- [ ] 4.2 Docs: README documents the charts; CHANGELOG entry.

## Final manual check (real engines)

- [ ] M.1 claude: tokens and labelled `$` appear in `/cost`; sub-agent turn sums.
- [ ] M.2 codex and gemini: tokens appear; no dollars in subscription mode.
- [ ] M.3 ollama and llama.cpp: tokens only, never `$`.
- [ ] M.4 History off: no rows, `/cost` shows session only; delete a chat and usage survives.
