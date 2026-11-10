# Exploration: AI cost visibility (#440)

**What**: Exploration of issue #440 (AI cost visibility, /cost view, OxideGate source). Recommendation: Approach A+ (engine-native usage first, persisted in chat.db schema v4, /cost as a user-only composer command), OxideGate as a later opt-in enrichment.

**Why**: All 5 engine streams already carry usage but decoders discard it:
- claude `result` has `total_cost_usd` + `modelUsage[model].costUSD` + `usage` (cache read/create) + `duration_api_ms`;
- codex `turn.completed.usage` (input, cached_input, output, reasoning_output; no cost);
- gemini `result.stats` (input_tokens, output_tokens, cached, duration_ms, models{});
- ollama final line `prompt_eval_count`/`eval_count`/`*_duration`;
- llama.cpp last SSE chunk `timings` (prompt_n, cache_n, predicted_n, ms) - no `usage` object in fixtures.

**Where**:
- decoders `src/core/ai/chat/{claude,codex,gemini}.rs`, `src/core/ai/chat/stream.rs` (`ndjson_decode` ~l.192, `sse_decode` ~l.219);
- `StreamEvent` in `src/core/ai/chat/engine.rs` l.196 (no Usage variant yet);
- `Done` consumed in `src/app.rs:2642`, `src/runtime/chat.rs:221`, `src/core/ai/chat/mod.rs:1047`;
- persistence `src/runtime/chat_store.rs` (SQLite chat.db, `SCHEMA_VERSION` 3, at `$XDG_DATA_HOME/lazarobox/chat.db`);
- only slash command today is `/cd` (`src/core/ai/chat/folder.rs` `parse_command`);
- no ratatui Sparkline/BarChart/Chart usage anywhere in `src/ui`.

**Learned**:
- OxideGate repo is 0.15.0 (Cargo.toml) so #440 is right; installed binary 0.13.0 is older.
- OxideGate default port 8080 collides with llamacpp catalog port 8080.
- OxideGate price table (`src/telemetry/pricing.rs`) has opus at 15/75, but the claude fixture costUSD (claude-opus-5-5, 606 1h-cache-write tokens, 2 in, 4 out = 0.004936) back-solves to ~4/20 per MTok, so OxideGate would overstate newer Opus ~3.75x; claude's own costUSD has costBasis "list" (list price, not billing - meaningless for subscription).
- `/health` is `{"status":"ok"}` only; `/version` gives `{oxidegate, contract:1, endpoints[], fields[]}`; `/stats` = array of ModelStatsRow (upstream, model, requests, tokens, cache, cost_usd, ttft, errors) with `?since=7d`; `/sessions` = `{sessions[{source,key,is_session,...}],saturated}`; session key via `X-OxideGate-Session` header (explicit) or `x-claude-code-session-id`; `/requests` recent rows incl. session, cost_estimate_usd, ttft_ms, total_ms; history persisted in `~/.config/oxidegate/telemetry.jsonl` rehydrated at start (`OXIDEGATE_HISTORY_DAYS` default 7).
- Wiring is env-only: `ANTHROPIC_BASE_URL` (no /v1), `GOOGLE_GEMINI_BASE_URL` (no /v1), `OPENAI_BASE_URL` (with /v1); codex OAuth handled via `/v1/codex/responses`; ollama via `/api/chat`.
- lazarobox CLI child env keeps `ANTHROPIC_*` (SCRUBBED list in `src/runtime/chat/cli.rs` l.151 does not remove it).
- Spec constraint: ai-engine-detection "Read-only and localhost-only" + chat-engines loopback client allow only 127.0.0.1 on catalog ports, POST only chat paths; polling OxideGate would be a new 5th loopback use needing a spec amendment (GET-only `/health`, `/version`, `/stats`, `/sessions` on configured port, no credentials).
