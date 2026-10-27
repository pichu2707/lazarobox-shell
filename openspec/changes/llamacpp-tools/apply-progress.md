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
