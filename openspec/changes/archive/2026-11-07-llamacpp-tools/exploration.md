# Exploration: llamacpp-tools

Engram mirror: `sdd/llamacpp-tools/explore` (id 3389).

## Problem

The OpenAI-compatible servers (llamacpp, lmstudio, vllm) are text-only:
`src/core/ai/chat/adapters.rs:203-214` `openai_invocation` sends the tool-less Ollama body. The user
wants every chat engine, including local llama.cpp, to work with the shell and the code.

## Key findings

1. `--jinja` is on by default in current llama-server (`--no-jinja` disables it); b11935 is past
   that change. Managed spawn does not need to inject it.
2. The existing "model rejects tools" fallback (`src/runtime/chat/ollama.rs:73`, `:138`) matches only
   `HTTP 400`. llama.cpp reportedly answers HTTP 500 "tools param requires --jinja flag"
   (single third-party report; unconfirmed).
3. Streaming tool-call deltas are not documented by llama.cpp itself; secondary sources describe the
   OpenAI shape (`choices[0].delta.tool_calls` with `index`, `id`, `function.name`, then
   `function.arguments` fragments, `finish_reason: "tool_calls"`). Must be pinned by a recorded
   b11935 fixture.
4. The only llamacpp fixture (`tests/fixtures/llamacpp/qwen2.5-plain.sse`, b11815) has no tools.
   Ollama tool tests use a synthetic fake server (`src/runtime/chat/ollama_tests.rs`).

## Current state

### Request
- `adapters.rs:193-195` route lmstudio, llamacpp and vllm through `openai_invocation`.
- `adapters.rs:263-331` `ollama_round_body(request, rounds, tools)`: system + history, per round an
  assistant message with `tool_calls` (`arguments` as object, `function.index`, optional
  `thinking`), tool messages with `tool_name` and optional `tool_call_id`; `tools` from
  `registry_with(external, request.families)`.
- `ToolExchange` / `ToolRound` (`adapters.rs:227-250`) are wire-neutral.

### Decode
- `src/core/ai/chat/stream.rs`: `SseParser` (l.481-567), `sse_event` (l.210-228) reads only
  `delta.content` and `finish_reason`.
- `OllamaToolDecoder` (l.362-478), `tool_call()` (l.327): validates via `parse_call_in`, accepts
  object or string arguments, rejects duplicate keys, `MAX_CALLS` 16, releases calls only after a
  clean end. `ToolRequest` is wire-neutral.

### Loop and runtime
- `src/runtime/chat/ollama.rs`: `tool_turn` (l.89), `rounds()` (l.104), `MAX_ROUNDS` 8 /
  `CODING_ROUNDS` 24, one retry without tools on `HTTP 400` before any round (l.138), `exchange()`
  (l.180, `RESULTS_CAP` 256 KiB), `round()` (l.217) hardcodes `OllamaToolDecoder` and `/api/chat`.
- `src/runtime/chat.rs`: l.420-426 tool loop only for `(Adapter::Ollama, Tools::On, Some(dispatch))`;
  `wires_tools` (l.620-629) true only for Ollama among servers; in-process dispatch session only for
  Ollama (l.793), others go to the CLI MCP hub; managed llamacpp lease (l.386-418).

### Gating
- `src/core/ai/chat/engine.rs:81` `MCP_VERIFIED = ["claude","codex","gemini","ollama"]`;
  `tools_for()` (l.85). `App::tool_engines` defaults to it (`src/app.rs:829`); pinned in
  `src/core/ai/chat/engine/tools_tests.rs:9`. `Kind::Server` is `Local` (`src/core/ai/mod.rs:45`).
- README l.362 says none of the three servers offers tools; `### Tools with Ollama` l.639-649.

### Managed spawn
- `src/runtime/chat/managed.rs:433-447`: `--host 127.0.0.1 --port <p> -m <model> <args...>`.
- `src/core/ai/config.rs:386` `MANAGED_ARGS = ["--host","--port","-m","--model"]`.
- `models_probe()` (managed.rs:95) reads `/props` only for `model_path`.

## Ollama-specific vs reusable

| Piece | Ollama-specific | Reusable |
|---|---|---|
| Round limits, caps, approvals, `exchange`, `Dispatch`, `ToolRequest`/`ToolExchange`/`ToolRound`, registry, prompts, timeouts, relay | no | yes |
| Body tool_calls shape (object args, `function.index`, `tool_name`) | yes | OpenAI variant needed |
| NDJSON decode with complete `message.tool_calls` | yes | SSE accumulator needed |
| Path `/api/chat`, decoder hardcoded in `round()` | yes | parameterize |
| Rejected-tools detection (`HTTP 400`) | partly | widen |
| Thinking echo | yes | drop for OpenAI in v1 |

OpenAI wire: `arguments` is a JSON string; tool message `{role:"tool", tool_call_id, content}` with
mandatory `tool_call_id` (synthesize `call_{round}_{n}` when the server sends none); streamed
fragments keyed by `index`; `finish_reason:"tool_calls"` then `[DONE]`.

## Delta

1. Decoder: `OpenAiToolDecoder` over `SseParser`, accumulating per `index`, flushing on
   `finish_reason == "tool_calls"` or `[DONE]`, releasing only after a clean end; reuse `tool_call()`.
2. Body: `openai_round_body`, sharing system/history assembly with `ollama_round_body`. No
   `parallel_tool_calls` in v1.
3. Runtime: parameterize `tool_turn`/`rounds`/`round` by a wire profile (path, body builder, decoder
   factory); Ollama profile byte-identical; route llamacpp; `wires_tools` and in-process dispatch
   for llamacpp; widen the rejected-tools fallback.
4. Managed args: no change (jinja default on).
5. Gating: add `"llamacpp"` to `MCP_VERIFIED` last, after the user's manual real-model check.
6. Docs: "Tools with llama.cpp" section; replace README l.362.

## llama.cpp specifics

- Native tool handlers: Llama 3.x, Functionary, Hermes 2/3, Qwen 2.5 / 2.5 Coder, Mistral Nemo,
  FireFunction v2, Command R7B, GPT-OSS, DeepSeek R1 (WIP); others use the less efficient Generic
  format (log: `Chat format: Generic`).
- `/props` exposes `chat_template`, `chat_template_tool_use`, `chat_template_caps` (fields
  undocumented).
- `parallel_tool_calls` off by default. Heavy KV quantization (`-ctk q4_0`) degrades tool calling.
- Exact no-jinja error status/body: verify empirically on b11935 with `--no-jinja`.

## Unmanaged servers without tool support

- (a) Reactive: retry without tools on a 4xx/5xx mentioning `tools` or `jinja`, before any round,
  plus a notice. Cheapest; matches Ollama.
- (b) Proactive `/props` check. More code, unstable signal.
- A Generic-template model accepts tools but may misuse them; undetectable up front.

## Reasoning

llama-server can return `reasoning_content`; not read today. Do not echo reasoning in tool rounds in
v1; reading it into `ThinkingDelta` is a separate follow-up.

## Approaches

| | A. One loop, wire profile (recommended) | B. Separate OpenAI loop | C. `/api/chat` shim |
|---|---|---|---|
| Idea | Parameterize the Ollama loop by path, body builder and decoder | Copy `ollama.rs` to `openai.rs` | Not viable |
| Pros | One place for caps, approvals, relay, timeouts; lmstudio/vllm nearly free | No risk to the Ollama path | n/a |
| Cons | Touches a verified path (Ollama tests must stay byte-identical) | Duplicated policy that drifts | n/a |

A small enum of profiles is likely enough (design decision).

## Slices (production lines, tests excluded)

| Slice | Content | Est. |
|---|---|---|
| S1 | `OpenAiToolDecoder` + `finish_reason:"tool_calls"` | ~150-200 |
| S2 | `openai_round_body` + shared helper | ~90-120 |
| S3 | Loop parameterization, routing, widened fallback + notice | ~100-140 |
| S4 | `MCP_VERIFIED`, `tools_tests.rs`, README | ~30 + docs |

S1+S2 in one PR, S3 its own PR (touches Ollama), S4 last after the manual check.

## Open product questions

1. v1 engines: llamacpp only, or also the lmstudio/vllm routing arm (not verified)?
2. Server without tool support: silent text fallback or a notice (once per pane)?
3. Explicit `--jinja` for older builds, or skip and document `--no-jinja`?
4. Round cap for small local models: same 8/24 or lower?
5. Verification: record a real b11935 tool-call fixture (which model) and do the manual check?
6. `reasoning_content` in this change or a follow-up?
7. Context size: recommend a minimum `-c`, or warn?

## Risks

- Adding `"llamacpp"` to `MCP_VERIFIED` enables tools for every llamacpp user with tools on: keep last.
- S3 touches the verified Ollama path: existing tests and request bodies must stay byte-identical.
- Unconfirmed wire details (fragment shape, `id`/`index` presence, no-jinja error) until a fixture.
- Small/quantized models: malformed arguments, hallucinated names, loops (contained by existing
  refusals and the round cap).
- Generic template fallback; context length and quantized KV cache.
- Prompt injection via tool output: same exposure as Ollama, from more engines.
