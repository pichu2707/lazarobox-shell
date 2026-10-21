# Delta for chat-engines

Slice tags as in agent-orchestrator.

## ADDED Requirements

### Requirement: Availability by task [S1]
Engine availability (installed, version, model, jail) MUST be computable for any task, not only `chat`, using the effective assignment of that task. The result for a worker MUST be the same as a chat turn would get for the same engine and model. An unavailable result MUST NOT select another engine.

#### Scenario: Availability per task [auto]
- GIVEN `chat` is `claude` (available) and `review` is `gemini` with no jail
- WHEN availability is computed for `chat` and for `review`
- THEN `chat` is usable and `review` is refused for no jail

#### Scenario: No fallback engine [auto]
- GIVEN `review` resolves to an unavailable engine and `chat` to an available one
- WHEN availability is computed for `review`
- THEN it stays unavailable and `chat`'s engine is not used

### Requirement: Worker turns [S1]
A worker turn MUST be a single stateless engine turn with the same adapter flags, jail, timeouts, output sanitizer and context rules as a chat turn, a private working folder, and the system prompt of its kind. The runtime MUST allow a turn to be started by a session key that is not a pane. The jail environment scrub MUST keep the provider API-key variables (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GEMINI_API_KEY`) available to the engine so API-key use stays viable.

#### Scenario: Same jail as chat [auto]
- GIVEN a worker on `codex`
- WHEN its command is built
- THEN it equals the chat command for codex apart from prompt and folder

#### Scenario: API keys survive the scrub [auto]
- GIVEN the three variables set in the environment
- WHEN a worker environment is built
- THEN all three are present

### Requirement: Provider error classification [S2]
Each adapter decoder MUST map a failed turn to a classified error (`rate_limit`, `conflict`, `auth`, `quota`, `timeout`, `jail_refusal`, `other`) carrying the provider's message sanitized and capped. A classified error MUST NOT contain secrets (the redaction rules apply) and MUST NOT be logged with content.

#### Scenario: Each adapter classifies [auto]
- GIVEN fixtures of 429 and 409 responses for ollama, claude, codex and gemini
- WHEN each is decoded
- THEN each gives `rate_limit` or `conflict` with the provider message

#### Scenario: Secret in message [auto]
- GIVEN an error message that contains a token
- WHEN it is classified
- THEN the message carries the redacted text

### Requirement: Usage events [S6]
Decoders MUST emit a usage event (input tokens, output tokens, and cost when the CLI reports it) at the end of a turn where the CLI reports it, and MUST emit nothing, never a guess, where it does not.

#### Scenario: Claude result usage [auto]
- GIVEN a claude `result` fixture with usage and cost
- WHEN it is decoded
- THEN one usage event carries those numbers

#### Scenario: No usage reported [auto]
- GIVEN a gemini fixture without usage
- WHEN it is decoded
- THEN no usage event is emitted
