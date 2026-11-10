# Delta for Chat Engines

Tags as in the main spec; [design-confirm] = mechanism that design MUST verify against the pinned engine version before the scenario becomes [auto]. With tools off, every request, argv, stdin and environment MUST be byte-identical to the ones of the main spec.

## MODIFIED Requirements

### Requirement: ChatEngine contract
Every adapter MUST implement one contract: it receives a model name, the system prompt and the ordered message history, and it produces events in order: zero or more content events, each either a text delta (part of the answer), a thinking delta (part of the engine's reasoning, a distinct event type that is never a text delta) or a tool event (`ToolCall` or `ToolResult`, see Tool events), then exactly one terminal event (final or error). An engine or model that exposes no reasoning MUST simply never produce a thinking delta. It MUST support cancel, after which it produces no more events. An adapter MUST NOT execute a tool itself: tools are executed only by the app through the shared registry (see agent-tools), and tool events are produced by that execution, never by parsing answer text. Adapters exist for `ollama` (server) and for `claude`, `codex` and `gemini` (CLIs). `lmstudio`, `llamacpp`, `vllm` and `opencode` have no adapter in this change: they resolve to the unsupported state and the guidance reads `not supported in chat yet`. The OpenAI-compatible adapters are specified in the follow-up change `ai-chat-openai-compat`. Each adapter is usable as soon as it exists; an engine without an adapter resolves to the unsupported state.
(Previously: the contract MUST NOT expose tool calls)

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

#### Scenario: Engines without an adapter [auto]
- GIVEN the adapter table
- WHEN it is queried for each catalog engine
- THEN `ollama`, `claude`, `codex` and `gemini` have one, and `lmstudio`, `llamacpp`, `vllm` and `opencode` have none and resolve to unsupported (`codex` and `gemini` are usable only where the CLI jail is available, see CLI jail)

#### Scenario: Tool events are content, not terminal [auto]
- GIVEN a fake adapter that emits text `a`, a tool call, its result, text `b` and a final event
- WHEN a turn runs
- THEN the consumer sees them in that order and exactly one final event

#### Scenario: Text cannot forge a tool event [auto]
- GIVEN an answer whose text imitates a tool call
- WHEN the stream is decoded
- THEN no tool event is produced and the text is a text delta

### Requirement: Loopback-only streaming client
Server adapters MUST use the project's hand-written HTTP client extended with POST. Requests MUST be HTTP/1.1 with a JSON body, sent only to `127.0.0.1` on the catalog port of the chosen server engine. The client MUST refuse any other host or address, MUST NOT follow redirects, MUST NOT send `Authorization`, `Cookie` or any credential, and MUST NOT read any credential or config file of any engine. The request body MUST contain only the model, the system prompt, the history, the streaming flag and, when tools are on and the engine supports them (see Ollama native tools), the tool definitions. The system prompt MAY include the workspace context block (see prompt-profiles); that block is the only environment or location information a request carries, with one exception: the tool messages of the current turn (the model's own tool calls and the sanitized, capped results returned by the registry, which may hold paths and file contents inside `$HOME`, see agent-tools). A request MUST NEVER otherwise carry an absolute path, an environment variable, file contents or terminal output. A reply MUST be accepted only with an HTTP/1.x status 200 and the same strict framing rules as ai-engine-detection (conflicting or signed Content-Length, Content-Length with Transfer-Encoding and malformed chunking are rejected). The body is decoded incrementally (chunked, Content-Length or close-delimited) and MUST NOT be buffered whole. A non-200 reply MUST become an error event carrying the status code and a short sanitized message, never the full body. The total text of one answer MUST be capped at 1 MiB; beyond the cap the stream ends with an error.
(Previously: the body held only the model, system prompt, history and streaming flag, and never any absolute path or file content)

#### Scenario: Traffic audit [auto]
- GIVEN a recording fake server on the Ollama port
- WHEN one turn runs against it
- THEN the recorded request is a `POST` to its chat path on `127.0.0.1` with no `Authorization`/`Cookie` header, and nothing else connected

#### Scenario: Body content [auto]
- GIVEN a turn with a system prompt and two messages and tools off
- WHEN the request is recorded
- THEN the JSON body holds the model, the system prompt, the two messages and the streaming flag, and nothing else

#### Scenario: Body content with tools [auto]
- GIVEN the same turn with tools on and a model that supports them
- WHEN the request is recorded
- THEN the JSON body holds additionally only the tool definitions of the registry

#### Scenario: Body carries the context block only inside the system prompt [auto]
- GIVEN a turn whose system prompt ends with the workspace context block
- WHEN the request is recorded
- THEN the JSON body still holds only the model, the system prompt, the messages and the streaming flag, and the block appears only inside the system prompt text

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

### Requirement: CLI adapters are text-only
The `claude`, `codex` and `gemini` adapters MUST launch the CLI in a mode where its own tools cannot execute commands, edit files, browse or read files; the only tools it can call are the lazarobox tools served over MCP when tools are on (see lazarobox-mcp-server), each guarded by lazarobox's own approval. Design MUST verify, against each CLI's real `--help` and output, the flags and the output format that give that guarantee, and MUST pin them; if a CLI cannot be made safe, that adapter MUST be gated off (resolution `gated off`) instead of shipping, and if only the MCP injection cannot be verified the adapter MUST ship text-only. Flags are a first layer only; the guarantee that a CLI has the same reach as an API connection comes from the CLI jail (see below). The CLI MUST be started directly (no shell, no `sh -c`), with each argument a separate argv element, stdout and stderr piped, and no credential, token or key of the user or the engine added to its arguments or environment by lazarobox. The one exception is the per-chat lazarobox MCP session token, which is lazarobox's own and MUST NOT appear in argv. The conversation (system prompt for CLIs without a system-prompt flag, and every message) MUST travel on stdin, which is written and then closed; argv MUST NEVER hold a message or history. lazarobox MUST NOT open for reading any credential or config file of the CLI; the CLI uses the user's own login. Holding a directory handle, or a handle on the claude config file, only to grant the jail access to it is not reading. The prompt text and the model name MUST be passed so they can never be read as an option (a message or model starting with `-` is data). Model `default` MUST mean no model option. Text is streamed to the pane as the CLI produces it. A non-zero exit, a launch failure or empty output MUST end the turn with an error event carrying the CLI's sanitized, capped stderr summary. The system prompt MAY include the workspace context block (see prompt-profiles); it is text in the prompt and does not change the CLI's neutral empty cwd or its jail. The conversation context for each turn MUST be the system prompt plus the capped history (see Context window); a CLI session of its own is not used.
(Previously: the CLI could call no tool at all, and no token was ever added to arguments or environment)

#### Scenario: Tools are disabled in argv [auto]
- GIVEN a fake `claude`, a fake `codex` and a fake `gemini` that record their argv
- WHEN a turn runs with each, with tools off
- THEN each recorded argv contains the design-pinned options that disable tools for that CLI

#### Scenario: Only lazarobox tools with tools on [auto]
- GIVEN the same fakes and tools on
- WHEN a turn runs with each
- THEN each argv or config keeps the CLI's own tools disabled and allows only the lazarobox MCP server

#### Scenario: No shell, no stdin [auto]
- GIVEN a fake CLI recording how it was started
- WHEN a turn runs with a prompt containing `; touch /tmp/x $(id) | cat`
- THEN the prompt arrives as data on stdin (never in argv), no shell process is involved, stdin is closed after the write and `/tmp/x` is not created

#### Scenario: Option injection [auto]
- GIVEN a user message `--dangerous-flag` and a model named `-m`
- WHEN a turn runs
- THEN neither is interpreted as an option by the adapter's argv construction

#### Scenario: Default model passes no model option [auto]
- GIVEN the model `default`
- WHEN the argv is built
- THEN it contains no model option

#### Scenario: Credentials never read [auto]
- GIVEN a home directory with decoy credential files for each CLI and a recording of file opens by the adapter
- WHEN a turn runs
- THEN no decoy file is opened for reading by lazarobox and no secret is present in argv or added environment

#### Scenario: Session token not in argv [auto]
- GIVEN a turn with tools on
- WHEN the argv and the process list are inspected
- THEN the lazarobox session token is in neither

#### Scenario: Failure becomes an error [auto]
- GIVEN a fake CLI that prints `boom` and an escape sequence on stderr and exits 1
- WHEN a turn runs
- THEN an error event carries `boom` with the escape sequence removed

#### Scenario: Gated off [auto]
- GIVEN an adapter whose tool-less mode is flagged unavailable
- WHEN the engine is resolved
- THEN the result is gated off and no process is started

#### Scenario: Injection unverified falls back to text [auto]
- GIVEN an adapter whose MCP injection is flagged unverified
- WHEN the engine is resolved and a turn runs
- THEN the engine is ready and the turn is text-only, identical to a turn with tools off

#### Scenario: A write request produces no file [manual]
- GIVEN each real CLI installed and logged in
- WHEN the message asks it to create a file in the current directory
- THEN the answer is text only and no file is created

## ADDED Requirements

### Requirement: Tool events
The stream carries two content events for tools: `ToolCall` (call id, tool name, short sanitized argument summary) and `ToolResult` (the same id, status `ok`, `error` or `denied`, short sanitized summary). For a CLI engine they are produced by the app as it serves the MCP calls (so they do not depend on the CLI's output format); for Ollama by the tool loop. A tool event counts as stream activity: it satisfies the first-byte timeout, stops the `Slow` timer and resets the idle timeout. A tool event that arrives after the terminal event, or after cancel, MUST be dropped. Summaries are capped and sanitized with the output sanitizer.

#### Scenario: Order and pairing [auto]
- GIVEN two tool calls in one turn, the second result delayed
- WHEN the turn runs
- THEN each `ToolCall` precedes its `ToolResult` and ids match

#### Scenario: Activity [auto]
- GIVEN a turn whose only events for 100 seconds are tool events every 10 seconds (injected time)
- WHEN the clock advances
- THEN no first-byte, idle or `Slow` event is produced

#### Scenario: Late tool event dropped [auto]
- GIVEN a cancelled turn
- WHEN a late `ToolResult` arrives
- THEN the transcript and state are unchanged

### Requirement: Approval wait and timeouts
While a tool call waits for the user's approval (see agent-tools), the first-byte and idle timeouts of the turn MUST be suspended, because the wait is the user's and not a stall; the 15-minute total cap MUST keep running. No timeout of any kind MUST ever approve a call: when a timer ends the wait, the call is `denied`. Cancel MUST end the wait immediately (denied) and release the engine as in Cancel releases the engine.

#### Scenario: User thinks for 4 minutes [auto]
- GIVEN a CLI turn waiting on a pending approval (injected time)
- WHEN 4 minutes pass without a key
- THEN no stalled error is produced and the approval is still pending

#### Scenario: Total cap still applies [auto]
- GIVEN the same wait
- WHEN 15 minutes pass in total
- THEN the turn ends with the too-long error and the approval is denied

#### Scenario: Timer never approves [auto]
- GIVEN any timeout that ends an approval wait
- WHEN it fires
- THEN the call result is `denied` and nothing is written

### Requirement: Ollama native tools
When tools are on, the `chat` engine is `ollama` and the model supports tools, the adapter MUST send the registry's tool definitions in the `tools` field of `/api/chat` and run a bounded tool loop: when a reply carries tool calls, the app executes them through the registry (read tools directly, write tools only after approval), appends the calls and the sanitized results as tool messages, and sends a follow-up request for the same turn, up to a maximum number of rounds per turn (pinned by design). When the cap is reached the turn ends with a short system line and the partial answer is kept. Tool messages belong to the current turn only: they MUST NOT be stored, MUST NOT be part of later history and MUST NOT count toward the message caps of Context window beyond the turn. A model without tool support (the server rejects `tools`, or the model is known not to support them) MUST degrade silently to a text turn without `tools`, with the run action of pane-commands still available; the assignment MUST NOT change. The loop uses the same timeouts, cancel, sanitizer and answer cap as any turn.

#### Scenario: One round [auto]
- GIVEN a fake Ollama that answers the first request with a `find_dir` tool call and the second with text
- WHEN a turn runs
- THEN the app executes `find_dir`, sends a second request holding the call and its result, and the answer is delivered with one `ToolCall`/`ToolResult` pair

#### Scenario: Round cap [auto]
- GIVEN a fake Ollama that always answers with a tool call
- WHEN a turn runs
- THEN the loop ends at the cap with a system line and the turn returns to idle

#### Scenario: Model without tools [auto]
- GIVEN a fake Ollama that rejects the `tools` field for the model
- WHEN a turn runs
- THEN a second request without `tools` is made, the text answer streams, and the assignment is unchanged

#### Scenario: Write tool in the loop [auto]
- GIVEN a fake Ollama that calls `run_in_pane`
- WHEN the turn runs
- THEN an approval is pending, the loop waits, `y` writes the command (no Enter) and the loop continues with `sent`; with `n` it continues with `denied`

#### Scenario: Cancel in the loop [auto]
- GIVEN a loop waiting for the follow-up request's first byte
- WHEN cancel is requested
- THEN the socket is closed and no further request is made

#### Scenario: Tool messages not kept [auto]
- GIVEN a turn that used tools
- WHEN the next turn's history is built
- THEN it holds only the user and assistant message texts

#### Scenario: Live Ollama tools [manual]
- GIVEN Ollama with a tool-capable model assigned to `chat`
- WHEN the user asks to find `curso-rust` and go there
- THEN a `find_dir` row, a confirm and, after `y`, the typed `cd` appear, and the engine and model are unchanged

### Requirement: Jail rights for the shim
When tools are on, the CLI jail spec (see CLI jail) MUST gain only the minimal grants: read and execute on the exact lazarobox binary path that serves `__mcp-serve`, the rule for the session socket file, and the shim's ELF loader when it differs from the CLI's own loader. It MUST NOT gain a directory-wide grant, `$HOME`, `/tmp`, or any execute right for another program, and the "no path to ssh keys" and "execve of system programs is denied" guarantees MUST hold unchanged.

#### Scenario: Only the minimal grants [auto]
- GIVEN the jail spec of each CLI with tools off and on
- WHEN the two are compared
- THEN they differ by the shim binary (rx), the socket file rule and, only when it differs from the CLI's loader, the shim's ELF loader (rx)

#### Scenario: Guarantees unchanged [auto, tests/jail.rs]
- GIVEN the jail with the shim granted
- WHEN a jailed program attempts to read `~/.ssh` and to execve `/usr/bin/ssh`
- THEN both fail with EACCES
