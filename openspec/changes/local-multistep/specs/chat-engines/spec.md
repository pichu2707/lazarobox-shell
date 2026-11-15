# Delta for Chat Engines

## Base of this delta

This delta targets the owning spec `openspec/specs/chat-engines/spec.md` (read on `main` at 7a2ef223). Each MODIFIED block is a full copy of the requirement text as it stands, with the edit marked "(Previously: ...)". Where a requirement is very long, only the scenarios that change or are added are listed after the copy, and every other existing scenario of it stays as it is. Slice tags: [S2] HTTP 400 disambiguation; [S3] per-model profiles and `think`; [S4] end-of-turn step check. S1 modifies no requirement of this capability (see `local-model-eval`, "S1 changes no behaviour"). The #442 follow-ups (S5+) are not specified here: each is a small separate change that carries its own delta.

Ordering constraint (C6): S2 MUST land before S3. No request carries `think` until the "tools refused" case is told apart from every other HTTP 400.

Traceability: C1, C2, C6, C7, C9, C10 and proposal scope items 5-7.

## MODIFIED Requirements

### Requirement: Ollama native tools [S2]
When tools are on, the `chat` engine is `ollama` and the model supports tools, the adapter MUST send the registry's tool definitions in the `tools` field of `/api/chat` and run a bounded tool loop: when a reply carries tool calls, the app executes them through the registry (read tools directly, write tools only after approval), appends the calls and the sanitized results as tool messages, and sends a follow-up request for the same turn, up to a maximum number of rounds per turn (pinned by design). When the cap is reached the turn ends with a short system line and the partial answer is kept. Tool messages belong to the current turn only: they MUST NOT be stored, MUST NOT be part of later history and MUST NOT count toward the message caps of Context window beyond the turn. A model without tool support (the server rejects `tools`, or the model is known not to support them) MUST degrade silently to a text turn without `tools`, with the run action of pane-commands still available; the assignment MUST NOT change. The silent degrade applies ONLY to a confirmed tools refusal (see HTTP 400 disambiguation); any other HTTP 400 MUST end the turn with a visible notice and MUST NOT be retried without `tools`. (Previously: "A model without tool support (the server rejects `tools`, ...) MUST degrade silently ..." applied to every HTTP 400 of the first request.) The loop uses the same timeouts, cancel, sanitizer and answer cap as any turn. The shared loop MUST be selected per engine by wire, and the Ollama wire (path `/api/chat`, object arguments, `tool_name`, thinking echo) MUST NOT be reachable from llamacpp nor the OpenAI wire from Ollama.

#### Scenario: One round [auto]
- GIVEN a fake Ollama that answers the first request with a `find_dir` tool call and the second with text
- WHEN a turn runs
- THEN the app executes `find_dir`, sends a second request holding the call and its result, and the answer is delivered with one `ToolCall`/`ToolResult` pair

#### Scenario: Round cap [auto]
- GIVEN a fake Ollama that always answers with a tool call
- WHEN a turn runs
- THEN the loop ends at the cap with a system line and the turn returns to idle

#### Scenario: Model without tools [auto]
- GIVEN a fake Ollama that rejects the `tools` field for the model with a tools-refusal message
- WHEN a turn runs
- THEN a second request without `tools` is made, the text answer streams, and the assignment is unchanged

#### Scenario: Other 400 is not a tools refusal [auto] [S2]
- GIVEN a fake Ollama that answers the first request with HTTP 400 and a body that is not a tools refusal
- WHEN a turn runs
- THEN exactly one request is made, the turn ends with a visible notice, and no request without `tools` is sent

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

#### Scenario: Wires are not mixed [auto]
- GIVEN recording fakes for Ollama and llamacpp running the same tool turn
- WHEN both requests are recorded
- THEN Ollama's go to `/api/chat` with object arguments and `tool_name`, llamacpp's go to `/v1/chat/completions` with string arguments and `tool_call_id`

#### Scenario: Live Ollama tools [manual]
- GIVEN Ollama with a tool-capable model assigned to `chat`
- WHEN the user asks to find `curso-rust` and go there
- THEN a `find_dir` row, a confirm and, after `y`, the typed `cd` appear, and the engine and model are unchanged

### Requirement: Ollama round limit with coding tools [S3]
The Ollama tool loop MUST allow at most 8 rounds per turn when no coding-tools family is on, and at most 24 rounds per turn when at least one family toggle (search, write, exec, git, git write) is on and tools are on. A built-in model profile (see Per-model profiles) MAY replace those numbers for its model family, but only with a value that a before and after table of `local-model-eval` supports (raises the score without lowering any combination); with no profile row the numbers above apply unchanged. (Previously: the numbers 8 and 24 applied to every model with no profile override.) The limit of calls per reply MUST NOT change. When the limit is reached the turn MUST end with the existing round-limit behaviour, and a staged batch MUST stay staged (see code-write).

#### Scenario: Default stays 8 [auto]
- GIVEN tools on and every family toggle off
- WHEN an Ollama turn keeps asking for tools
- THEN the loop stops after 8 rounds

#### Scenario: Raised to 24 [auto]
- GIVEN tools on and the Search toggle on
- WHEN an Ollama turn keeps asking for tools
- THEN the loop stops after 24 rounds

#### Scenario: Tools off [auto]
- GIVEN tools off and a family toggle on
- WHEN the request is built
- THEN it is identical to a request without tools and the limit is irrelevant

#### Scenario: Limit leaves the batch [auto]
- GIVEN a staged batch and a turn that hits the limit
- WHEN the turn ends
- THEN the batch is still staged

#### Scenario: Profile limit applies only to its family [auto] [S3]
- GIVEN a profile row that sets a round limit for one model family and a model of another family
- WHEN each runs a turn that keeps asking for tools
- THEN the first stops at the profile limit and the second at 8 or 24 as before

### Requirement: Loopback-only streaming client [S3]
Server adapters MUST use the project's hand-written HTTP client extended with POST. Requests MUST be HTTP/1.1 with a JSON body, sent only to `127.0.0.1` on the configured port of the chosen server engine (the catalog port unless `[ai.servers.<id>] port` sets another one). The client MUST refuse any other host or address, MUST NOT follow redirects, MUST NOT send `Authorization`, `Cookie` or any credential, and MUST NOT read any credential or config file of any engine. The request body MUST contain only the model, the system prompt, the history, the streaming flag and, when tools are on and the engine supports them (see Ollama native tools), the tool definitions; the Ollama adapter additionally carries `options.num_ctx` (when the window is known) and `keep_alive` (see Ollama context and keep-alive options), and, only when the model's built-in profile sets it (see Reasoning control per model), `think`, and no other field. (Previously: "...and `keep_alive` ..., and no other field." with no `think`.) The system prompt MAY include the workspace context block (see prompt-profiles); that block is the only environment or location information a request carries, with one exception: the tool messages of the current turn (the model's own tool calls and the sanitized, capped results returned by the registry, which may hold paths and file contents inside `$HOME`, see agent-tools). A request MUST NEVER otherwise carry an absolute path, an environment variable, file contents or terminal output. A reply MUST be accepted only with an HTTP/1.x status 200 and the same strict framing rules as ai-engine-detection (conflicting or signed Content-Length, Content-Length with Transfer-Encoding and malformed chunking are rejected). The body is decoded incrementally (chunked, Content-Length or close-delimited) and MUST NOT be buffered whole. A non-200 reply MUST become an error event carrying the status code and a short sanitized message, never the full body. The total text of one answer MUST be capped at 1 MiB; beyond the cap the stream ends with an error.

Existing scenarios of this requirement are unchanged, including "Body content" (a model with no profile row sends no `think`, so its body stays exactly as described there). One scenario is added:

#### Scenario: `think` only from a profile [auto] [S3]
- GIVEN a model with no profile row, and a model whose profile sets `think` off
- WHEN each request is recorded
- THEN the first body holds no `think` field and the second holds `think` false, and nothing else differs from the Body content scenario

### Requirement: Reasoning events [S3]
When an engine streams its reasoning separately from the answer, the adapter's decoder MUST emit it as thinking deltas, distinct from text deltas, in arrival order. The app keeps reasoning only until the first answer text of the turn: a thinking delta that arrives after the first non-empty text delta MUST be dropped without an error (by the stream pipe and again by the chat state), so reasoning never appears after the answer began. Mapping per adapter (design pins the exact field names from captured fixtures, never assumed):
- Ollama: a streamed message carrying a separate `thinking` field yields a thinking delta for that field and a text delta for `content`; when one line carries both, the thinking delta comes first. A turn sent with `think` off (see Reasoning control per model) carries no `thinking` field, produces no thinking delta and shows no reasoning line, exactly like a model with no reasoning. (Added by S3.)
- claude (stream-json): `thinking` content blocks yield thinking deltas. This applies only if the pinned flags make the CLI emit them (verified by manual check M2); otherwise the adapter emits none.
- codex and gemini: a reasoning field or block is mapped only if that engine exposes one in its captured fixtures ("if available"); otherwise the adapter emits none and nothing changes for it. (OpenAI-compatible servers: see `ai-chat-openai-compat`.)
Empty reasoning fragments MUST yield no event. The request sent to the engine MUST NOT change because of reasoning: reasoning text is never sent back to any engine as history (see Context window), and the adapters' request bodies and argv stay as pinned elsewhere unless a manual check proves a request option is required; the one request option allowed for Ollama is `think` from a built-in profile (see Reasoning control per model). (Previously: no request option was allowed.) Reasoning text MUST be capped at 256 KiB per turn (design to confirm): thinking deltas beyond the cap are dropped without an error, and the answer cap is unaffected. A thinking delta is stream activity: it counts as the first byte and resets the idle timeout (see Timeouts).

Existing scenarios of this requirement are unchanged, except that "Request unchanged by reasoning" holds for a model with no profile row. One scenario is added:

#### Scenario: Turn with `think` off shows no reasoning [auto] [S3]
- GIVEN a model whose profile sets `think` off and a fake server that honours it (no `thinking` field)
- WHEN a turn runs
- THEN the consumer sees text deltas and one final event, no thinking event, and the history of the next turn is unchanged

### Requirement: A next step announced in prose gets one reminder [S4]
When a reply of the Ollama or llama.cpp tool loop holds no structured tool call and no text call, and the last sentence of its text announces an action the model is about to take (one of the whole-word phrases `I'll`, `I will`, `I need to`, `I'm going to`, `I am going to`, `let me`, `next, I`, `next step`, `voy a`, `vamos a`, `ahora voy`, `a continuación`, `lo siguiente`, `procedo a`, `procederé`, followed by a word other than `be`, `help`, `wait`, `know`, `summarize`, `explain`, `ayudar`, `esperar`, `explicar`, `resumir` or `dar`), the loop MUST send one more request in the same turn. That request MUST carry the reply as plain assistant text, then a user message: `Your answer says what you will do next, but you made no tool call, so nothing happened. If a step the user asked for is still undone, call its tool now. If everything is done, reply with a one-line summary of what the tool results show.` The pane MUST get the system line `the model said what it would do next without a tool call; it was asked to do it`. An answer that ends with a question, or whose last sentence is an offer to the user (the whole words `if you`, `let me know`, `do you want`, `would you like`, `si quieres`, `si lo deseas`, `si te parece`), or a summary or courtesy such as `Now I have created fibonacci.py and the tests pass.` or `I'll be happy to help with anything else.`, MUST NOT get it. This MUST happen at most once per turn, apart from the text call reminder, MUST count toward the turn's request cap, and MUST NOT happen in the retry without tools (#439). The end-of-turn step check (see End-of-turn step check) shares this one reminder: a turn that already got the announced-step, empty-answer or unfinished reminder gets no step-check reminder, and the reverse. (Added by S4.)

#### Scenario: One reminder [auto]
- GIVEN tools on and a reply that ends "I'll need to create a `test_fibonacci.py` file next."
- WHEN the loop reads the reply
- THEN a second request carries the text and the reminder, with tools still offered, and the call the model makes after it runs

An empty answer (only whitespace) MUST get the same treatment with the user message `Your answer was empty. If a step the user asked for is still undone, call its tool now. If everything is done, reply with a one-line summary of what the tool results show.` and the system line `the model sent an empty answer; it was asked to go on`; the empty answer and the announced step share the one reminder of the turn. So MUST an answer whose last sentence, not a question, starts with the whole words `ready for`, `ready to`, `listo para`, `lista para` or `listos para`, with the user message `You stopped before finishing. If a step the user asked for is still undone, call its tool now. If everything is done, reply with a one-line summary of what the tool results show.` and the system line `the model stopped before finishing; it was asked to go on`.

#### Scenario: Empty answer [auto]
- GIVEN tools on, a tool round, then an empty reply
- WHEN the loop reads the empty reply
- THEN one more request carries the empty-answer reminder, and an announcement after it ends the turn

#### Scenario: Second announcement, question or offer [auto]
- GIVEN a model that announces a step twice, or a reply that ends with a question or an offer, or a turn without tools
- WHEN the loop reads it
- THEN the turn ends with `Done`, after two requests for the second announcement and after one otherwise

#### Scenario: One reminder shared with the step check [auto] [S4]
- GIVEN a turn whose first reply announced a step and got the reminder, and whose next reply claims a commit that no tool result shows
- WHEN the loop reads the second reply
- THEN no further reminder is sent and the turn ends with `Done` after two requests

## ADDED Requirements

### Requirement: HTTP 400 disambiguation [S2]
The Ollama adapter MUST tell a tools refusal apart from every other HTTP 400 by reading the status and the sanitized, capped error text of the reply, never by the status alone. A tools refusal is a 400 whose error text says that the model does not support tools (the exact phrases are pinned by design from captured Ollama replies). Only a confirmed tools refusal MAY keep the silent retry of the same turn without `tools` (the existing behaviour of "Model without tools"). The refusal is decided per turn, as the loop does today: nothing is remembered for later turns or for the session, and a per-session memory is out of scope for this change. Any other 400 (including a 400 because of `think`, `options`, a malformed body or an unknown model) MUST end the turn with an error event carrying the status and the short sanitized message, shown in the pane as a visible notice; it MUST NOT be retried without `tools` and MUST NOT be hidden. A 400 whose body cannot be read or parsed MUST be treated as "not a tools refusal". No other status code changes behaviour. The error text MUST follow "No content in logs": it MUST NOT echo the user's message. This requirement MUST be in force before any request carries `think` (C6).

#### Scenario: Tools refusal still degrades silently [auto]
- GIVEN a fake Ollama that answers 400 with the message that the model does not support tools
- WHEN a turn runs
- THEN a second request without `tools` is made, the text answer streams, no error notice is shown and nothing is remembered for later turns (the refusal is decided per turn)

#### Scenario: Other 400 shows a notice [auto]
- GIVEN a fake Ollama that answers 400 with a message about an invalid option
- WHEN a turn runs
- THEN the turn ends with an error event holding `400` and a short sanitized message, no second request is made

#### Scenario: Unreadable 400 body is not a refusal [auto]
- GIVEN a fake Ollama that answers 400 with an empty body, and another with a body that is not JSON
- WHEN each turn runs
- THEN each ends with the visible notice and no retry without `tools`

#### Scenario: Notice does not echo the message [auto]
- GIVEN a turn with the message `SECRET-TOKEN-123` and a 400 that is not a tools refusal
- WHEN the error event and the logs are inspected
- THEN neither contains `SECRET-TOKEN-123`

#### Scenario: Long error body is capped [auto]
- GIVEN a 400 with a 1 MiB body
- WHEN the turn runs
- THEN the notice is at most 200 characters

### Requirement: Per-model profiles [S3]
The Ollama adapter MUST resolve a built-in profile for the model of the turn from a table in code (C7), keyed by model family: the model name with its namespace removed, and the family is the exact name before the first `:` (equality, not a prefix), so `qwen3:14b` and `qwen3:14b-nothink` resolve by the family `qwen3`, while `qwen3-coder:30b` does not; a row may also name the full model name when it needs it (an exact row wins over a family row). A profile MAY set: a system prompt addendum (appended after the pinned prompt of the conversation's profile, counted in the history budget like any head text), a round limit (see Ollama round limit with coding tools), `think` (see Reasoning control per model) and the use of the reminders of this spec. A model with no row MUST get exactly today's generic behaviour, byte for byte. A row MUST be added only with a before and after table of `local-model-eval` that supports every value it sets (C2); a row that later lowers a combination MUST be removed or changed. There is no `config.toml` override in this change (C7): a model name in the config never changes a profile, and the profile table is not read from any file. Profiles apply to Ollama only; llama.cpp keeps today's behaviour.

#### Scenario: No row, no change [auto]
- GIVEN a model with no profile row
- WHEN a request is built
- THEN it equals the request built before this requirement, byte for byte

#### Scenario: Family and exact match [auto]
- GIVEN rows for the family `qwen3` and for the exact name `qwen3:14b-nothink`
- WHEN `qwen3:14b`, `qwen3:14b-nothink`, `qwen3-coder:30b` and `llama3:8b` are resolved
- THEN the first gets the family row, the second the exact row, the third and the fourth none (the family is the name before `:`, not a prefix)

#### Scenario: Addendum counted in the head [auto]
- GIVEN a profile with a prompt addendum
- WHEN the context is built
- THEN the addendum is in the system message after the pinned prompt and its characters are subtracted from the history cap

#### Scenario: Profile is not configurable [auto]
- GIVEN a `config.toml` that names a model and a profile field
- WHEN the config loads and a turn runs
- THEN the profile is unchanged and no field of it is read from the file

#### Scenario: Seed values are justified [manual] [S3]
- GIVEN the PR that adds or changes a profile row
- WHEN it is reviewed
- THEN it attaches a before and after table that raises the score and lowers no combination of the accepted set

### Requirement: Reasoning control per model [S3]
`think` MUST be decided per model by a profile row (C6), never as a global default: reasoning off is NOT a default. A profile MAY set `think` to false (reasoning off), true or leave it unset; an unset value MUST send no `think` field. The adapter MUST send `think` only for a model whose profile sets it, as a top-level boolean of the `/api/chat` body (see Loopback-only streaming client). The first rows with `think` set MUST be justified by a comparison from `local-model-eval` (for example `qwen3:14b` against `qwen3:14b-nothink`). A request with `think` that gets an HTTP 400 MUST be handled by HTTP 400 disambiguation: a 400 for `think` is not a tools refusal, shows the visible notice and MUST NOT fall back silently. This requirement MUST NOT be applied before HTTP 400 disambiguation is in force. A turn sent with `think` off MUST keep every other behaviour of this spec (timeouts, sanitizer, reminders) unchanged.

#### Scenario: Unset sends nothing [auto]
- GIVEN a profile that leaves `think` unset, and a model with no profile
- WHEN each request is recorded
- THEN neither body holds `think`

#### Scenario: Set sends a boolean [auto]
- GIVEN a profile with `think` false and another with `think` true
- WHEN each request is recorded
- THEN the bodies hold `"think": false` and `"think": true` at the top level and no other new field

#### Scenario: Rejected `think` is visible [auto]
- GIVEN a model whose profile sets `think` and a fake Ollama that answers 400 about `think`
- WHEN a turn runs
- THEN the turn ends with the visible notice, no request is retried without `think` or `tools`, and nothing is hidden

#### Scenario: Off is never the default [auto]
- GIVEN every model name that has no profile row
- WHEN requests are built
- THEN none carries `think`

### Requirement: End-of-turn step check [S4]
Before the Ollama or llama.cpp tool loop ends a turn that offered tools and ended on a reply with no structured tool call, the loop MUST compare what the user asked or what the answer claims against the tool exchanges of the turn ("claimed vs shown"). When the answer claims a step that no tool result of the turn shows (a file created, a test run, a commit made), or when the user's request names a step that no exchange shows and the answer does not ask the user for something it could decide itself, the loop MUST send one more request in the same turn, carrying the reply as plain assistant text and then a user message that names the missing step and tells the model to call its tool now, or to say plainly that it did not do it. The pane MUST get a system line saying that the model claimed or left a step undone and was asked to go on. The check MUST share the single reminder of the turn with the announced-step, empty-answer and unfinished reminders (see "A next step announced in prose gets one reminder"), MUST count toward the turn's request cap, MUST NOT happen in the retry without tools, MUST NOT happen in a turn without tools, and MUST NOT run a tool or turn any text into a call. The check MUST be a pure function of the user's request, the answer text and the exchanges (no model call, no I/O). A clarifying question is never a reason for the check to fire, and the check MUST NOT fire for an answer that ends with a question or that is a refusal the tools produced. The slice is accepted only by the acceptance rule of `local-model-eval` (C1, C2): if the numbers do not rise, the slice is not shipped.

#### Scenario: Claimed commit not shown [auto]
- GIVEN tools on and a reply that says the commit was made while no commit exchange exists in the turn
- WHEN the loop reads the reply
- THEN one more request carries the check reminder naming the commit, tools still offered, and the call the model makes after it runs

#### Scenario: Requested pytest not run [auto]
- GIVEN a request that asks to run pytest and a reply with no exchange that ran it, without a question
- WHEN the loop reads the reply
- THEN one more request carries the check reminder naming the test run

#### Scenario: Everything shown [auto]
- GIVEN a reply that claims a file and a commit and exchanges that show both
- WHEN the loop reads it
- THEN the turn ends with `Done` after one request

#### Scenario: Shares the single reminder [auto]
- GIVEN a turn that already got a reminder of the "next step" requirement, and a later reply that fails the check
- WHEN the loop reads it
- THEN no further reminder is sent and the turn ends after two requests

#### Scenario: Question or no tools [auto]
- GIVEN a reply that ends with a question, and a turn without tools
- WHEN the loop reads each
- THEN neither gets the check reminder

#### Scenario: Never turns text into a call [auto]
- GIVEN a reply whose text holds a JSON call to the commit tool and no commit exchange
- WHEN the loop reads it
- THEN no tool runs from that text and at most the one reminder is sent

#### Scenario: Measured acceptance [manual] [S4]
- GIVEN the S4 PR
- WHEN it is reviewed
- THEN it attaches a before and after table of the accepted set that raises the score and lowers no combination below 4 of 5
