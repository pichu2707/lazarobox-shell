# Proposal: Model Context Window

## Intent

lazarobox does not know a local Ollama model's real context window. Ollama silently cut a 4,599-token prompt to 4,095, and lazarobox did not notice. The unknown-window budget (12k chars) also ignores the system prompt and the tool schemas, which are about 2.26k tokens with 15 tools. When a model sits idle, the next request pays a cold reload of about 1.9 s. This change is step 1 of #515: learn each local window at detection, send a stable `num_ctx`, and never let the server truncate silently.

## Scope

### In Scope
- Ollama: read `context_length` from `/api/show`, which detection already calls. Send `options.num_ctx = min(model max, num_ctx_cap)`. The cap defaults to 16384 and the owner can override it. The value is stable per model, so Ollama never reloads.
- Ollama: send `keep_alive`, `30m` unless `[ai.servers.ollama] keep_alive` (released in 1.4.0) says otherwise.
- llama.cpp: reuse the existing `/props` `n_ctx` probe.
- Windows are kept in RAM and re-read at each engine detection.
- Budget: subtract the system prompt and the tool schemas in every path, including unknown windows.
- Overflow: trim the oldest history in blocks and show a visible notice.

### Out of Scope (non-goals)
- **Any chat.db schema change.** Storing windows in a v4 `model_windows` table is a possible future change, but it needs measured evidence first (for example, the cost of a probe at detection). Its known cost is a downgrade lock: a v3 build refuses a v4 database (`StoreError::Newer`) and shows the history as unavailable.
- **Window logic for cloud CLIs** (claude, codex, gemini). Confirmed by the owner.
- Persisted summaries
- FTS recall
- Prompt and tool slimming
- Moving `told` and the workspace block
- llama-server `--cache-reuse`

## Capabilities

### New Capabilities
None

### Modified Capabilities
- `chat-engines`: the "Context window" requirement uses the real window when known and counts the system prompt and tools. Overflow trims in blocks with a notice. The Ollama body gains `options.num_ctx` and `keep_alive` (`30m` when unset).
- `ai-engine-detection`: the Ollama window comes from `/api/show`, kept in RAM and refreshed at each detection.
- `configuration`: two new keys under `[ai.servers.ollama]`.
- `chat-pane` (if needed): the overflow notice reuses or extends the truncation marker.

## Decided by design (owner may adjust)

| Item | Default |
|------|---------|
| Cap key | `[ai.servers.ollama] num_ctx_cap`, an integer, default 16384. A bad value falls back with a startup notice, the same pattern as `idle_minutes`. |
| Keep-alive key | The released `[ai.servers.ollama] keep_alive` (duration text or whole seconds). Unset, Ollama turns send `"30m"`. No second key (owner decision). |
| Menu | The menu does not write `num_ctx_cap` (same rule as `args`). |
| Trim block | The oldest user+assistant pair, so the window still starts on a user message. |

## Approach

Test-first (Strict TDD), one requirement at a time:
1. A pure parser for `context_length` from `/api/show`, next to `window::llamacpp_n_ctx`.
2. A pure resolver: `min(max, cap)`; when the max is unknown, no `num_ctx` is sent.
3. `budget.rs` subtracts the system prompt and the tools reserve in the `Chars` path too, then trims in pairs and emits the notice.
4. The adapter body adds `options` and `keep_alive`. This updates the pin at `adapters/tests.rs:133`.
5. Config parsing for the two keys.

Privacy: no new files and no new stored data. chat.db is untouched.

**VRAM tradeoff:** a larger `num_ctx` reserves more KV memory, so the 16k cap protects small GPUs. A stable value avoids reloads.

## Affected Areas

| Area | Impact |
|------|--------|
| `src/core/ai/window.rs` | Ollama parser |
| `src/runtime/ai_detect.rs` (~659-676) | Ollama window into `Detection.windows` |
| `src/core/ai/chat/adapters.rs` (~427-441) | `num_ctx`, `keep_alive` |
| `src/core/ai/chat/budget.rs`, `engine.rs:536-580` | Head-aware budget, pair trim, notice |
| config module | Two keys |

## Risks

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| VRAM out of memory at 16k on small GPUs | Med | Cap override, visible error |
| The 4-chars-per-token estimate overstates by about 12% | Low | Conservative, so it errs toward trimming |
| The effective `num_ctx` disagrees between obs 3606 and 3607 | Med | Confirm with `/api/ps` in design |
| A Modelfile edit leaves a stale window | Low | Re-read at each detection |

## Rollback Plan

A plain revert. There is no data or schema change, and old configs simply ignore the unknown keys.

## Estimate

About 220 production lines in one PR, under the 400-line budget.

## Success Criteria

- [ ] An Ollama turn's `prompt_eval_count` in `/cost` matches the estimate and never sits at `num_ctx - 1`.
- [ ] A notice test passes: a forced small window trims the oldest pairs and shows the notice.
- [ ] Prefill durations in `/cost` show no reload after a 10-minute idle under the 30-minute default.
- [ ] No new files appear, and chat.db is unchanged (still `user_version` 3, mode 0600).
