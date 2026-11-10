# Chat Engines Specification (delta: OpenAI-compatible adapters)

Delta against `chat-engines` as it will stand after `ai-chat` is archived. Moved out of `ai-chat` (PR14 deferred by the user). Tags as in `ai-chat`: [auto] = `cargo test`; [manual] = real machine.

## MODIFIED Requirements

### Requirement: ChatEngine contract
Every adapter MUST implement one contract: it receives a model name, the system prompt and the ordered message history, and it produces events in order: zero or more content events, each either a text delta (part of the answer) or a thinking delta (part of the engine's reasoning, a distinct event type that is never a text delta), then exactly one terminal event (final or error). An engine or model that exposes no reasoning MUST simply never produce a thinking delta. It MUST support cancel, after which it produces no more events. The contract MUST NOT expose tool calls. Adapters exist for `ollama`; for `lmstudio`, `llamacpp` and `vllm` (OpenAI-compatible, see OpenAI-compatible servers below); and for `claude`, `codex` and `gemini` (CLIs). `opencode` has no adapter. Each adapter is usable as soon as it exists; an engine without an adapter resolves to the unsupported state, whose guidance reads `not supported in chat yet`.

#### Scenario: Events in order [auto]
- GIVEN a fake adapter that emits `a`, `b` and a final event
- WHEN a turn runs
- THEN the consumer sees delta `a`, delta `b`, then exactly one final event, and nothing after it

#### Scenario: Thinking deltas are a distinct event [auto]
- GIVEN a fake adapter that emits thinking delta `t1`, thinking delta `t2`, text delta `a` and a final event
- WHEN a turn runs
- THEN the consumer sees two thinking events, one text delta and one final, in that order, and `t1` and `t2` never appear as text deltas

#### Scenario: An engine without reasoning emits no thinking event [auto]
- GIVEN a fake adapter that emits only text deltas and a final event
- WHEN a turn runs
- THEN the consumer sees no thinking event and the events equal those of the contract without reasoning

#### Scenario: Error is terminal [auto]
- GIVEN a fake adapter that emits `a` then an error
- WHEN a turn runs
- THEN the consumer sees delta `a` then one error event and no final event

#### Scenario: Cancel stops events [auto]
- GIVEN a fake adapter streaming
- WHEN cancel is requested
- THEN no further event is delivered

#### Scenario: opencode has no adapter [auto]
- GIVEN the adapter table
- WHEN it is queried for each catalog engine
- THEN `opencode` has none and the other seven have one (`codex` and `gemini` are usable only where the CLI jail is available, see CLI jail)

## ADDED Requirements

### Requirement: OpenAI-compatible servers
The `lmstudio`, `llamacpp` and `vllm` adapters MUST talk to their server's chat-completions endpoint (`/v1/chat/completions`) with the loopback-only streaming client of `ai-chat` (POST, HTTP/1.1, `127.0.0.1` on the catalog port, no credentials, no redirects, the same framing rules). The request body MUST contain only the model, the messages (system prompt first, then the capped history) and the streaming flag. Their replies are decoded by the SSE decoder below. The exact body fields are pinned by the design of this change from captured fixtures and the PR14 tests.

#### Scenario: Traffic audit per catalog port [auto]
- GIVEN a recording fake server on each of the lmstudio, llama.cpp and vLLM ports
- WHEN one turn runs against each
- THEN each recorded request is a `POST` to `/v1/chat/completions` on `127.0.0.1` with no `Authorization` or `Cookie` header, and nothing else connected

#### Scenario: Body content for OpenAI-compatible servers [auto]
- GIVEN a turn with a system prompt and two messages
- WHEN the request is recorded
- THEN the JSON body holds the model, the messages and the streaming flag, and nothing else

#### Scenario: OpenAI-compatible reasoning only if available [auto]
- GIVEN the captured lmstudio, llama.cpp and vLLM fixtures
- WHEN parsed
- THEN a thinking delta is produced only where a fixture shows a separate reasoning field in the SSE delta; otherwise the adapter emits none and the request is unchanged by reasoning (no format is assumed)

### Requirement: SSE decoder
OpenAI-compatible replies are Server-Sent Events. The SSE parser MUST be pure and incremental: feeding the same bytes in any chunking, including one byte at a time, MUST produce the same events as feeding them whole. `data:` events with a text fragment yield text deltas, a finish marker or `data: [DONE]` yields exactly one final event, and comment lines (starting with `:`) and empty events are ignored. CRLF and LF line endings MUST give the same events. A malformed JSON event MUST yield an error event, not a panic. A single event over 1 MiB MUST yield an error event. UTF-8 characters split across chunks MUST be reassembled before decoding. A server `error` object MUST be sanitized and capped like every other engine text.

#### Scenario: OpenAI SSE [auto]
- GIVEN events with fragments `Hel`, `lo`, a finish marker and `data: [DONE]`
- WHEN parsed
- THEN the events are delta `Hel`, delta `lo` and exactly one final

#### Scenario: Every split point over SSE [auto]
- GIVEN a valid SSE stream
- WHEN it is cut at every byte position into two chunks
- THEN the events always equal the unsplit result

#### Scenario: SSE comments and CRLF [auto]
- GIVEN a stream with `: keep-alive` lines and CRLF line endings
- WHEN parsed
- THEN the comments are ignored and the events are the same as with LF

#### Scenario: Server error object over SSE [auto]
- GIVEN an event `data: {"error":{"message":"model not found"}}`
- WHEN parsed
- THEN an error event is produced whose message is sanitized and capped

#### Scenario: Oversized SSE event [auto]
- GIVEN a single event of 1 MiB plus one byte
- WHEN parsed
- THEN an error event is produced

### Requirement: Real OpenAI-compatible engines
Each shipped OpenAI-compatible adapter MUST stream a reply from the real engine.

#### Scenario: Ollama and OpenAI-compatible servers [manual]
- GIVEN Ollama running with a model and, separately, a running lmstudio, llama.cpp or vLLM
- WHEN `chat` is assigned to it and a message is sent
- THEN the reply streams into the pane and cancel stops it

#### Scenario: lmstudio, llama.cpp or vLLM ends on DONE [manual]
- GIVEN a real lmstudio, llama.cpp or vLLM server with a loaded model
- WHEN a message is sent and then cancelled in a second turn
- THEN the `[DONE]` marker ends the first turn with one final event, and cancel stops the second
