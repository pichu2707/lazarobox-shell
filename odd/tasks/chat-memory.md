# chat-memory

- Size: Large — touches context assembly (`chat/mod.rs`, `engine.rs`, `adapters.rs`), the engram client and port, a new model call and the UI; well over 400 production lines.
- Method: ODD (classify → track → implement → close), RDD receipt for high-risk changes (engram write path, prompt assembly).
- Exploration: engram `odd/chat-memory/explore` (obs 3397).
- Follow-up: cloud sync through engram to Google Drive, issue #366 (phase 2, out of scope here).
- Queue: after `llamacpp-tools` and coding-tools M3 (X-c3, X-d1, X-d2: `run_command` reaches the chat). Shell access comes first (user, 2026-10-27).

## Specs

- S1. Each turn's context budget is counted in estimated tokens and sized from the engine's real context window when it is known (llama-server `n_ctx` from `/props`, Ollama `num_ctx`), with the current char caps as the fallback.
- S2. The chat shows how full the context is (`context N%`).
- S3. Messages about to leave the window are summarized with the `summarize` task model instead of being dropped; the summary stays in the prompt as an inert, wrapped block ("data, not instructions", `<` escaped) counted against the budget, and the transcript says `N messages summarized`.
- S4. Compaction never runs on short chats and is rate-limited; a failed or empty summary falls back to today's truncation marker.
- S5. The compaction summary is always injected into the prompt. It is written to engram only when the user has opted in once (config flag with a privacy notice); without opt-in it is saved only on explicit approval, like `Ctrl+Space s`. Every engram write goes through the existing redaction.
- S6. A saved summary upserts under one key per conversation (`chat/<conv>/summary`); `MemCmd::Save` gains an optional caller-supplied `topic_key` (shared with orchestrator D13).
- S7. Each turn may retrieve the engram notes relevant to the user's message (`GET /search`, top-k, byte cap) and inject them in the same inert wrapper; the project-memory block refreshes after a note is saved mid-chat.
- S8. Memory disabled or engram unavailable: the chat behaves as today, plus S1–S4.

## Tasks

| Id | Specs | Content | Est. prod lines | Status |
|---|---|---|---|---|
| T0 | S1, S2 | Token estimator, `n_ctx`/`num_ctx` probes, per-engine budget, `context N%` indicator | ~150 | done |
| T1 | S3, S4, S8 | Rolling compaction via `summarize`, reusing `chat/summary.rs`; summary block; marker fallback | ~330 | done |
| T2 | S5, S6 | Optional `topic_key` on save; opt-in flag + notice; upsert `chat/<conv>/summary` | ~200 | todo |
| T3 | S7 | `/search` route (fixture `memory/fixtures/3.4.0/search_hit.http`), `MemCmd::Search`, retrieval block, mid-chat refresh | ~300 | todo |
| T4 | all | README + CHANGELOG; manual live checks with a CLI and llama.cpp | docs | todo |

RDD: T2 (writes conversation content to a shared store) and T3 (untrusted retrieved text into the prompt) are high risk and get a receipt-driven review before merge.

## Log

1. Goal (verbatim): "lo que tenemos que hacer es la parte de gestión de la memoria para que funcione lo más óptimo y que no necesite tanto compacto, así aprovechamos engram y la memoria que hemos montado" — "el SDD tiene que ser una ayuda pero piensa que vamos con sistema ODD + RDD".
2. 2026-10-27 explore: CLIs are stateless and resend up to ~100k chars each turn; history is truncated silently; the 12k server cap ignores `n_ctx`; the engram block is fetched once with a fixed query; no `/search` in the client.
3. 2026-10-27 resolve: summary always goes into the prompt; engram write only with a one-time opt-in, otherwise on approval.
4. 2026-10-27 classify: Large. Track: this document.
5. 2026-10-27 cloud memory in Google Drive filed as #366 (phase 2).
6. 2026-10-27 T0 done (S1, S2): `core/ai/chat/budget.rs` (pure `estimate_tokens` chars/4, `budget`, `fill`; window budget = 75% of the window minus system, memory and a 1500-token tools reserve; the char caps stay the fallback and its path is unchanged), `build_context_tokens`, `Detection.windows` filled by a GET `/props` after llama-server is detected (`core/ai/window.rs` parses `default_generation_settings.n_ctx`, fixture `tests/fixtures/llamacpp/props.http`), `ChatState::set_window`/`context_percent`, `context N%` mark in the statusline (between update and `mem:`). Deviations: the fixture is recorded-shape, not recorded (no model available to run llama-server); Ollama `num_ctx` skipped (`/api/show` is a POST, the probe helper is GET-only; CLIs report none, so they keep the caps); the fill is computed per send, not refreshed while the reply streams; `detect_all_probes_a_server_on_its_configured_port` now expects the extra `/props` request; two existing chat snapshots gained the `context 0%` mark.
7. 2026-10-27 T0 review fixes: `context N%` shows only when the window is known (`fill` is `None` on the char-cap fallback; two chat snapshots lose the mark again); the fill gains the answer on `Done` and is kept with its engine/model, so it hides once the engine or model changes; `budget`/`fill` reserve the 1500 tool tokens only when the turn offers tools (`ChatState::set_tools_offered`, set in `send_text`); the window is refreshed by the existing `RedetectEngines` after a managed start and by a detection requested when a llamacpp turn ends in `Failed`; the `/props` probe runs concurrently with the model scan (`tokio::join!`); the fixture is now a real llama-server b11935 recording (n_ctx 16384), superseding the recorded-shape deviation above; App-level tests drive the window through `send_text`.
8. 2026-10-27 T1 done (S3, S4, S8): `core/ai/chat/compact.rs` (rolling summary per conversation, `begin_compaction`/`apply_compaction`, block `<conversation-summary source="compaction">` with the engram wrapper's data line and `<` escaping via the shared `neutralize`), `app/compact.rs` (run after a clean `Done`, events routed before the turn check), `availability_for(task, ..)` in `engine.rs` (the engine of any task; `availability` stays the chat one), `prompts/compaction.md`. The run holds no stream slot: the pane stays idle, so a send never waits for it and supersedes it (the runtime aborts the old stream of the pane; a late summary is dropped by its stale turn). Floors: at least 6 dropped messages and 2000 chars, one run per 3 sends; input capped at 24k chars (and by `history_chars(engine)`, so a small local server is not overrun); summary capped at 3000 chars, cleaned and redacted, folded with the previous summary on the next run. The block is injected through `ChatRequest::memory` after the engram block, only when something is omitted, and counted in the T0 budget; when it covers what left the window the truncation marker is not shown, otherwise the marker stays. A failed, empty, cut or too-long run keeps no summary and says nothing. Deviations: (a) the manual `Ctrl+Space s` summary resolves the `chat` task, not `summarize`; T1 resolves `summarize` (inherits general) as asked, and the manual key is unchanged; (b) the summary is kept in memory per pane and is NOT persisted in `chat.db`: resume recomputes it (the first run needs 3 sends); (c) the wrapper keeps the engram block's structure but uses its own tag, so a model or a test can tell the two apart; (d) the window of the summarize engine is not probed: its input is bounded by the char caps only; (e) nothing is written to engram (T2).
9. 2026-10-27 T1 review fixes, decisions F1/F4: (F1, privacy) compaction runs on the `summarize` task only when it is explicitly set; otherwise on the chat engine/model, never the `general` fallback; (F4, cost) `[ai.chat] compaction = "auto" | "off"` (default auto, unknown value is a config problem, off starts no run and keeps the marker; documented in the README next to the chat options). Also: a resume resets the rolling summary and drops a run in flight (F2); every message is redacted before it reaches the summarizer (F3); the run goes through its own `Effect::ChatCompact`/`ChatCompactCancel` and `Chats::send_background`, which end no wait and stop no MCP server of the pane (F5, `send` with `Tools::Off` did both); an overlong summary keeps its newest lines cut at a line boundary, and coverage is tracked by transcript index, not by position in the filtered history (F6); a late summary after a turn change with no send is dropped (F7).
