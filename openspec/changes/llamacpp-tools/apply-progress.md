# Apply progress: llamacpp-tools

## Batch 1 (PR1, branch feat/llamacpp-tools-pr1)

Done: T1.7 (byte pin of the Ollama request body, test-only, no production change).

- `src/core/ai/chat/adapters/tests.rs`: four `pin_*` tests. Golden bytes for no rounds + system + memory, one round without ids/index/thinking, one round with ids/index/thinking (two calls), and tools-on = tools-off bytes plus a trailing `,"tools":[...]` (structural, so registry text changes do not break it).
- Mutation check: perturbing two golden strings made two pins fail; restored.

Remaining PR1: T1.0 (manual, blocked on user GPU), T1.1-T1.6, T1.8-T1.10.

## Batch 2 (PR1)

Done: T1.7 tightened, T1.2 (partial, see deviation), T1.8.

- T1.7: `pin_tools_on_sends_the_registry_byte_for_byte_after_the_stream_flag` replaces the structural tools check; the whole tools-on body is a golden (registry text included, so a registry text change must update it knowingly). Mutation: adding a key to the tool JSON failed it; restored.
- T1.2: characterization `the_text_sse_parser_pins_finish_reasons_tool_fragments_and_reasoning` committed first (green on old code; mutation on the `length` mapping failed it). Then `sse_event` split into `sse_decode(data) -> (events, Option<Value>)` (`sse_event` kept as `.0` wrapper), `ToolCalls` trait with `impl` for `OllamaToolDecoder`. Tests for both.
- T1.2 deviation: the `calls: Option<Calls>` hook on `SseParser` and the `mod openai` stub are NOT added yet: with no `Calls` type they would be dead code (clippy -D warnings). They land with T1.3, where they are exercised.
- T1.8: `base_messages()` and `offered_tools()` extracted from `ollama_round_body`; the four T1.7 pins unchanged and green before and after.

Remaining PR1: T1.0 (manual), T1.1, T1.2 hook, T1.3-T1.6, T1.9, T1.10.

## Batch 3 (PR1 finished)

Done: T1.0, T1.1, T1.2 (hook landed), T1.3, T1.4, T1.5, T1.6, T1.9, T1.10. PR1 is complete; PR2 and PR3 not started.

- T1.0: F1, F2, F3 and the two request files recorded by the orchestrator (real b11935, Vulkan0 RTX 4080 SUPER, Qwen2.5-7B Q4_K_M); commit `test(chat): record llama.cpp b11935 tool-call fixtures`.
- T1.1: answers recorded in design.md Open Questions. F3 is `HTTP 500` with `tools param requires --jinja flag`; rendered as `HTTP 500: {"error":...}`, `tools` at char 42 and `jinja` at 65 (cap 200). Pinned in `runtime/loopback/tests.rs`. `content:""` accepted (D7 kept). Spec unchanged.
- T1.2/T1.3: `SseParser.calls: Option<openai::Calls>`, `stream/openai.rs` (`Calls`, `OpenAiToolDecoder`). `sse_event` wrapper removed (dead). Hook output is spliced before the chunk events so a cap error wins.
- T1.4: tests only (clean-end rule was built with T1.3). The mutant "Error clears clean" survives: equivalent, because the parser stops at the first final event so no Error follows a Done. Kept as in the Ollama decoder.
- T1.5: bounds (16 slots, 1 MiB), `broken` slots (non-string id/name/arguments), empty arguments become `{}`. Mutation checked per rule.
- T1.6: F1 decodes to `find_dir {"query":"src"}`, id as recorded, index 0, at every 7th split point; F2 gives text and no call.
- T1.9: `openai_round_body`, `call_id(round, n)` (both 0-based, `call_{round}_{n}`), shared `finish_body` tail also used by `ollama_round_body` (T1.7 pins unchanged).
- T1.10: the recorded req2 equals our body (arguments compared as JSON: the server text has a space after the colon; the recording offered only `find_dir` and `list_dir`, so each of them must be in our tools and the rest is equal).

PR1 production lines vs main: +309/-26 (net +283); tests/fixtures excluded. Gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (6119 passed lib) all green.

Remaining: PR2 (T2.1-T2.8), PR3 (T3.0-T3.3).

## PR1 review fixes

Four confirmed findings of a fresh review, each RED first, then GREEN.

- Clean end: `Calls` now records a `finish_reason` other than `tool_calls` or `stop` (`length`, `content_filter`, unknown) as `cut`; `take_calls` releases nothing then, even after `[DONE]`. A bare `[DONE]`, `tool_calls` and `stop` stay clean.
- Overflow: a fragment dropped past `MAX_CALLS` sets `dropping`; its index-less continuations are dropped instead of being appended to slot 16. Any fragment that lands in a slot clears it.
- Call ids: `unique_id` keeps the server id only when new to the whole body, else synthesizes `call_{r}_{n}` (suffix `_k` on collision). Covers duplicate ids, two long ids cut to one 64-char prefix, and a server id equal to a synthesized one. Assistant `tool_calls[i].id` and its tool message share one value.
- Assistant content: `round.text` was already `""` when empty (never null); added a test with non-empty and empty text, and aligned design D7 wording.
- Extra tests that passed without code: a late `id` after the name, `choices[0]` without `delta`.

Gates: fmt, clippy -D warnings, cargo test (6124 passed lib). Ollama byte pins (T1.7) and fixture equality (T1.10) unchanged and green.
