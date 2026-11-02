# Tasks: Model Context Window

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | ~250 production (range 220-300); tests, fixtures, docs excluded |
| 400-line budget risk | Low |
| Chained PRs recommended | No |
| Suggested split | Single PR, 7 work-unit commits |
| Delivery strategy | ask-on-risk |
| Chain strategy | pending (not needed) |

Decision needed before apply: Yes (owner question below only; no PR-size decision)
Chained PRs recommended: No
Chain strategy: pending
400-line budget risk: Low

**Owner question (answered: NO, the cap wins; design default kept):** should a Modelfile `PARAMETER num_ctx` win over the cap? Design default: NO (about +10 lines if yes). Apply MUST NOT start until the owner has seen this.

### Suggested Work Units

| Unit | Goal | Commit |
|------|------|--------|
| A | Config keys | `feat(config): add ollama num_ctx_cap and keep_alive_minutes` |
| B | Window in RAM | `feat(detect): read ollama context_length from api/show` |
| C | Request options | `feat(chat): send num_ctx and keep_alive to ollama` |
| D | Budget | `fix(chat): count system prompt and tools in context budget` |
| E | Notices | `feat(chat): warn on over-window and unknown window` |
| F | Docs | `docs: document ollama context window and keep_alive` |

Each commit includes its own tests. No attribution lines.

## Phase 0: Anchors

- [x] 0.1 Verify every file:line anchor in design.md (runtime.rs:500-514, app/chat.rs:541-551, runtime/chat.rs:391, core/ai/mod.rs:279, ai_detect.rs:112/136-182/~546, ai/config.rs:1193, budget.rs:15, engine.rs:567-587, adapters/tests.rs:133, app/tool.rs:250) with `git show origin/main:<path>`; record drift in this file before RED.

## Phase 1: Config (Unit A)

- [x] 1.1 RED: tests in `src/core/ai/config.rs` for defaults 16384/30, valid values, bad values with exact problem texts, `0` allowed for keep_alive only.
- [x] 1.2 GREEN: `read_ollama_options` called from `read_servers`; pass values to `LiveServers` next to `set_idle`. Menu writes neither key.

## Phase 2: Window into RAM (Unit B)

- [x] 2.1 RED: `window::ollama_context_length` parser tests in `src/core/ai/window.rs` (arch key, missing key, 0, above MAX_WINDOW, non-number).
- [x] 2.2 GREEN: implement parser; add `ModelFacts.window` and `Detection::model_window` (matches `:latest`).
- [x] 2.3 RED: `show_phase` test in `src/runtime/ai_detect.rs`: one reply fills tools and window, digest cache hit, no extra call, failed probe is unknown.
- [x] 2.4 GREEN: `ShowCache` stores `(Tools, Option<usize>)`; remove stale `windows()` comment.
- [x] 2.5 RED+GREEN: pure `window::ollama_num_ctx(max, cap)` = `min(max, cap)`, `None` when max unknown.

## Phase 3: Request body (Unit C)

- [x] 3.1 RED: update `adapters/tests.rs:133` to 5 keys when set, 3 by default, none on llama.cpp; `keep_alive_minutes = 0` gives `"0m"`.
- [x] 3.2 GREEN: `ChatRequest.ollama: OllamaWire` (Default none); `ollama_round_body` adds keys only if `Some`; fix ~27 test literals.
- [x] 3.3 RED+GREEN: runner stamping in `runtime/chat.rs:391`; stable across chat, compaction, summary, workers.

## Phase 4: Budget (Unit D)

- [x] 4.1 RED: budget tests in `budget.rs`: known window uses `min(max, cap)` with 75% share; unknown gives `Chars(12000 - memory - system - 4*reserve)`; CLIs unchanged.
- [x] 4.2 GREEN: `budget()` takes `tools_reserve`; add `adapters::tools_reserve_tokens(families)`; app resolves Ollama window (`app/chat.rs:541-551`).
- [x] 4.3 Pinning test (no new code): `fit` in `engine.rs` trims oldest user+assistant pair, keeps newest, window starts on user.

## Phase 5: Notices (Unit E)

- [x] 5.1 RED: `OVER_WINDOW` shown once via `push_system_once` when head + newest > 100% of known window; `TRUNCATION_MARKER` on real-window trims.
- [x] 5.2 GREEN: add `OVER_WINDOW` in engine.
- [x] 5.3 RED: statusline `window unknown for <model>: 12k chars`, once per model per run, only marked told when really shown.
- [x] 5.4 GREEN: App-level HashSet; set `self.notice` (route of `app/tool.rs:250`).

## Phase 6: Docs (Unit F)

- [x] 6.1 README in neutral Spanish prose (repo convention): keys, defaults, VRAM tradeoff, `0` unloads.
- [x] 6.2 CHANGELOG entry and config docs for both keys.

## Phase 7: Verify

- [x] 7.1 `cargo test`, clippy, fmt; confirm chat.db `user_version` 3, mode 0600.
- [ ] 7.2 (deferred to the owner: not run by the apply agent) Manual: `/api/ps` `context_length` equals `num_ctx`; `prompt_eval_count` < `num_ctx - 1`; no reload after 10 min idle.

## Apply notes

Anchor drift found by 0.1 against `origin/main` (4cb10e48), all cosmetic or structural, none blocking:

- `budget()` and `fill()` took `tools: bool` with a private `tools_reserve(bool)` helper; tests live in `budget/tests.rs`. Replaced by a `tools_reserve: usize` parameter.
- `ChatState` kept `tools_offered: bool`; it is now `tools_reserve: usize` (`set_tools_reserve`).
- `show_phase` and `ShowCache` are in `runtime/ai_detect.rs` with tests in `runtime/ai_detect/show_tests.rs`; the `windows()` comment was rewritten.
- `read_servers` returns a tuple; `read_ollama_options` is called from `read_ai` instead, to avoid a 4-tuple (same problem route).
- `finish_body` is shared with the OpenAI wire, so the Ollama keys are gated on `request.engine == "ollama"`.
- `spawn_detection` takes the `LiveServers` and derives the ports from it (clippy `too_many_arguments`).
- Line numbers in design.md otherwise match by function name.
