# Delta for Chat Engines (llama.cpp native tools)

Baseline note: this delta is written against the PENDING deltas of the unarchived change `agent-tools` (`openspec/changes/agent-tools/specs/chat-engines/spec.md`, requirement "Ollama native tools", and the coding-tools deltas for the round cap of coding families), NOT against `openspec/specs/chat-engines/spec.md`, whose main spec still says CLI adapters are text-only and has no tool loop. This change MUST be archived after `agent-tools` (and `coding-tools`). Tags as in the main spec; [manual] scenarios need a real llama-server. With tools off, every request MUST be byte-identical to the one without this change. For `ollama`, every request body, decode path and fallback behavior MUST be byte-identical to the ones of "Ollama native tools".

## ADDED Requirements

### Requirement: llama.cpp native tools
When tools are on, the `chat` engine is `llamacpp` (managed or unmanaged), and the engine is in the verified-tools list (see Verified list gating), the adapter MUST send the registry's tool definitions in the OpenAI `tools` field of `/v1/chat/completions` and run the same bounded tool loop as "Ollama native tools": the app executes the calls through the registry (read tools directly, write tools only after approval), appends the calls and the sanitized results, and sends a follow-up request for the same turn. Caps, approvals, timeouts, cancel, sanitizer, answer cap, result cap, relay and the rule that tool messages belong to the current turn only are shared with Ollama and unchanged. The round cap MUST equal Ollama's: 8 rounds per turn, or 24 when a coding family is active. When the cap is reached the turn ends with a short system line and the partial answer is kept.

Request wire (OpenAI shape). The system prompt and history are assembled exactly as for Ollama. Per round, the assistant message MUST carry `tool_calls` with `id`, `type: "function"`, `function.name` and `function.arguments` as a JSON STRING (never an object); each result MUST be a message `{role: "tool", tool_call_id, content}` whose `tool_call_id` equals the id of the call it answers. When the server sent no id for a call, the app MUST synthesize a deterministic id (`call_{round}_{n}`) and use it both in the echoed assistant message and as `tool_call_id`. The body MUST NOT carry `parallel_tool_calls` and MUST NOT echo any reasoning (`reasoning_content`) back.

Response decode. The SSE stream carries tool calls as `choices[0].delta.tool_calls` fragments: the first fragment of a call carries `index` and, normally, `id` and `function.name`; later fragments carry `index` and `function.arguments` pieces. The decoder MUST accumulate fragments per `index` (arguments concatenated in arrival order) and MUST release the calls only after a clean end: `finish_reason: "tool_calls"` or `[DONE]` after the stream ended normally. A stream that is truncated (connection closed, framing error, idle timeout, cancel, answer cap) before a clean end MUST run no accumulated call. Validation of the accumulated calls MUST reuse the Ollama rules: a name unknown to the registry or a call whose arguments hold duplicate keys is refused (the model gets a refusal result, nothing is executed), arguments that are not valid JSON are refused, and at most 16 calls are accepted per reply. Text deltas keep streaming as today and tool-call text cannot forge a tool event.

Server without tool support. When the server rejects the request that carries `tools` (HTTP 4xx or 5xx whose sanitized message mentions `tools` or `jinja`, for example a server started with `--no-jinja`) and NO round has started yet in the turn, the adapter MUST retry once as a text turn without `tools`, the text answer streams, and the assignment MUST NOT change. For `llamacpp` ONLY, a system notice that the server does not accept tools MUST then be shown ONCE per pane (not once per turn); later turns in that pane MUST NOT repeat it. Ollama is explicitly different: its rejection fallback (HTTP 400 on the first request) stays silent, with no notice, exactly as in "Ollama native tools". Once a round has started (at least one tool result was sent), a rejection or any error MUST NOT trigger the fallback: it ends the turn as an ordinary error and no request without `tools` is made.

`lmstudio` and `vllm` remain text-only: they MUST NOT be offered tools and MUST NOT get a routing arm into the tool loop; their request bodies stay the tool-less body of today.

#### Scenario: Tools offered on a verified llamacpp turn [auto]
- GIVEN tools on, `chat` assigned to `llamacpp`, llamacpp in the verified list and a recording fake server
- WHEN a turn runs
- THEN the `POST /v1/chat/completions` body holds the registry's tool definitions in `tools`, the streaming flag and no `parallel_tool_calls`

#### Scenario: Tools off sends the tool-less body [auto]
- GIVEN tools off and `chat` assigned to `llamacpp`
- WHEN a turn runs
- THEN the body is byte-identical to the tool-less body without this change

#### Scenario: Fragments accumulated per index [auto]
- GIVEN a fake SSE stream where call index 0 arrives as a first fragment with `id` and `function.name` and then three `function.arguments` fragments, and index 1 interleaves the same way, ended by `finish_reason: "tool_calls"` and `[DONE]`
- WHEN the stream is decoded
- THEN two calls are produced, each with its name and its arguments concatenated in arrival order, and none is released before the clean end

#### Scenario: Recorded b11935 fixture [auto]
- GIVEN the recorded real b11935 tool-call SSE fixture under `tests/fixtures/llamacpp/`
- WHEN it is decoded and run through one loop round against a fake server replaying it
- THEN the call is executed through the registry and the follow-up request holds the call and its result

#### Scenario: Truncated stream runs no call [auto]
- GIVEN a stream that delivers a complete-looking tool call and is then cut before `finish_reason` and `[DONE]`
- WHEN the turn runs
- THEN no tool is executed, no `ToolCall` event is produced and the turn ends with an error

#### Scenario: Missing call id is synthesized [auto]
- GIVEN a stream whose tool call carries no `id`
- WHEN the loop sends the follow-up request
- THEN the echoed assistant `tool_calls` entry and the `tool_call_id` of the result message carry the same synthesized id `call_{round}_{n}`

#### Scenario: Server id is echoed [auto]
- GIVEN a stream whose tool call carries `id: "abc"`
- WHEN the loop sends the follow-up request
- THEN the result message has `role: "tool"` and `tool_call_id: "abc"`

#### Scenario: Arguments sent back as a JSON string [auto]
- GIVEN a call with arguments `{"query":"curso-rust"}`
- WHEN the follow-up request is recorded
- THEN the assistant `tool_calls[0].function.arguments` is the string `"{\"query\":\"curso-rust\"}"`, not an object, and the body has no `reasoning_content`

#### Scenario: Duplicate keys refused like Ollama [auto]
- GIVEN a call whose argument string repeats a key
- WHEN the stream is decoded
- THEN the call is refused with a refusal result sent to the model and nothing is executed

#### Scenario: Unknown tool name refused like Ollama [auto]
- GIVEN a call whose name is not in the registry
- WHEN the stream is decoded
- THEN the call is refused with a refusal result and nothing is executed

#### Scenario: Too many calls [auto]
- GIVEN a reply with more than 16 tool calls
- WHEN the stream is decoded
- THEN the reply is refused as with Ollama and nothing is executed

#### Scenario: One round on llamacpp [auto]
- GIVEN a fake llamacpp server that answers the first request with a `find_dir` call and the second with text
- WHEN a turn runs
- THEN the app executes `find_dir`, the second request holds the call and its result, and the answer is delivered with one `ToolCall`/`ToolResult` pair

#### Scenario: Write tool in the llamacpp loop [auto]
- GIVEN a fake llamacpp server that calls `run_in_pane`
- WHEN the turn runs
- THEN an approval is pending, the loop waits, `y` writes the command (no Enter) and the loop continues with `sent`; with `n` it continues with `denied`

#### Scenario: Round cap is 8, or 24 with a coding family [auto]
- GIVEN a fake llamacpp server that always answers with a tool call
- WHEN a turn runs without a coding family, then with one
- THEN the loop ends at 8 rounds, then at 24 rounds, each with a system line and the turn returns to idle

#### Scenario: Tool messages not kept on llamacpp [auto]
- GIVEN a llamacpp turn that used tools
- WHEN the next turn's history is built
- THEN it holds only the user and assistant message texts

#### Scenario: Cancel in the llamacpp loop [auto]
- GIVEN a llamacpp loop waiting for the follow-up request's first byte
- WHEN cancel is requested
- THEN the socket is closed, no further request is made and no accumulated call runs

#### Scenario: Rejected tools fall back to text with one notice per pane [auto]
- GIVEN a fake llamacpp server that answers the first request with HTTP 500 whose message mentions `tools param requires --jinja`
- WHEN a turn runs
- THEN a second request without `tools` is made, the text answer streams, the assignment is unchanged and one system notice that the server does not accept tools is shown

#### Scenario: Fallback on HTTP 400 and 5xx mentioning tools or jinja [auto]
- GIVEN fake servers answering 400, 422 and 500 with a message mentioning `tools` or `jinja`
- WHEN a turn runs against each
- THEN each triggers the single retry without `tools`

#### Scenario: Unrelated errors do not trigger the fallback [auto]
- GIVEN a fake llamacpp server that answers 500 with a message that mentions neither `tools` nor `jinja`
- WHEN a turn runs
- THEN the turn ends with the ordinary short error and no request without `tools` is made

#### Scenario: Notice shown once per pane [auto]
- GIVEN a pane whose llamacpp server rejects tools
- WHEN two turns run in that pane, and then one turn in another pane
- THEN the notice appears once in the first pane (not on the second turn) and once in the other pane

#### Scenario: No fallback after a round started [auto]
- GIVEN a fake llamacpp server that serves round one with a tool call and then answers the follow-up with HTTP 500 mentioning `tools`
- WHEN the turn runs
- THEN the turn ends as an ordinary error, no request without `tools` is made and no fallback notice is shown

#### Scenario: Ollama keeps degrading silently [auto]
- GIVEN a fake Ollama that rejects the `tools` field with HTTP 400
- WHEN a turn runs
- THEN a second request without `tools` is made, the text answer streams and NO notice is shown, exactly as before this change

#### Scenario: Ollama bodies and behavior byte-identical [auto]
- GIVEN the existing Ollama tool tests and recorded Ollama request bodies (first request, follow-up with `tool_name`, object arguments, `function.index`, thinking echo)
- WHEN they run with this change applied
- THEN every test passes and every recorded body is byte-identical

#### Scenario: lmstudio and vllm stay text-only [auto]
- GIVEN tools on and `chat` assigned to `lmstudio`, then to `vllm`
- WHEN a turn runs against a recording fake server
- THEN the body is the tool-less body, no `tools` field is sent and the tool loop is not entered

#### Scenario: Live llamacpp tools [manual]
- GIVEN a real llama-server b11935 with `--jinja` (default) and Qwen3.8-27B IQ4_XS, then Qwen2.5-7B-Instruct, assigned to `chat`
- WHEN the user asks to find `curso-rust` and go there
- THEN a `find_dir` row, a confirm and, after `y`, the typed `cd` appear, and the engine and model are unchanged

#### Scenario: Live server with --no-jinja [manual]
- GIVEN a real llama-server started with `--no-jinja`
- WHEN two turns run in the same pane with tools on
- THEN both answer as text and the not-accepting-tools notice appears once

### Requirement: Verified list gating for llamacpp
The engine verified-tools list (`MCP_VERIFIED`, which `tools_for()` and the default of `App::tool_engines` read) MUST gain `llamacpp` only in the last slice of this change, after the manual check with a real model has passed and the b11935 fixture is recorded. Until then llamacpp MUST NOT be offered tools, with tools on or off, and its request body MUST stay the tool-less body. After it, the list MUST hold exactly `claude`, `codex`, `gemini`, `ollama` and `llamacpp`; `lmstudio` and `vllm` MUST NOT be in it. A user who sets tools off, or runs the server with `--no-jinja`, MUST get text-only llamacpp turns.

#### Scenario: Not offered until verified [auto]
- GIVEN the verified list without `llamacpp` (before the last slice)
- WHEN a llamacpp turn runs with tools on
- THEN the body carries no `tools` field and the tool loop is not entered

#### Scenario: Verified list after the change [auto]
- GIVEN the final verified list
- WHEN `tools_for()` is queried for every catalog engine
- THEN `claude`, `codex`, `gemini`, `ollama` and `llamacpp` are tool-capable and `lmstudio`, `vllm` and `opencode` are not

#### Scenario: Tools off keeps llamacpp text-only [auto]
- GIVEN llamacpp in the verified list and tools off
- WHEN a turn runs
- THEN the body carries no `tools` field

### Requirement: llama.cpp tools documented
The README (Spanish, as the rest of the file) MUST no longer state that no local server offers tools, and MUST gain a "Tools with llama.cpp" section that says: llamacpp offers the same tools as Ollama when tools are on, it needs llama-server with Jinja (the default; lazarobox does not inject `--jinja`), `--no-jinja` or tools off turns the tools off, a server without tool support answers as text with a one-time notice per pane, `lmstudio` and `vllm` stay text-only, and the round caps equal those of Ollama.

#### Scenario: README states the new support and the off switch [auto]
- GIVEN the README after this change
- WHEN it is checked
- THEN the old "none of the three servers offers tools" statement for llamacpp is gone, the "Tools with llama.cpp" section exists and it mentions `--no-jinja`, tools off, lmstudio/vllm text-only and the per-pane notice

## MODIFIED Requirements

### Requirement: Ollama native tools
(Builds on the pending "Ollama native tools" of `agent-tools`; nothing about Ollama changes.) The Ollama loop, bodies, decode, round caps and its SILENT text fallback (no notice) MUST remain byte-identical and behaviorally identical after the loop is shared with llamacpp. The shared loop MUST be selected per engine by wire, and the Ollama wire (path `/api/chat`, object arguments, `tool_name`, thinking echo) MUST NOT be reachable from llamacpp nor the OpenAI wire from Ollama.
(Previously: the loop was Ollama-only; it is now shared with llamacpp through a per-engine wire)

#### Scenario: Wires are not mixed [auto]
- GIVEN recording fakes for Ollama and llamacpp running the same tool turn
- WHEN both requests are recorded
- THEN Ollama's go to `/api/chat` with object arguments and `tool_name`, llamacpp's go to `/v1/chat/completions` with string arguments and `tool_call_id`
