# Tasks: llamacpp-tools (native tools for llama.cpp)

Inputs: `proposal.md` (#3390), `specs/chat-engines/spec.md` (#3391), `design.md` (#3392).

Mode: STRICT TDD (`cargo test`). Every implementation task is RED (failing test) -> GREEN (smallest code) -> REFACTOR. One task = one work-unit commit. Estimates: "prod" lines count toward the 400-line PR budget; "test" lines (tests, fixtures, snapshots) do not. Conventional commits, no AI attribution.

## Review Workload Forecast

| Field | Value |
|---|---|
| Estimated production lines per PR | PR1 ~245, PR2 ~97, PR3 ~3 (+ README docs) |
| Estimated test/fixture lines (not counted) | PR1 ~450, PR2 ~350, PR3 ~60 |
| PR count | 3 (all stacked-to-main) |
| Chained PRs recommended | **Yes** (3 independent work units, each dormant until PR3, each revertible alone) |
| 400-line budget risk | **Low** (largest slice ~245 production lines; PR1 has ~155 lines of headroom) |
| Decision needed before apply | **No** (delivery_strategy ask-on-risk does not fire; no slice nears 400 production lines) |
| Delivery strategy | ask-on-risk |
| Chain strategy | stacked-to-main |
| Hard gates | PR1 starts with the manual fixture recording (T1.0); PR3 needs the manual check (T3.0) before `MCP_VERIFIED` changes |

### PR list and bases (stacked-to-main)

| # | PR | Est. prod | Base (target) | Status |
|---|---|---|---|---|
| 1 | Fixtures + OpenAI decoder + `openai_round_body` | ~245 | `main` | Blocked on T1.0 (manual, user GPU) |
| 2 | Ollama byte pin, wire `Profile`, routing, fallback + notice | ~97 | `main` (after PR1 merges; branch off PR1 meanwhile) | After PR1 |
| 3 | Manual check, `MCP_VERIFIED`, README | ~3 + docs | `main` (after PR2 merges) | After PR2 + T3.0 |

Rollback: revert PR3 alone (llamacpp tools off, loop dormant); then PR2, then PR1.

## PR 1: fixtures, decoder, round body (~245 prod, base `main`)

Spec: "llama.cpp native tools" (request wire, response decode); design D2-D7.

### Fixture recording (FIRST task, manual, needs the user's GPU and model)

- [x] T1.0 [manual] (recorded by the orchestrator with the design.md commands, committed as `test(chat): record llama.cpp b11935 tool-call fixtures`) Record F1, F2, F3 against the b11935 Vulkan build. No code. Output: `tests/fixtures/llamacpp/qwen2.5-7b-tool-call.sse` (F1), `qwen2.5-7b-tool-result.sse` (F2), `no-jinja-tools.http` (F3). Commit in its own `test(fixtures)` commit. Steps (from design.md):
  1. `~/.local/bin/llama-server --list-devices`; pick the RTX 4080 SUPER (expected `Vulkan0`; the Intel iGPU is the other). Run the binary in place (RUNPATH=`$ORIGIN`).
  2. `~/.local/bin/llama-server --host 127.0.0.1 --port 39300 -m <Qwen2.5-7B-Instruct-Q4_K_M.gguf> -c 16384 -ngl 99 --device Vulkan0`. The log must show a native chat format (Hermes/Qwen), not `Generic`.
  3. F1: `curl -sN http://127.0.0.1:39300/v1/chat/completions -H 'Content-Type: application/json' -d @req1.json > qwen2.5-7b-tool-call.sse`. `req1.json`: `stream:true`, one user message ("Find the directory named src under /tmp/lab using a tool"), `tools` with the real `find_dir` and `list_dir` schemas. No `--raw` (curl de-chunks).
  4. F2: `req2.json` = `req1.json` + assistant `tool_calls` (string args, the F1 id, `content:""`) + `{role:"tool",tool_call_id,content}`, shaped exactly as `openai_round_body` will build it. Same curl, save as `qwen2.5-7b-tool-result.sse`.
  5. Restart with `--no-jinja`. F3: `curl -s -i http://127.0.0.1:39300/v1/chat/completions -H 'Content-Type: application/json' -d @req1.json > no-jinja-tools.http` (keep status line and body).
  6. Replace the model path in all fixtures with `/models/<file>.gguf`, as the b11815 fixture does.
  Maps to: scenarios "Recorded b11935 fixture", "Rejected tools fall back..." (F3 pins the status), "Live server with --no-jinja".

- [x] T1.1 (answers a-e recorded in design.md Open Questions; F3 rendering pinned in `runtime/loopback/tests.rs`; spec unchanged) Fixture review and spec/design adjustment (no production code; edit design.md/spec text if needed). Confirm from F1-F3 and record the answers in the commit message:
  - a. Fragment shape: is the first fragment `index`+`id`+`function.name` with empty `arguments`, then argument pieces? Any `type` field? Text deltas mixed in?
  - b. `id`/`index` presence: does every fragment carry `index`? Is `id` always present (else the synthesized `call_{round}_{n}` path is exercised for real)?
  - c. No-jinja (F3): status (400/500/200-ignoring `tools`) and body. The keyword `tools` or `jinja` MUST fall within the first 200 characters (`ERROR_MESSAGE_CAP`) of the sanitized message, or D8 must change. If the server ignores `tools`, no notice fires (acceptable; adjust PR2 expectations).
  - d. `content:""` vs `null`: did F2 succeed with `content:""` on the assistant message with `tool_calls`? If not, switch D7 to `null` before T1.7.
  - e. Finish: does F1 end with `finish_reason:"tool_calls"` then `[DONE]`?
  Dep: T1.0. Gate: do not make the decoder final before this task.

### Stream hook and decoder (stream.rs, stream/openai.rs)

- [x] T1.2 RED (`stream/tests.rs`): the existing text SSE tests and `qwen2.5-plain.sse` produce the same events through the refactored `SseParser` (characterization, passes on old code, committed FIRST). GREEN: split `sse_event` into `sse_decode(data) -> (events, Option<Value>)`, add `calls: Option<Calls>` hook (`server()` sets `None`), add `ToolCalls` trait with `impl` for `OllamaToolDecoder`, `mod openai` stub. REFACTOR. ~30 prod. Dep: T1.1. Scenarios: "Ollama bodies and behavior byte-identical" (decode side), "Tools off sends the tool-less body".
- [x] T1.3 RED: `OpenAiToolDecoder` accumulates `delta.tool_calls` per `index` (slots in first-seen order, `id` first non-empty, `name`/`arguments` appended); two interleaved indices; fragmented arguments at every byte split; releases calls only on clean end (`finish_reason` or `[DONE]`). GREEN: `stream/openai.rs` `Calls`, `OpenAiToolDecoder { parser, clean }`, `server_with`, `with_families`. ~70 prod. Dep: T1.2. Scenarios: "Fragments accumulated per index".
- [ ] T1.4 RED: truncated stream (cut before `finish_reason`/`[DONE]`), error payload, idle timeout or cancel releases no call; `Done` sets `clean`, `Error` clears it. GREEN: clean-end rule (D6). ~15 prod. Dep: T1.3. Scenarios: "Truncated stream runs no call", "Cancel in the llamacpp loop" (decoder side).
- [ ] T1.5 RED: malformed and bounded input: missing `id` (synthesized later), missing `index` (opens a slot only with `id`/`name`, else continues last), non-array `tool_calls` and non-object entries ignored, non-string `name`/`arguments` marks the slot broken (only that call refused), empty arguments become `"{}"`, 17th slot sets overflow (16 calls + one `TooMany` refusal), 1 MiB byte cap gives `too_long()` and no calls. GREEN: D4/D5, release through the existing `tool_call()` (same duplicate-key check, unknown name refusal, arg validation). ~55 prod. Dep: T1.3. Scenarios: "Duplicate keys refused like Ollama", "Unknown tool name refused like Ollama", "Too many calls", "Missing call id is synthesized" (decode side).
- [ ] T1.6 RED: F1 decodes to the expected `ToolRequest` (name, parsed arguments, id as recorded); text before the call still streams as `Delta`. GREEN: adjust decoder to the real shape found in T1.1 (no new code expected beyond fixes). ~5 prod. Dep: T1.1, T1.3, T1.5. Scenarios: "Recorded b11935 fixture" (decode half; the loop half is PR2 T2.5).

### Round body (adapters.rs)

- [x] T1.7 RED (`adapters/tests.rs`): characterization pin of the CURRENT `ollama_round_body` bytes (first request, follow-up with `tool_name`, object arguments, `function.index`, thinking echo) captured on UNREFACTORED code and committed FIRST in its own commit, as golden bytes. This is also the pin PR2 needs. GREEN-on-old-code (no production change). Dep: none (may run in parallel with T1.2). Scenarios: "Ollama bodies and behavior byte-identical".
- [x] T1.8 RED: refactor extracts `base_messages()` and `offered_tools()`; the T1.7 pin stays byte-identical. GREEN: extraction in `adapters.rs` (serde_json has no `preserve_order`, so key order is sorted). ~30 prod. Dep: T1.7.
- [ ] T1.9 RED: `openai_round_body`: assistant message `{role:"assistant", content:<text, "" when empty (or null per T1.1.d)>, tool_calls:[{id,type:"function",function:{name,arguments:<string>}}]}`; result message `{role:"tool",tool_call_id,content}`; `tools` offered with the streaming flag; no `parallel_tool_calls`, no `tool_choice`, no `reasoning_content` even when `round.thinking` is `Some`; tools off equals the tool-less body. GREEN: `openai_round_body` plus one call-id helper `call_{round}_{n}` used by BOTH the assistant entry and the tool message. ~45 prod. Dep: T1.8, T1.1.d. Scenarios: "Tools offered on a verified llamacpp turn" (body half), "Arguments sent back as a JSON string", "Server id is echoed", "Missing call id is synthesized" (body half), "Tools off sends the tool-less body", "lmstudio and vllm stay text-only" (body unchanged).
- [ ] T1.10 RED: F2 request body built by `openai_round_body` equals the `req2.json` recorded in T1.0 (confirms the shape llama-server accepted). GREEN: none expected. ~0 prod. Dep: T1.9.

PR1 exit: `cargo test` green, nothing wired to any caller (dormant), Ollama bytes unchanged.

## PR 2: wire profile, routing, fallback and notice (~97 prod, base `main` after PR1)

Spec: "llama.cpp native tools" (loop, fallback, notice), "Ollama native tools" MODIFIED; design D1, D8-D11.

- [ ] T2.1 RED: before any refactor, pin the Ollama loop behavior: the existing `ollama_tests.rs` plus recorded bodies from T1.7 pass (run and commit as a no-op characterization if any gap, e.g. the silent HTTP 400 fallback with no notice). GREEN-on-old-code. Dep: PR1 merged. Scenarios: "Ollama keeps degrading silently", "Ollama bodies and behavior byte-identical".
- [ ] T2.2 RED (new `src/runtime/chat/llamacpp_tests.rs`, fake-server pattern of `ollama_tests.rs` with SSE helpers): `tool_turn_with(Profile::Ollama, ..)` behaves identically and `tool_turn(..)` delegates to it; `Profile::OpenAi` posts to `/v1/chat/completions`, `Profile::Ollama` to `/api/chat`; wires are not mixed. GREEN: `enum Profile { Ollama, OpenAi }` with `path()`, `body()`, `rejected(&str)`, `notice()`; generic `round<D: StreamDecoder + ToolCalls + Send>`; `rounds` matches on profile to build the decoder. ~55 prod. Dep: T2.1. Scenarios: "Wires are not mixed", "One round on llamacpp", "Tools offered on a verified llamacpp turn".
- [ ] T2.3 RED: loop behavior on OpenAi: write tool approval (`y` writes, no Enter; `n` denies), round caps 8 and 24 with a coding family, tool messages not kept in next-turn history, cancel while waiting for the follow-up's first byte (socket closed, no further request, no accumulated call runs). GREEN: only if the shared loop needs profile-specific tweaks; otherwise tests pass on T2.2. ~5 prod. Dep: T2.2. Scenarios: "Write tool in the llamacpp loop", "Round cap is 8, or 24 with a coding family", "Tool messages not kept on llamacpp", "Cancel in the llamacpp loop".
- [ ] T2.4 RED: rejected-tools fallback. OpenAi: text starts with `HTTP 4xx` or `HTTP 5xx` and lowercase contains `tools` or `jinja`, only while `offer && history.is_empty()`; F3 status/body (from T1.1.c) triggers exactly one retry without `tools` preceded by one `NO_TOOLS_NOTICE`; 400, 422 and 500 mentioning `tools`/`jinja` all retry; 500 mentioning neither is an ordinary error with no retry; rejection after a round started is an ordinary error with no retry and no notice; in-stream errors (no `HTTP ` prefix) never match; Ollama keeps `starts_with("HTTP 400")` and sends no notice. GREEN: widened `rejected()` and constant notice in `ollama.rs` (l.73, l.138 sites). ~27 prod. Dep: T2.2, T1.1.c. Scenarios: "Rejected tools fall back to text with one notice per pane", "Fallback on HTTP 400 and 5xx mentioning tools or jinja", "Unrelated errors do not trigger the fallback", "No fallback after a round started", "Ollama keeps degrading silently".
- [ ] T2.5 RED: one full round replaying F1 then F2 through a fake server: `find_dir` executes through the registry, the follow-up request holds the call and its result (`tool_call_id` equal), answer delivered with one `ToolCall`/`ToolResult` pair; Missing-id and server-id variants. GREEN: none expected. ~0 prod. Dep: T2.2, T1.6. Scenarios: "Recorded b11935 fixture" (loop half), "Missing call id is synthesized", "Server id is echoed", "Arguments sent back as a JSON string".
- [ ] T2.6 RED (`ChatState` test): the same notice pushed in two turns of one pane gives one `System` entry; another pane shows its own (relies on `chat/mod.rs:878-882` dedupe, D9; no new state). GREEN: none expected, or the smallest fix if the dedupe does not hold. ~0-5 prod. Dep: T2.4. Scenarios: "Notice shown once per pane".
- [ ] T2.7 RED (`chat.rs`): routing arm `(Adapter::Llamacpp, Tools::On, Some(d))` calls `tool_turn_with(Profile::OpenAi, ..)`; `wires_tools` is true for `Ollama | Llamacpp`; the session opener (l.793) gives llamacpp the in-process dispatch and closes the hub (never the CLI MCP hub); lmstudio and vllm get no arm and keep the tool-less body; tools off keeps llamacpp text-only; `App::tool_engines` is still `MCP_VERIFIED` so production stays dormant. GREEN: ~12 prod in `src/runtime/chat.rs`. Dep: T2.2. Scenarios: "lmstudio and vllm stay text-only", "Tools off keeps llamacpp text-only", "Not offered until verified" (llamacpp not in `MCP_VERIFIED` at this point), "One round on llamacpp".
- [ ] T2.8 Regression sweep: run the complete Ollama suite and the T1.7/T2.1 pins; no snapshot or body changes. Dep: T2.7. Scenarios: "Ollama bodies and behavior byte-identical".

PR2 exit: all green, llamacpp loop reachable only in tests (not in `MCP_VERIFIED`).

## PR 3: gating and docs (~3 prod + docs, base `main` after PR2)

Spec: "Verified list gating for llamacpp", "llama.cpp tools documented".

- [ ] T3.0 [manual] GATE: real llama-server b11935 with `--jinja` (default), once with Qwen3.8-27B IQ4_XS and once with Qwen2.5-7B-Instruct, assigned to `chat`, with a build carrying PR1+PR2 and a local-only override making llamacpp tool-enabled (do not merge the override). User asks to find `curso-rust` and go there. Expect: a `find_dir` row, a confirm and, after `y`, the typed `cd`; engine and model unchanged. Then with `--no-jinja`: two turns in one pane answer as text and the notice appears once. Record both results (and any wire surprise, e.g. thinking-model output or `<think>` text) in the PR description. If it fails, STOP: do not start T3.1; fix in PR1/PR2 follow-ups. Scenarios: "Live llamacpp tools", "Live server with --no-jinja".
- [ ] T3.1 RED (`engine/tools_tests.rs`): after the change `tools_for()` is true for `claude`, `codex`, `gemini`, `ollama`, `llamacpp` and false for `lmstudio`, `vllm`, `opencode`; the default `App::tool_engines` holds exactly those five; llamacpp with tools off gets no `tools` field; update any existing test that asserted the old list. GREEN: `MCP_VERIFIED += "llamacpp"` in `src/core/ai/chat/engine.rs`. ~3 prod. Dep: T3.0 passed. Scenarios: "Verified list after the change", "Tools off keeps llamacpp text-only", "Not offered until verified" (inverted, test the earlier state is gone).
- [ ] T3.2 RED (doc check test or grep-style test, as other README checks in the repo): README no longer states that none of the three local servers offers tools for llamacpp; a `### Tools with llama.cpp` section exists mentioning `--no-jinja`, tools off, lmstudio/vllm text-only, the once-per-pane notice, same tools as Ollama, round caps 8/24, Jinja default (lazarobox does not inject `--jinja`), and `-c` of 16384 or more recommended. GREEN: replace README l.362 and add the section next to l.639. README is in SPANISH. Docs only. Dep: T3.1. Scenarios: "README states the new support and the off switch".
- [ ] T3.3 Closing: final `cargo test`; note in the change that archive must follow `agent-tools` and `coding-tools` (spec baseline note). Dep: T3.2.

## Dependency summary

```
T1.0 (manual) -> T1.1 -> T1.2 -> T1.3 -> T1.4
                              \-> T1.5 -> T1.6
T1.7 (parallel with T1.2) -> T1.8 -> T1.9 -> T1.10     [PR1]
PR1 merged -> T2.1 -> T2.2 -> T2.3, T2.4 -> T2.5, T2.6 -> T2.7 -> T2.8   [PR2]
PR2 merged -> T3.0 (manual) -> T3.1 -> T3.2 -> T3.3     [PR3]
```

Parallel inside PR1: the decoder track (T1.2-T1.6) and the body track (T1.7-T1.10) share only `stream.rs`/`adapters.rs` boundaries; T1.7 (byte pin) can be done first by itself. Everything else is sequential because the PRs are stacked.

## Spec scenario coverage

| Scenario | Task(s) |
|---|---|
| Tools offered on a verified llamacpp turn | T1.9, T2.2, T2.7 |
| Tools off sends the tool-less body | T1.2, T1.9 |
| Fragments accumulated per index | T1.3 |
| Recorded b11935 fixture | T1.6, T2.5 |
| Truncated stream runs no call | T1.4 |
| Missing call id is synthesized | T1.5, T1.9, T2.5 |
| Server id is echoed | T1.9, T2.5 |
| Arguments sent back as a JSON string | T1.9, T2.5 |
| Duplicate keys / unknown name / too many calls | T1.5 |
| One round on llamacpp | T2.2, T2.7 |
| Write tool in the llamacpp loop | T2.3 |
| Round cap 8 / 24 | T2.3 |
| Tool messages not kept | T2.3 |
| Cancel in the llamacpp loop | T1.4, T2.3 |
| Rejected tools fallback + one notice | T2.4 |
| Fallback on 400/422/500 with tools or jinja | T2.4 |
| Unrelated errors no fallback | T2.4 |
| Notice once per pane | T2.6 |
| No fallback after a round started | T2.4 |
| Ollama keeps degrading silently | T2.1, T2.4 |
| Ollama bodies byte-identical | T1.2, T1.7, T1.8, T2.1, T2.8 |
| lmstudio and vllm stay text-only | T1.9, T2.7 |
| Live llamacpp tools [manual] | T3.0 |
| Live server with --no-jinja [manual] | T1.0 (F3), T3.0 |
| Not offered until verified | T2.7 |
| Verified list after the change | T3.1 |
| Tools off keeps llamacpp text-only | T2.7, T3.1 |
| README states the new support | T3.2 |
| Wires are not mixed | T2.2 |

## Risks

- T1.0 needs the user's GPU and model; PR1's decoder cannot be final without it (High likelihood the wire differs: fragments, `id`/`index`, no-jinja status).
- D8 keyword match depends on F3 landing inside the first 200 characters; if not, D8 changes (T1.1.c).
- `content:""` vs `null` is decided by F2 (T1.1.d); wrong guess breaks every follow-up.
- PR3 is gated on a manual check across two models; a failure sends work back to PR1/PR2 follow-ups.
- PR2 touches `runtime/chat.rs`, shared with other in-flight chains (agent-orchestrator, coding-tools): expect rebases; keep the diff to ~12 lines.
- Archive order: this change must be archived after `agent-tools` and `coding-tools`.
