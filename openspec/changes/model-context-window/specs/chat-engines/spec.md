# Delta for Chat Engines

## ADDED Requirements

### Requirement: Ollama context and keep-alive options
When the Ollama adapter builds a request it MUST add two fields to the body and nothing else. `options.num_ctx` is `min(model max window, ai.servers.ollama.num_ctx_cap)` and is sent only when the model's max window is known (see ai-engine-detection); when it is unknown, no `options` object is sent. `keep_alive` is always sent, as `<keep_alive_minutes>m`; `keep_alive_minutes = 0` is sent as `0m` and means Ollama unloads the model right after the reply (see configuration). For one model both values MUST be identical on every turn, so Ollama never reloads the model because of them. No other adapter (lmstudio, llamacpp, vllm, claude, codex, gemini) changes its body, argv or stdin.

#### Scenario: Known window is capped [auto]
- GIVEN a model whose max window is 131072 and a cap of 16384
- WHEN an Ollama request is recorded
- THEN `options.num_ctx` is 16384 and `keep_alive` is `30m`

#### Scenario: Small model keeps its own window [auto]
- GIVEN a model whose max window is 4096 and a cap of 16384
- WHEN an Ollama request is recorded
- THEN `options.num_ctx` is 4096

#### Scenario: Unknown window sends no num_ctx [auto]
- GIVEN a model whose window could not be read
- WHEN an Ollama request is recorded
- THEN the body has no `options` and still has `keep_alive`

#### Scenario: Zero keep-alive unloads [auto]
- GIVEN `keep_alive_minutes = 0`
- WHEN an Ollama request is recorded
- THEN `keep_alive` is `0m`

#### Scenario: Stable across turns [auto]
- GIVEN three turns with the same model and config
- WHEN the requests are recorded
- THEN `options.num_ctx` and `keep_alive` are equal in all three

#### Scenario: Other engines unchanged [auto]
- GIVEN requests built for lmstudio, llamacpp, vllm and the three CLIs
- WHEN compared with the pinned adapters
- THEN each is byte-identical, with no window logic for the cloud CLIs

### Requirement: Overflow trims in blocks with a visible notice
When the system context, the tool schemas and the history do not fit the budget (see Context window), the oldest history MUST be dropped in blocks of one user message plus its assistant answer, so the sent history still starts on a user message. The newest message is always kept. Every trim MUST show the notice of chat-pane (Context truncation marker); it MUST NEVER be silent, and a turn MUST NOT be sent knowing it exceeds the window.

#### Scenario: Oldest pair dropped [auto]
- GIVEN a forced small window and history u1 a1 u2 a2 u3
- WHEN the context is built
- THEN u1 and a1 are omitted together, the history starts on u2, and the notice is set

#### Scenario: Nothing to trim, no notice [auto]
- GIVEN a history that fits the budget
- WHEN the context is built
- THEN no block is dropped and no notice is set

#### Scenario: Newest alone does not fit [auto]
- GIVEN a newest message larger than the whole budget
- WHEN the context is built
- THEN all older messages are dropped, the newest is sent whole and the notice is set

## MODIFIED Requirements

### Requirement: Loopback-only streaming client
Server adapters MUST use the project's hand-written HTTP client extended with POST. Requests MUST be HTTP/1.1 with a JSON body, sent only to `127.0.0.1` on the configured port of the chosen server engine (the catalog port unless `[ai.servers.<id>] port` sets another one). The client MUST refuse any other host or address, MUST NOT follow redirects, MUST NOT send `Authorization`, `Cookie` or any credential, and MUST NOT read any credential or config file of any engine. The request body MUST contain only the model, the system prompt, the history, the streaming flag and, when tools are on and the engine supports them (see Ollama native tools), the tool definitions; the Ollama adapter additionally carries `options.num_ctx` (when the window is known) and `keep_alive` (see Ollama context and keep-alive options), and no other field. The system prompt MAY include the workspace context block (see prompt-profiles); that block is the only environment or location information a request carries, with one exception: the tool messages of the current turn (the model's own tool calls and the sanitized, capped results returned by the registry, which may hold paths and file contents inside `$HOME`, see agent-tools). A request MUST NEVER otherwise carry an absolute path, an environment variable, file contents or terminal output. A reply MUST be accepted only with an HTTP/1.x status 200 and the same strict framing rules as ai-engine-detection (conflicting or signed Content-Length, Content-Length with Transfer-Encoding and malformed chunking are rejected). The body is decoded incrementally (chunked, Content-Length or close-delimited) and MUST NOT be buffered whole. A non-200 reply MUST become an error event carrying the status code and a short sanitized message, never the full body. The total text of one answer MUST be capped at 1 MiB; beyond the cap the stream ends with an error.
(Previously: the body had no Ollama `options` or `keep_alive`.)

#### Scenario: Traffic audit [auto]
- GIVEN a recording fake server on the Ollama port
- WHEN one turn runs against it
- THEN the recorded request is a `POST` to its chat path on `127.0.0.1` with no `Authorization`/`Cookie` header, and nothing else connected

#### Scenario: Body content [auto]
- GIVEN a turn with a system prompt and two messages and tools off
- WHEN the request is recorded
- THEN the JSON body holds the model, the system prompt, the two messages and the streaming flag, plus for Ollama `options.num_ctx` (when known) and `keep_alive`, and nothing else

#### Scenario: Body content with tools [auto]
- GIVEN the same turn with tools on and a model that supports them
- WHEN the request is recorded
- THEN the JSON body holds additionally only the tool definitions of the registry

#### Scenario: Body carries the context block only inside the system prompt [auto]
- GIVEN a turn whose system prompt ends with the workspace context block
- WHEN the request is recorded
- THEN the JSON body still holds only the fields of the Body content scenario, and the block appears only inside the system prompt text

#### Scenario: Paths only from the current turn's tool results [auto]
- GIVEN a turn whose tool result holds `~/dev/curso-rust` and a following turn
- WHEN both requests are recorded
- THEN the first carries the result as a tool message and the second carries no tool message or path

#### Scenario: Foreign host refused [auto]
- GIVEN an engine endpoint whose host is not `127.0.0.1`
- WHEN a turn is requested
- THEN no connection is attempted and an error event is produced

#### Scenario: Redirect not followed [auto]
- GIVEN a server that answers 307 with a Location header
- WHEN a turn runs
- THEN the result is an error event and the Location is never requested

#### Scenario: Non-200 becomes a short error [auto]
- GIVEN a server that answers 404 with a 1 MiB body
- WHEN a turn runs
- THEN the error event carries `404` and a message of at most 200 characters

#### Scenario: Ambiguous framing rejected [auto]
- GIVEN a reply with both `Content-Length` and `Transfer-Encoding: chunked`
- WHEN a turn runs
- THEN an error event is produced and no delta is delivered

#### Scenario: Answer cap [auto]
- GIVEN a server that streams more than 1 MiB of text
- WHEN a turn runs
- THEN the stream ends with an error after at most 1 MiB was delivered

#### Scenario: Streaming is incremental [auto]
- GIVEN a server that sends one delta, waits, then sends the rest
- WHEN the turn runs
- THEN the first delta is delivered before the server sends the second

### Requirement: Context window
Each turn MUST send the pinned system prompt followed by the most recent messages that fit a budget, never splitting a message and always including the message just sent. The budget has three parts. (1) At most the last 40 messages. (2) A character cap per engine: for a local server whose window is known (Ollama: the effective `num_ctx`; llama.cpp: its `n_ctx`) the window in tokens times 4 characters; for a local server whose window is unknown, 12,000 characters; 100,000 characters for CLIs. (3) For local servers in every case, known window or not, the characters of the system prompt and of the tool schemas sent with the request MUST be subtracted from the cap before history is counted. Whether a small reserve for the model's output is also subtracted is a design decision. Messages beyond the budget MUST be omitted oldest first in blocks (see Overflow trims in blocks with a visible notice), the system prompt MUST never be dropped, and the pane MUST show the truncation marker (see chat-pane). System lines are never included, and neither is reasoning: only the user and assistant message texts are history. CLI behavior is unchanged: no window is learned or used for claude, codex or gemini.
(Previously: a fixed 12,000-character cap for local servers that ignored the real window, the system prompt and the tool schemas; trimming was per message.)

#### Scenario: Reasoning is not history [auto]
- GIVEN a conversation whose assistant answer was preceded by reasoning `thinking hard`
- WHEN the context for the next send is built
- THEN the history holds the answer text only and no message or character count includes the reasoning

#### Scenario: Within the cap [auto]
- GIVEN 6 short messages
- WHEN the context is built
- THEN all 6 follow the system prompt in order

#### Scenario: Over the cap [auto]
- GIVEN 60 short messages
- WHEN the context is built
- THEN the system prompt plus the 40 most recent messages are sent and the omitted flag is set

#### Scenario: Character cap for a server with an unknown window [auto]
- GIVEN 10 messages of 5,000 characters each, a local server engine with an unknown window, and a system prompt plus tool schemas of 3,000 characters
- WHEN the context is built
- THEN the oldest messages are omitted until the history is at most 9,000 characters and the newest message is present

#### Scenario: Known window replaces the fixed cap [auto]
- GIVEN an Ollama model with an effective `num_ctx` of 8192 and a head (system prompt plus tool schemas) of 9,000 characters
- WHEN the context is built
- THEN the history fits in `8192 * 4 - 9000` characters, not in 12,000

#### Scenario: Head counted even when the window is unknown [auto]
- GIVEN a local server with an unknown window and tool schemas of 9,000 characters
- WHEN the context is built
- THEN the history budget is 12,000 minus the system prompt and the 9,000 characters, never 12,000

#### Scenario: Character cap for a CLI [auto]
- GIVEN 40 messages of 5,000 characters each and a CLI engine
- WHEN the context is built
- THEN the oldest messages are omitted until the total is at most 100,000 characters and the newest message is present

#### Scenario: Newest message alone exceeds the cap [auto]
- GIVEN a newest message of 40,000 characters and a local server engine
- WHEN the context is built
- THEN the newest message is still sent whole, with the system prompt, and older messages are omitted

#### Scenario: Over-window notice once per streak [auto]
- GIVEN several consecutive sends whose newest message and setup pass a known window
- WHEN each is sent
- THEN the over-window line shows once, and shows again only after a send that fits

#### Scenario: Unknown Ollama window is visible [auto]
- GIVEN an Ollama model whose window probe failed
- WHEN the first turn with it is sent
- THEN the statusline notice reads `window unknown for <model>: 12k chars` and the unknown-window budget is used
