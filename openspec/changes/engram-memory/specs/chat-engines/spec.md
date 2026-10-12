# Delta for Chat Engines

Tags: [auto] = `cargo test` (fake servers on ephemeral ports, fake engine). Existing requirements are unchanged. The streaming POST client stays as specified there; the plain JSON exchange used for engram is specified in engram-client and ai-engine-detection.

## ADDED Requirements

### Requirement: Memory block in the system context
When a memory block exists for a conversation (see chat-memory), every turn sent after it arrived MUST carry it in the system context, after the pinned system prompt of the conversation's profile and before the history, for every adapter. For server adapters it is part of the system message; for CLI adapters it travels on stdin with the system prompt and never in argv. The block's characters MUST be subtracted from the character cap of the history in Context window (the 40-message cap is unchanged), the block itself MUST NOT be dropped by the caps, MUST NOT be stored, and MUST NOT appear in the history of any later turn. Without a block the request body, argv and stdin MUST be byte-identical to those of the pinned adapters without this requirement. The request body of a server adapter still contains only the model, the system context, the history and the streaming flag.

#### Scenario: Server request with a block [auto]
- GIVEN a block `B` and two history messages
- WHEN an ollama request is recorded
- THEN the system message is the profile prompt followed by `B`, the history follows, and the body holds nothing else

#### Scenario: CLI carries the block on stdin [auto]
- GIVEN a block `B` and a fake CLI recording argv and stdin
- WHEN a turn runs
- THEN `B` is on stdin and neither argv nor the environment contains it

#### Scenario: Block shrinks the history cap [auto]
- GIVEN a block of 2,000 characters and 10 messages of 1,500 characters for a local server engine
- WHEN the context is built
- THEN the history holds at most 10,000 characters, the newest message is present and the block is intact

#### Scenario: No block, no change [auto]
- GIVEN a conversation without a block
- WHEN requests are built for every adapter
- THEN each equals the request built without this requirement

#### Scenario: Block is not history [auto]
- GIVEN three turns with a block
- WHEN the history of the fourth turn is built
- THEN it holds only user and assistant message texts

### Requirement: Summary request
A summary request (see chat-memory) MUST use the same engine resolution, adapters, timeouts, cancel and sanitizer as a chat turn, with the pinned summary prompt as the system prompt, the capped text history as messages, and a final instruction message asking for the summary. It carries no memory block and no reasoning. Thinking deltas produced by the engine for it are dropped. Its text deltas are collected, not appended to the transcript.

#### Scenario: Request content [auto]
- GIVEN a conversation of four messages and a fake server
- WHEN a summary is requested
- THEN the recorded body holds the summary system prompt, the four message texts and the final instruction, and no memory block or reasoning

#### Scenario: Thinking dropped, text kept [auto]
- GIVEN a fake adapter that emits thinking deltas, text deltas `We `, `chose Axum.` and a final event
- WHEN the summary runs
- THEN the collected summary is `We chose Axum.`

#### Scenario: Timeouts and cancel apply [auto]
- GIVEN a fake server that stalls after one delta
- WHEN the summary runs, and in another run cancel is requested
- THEN the first ends with the stalled error and the second closes the socket within 1 second
