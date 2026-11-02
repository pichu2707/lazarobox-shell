# Delta for Chat Engines

Tags as in the main spec. Usage semantics live in `ai-usage`.

## MODIFIED Requirements

### Requirement: ChatEngine contract
Every adapter MUST implement one contract: it receives a model name, the system prompt and the ordered message history, and it produces events in order: zero or more content events, each either a text delta (part of the answer) or a thinking delta (part of the engine's reasoning, a distinct event type that is never a text delta), optionally one usage event per engine request (see Usage events), then exactly one terminal event (final or error). An engine or model that exposes no reasoning MUST simply never produce a thinking delta. An engine that reports no usage MUST simply never produce a usage event. It MUST support cancel, after which it produces no more events. The contract MUST NOT expose tool calls. Adapters exist for `ollama`; for `lmstudio`, `llamacpp` and `vllm` (OpenAI-compatible, see OpenAI-compatible servers below); and for `claude`, `codex` and `gemini` (CLIs). `opencode` has no adapter. Each adapter is usable as soon as it exists; an engine without an adapter resolves to the unsupported state, whose guidance reads `not supported in chat yet`.
(Previously: no usage event in the contract)

#### Scenario: Events in order [auto]
- GIVEN a fake adapter that emits `a`, `b` and a final event
- WHEN a turn runs
- THEN the consumer sees delta `a`, delta `b`, then exactly one final event, and nothing after it

#### Scenario: Usage event precedes the terminal event [auto]
- GIVEN a fake adapter that emits `a`, one usage event and a final event
- WHEN a turn runs
- THEN the consumer sees delta `a`, the usage event, then exactly one final event, and nothing after it

#### Scenario: At most one usage event per request [auto]
- GIVEN a stream that carries two usage objects in one request
- WHEN a turn runs
- THEN the consumer sees one usage event (the last complete one) and the answer is unchanged

#### Scenario: Thinking deltas are a distinct event [auto]
- GIVEN a fake adapter that emits thinking delta `t1`, thinking delta `t2`, text delta `a` and a final event
- WHEN a turn runs
- THEN the consumer sees two thinking events, one text delta and one final, in that order, and `t1` and `t2` never appear as text deltas

#### Scenario: Error is terminal [auto]
- GIVEN a fake adapter that emits `a` then an error
- WHEN a turn runs
- THEN the consumer sees delta `a` then one error event and no final event

#### Scenario: Cancel stops events [auto]
- GIVEN a fake adapter streaming
- WHEN cancel is requested
- THEN no further event is delivered, including no usage event

## ADDED Requirements

### Requirement: Usage events
Each adapter's decoder MUST emit at most one usage event per engine request, carrying the usage that request reported (see `ai-usage`, Capture per engine), after the last content event and before the terminal event. The event reaches the chat state through the same stream pipe as other events and MUST be handled in every consumer of the stream (no consumer may ignore an unknown event by failing). The usage event MUST NOT count toward the answer cap, the reasoning cap, any timeout reset other than as stream activity, or the first-byte rule for text. Usage decoding is lenient and separate from answer decoding: a usage failure never produces an error event, never fails the turn and never alters text (see `ai-usage`, Lenient usage parsing). The gemini fail-closed decoder rules apply to the answer path only. When no final event arrives (cut stream, cancel, error) a usage event already received is still delivered to the consumer. The pipe MUST keep the same events for any chunking of the input bytes.

#### Scenario: Usage at every split point [auto]
- GIVEN a valid Ollama NDJSON stream whose final line carries usage
- WHEN it is cut at every byte position into two chunks
- THEN the events always equal the unsplit result, usage event included

#### Scenario: Usage does not end the turn [auto]
- GIVEN a CLI stream where the usage object appears before the final `result` line
- WHEN a turn runs
- THEN the turn ends only with the final event, not at the usage

#### Scenario: Usage after a cut stream [auto]
- GIVEN a stream that delivered a usage event and then ended without a final event
- WHEN a turn runs
- THEN the consumer sees the usage event, the interruption as before, and no panic

#### Scenario: Usage is not answer text [auto]
- GIVEN a stream with usage numbers in its last line
- WHEN a turn runs
- THEN no text delta contains them and the answer cap counts text only

#### Scenario: Every consumer handles the event [auto]
- GIVEN the chat loop, the runtime and the UI-side consumers of stream events
- WHEN a usage event is fed to each
- THEN none panics or reports an error, and a consumer that does not use usage ignores it
