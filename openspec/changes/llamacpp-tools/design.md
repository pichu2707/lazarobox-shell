# Design: Native tools for llama.cpp

Inputs: `proposal.md`, `exploration.md`, and the code on `main` (v0.27.0).

## Technical Approach

Approach A of the exploration. The Ollama tool loop in `src/runtime/chat/ollama.rs` gets a two-value wire profile. Caps, approvals, `exchange`, the relay and timeouts stay shared. The OpenAI wire needs two new pure pieces:

- a streaming tool-call accumulator that plugs into `SseParser` (`src/core/ai/chat/stream/openai.rs`);
- `openai_round_body` (`adapters.rs`).

Everything stays dormant in production until `"llamacpp"` joins `MCP_VERIFIED`. `App::tool_engines` is `MCP_VERIFIED` outside tests (`app.rs:829`), so `Tools::On` never reaches llamacpp before PR 3.

## Architecture Decisions

| # | Topic | Options / tradeoff | Decision |
|---|---|---|---|
| D1 | Wire profile | Trait object (open, vtable, more code), enum (closed, exhaustive), or a copied loop (drift). | **`enum Profile { Ollama, OpenAi }`** with `path()`, `body()`, `rejected(&str)` and `notice()`. `round<D: StreamDecoder + ToolCalls + Send>` is generic, and `rounds` matches on the profile to build the decoder. `tool_turn(..)` keeps its signature and delegates to `tool_turn_with(Profile::Ollama, ..)`, so the Ollama tests do not change. |
| D2 | Decoder plug-in | Copy the SSE loop as `OllamaToolDecoder` copies NDJSON (about 50 duplicated lines), or add a hook. | **A hook.** `sse_event` splits into `sse_decode(data) -> (events, Option<Value>)`. `SseParser` gains `calls: Option<Calls>`, and `server()` sets it to `None`, so the text path runs the same code and gives the same events. `OpenAiToolDecoder { parser, clean }` delegates and scans the events: `Done` sets `clean`, `Error` clears it, the same rule as Ollama. |
| D3 | Accumulator state | Key by `index`, by `id`, or by position. | **Slots in first-seen order, keyed by `index`.** `id` is the first non-empty string. `name` and `arguments` string fragments are appended. A fragment with no `index` opens a new slot when it carries `id` or `name`; otherwise it continues the last slot. |
| D4 | Bounds | | Opening a slot past `MAX_CALLS` (16) sets `overflowed` and drops its fragments; one `refused(TooMany)` is appended on release. All `id`, `name` and `arguments` bytes together are capped at `SERVER_LINE_CAP` (1 MiB, the same as one Ollama line). Past the cap the decoder emits `too_long()`, the stream ends, and no call is released. |
| D5 | Malformed fragments | | `tool_calls` that is not an array, or an entry that is not an object, is ignored. A non-string `name` or `arguments` marks the slot `broken`, so `arguments` becomes `Null` and `tool_call()` refuses only that call. Empty accumulated `arguments` become `"{}"`. Release builds `{"id","function":{"name","arguments":<string>,"index"}}` and passes it through the existing `tool_call()`: the same validation, duplicate-key check and echo caps. |
| D6 | Flush / release | Release on `finish_reason:"tool_calls"` only, or on any clean end. | **Any clean end.** This matches Ollama, which ignores `done_reason`. `SseParser` already ends on the first `finish_reason` (`tool_calls` maps to `Finish::Stop`) or on `[DONE]`. A truncated stream, an error payload or an overflow releases nothing. Text deltas interleaved with tool fragments stream as `Delta` and are kept for echo by the relay in `round()`. |
| D7 | Body | | `openai_round_body` shares `base_messages()` and `offered_tools()` with `ollama_round_body`. serde_json has no `preserve_order`, so key order is sorted and the refactor cannot reorder bytes. Assistant message: `{role:"assistant", content:<the round's text; `""` when empty, never null>, tool_calls:[{id, type:"function", function:{name, arguments: arguments.to_string()}}]}`. Tool message: `{role:"tool", tool_call_id, content}`. The id is the server's when it is new to the body, else `call_{round}_{n}` (suffixed `_k` if that is taken too), so ids are unique across the body; one value feeds both messages. No reasoning echo, no `parallel_tool_calls`, no `tool_choice`. |
| D8 | Rejected tools | | Ollama: `starts_with("HTTP 400")`, unchanged. OpenAi: the text starts with `HTTP 4xx` or `HTTP 5xx`, and its lowercase form contains `--jinja` (the recorded llama.cpp text, F3: `tools param requires --jinja flag`); a Jinja template exception or a 4xx mentioning `tools` for another reason is a real error, with no retry and no notice. In-stream errors lack the `HTTP ` prefix and never match. The rule applies only while `offer && history.is_empty()`. The exact strings are pinned by fixture F3. |
| D9 | Notice once per pane | Per-pane runtime flag (new state), or rely on the chat. | **Rely on the chat.** `ChatState` never pushes a `System` entry it already holds (`chat/mod.rs:878-882`). The OpenAi profile sends the constant `NO_TOOLS_NOTICE` before the retry, and the Ollama profile sends nothing. After `/clear` the notice may show once more. The retry is decided every turn: a server without `--jinja` costs one refused request per turn; a per-pane memory is a follow-up. |
| D10 | Routing | | `chat.rs:420`: a `(Adapter::Llamacpp, Tools::On, Some(d))` arm calls `tool_turn_with(Profile::OpenAi, ..)`. `wires_tools` returns `true` for `Ollama \| Llamacpp`. The session opener (l.793) gives llamacpp the in-process dispatch and closes the hub, never the CLI MCP hub. lmstudio and vllm are untouched. |
| D11 | Managed lease | | No change. The lease is taken before routing and held for every round and the retry, and the lease port is used for each round. The idle stop cannot cut a loop. Approval waits count against `Timeouts::SERVER.total`. |

## Data Flow

    chat.rs runner ─lease─▶ tool_turn_with(OpenAi)
        rounds: openai_round_body ─POST /v1/chat/completions─▶ llama-server
        SSE ─▶ SseParser{calls:Some} ─▶ Delta/Done ─▶ relay ─▶ tx
                         └─▶ Calls ─(clean end)─▶ ToolRequest ─▶ exchange ─▶ Dispatch
        HTTP 4xx/5xx "--jinja", no round yet ─▶ Notice + retry without tools

## File Changes

| PR | File | Action | Production lines (est.) |
|---|---|---|---|
| 1 | `src/core/ai/chat/stream.rs` | `sse_decode` split, `calls` hook, `ToolCalls` trait (+ impl for `OllamaToolDecoder`), `mod openai` | ~30 |
| 1 | `src/core/ai/chat/stream/openai.rs` | Create: `Calls`, `OpenAiToolDecoder` | ~140 |
| 1 | `src/core/ai/chat/adapters.rs` | helpers, `openai_round_body`, call-id helper | ~75 |
| 1 | `tests/fixtures/llamacpp/*` | F1-F3 (excluded from budget) | 0 |
| 2 | `src/runtime/chat/ollama.rs` | `Profile`, generic `round`, `tool_turn_with`, widened rejection, notice | ~85 |
| 2 | `src/runtime/chat.rs` | routing arm, `wires_tools`, session opener | ~12 |
| 3 | `src/core/ai/chat/engine.rs` (+ `engine/tools_tests.rs`) | `MCP_VERIFIED` += `llamacpp`, after the manual check | ~3 |
| 3 | `README.md` | replace l.362, add `### Tools with llama.cpp` next to l.639 (`--no-jinja` turns tools off) | docs |

The PR totals are about 245, about 97 and about 3 plus docs, all under 400.

## Interfaces

```rust
pub trait ToolCalls { fn take_calls(&mut self) -> Vec<ToolRequest>; }
pub struct OpenAiToolDecoder { parser: SseParser, clean: bool }
impl OpenAiToolDecoder { pub fn server_with(external: Qualified) -> Self; pub fn with_families(self, f: FamilySet) -> Self; }
pub fn openai_round_body(request: &ChatRequest, rounds: &[ToolRound], tools: bool) -> Vec<u8>;
#[derive(Clone, Copy)] pub enum Profile { Ollama, OpenAi }
pub async fn tool_turn_with(profile: Profile, request: &ChatRequest, port: u16, dispatch: Arc<dyn Dispatch>, timeouts: &Timeouts, tx: &mpsc::Sender<StreamEvent>);
```

## Testing Strategy (Strict TDD, `cargo test`)

| Layer | What | Approach |
|---|---|---|
| Unit (stream) | Fragmented call at every byte split. Two interleaved indices. Missing `id` or `index`. Text before the call. Non-string args refused. 17 indices give 16 calls plus `TooMany`. Byte cap gives an error and no calls. Truncated stream or error payload gives no calls. `qwen2.5-plain.sse` gives the same events through both decoders. F1 decodes to the expected `ToolRequest`. | `stream/tests.rs` |
| Unit (body) | String args. Synthesized ids match across assistant and tool messages. No thinking even when `round.thinking` is `Some`. Tools offered. Ollama bytes pinned before the refactor. | `adapters/tests.rs` |
| Integration | Call, then a follow-up body holding `tool_call_id`, then text. F3 status gives a retry plus one `Notice`. The same status on Ollama surfaces as an error, with no notice. An unrelated 500 is an error. A rejection after a round is an error. Caps of 8 and 24. Routing, `wires_tools`, in-process dispatch. lmstudio and vllm unchanged. | new `src/runtime/chat/llamacpp_tests.rs`, reusing the fake server pattern of `ollama_tests.rs` with SSE helpers |
| Chat | The same notice in two turns gives one entry. | `ChatState` test |
| Manual (PR 3 gate) | Qwen3.8-27B IQ4_XS and Qwen2.5-7B-Instruct find a directory and type `cd` after approval. | user |

### Fixture recording (first task of PR 1, before the decoder is final)

The user's b11935 Vulkan build is at `~/.local/bin/llama-server`. Run it in place, because RUNPATH=`$ORIGIN` loads the libraries that sit next to it.

1. Run `~/.local/bin/llama-server --list-devices` and pick the RTX 4080 SUPER (expected `Vulkan0`; the Intel iGPU is the other device).
2. `~/.local/bin/llama-server --host 127.0.0.1 --port 39300 -m <Qwen2.5-7B-Instruct-Q4_K_M.gguf> -c 16384 -ngl 99 --device Vulkan0`. The log must show a native chat format (Hermes/Qwen), not `Generic`.
3. **F1** `qwen2.5-7b-tool-call.sse`: `curl -sN http://127.0.0.1:39300/v1/chat/completions -H 'Content-Type: application/json' -d @req1.json`. `req1.json` holds `stream:true`, a user message ("Find the directory named src under /tmp/lab using a tool"), and `tools` with the real `find_dir` and `list_dir` schemas. curl without `--raw` de-chunks.
4. **F2** `qwen2.5-7b-tool-result.sse`: `req2.json` adds the assistant `tool_calls` (string args, the F1 id) and a `{role:"tool",tool_call_id,content}` message, built exactly as `openai_round_body` would. This proves llama-server accepts our body.
5. Restart with `--no-jinja`. **F3** `no-jinja-tools.http`: `curl -s -i ... -d @req1.json`. Keep the status line and the body.
6. Replace the model path with `/models/<file>.gguf`, as the b11815 fixture does.

## Migration / Rollout

No migration. PR 1 and PR 2 are dormant until PR 3. Revert PR 3 alone to switch llamacpp tools off. Users can turn tools off or run `--no-jinja`.

## Open Questions

Closed by the T1.0 recording (real b11935, Vulkan0 RTX 4080 SUPER, Qwen2.5-7B-Instruct Q4_K_M, `-c 16384 -ngl 99`; fixtures in `tests/fixtures/llamacpp/`):

- [x] F3 status and body: `HTTP/1.1 500 Internal Server Error`, body `{"error":{"code":500,"message":"tools param requires --jinja flag","type":"server_error"}}`. Ollama's `HTTP 400` match would miss it, so D8 (4xx or 5xx plus `--jinja`) is confirmed. The runtime renders it as `HTTP 500: {"error":...}` (status prefix + flattened body); `tools` is at char 42 and `jinja` at 65, well inside `ERROR_MESSAGE_CAP` (200). Pinned by `the_recorded_no_jinja_rejection_keeps_tools_and_jinja_inside_the_cap` (`runtime/loopback/tests.rs`).
- [x] F2: llama.cpp accepted assistant `content:""` and `content:null` next to `tool_calls` (both HTTP 200, finish `stop`). D7 keeps `""`.
- [x] F1 shape: the first fragment carries `index:0`, `id` (32 chars), `type:"function"` and `function.name` with a first `arguments` piece (`"{"`); the next fragments carry only `{index:0, function:{arguments:"<piece>"}}`. Every fragment has `index`; `id` only on the first. No text deltas were mixed in (the opening chunk has `role:"assistant"`, `content:null`). It ends with `finish_reason:"tool_calls"` and an empty delta, then `[DONE]`. Assembled arguments: `{"query": "src"}` (note the space after the colon). D3, D5 and D6 hold as designed.
- [ ] A non-jinja server pays one rejected request per turn. Caching that per pane is a follow-up.
- [ ] The tool schemas grow the prompt. A context-overflow 400 does not mention `tools`, so it surfaces as an error. That is acceptable, and the README recommends `-c` of 16384 or more.
