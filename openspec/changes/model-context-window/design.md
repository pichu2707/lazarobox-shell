# Design: Model Context Window

Ollama windows come from the `/api/show` reply detection already fetches. They live in RAM on `Detection` and `LiveServers`, and one pure resolver gives both the budget and the `num_ctx` sent. The budget always counts the head (system prompt, tools block and schemas). Trimming keeps the existing pair-aligned window, and a new notice covers the one silent case left.

> Code refs are from v1.2.1 (`a3dedde5`), read in worktree `wt-515b`. Lines may drift by a few; function names are the anchor.

## Data flow

```
detect_all ── show_phase (same POST /api/show) ── ModelFacts.window (max)
     │                                               │
     └─ detector closure: live.set_ollama_windows ───┤ Detection → app
                                                     ▼
 app send_with: window::ollama_num_ctx(max, cap) → chat.set_window   (budget)
 runner_live:   window::ollama_num_ctx(max, cap) → request.ollama    (body)
```

Both callers use the same function with the same inputs, so the budget and the wire always agree.

## Decisions

| # | Topic | Choice | Rejected (why) |
|---|---|---|---|
| 1 | Window source | Parse `model_info["general.architecture"]`, then `model_info["<arch>.context_length"]`. Accept 1..=`MAX_WINDOW`, else `None`. The parser is `window::ollama_context_length` (window.rs, next to `llamacpp_n_ctx`:10). | A second call (forbidden). `/api/tags` `details.context_length` (only newer Ollama). Any key ending in `.context_length` (ambiguous). |
| 2 | RAM home | `ModelFacts.window: Option<usize>` (core/ai/mod.rs:279), plus `Detection::model_window(engine, model)`, which matches `:latest` like `same_model`. `ShowCache` (ai_detect.rs:112) stores `(Tools, Option<usize>)` by digest. A Modelfile edit changes the digest, so the window is read again. `show_phase`:136-182 fills both from one reply. Drop the stale comment on `windows()` (ai_detect.rs:659-661; ~546 on main). | A separate Ollama `windows` vec (would duplicate the facts). chat.db (non-goal). |
| 3 | Window to budget | app/chat.rs:541-551: for `ollama`, `ollama_num_ctx(d.model_window(..), cap)`; other engines keep `d.window(engine)`. | |
| 4 | Body | `ChatRequest.ollama: OllamaWire { num_ctx: Option<usize> }` (Default = none), engine.rs:43. `runner_live` (runtime/chat.rs:391) fills it from `LiveServers` for `engine == "ollama"` only. The keep-alive is not in it: it has one carrier, the released `ChatRequest.cache` (`CacheOptions.keep_alive`), which `runner_live` stamps on every request too. Every Ollama turn (chat, compaction, summary, orchestra workers) goes through this runner, so `num_ctx` stays stable and the model never reloads. `finish_body` adds `options.num_ctx` when it is `Some`, and `keep_alive` for every Ollama request. `finish_body`:429 is shared, but llama.cpp, LM Studio and vLLM requests never get the field, so their bytes do not change. | The app fills it per request (orchestra.rs:1486 builds worker requests in the runtime and would miss it, which causes a reload). Re-serializing the body (wasteful). |
| 5 | Unknown max | Send **no** `num_ctx`. | Send the cap: on a model trained for fewer tokens it goes past the trained context (RoPE garbage). Once detection learns a max below the cap, the value also changes and forces a reload. With nothing sent, Ollama's own default is just as stable, and the head-aware char budget (#7) stays under 4096 tokens. |
| 6 | keep_alive | One key, the released `[ai.servers.ollama] keep_alive` (#515, parsed into `CacheOptions.keep_alive`). Always sent for Ollama: as written when set, else `DEFAULT_KEEP_ALIVE = "30m"`, applied in `finish_body` so the default has one place and no second carrier. Other engines never get it. | A second, minutes-based key with its own `OllamaWire` carrier (owner decision: two keys for one value, never released). Stamping the default in `LiveServers` (the adapter would still need the unset case for tests and workers). |
| 7 | Budget | budget.rs:43: `tools: bool` becomes `tools_reserve: usize` (tokens). The reserve is `estimate_tokens(tools block) + estimate_tokens(built-in tools JSON for families)`, from the new `adapters::tools_reserve_tokens(families)`. It replaces the flat `TOOLS_RESERVE_TOKENS` (1500 is under the measured ~1.98k est for 15 tools). The window is `min(max, cap)`, i.e. the sent `num_ctx`. For an unknown window, servers get `Chars(12_000 − memory − system − 4×reserve)`. CLIs keep 100k with no change (non-goal). **Output reserve (spec leaves it to design):** with a known window, the existing `PROMPT_SHARE` = 75% (budget.rs:15) is the reserve, so the room is `75% × window − head` and 25% is left for the reply. That stays inside the spec bound `window×4 − head` chars, and today's llama.cpp behaviour is kept. With an unknown window there is no extra reserve: 12,000 chars ≈ 3k tokens, which already leaves about 25% of a 4096 default. | A separate fixed token reserve (a second knob doing the same job). Assuming a 4096-token window (it would show a fake fill %). |
| 8 | Estimator | Keep 4 chars per token. It runs about 12% high (obs 3608), so it errs toward trimming. | 4.5 c/t (undercounts code and non-English text). A tokenizer endpoint (Ollama has none). |
| 9 | Trim block | Already pair-aligned: `fit` (engine.rs:567-587) drops a prefix and then skips leading assistants. `sendable_indices` keeps the history strictly U/A. So the dropped prefix is always whole user+assistant pairs. `MAX_HISTORY_MESSAGES`=40 and `compact::context_for`:88 do not change; the summary refit (mod.rs:1324-1346) inherits the head-aware budget. Pin this with a test; no new code. | A new pair-walker (duplicates `fit`). |
| 10 | Notices | Omitted history keeps the existing `TRUNCATION_MARKER` (mod.rs:107, `place_marker`). New: `OVER_WINDOW`, pushed by `push_system_once` when the estimated head plus the newest message is larger than 100% of a known window: `"this message and the setup are about {n} tokens, over the {w}-token window: the server may cut them"`. Same lowercase style as `BUSY` and `FULL`. `OVER_WINDOW` is deduplicated by kind, not by text: it shows once per overflow streak and again only after a send that fits. **Unknown Ollama window:** on the first send to an Ollama model with no known window, `send_with` (app/chat.rs) sets the statusline notice `self.notice = Some(..)` (the route used in app/tool.rs:250 and app/summary.rs:34) to `window unknown for <model>: 12k chars`. It shows once per model per run, through an App-level `HashSet<String>`, and a model is marked told only when the notice is really set; a notice already showing is never replaced, so the next send tries again. The status line (not the transcript) keeps the spec route and adds no line to every Ollama chat. The text still holds: 12,000 is the total cap, and the setup is subtracted from it. So neither the spec nor the notice changes. | Refuse the send (blocks a turn that may still fit, since the estimate is high). Repeat the notice every turn (noise). |
| 11 | Config | ai/config.rs `read_servers`:1193: `if id == "ollama"`, call `read_ollama_options` and return `OllamaOptions { num_ctx_cap: 16384 }`. `num_ctx_cap` is a whole number from 1024 to 1048576. A bad value falls back with the problem `"ai.servers.ollama.num_ctx_cap: expected a whole number of tokens from 1024 to 1048576"`, the exact spec text. `keep_alive` is read by the released `read_keep_alive`, unchanged. This is the `read_idle_minutes`:1251 pattern, so the problem reaches the same startup notice. Read at startup and passed to `LiveServers` next to `set_idle` (runtime.rs:454). The menu does not write `num_ctx_cap`, so core/config.rs `is_integer` does not change. | |
| 12 | Verify `num_ctx` | Manual check only. After a turn, run `curl -s localhost:11434/api/ps \| jq '.models[]\|{name,context_length}'`; it should show `min(max,16384)`. `/cost` should show `prompt_eval_count` < `num_ctx−1`. | A runtime `/api/ps` call (an extra GET per turn for a fact we already send). |

`LiveServers` (runtime/chat/live.rs) gains `ollama: OllamaOptions` and `windows: HashMap<String, usize>`. The detector closure (runtime.rs:500-514) wraps `detect_all`: it calls `live.set_ollama_windows(d.ollama_windows())` before sending the Detection, so the runtime never knows less than the app.

## File changes (production)

| File | Est. lines |
|---|---|
| core/ai/window.rs: `ollama_context_length`, `ollama_num_ctx` | 22 |
| runtime/ai_detect.rs: cache value, `show_phase`, comment | 20 |
| core/ai/mod.rs (+parse.rs init): `ModelFacts.window`, `model_window`, `ollama_windows` | 22 |
| core/ai/config.rs: `OllamaOptions`, reader, accessor | 45 |
| core/ai/chat/engine.rs: `OllamaWire`, field, ~8 literal sites | 22 |
| core/ai/chat/adapters.rs: body keys, `tools_reserve_tokens` | 25 |
| core/ai/chat/budget.rs, mod.rs: reserve param, char head, `OVER_WINDOW` | 30 |
| runtime/chat/live.rs, runtime/chat.rs, runtime.rs: state + stamping + wiring | 40 |
| app/chat.rs: window resolve, reserve, unknown-window notice | 20 |
| **Total** | **~250 (< 400)** |

Tests and fixtures do not count toward the budget: there are ~27 `ChatRequest` literals in tests, at +1 line each.

## Test strategy (Strict TDD, RED first)

| Order | RED test | Layer |
|---|---|---|
| 1 | `ollama_context_length`: the recorded show fixture gives 131072. A missing arch, a missing key, 0, a string or an absurd value gives `None` | unit, window.rs |
| 2 | `ollama_num_ctx(Some(131072),16384)=16384`, `(Some(8192),16384)=8192`, `(None,_)=None` | unit |
| 3 | `show_phase` fills `window` and caches it by digest, so a second run makes no call | ai_detect fake server |
| 4 | Config: defaults, overrides, and bad values give a fallback plus a problem | config.rs |
| 5 | adapters/tests.rs:133: change it so the Ollama body has 5 keys with `options.num_ctx` and `keep_alive`; the default `OllamaWire` has 4 (`keep_alive` is always there); a llama.cpp body never has them | adapters |
| 6 | Unknown-window server: history ≤ 12k − head, never below the 4,000-character floor; known window counts the real reserve | budget, window_tests.rs |
| 7 | A forced small window trims whole pairs and shows the marker; a head over the window shows `OVER_WINDOW` once; an unknown Ollama window shows the statusline notice once per model | chat state, app + snapshot |
| 5b | An explicit `keep_alive` is sent as written (`0`, `5m`, `-1s`, `1h`); unset gives `"30m"`, also on tool rounds | adapters |
| 8 | `runner_live` stamps Ollama only, with the same value for chat and worker turns | runtime |

## Migration / Rollout

No migration is required. chat.db is untouched. Old configs leave out the keys and get the defaults; a revert restores today's behaviour.

## Open questions

- [x] **Resolved (owner: the cap wins):** a Modelfile `PARAMETER num_ctx` (shown in the `/api/show` `parameters` field) is overridden by the `options.num_ctx` we send. Should an explicit Modelfile value win over the cap? The design's default is no: the cap is the single control. Changing that would mean parsing `parameters`, at about 10 extra lines.
- [x] **Resolved (accepted for now):** external MCP tool schemas are resolved in the runtime and are not in the reserve. A follow-up issue will be opened by the coordinator.
