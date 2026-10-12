# Chat Engines Specification

## Purpose

How a chat turn reaches a model and how the answer comes back: the `ChatEngine` contract, the choice of engine for the `chat` task, the loopback-only streaming client, the stream parsers, the text-only CLI adapters, timeouts, cancel, and the stateful output sanitizer. Everything here is local: servers are reached on `127.0.0.1` only and the CLIs run on this machine with the user's own login. The exact CLI flags and output formats are a design decision (verified against each CLI, never assumed); the scenarios below are behavioural and design pins the flags. Tags: [auto] = `cargo test` (fake servers on ephemeral ports, fake CLI executables that record their argv); [manual] = real machine.

## ADDED Requirements

### Requirement: ChatEngine contract
Every adapter MUST implement one contract: it receives a model name, the system prompt and the ordered message history, and it produces events in order: zero or more content events, each either a text delta (part of the answer) or a thinking delta (part of the engine's reasoning, a distinct event type that is never a text delta), then exactly one terminal event (final or error). An engine or model that exposes no reasoning MUST simply never produce a thinking delta. It MUST support cancel, after which it produces no more events. The contract MUST NOT expose tool calls. Adapters exist for `ollama`; for `lmstudio`, `llamacpp` and `vllm` (OpenAI-compatible); and for `claude`, `codex` and `gemini` (CLIs). `opencode` has no adapter. Each adapter is usable as soon as it exists; an engine without an adapter resolves to the unsupported state.

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

### Requirement: Engine resolution for chat
The engine of a chat turn MUST come from the effective assignment of the `chat` task (its own assignment, else `general`; see ai-task-models) combined with the detection result. The result MUST be exactly one of: ready (engine and model), unassigned, undetected (with the status), unsupported, no jail (`codex` or `gemini` on a system without a working jail, see CLI jail), gated off (a CLI whose manual gate failed), or model problem (no model for a server, or a model that is not detected). Resolution MUST be pure, MUST be evaluated on every send, and MUST NEVER change the assignment or pick another engine.

#### Scenario: Ready server [auto]
- GIVEN `chat` is `ollama` / `llama3:8b`, ollama is `available` and lists `llama3:8b`
- WHEN resolved
- THEN the result is ready with that engine and model

#### Scenario: Falls back to general [auto]
- GIVEN `chat` is unassigned and `general` is `claude` / `sonnet`, claude `available`
- WHEN resolved
- THEN the result is ready with `claude` / `sonnet`

#### Scenario: Each guidance result [auto]
- GIVEN (a) nothing assigned, (b) `chat` ollama and ollama `not installed`, (c) `chat` opencode `available`, (d) `chat` codex available but gated off, (e) `chat` ollama with an empty model, (f) `chat` ollama / `x` not listed, (g) `chat` codex available on a system whose jail probe is unavailable
- WHEN each is resolved
- THEN they are unassigned, undetected, unsupported, gated off, model problem, model problem, no jail respectively

#### Scenario: Never switches [auto]
- GIVEN `chat` is `ollama` / `llama3:8b`, ollama is `not running` and claude is `available`
- WHEN resolved
- THEN the result is undetected for ollama and the assignment of `chat` is unchanged

#### Scenario: Ollama latest equivalence [auto]
- GIVEN `chat` is `ollama` / `llama3` and ollama lists `llama3:latest`
- WHEN resolved
- THEN the result is ready

#### Scenario: Re-resolved on every send [auto]
- GIVEN a ready chat and then the menu saves a different `chat` engine
- WHEN the next message is sent
- THEN the new engine is used for that message and the old one is not contacted

### Requirement: Loopback-only streaming client
Server adapters MUST use the project's hand-written HTTP client extended with POST. Requests MUST be HTTP/1.1 with a JSON body, sent only to `127.0.0.1` on the catalog port of the chosen server engine. The client MUST refuse any other host or address, MUST NOT follow redirects, MUST NOT send `Authorization`, `Cookie` or any credential, and MUST NOT read any credential or config file of any engine. The request body MUST contain only the model, the system prompt, the history and the streaming flag: no environment, cwd, file contents or terminal output. A reply MUST be accepted only with an HTTP/1.x status 200 and the same strict framing rules as ai-engine-detection (conflicting or signed Content-Length, Content-Length with Transfer-Encoding and malformed chunking are rejected). The body is decoded incrementally (chunked, Content-Length or close-delimited) and MUST NOT be buffered whole. A non-200 reply MUST become an error event carrying the status code and a short sanitized message, never the full body. The total text of one answer MUST be capped at 1 MiB; beyond the cap the stream ends with an error.

#### Scenario: Traffic audit [auto]
- GIVEN a recording fake server on each engine port
- WHEN one turn runs against each server engine
- THEN each recorded request is a `POST` to its chat path on `127.0.0.1` with no `Authorization`/`Cookie` header, and nothing else connected

#### Scenario: Body content [auto]
- GIVEN a turn with a system prompt and two messages
- WHEN the request is recorded
- THEN the JSON body holds the model, the system prompt, the two messages and the streaming flag, and nothing else

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

### Requirement: Stream parsers
Ollama replies are NDJSON (one JSON object per line); OpenAI-compatible replies (lmstudio, llamacpp, vllm) are SSE. The parsers MUST be pure and incremental: feeding the same bytes in any chunking, including one byte at a time, MUST produce the same events as feeding them whole. For NDJSON, a line with a text fragment yields a delta, a line marked done yields the final event, and a line with an `error` yields an error event. For SSE, `data:` events with a text fragment yield deltas, a finish marker or `data: [DONE]` yields the final event, and comment lines (starting with `:`) and empty events are ignored. A malformed JSON line or event MUST yield an error event, not a panic. A single line or event over 1 MiB (2 MiB for the CLI decoders, because the final `result` event of a CLI repeats the whole answer) MUST yield an error event. UTF-8 characters split across chunks MUST be reassembled before decoding.

#### Scenario: Ollama NDJSON [auto]
- GIVEN lines with fragments `Hel`, `lo` and a final line with done true
- WHEN parsed
- THEN the events are delta `Hel`, delta `lo`, final

#### Scenario: OpenAI SSE [auto]
- GIVEN events with fragments `Hel`, `lo`, a finish marker and `data: [DONE]`
- WHEN parsed
- THEN the events are delta `Hel`, delta `lo` and exactly one final

#### Scenario: Every split point [auto]
- GIVEN a valid NDJSON stream and a valid SSE stream
- WHEN each is cut at every byte position into two chunks
- THEN the events always equal the unsplit result

#### Scenario: UTF-8 split across chunks [auto]
- GIVEN a fragment containing a 4-byte character split between two chunks
- WHEN parsed
- THEN the delta contains the character intact, with no replacement character

#### Scenario: SSE comments and CRLF [auto]
- GIVEN a stream with `: keep-alive` lines and CRLF line endings
- WHEN parsed
- THEN the comments are ignored and the events are the same as with LF

#### Scenario: Malformed data [auto]
- GIVEN a line `{"message":` followed by a newline
- WHEN parsed
- THEN an error event is produced and nothing panics

#### Scenario: Server error object [auto]
- GIVEN a line `{"error":"model not found"}`
- WHEN parsed
- THEN an error event is produced whose message is sanitized and capped

#### Scenario: Oversized line [auto]
- GIVEN a single line of 1 MiB plus one byte for a server decoder, and one of 2 MiB plus one byte for a CLI decoder
- WHEN parsed
- THEN an error event is produced in each case

#### Scenario: CLI decoder accepts a long result event [auto]
- GIVEN a CLI `result` event line of 1.5 MiB
- WHEN parsed by a CLI decoder
- THEN no error is produced by the line cap

### Requirement: Reasoning events
When an engine streams its reasoning separately from the answer, the adapter's decoder MUST emit it as thinking deltas, distinct from text deltas, in arrival order. The app keeps reasoning only until the first answer text of the turn: a thinking delta that arrives after the first non-empty text delta MUST be dropped without an error (by the stream pipe and again by the chat state), so reasoning never appears after the answer began. Mapping per adapter (design pins the exact field names from captured fixtures, never assumed):
- Ollama: a streamed message carrying a separate `thinking` field yields a thinking delta for that field and a text delta for `content`; when one line carries both, the thinking delta comes first.
- claude (stream-json): `thinking` content blocks yield thinking deltas. This applies only if the pinned flags make the CLI emit them (verified by manual check M2); otherwise the adapter emits none.
- OpenAI-compatible servers, codex and gemini: a reasoning field or block is mapped only if that engine exposes one in its captured fixtures ("if available"); otherwise the adapter emits none and nothing changes for it.
Empty reasoning fragments MUST yield no event. The request sent to the engine MUST NOT change because of reasoning: reasoning text is never sent back to any engine as history (see Context window), and the adapters' request bodies and argv stay as pinned elsewhere unless a manual check proves a request option is required. Reasoning text MUST be capped at 256 KiB per turn (design to confirm): thinking deltas beyond the cap are dropped without an error, and the answer cap is unaffected. A thinking delta is stream activity: it counts as the first byte and resets the idle timeout (see Timeouts).

#### Scenario: Ollama thinking field [auto]
- GIVEN NDJSON lines whose message holds `thinking` `Let me`, then `thinking` ` see`, then `content` `Hi`, then a final line with done true
- WHEN parsed
- THEN the events are thinking delta `Let me`, thinking delta ` see`, text delta `Hi`, final

#### Scenario: Late reasoning after the answer began is dropped [auto]
- GIVEN a stream that delivered the thinking delta `first`, then the text delta `ok`, then the thinking delta `late`, then a final event
- WHEN the events pass through the stream pipe and the chat state
- THEN the app shows the reasoning `first` and the answer `ok` only, `late` is dropped, no error is produced and the turn ends normally

#### Scenario: Ollama line with both fields [auto]
- GIVEN one NDJSON line whose message holds `thinking` `x` and `content` `y`
- WHEN parsed
- THEN the events are thinking delta `x` then text delta `y`

#### Scenario: Ollama without a thinking field [auto]
- GIVEN the NDJSON lines of a model with no reasoning (`content` only)
- WHEN parsed
- THEN no thinking event is produced and the events equal those of the existing Ollama NDJSON scenario

#### Scenario: Empty reasoning fragment [auto]
- GIVEN a line whose `thinking` is the empty string and whose `content` is `Hi`
- WHEN parsed
- THEN the only event is text delta `Hi`

#### Scenario: claude thinking blocks [auto]
- GIVEN claude stream-json fixtures captured by M2 that contain `thinking` content blocks and text blocks
- WHEN parsed
- THEN thinking blocks yield thinking deltas, text blocks yield text deltas, and the final event is unchanged; if the M2 fixtures hold no thinking blocks, this scenario is replaced by one asserting no thinking event is produced

#### Scenario: Other adapters only if available [auto]
- GIVEN the OpenAI-compatible, codex and gemini fixtures
- WHEN parsed
- THEN a thinking delta is produced only where a fixture of that engine contains a reasoning field, and no format is assumed for the others

#### Scenario: Reasoning split points [auto]
- GIVEN a valid Ollama NDJSON stream that contains `thinking` fields
- WHEN it is cut at every byte position into two chunks
- THEN the events always equal the unsplit result

#### Scenario: Reasoning cap [auto]
- GIVEN a turn whose thinking deltas total more than 256 KiB, followed by text and a final event
- WHEN the turn runs
- THEN the thinking deltas beyond the cap are dropped without an error and the answer and final event are delivered intact

#### Scenario: Request unchanged by reasoning [auto]
- GIVEN a conversation whose earlier answers carried reasoning
- WHEN the next request is built for any adapter
- THEN the body or argv and stdin contain no reasoning text

### Requirement: Timeouts
A server turn MUST use separate timeouts: connect 2 s, first byte 120 s (a cold model load can be slow), and idle 60 s between chunks once streaming. A CLI turn MUST use first byte 120 s and idle 90 s. Every turn MUST also end after a total of 15 minutes. Each timeout MUST end the turn with a distinct error event (cannot connect, no answer, stalled, too long) and release its resources (socket closed or child killed). When no first byte has arrived after 3 seconds the runtime MUST emit a `Slow` event, which the pane shows as `loading model…`. A thinking delta counts as the first byte and as stream activity: it stops the `Slow` timer, satisfies the first-byte timeout and resets the idle timeout.

#### Scenario: Connect refused [auto]
- GIVEN nothing listens on the engine port
- WHEN a turn runs
- THEN an error event arrives at once saying the engine could not be reached

#### Scenario: First byte slower than the idle timeout [auto]
- GIVEN a fake server that answers after the idle timeout but before the first-byte timeout (time injected)
- WHEN a turn runs
- THEN the answer is delivered and no timeout error occurs

#### Scenario: Stalled stream [auto]
- GIVEN a server that sends one delta and then nothing for longer than the idle timeout
- WHEN the turn runs
- THEN the delta is delivered, then a stalled error event, and the socket is closed

#### Scenario: CLI timeouts [auto]
- GIVEN a fake CLI that prints nothing, and another that prints one delta and then stalls (time injected)
- WHEN each turn runs
- THEN the first ends with a no-answer error after 120 s, the second with a stalled error after 90 s, and the child group is killed

#### Scenario: Total time cap [auto]
- GIVEN a server (and a fake CLI) that keeps sending a delta every 10 s (time injected)
- WHEN 15 minutes pass
- THEN the turn ends with a too-long error event and resources are released

#### Scenario: Thinking counts as activity [auto]
- GIVEN a turn whose only events for 100 seconds are thinking deltas arriving every 10 seconds, with injected time
- WHEN the clock advances
- THEN no first-byte, idle or `Slow` event is produced because of those 100 seconds

#### Scenario: Slow event at 3 seconds [auto]
- GIVEN a send with no first byte (time injected)
- WHEN 2.9 s and then 3 s pass
- THEN no `Slow` event is emitted at 2.9 s and exactly one at 3 s

### Requirement: Cancel releases the engine
Cancelling a server turn MUST close the socket, so the server sees the connection end and no further bytes are read. Cancelling a CLI turn MUST kill the child process and its process group and reap it, within 1 second, leaving no zombie. Cancel MUST work during the connect, first-byte and streaming phases.

#### Scenario: Server sees the close [auto]
- GIVEN a fake server streaming slowly
- WHEN cancel is requested
- THEN the fake server observes the connection closing within 1 second

#### Scenario: CLI child is killed [auto]
- GIVEN a fake CLI that prints text slowly and ignores SIGTERM
- WHEN cancel is requested
- THEN the child and its group are gone and reaped within 1 second

#### Scenario: Cancel before first byte [auto]
- GIVEN a fake server that never answers
- WHEN cancel is requested
- THEN the connection is closed and no event is delivered afterwards

### Requirement: CLI adapters are text-only
The `claude`, `codex` and `gemini` adapters MUST launch the CLI in a mode where it cannot execute tools, run commands or edit files. Design MUST verify, against each CLI's real `--help` and output, the flags and the output format that give that guarantee, and MUST pin them; if a CLI cannot be made tool-less safely, that adapter MUST be gated off (resolution `gated off`) instead of shipping. Flags are a first layer only; the guarantee that a CLI has the same reach as an API connection comes from the CLI jail (see below). The CLI MUST be started directly (no shell, no `sh -c`), with each argument a separate argv element, stdout and stderr piped, and no credential, token or key added to its arguments or environment by lazarobox. The conversation (system prompt for CLIs without a system-prompt flag, and every message) MUST travel on stdin, which is written and then closed; argv MUST NEVER hold a message or history. lazarobox MUST NOT open for reading any credential or config file of the CLI; the CLI uses the user's own login. Holding a directory handle, or a handle on the claude config file, only to grant the jail access to it is not reading. The prompt text and the model name MUST be passed so they can never be read as an option (a message or model starting with `-` is data). Model `default` MUST mean no model option. Text is streamed to the pane as the CLI produces it. A non-zero exit, a launch failure or empty output MUST end the turn with an error event carrying the CLI's sanitized, capped stderr summary. The conversation context for each turn MUST be the system prompt plus the capped history (see Context window); a CLI session of its own is not used.

#### Scenario: Tools are disabled in argv [auto]
- GIVEN a fake `claude`, a fake `codex` and a fake `gemini` that record their argv
- WHEN a turn runs with each
- THEN each recorded argv contains the design-pinned options that disable tools for that CLI

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

#### Scenario: Failure becomes an error [auto]
- GIVEN a fake CLI that prints `boom` and an escape sequence on stderr and exits 1
- WHEN a turn runs
- THEN an error event carries `boom` with the escape sequence removed

#### Scenario: Gated off [auto]
- GIVEN an adapter whose tool-less mode is flagged unavailable
- WHEN the engine is resolved
- THEN the result is gated off and no process is started

#### Scenario: A write request produces no file [manual]
- GIVEN each real CLI installed and logged in
- WHEN the message asks it to create a file in the current directory
- THEN the answer is text only and no file is created

### Requirement: CLI jail
On Linux, a CLI MUST run inside a Landlock jail applied by a re-exec helper (`__jail-exec`) before the CLI is executed. The jail has two goals: a CLI MUST NOT read, write or list the user's files outside its own state, and a CLI MUST NOT be able to execute any program other than its own interpreter, its own package and the helper binaries proven necessary. This gives a CLI connection the same guarantee as an API connection: text only. The jail MUST be applied in a single-threaded helper (never in `pre_exec`), MUST require full enforcement of every right of the probed ABI (Landlock ABI 3 or newer), and a partial or absent enforcement MUST count as not jailed: the helper exits 125 with `jail not enforced` and never executes the CLI.

Filesystem rules. Read and execute (rx) is granted only to the CLI's interpreter binary (for example the canonical node path), the CLI's own package root, and the specific helper binaries a manual check proves necessary, each by exact path and never as a whole directory. `/usr`, `/bin`, `/lib` and `/lib64` MUST be read-only with NO execute right (shared libraries still load, because mapping needs only read). Read-only is also granted to `/etc`, the minimal `/proc` and `/sys/fs/cgroup` entries, and `/dev/urandom`, `/dev/random`; `/dev/null` is read-write. The private empty cwd is read-only and a private per-turn `TMPDIR` is read-write. Each CLI's own state directory is read-write (claude `~/.claude/` and `~/.claude.json`; codex `$CODEX_HOME` else `~/.codex/`; gemini `~/.gemini/`). Everything else, including `$HOME`, projects, `~/.ssh`, `/tmp` and other CLIs' directories, is denied. The network is not restricted, because the CLIs need their APIs. Paths MUST be resolved by existence checks and directory handles, never by reading a credential file.

Policy. `codex` and `gemini` are jail-required: without a working jail they MUST NOT start and resolve to `no jail`, shown as `<engine>: not available on this system yet`. A helper exit of 125 becomes a jail error. `claude` is jail-layered: jailed when the jail is available, and run with its flags alone when it is not. Any OS without a backend (everything except Linux for now) is `Unavailable`; Ollama, the OpenAI-compatible servers and `claude` keep working there.

If a CLI cannot work without a general shell or a wider execute right, its slice is GATED (`gated off`) and the user is told; the jail is never widened to a whole directory.

#### Scenario: Landlock probe [auto]
- GIVEN a probe reporting ABI 2, and another reporting ABI 6
- WHEN the jail support is derived
- THEN the first is Unavailable and the second is Available with its ABI

#### Scenario: Jail spec for an nvm layout [auto]
- GIVEN the resolved paths of a node CLI installed under an nvm directory
- WHEN the jail spec is built
- THEN the rx set holds exactly the canonical node binary and the CLI package root, `/usr`, `/bin`, `/lib` and `/lib64` are read-only and not in the rx set, and no path under `$HOME` other than the CLI's own package and state appears

#### Scenario: No path to ssh keys [auto]
- GIVEN any CLI layout
- WHEN the jail spec is built
- THEN `~/.ssh`, `$HOME` itself and `/tmp` are in no rule

#### Scenario: Jailed read outside the allow-list fails [auto, tests/jail.rs]
- GIVEN the jail helper started with an allow-list that does not contain a temporary `outside` directory
- WHEN the jailed program reads `outside/secret`
- THEN the read fails with EACCES and the exit status is not zero; with the directory added as read-only the file is printed

#### Scenario: Write rules [auto, tests/jail.rs]
- GIVEN a read-only directory and a read-write directory in the allow-list
- WHEN the jailed program writes in each
- THEN the write to the read-only directory fails with EACCES and the other succeeds

#### Scenario: Execve of system programs is denied [auto, tests/jail.rs]
- GIVEN the jail helper with `/usr`, `/bin`, `/lib` and `/lib64` read-only and no execute right, and one exact helper binary granted
- WHEN a jailed program attempts to execve `/usr/bin/ssh`, `/usr/bin/curl` and `/bin/sh`
- THEN each attempt fails with EACCES, while the granted binary runs

#### Scenario: Shared libraries still load [auto, tests/jail.rs]
- GIVEN the granted binary is dynamically linked against libraries under `/usr/lib` or `/lib`
- WHEN it runs inside the jail
- THEN it starts without a loader error

#### Scenario: Partial enforcement refuses to run [auto, tests/jail.rs]
- GIVEN the helper started with a test-only minimum ABI above the kernel's
- WHEN it runs
- THEN it exits with status 125, prints `jail not enforced` and never executes the program

#### Scenario: No jail, no codex or gemini [auto]
- GIVEN a jail probe that is Unavailable and `chat` assigned to `codex`
- WHEN a message is sent
- THEN no process is started and the guidance reads `codex: not available on this system yet`

#### Scenario: Claude runs bare without a jail [auto]
- GIVEN a jail probe that is Unavailable and `chat` assigned to `claude`
- WHEN a message is sent
- THEN the CLI is started directly with its flags and no helper

#### Scenario: Helper exit 125 is a jail error [auto]
- GIVEN a fake helper that exits with status 125
- WHEN a turn runs
- THEN the turn ends with a jail error event

#### Scenario: A jailed CLI cannot run ssh or curl [manual]
- GIVEN a jailed `codex` or `gemini` with network access
- WHEN the message asks it to `ssh` to a host or to `curl` a URL
- THEN it cannot do it, no connection is made by those programs, and the answer is text only

### Requirement: Context window
Each turn MUST send the pinned system prompt followed by the most recent messages that fit a client-side cap, never splitting a message and always including the message just sent. The cap is the last 40 messages and a per-engine character cap: 12,000 characters for local servers (a default 4k-token context would otherwise silently drop the system prompt) and 100,000 characters for CLIs. Messages beyond the cap MUST be omitted oldest first, the system prompt MUST never be dropped, and the pane MUST show the truncation marker (see chat-pane). System lines are never included, and neither is reasoning: only the user and assistant message texts are history.

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

#### Scenario: Character cap for a server [auto]
- GIVEN 10 messages of 5,000 characters each and a local server engine
- WHEN the context is built
- THEN the oldest messages are omitted until the total is at most 12,000 characters and the newest message is present

#### Scenario: Character cap for a CLI [auto]
- GIVEN 40 messages of 5,000 characters each and a CLI engine
- WHEN the context is built
- THEN the oldest messages are omitted until the total is at most 100,000 characters and the newest message is present

#### Scenario: Newest message alone exceeds the cap [auto]
- GIVEN a newest message of 40,000 characters and a local server engine
- WHEN the context is built
- THEN the newest message is still sent whole, with the system prompt, and older messages are omitted

### Requirement: Output sanitizer
All text coming from an engine (text deltas, thinking deltas, error messages, stderr summaries) MUST pass through a stateful, stream-aware sanitizer before it is stored, drawn or copied. The sanitizer MUST remove:
- C0 control characters except newline and tab, DEL, and carriage return (CRLF becomes LF; a lone CR is dropped);
- C1 controls U+0080 to U+009F;
- complete escape sequences with their payload: CSI, OSC (ended by BEL or ST), DCS, APC, PM and SOS (ended by ST), and two-character ESC sequences;
- bidi controls (U+061C, U+200E, U+200F, U+202A to U+202E, U+2066 to U+2069) and other format characters (zero-width characters U+200B to U+200D, U+2060, U+FEFF, soft hyphen U+00AD, and the tag characters U+E0000 to U+E007F).
State MUST carry across chunks: an escape sequence or a UTF-8 character split across any two chunks MUST be handled exactly as if whole. Invalid UTF-8 MUST become U+FFFD, and an incomplete character at the end of the stream too. At the end of the stream an unterminated sequence MUST be dropped. An unterminated string sequence (OSC, DCS, APC, PM, SOS) MUST NOT swallow more than 4096 characters; beyond that it is abandoned and normal text resumes. The output MUST never contain ESC or any C0 or C1 control other than newline and tab.

#### Scenario: Control characters [auto]
- GIVEN the input `a\u{07}b\u{00}c\u{7f}d\u{9b}e\r\nf\rg`
- WHEN sanitized
- THEN the output is `abcde\nfg`

#### Scenario: CSI and OSC [auto]
- GIVEN `x\u{1b}[31mred\u{1b}[0m` and `y\u{1b}]0;title\u{07}z` and `y\u{1b}]8;;http://e\u{1b}\\link`
- WHEN each is sanitized
- THEN the outputs are `xred`, `yz` and `ylink`

#### Scenario: DCS, APC, PM, SOS [auto]
- GIVEN `a\u{1b}P1$r0m\u{1b}\\b\u{1b}_payload\u{1b}\\c\u{1b}^pm\u{1b}\\d\u{1b}Xsos\u{1b}\\e`
- WHEN sanitized
- THEN the output is `abcde`

#### Scenario: Bidi and format characters [auto]
- GIVEN text with U+202E, U+2066, U+200B, U+FEFF and U+00AD between letters
- WHEN sanitized
- THEN only the letters remain

#### Scenario: Sequence split across chunks [auto]
- GIVEN `\u{1b}[31mred` and `\u{1b}]0;title\u{07}`
- WHEN each is cut at every position into two chunks and sanitized chunk by chunk
- THEN the output equals the unsplit result

#### Scenario: UTF-8 split across chunks [auto]
- GIVEN a 3-byte and a 4-byte character cut at every internal byte boundary
- WHEN sanitized chunk by chunk
- THEN the characters arrive intact and no U+FFFD appears

#### Scenario: Invalid UTF-8 [auto]
- GIVEN bytes `a\xffb` and a stream ending after the first byte of a 3-byte character
- WHEN sanitized
- THEN the outputs are `a\u{fffd}b` and a trailing U+FFFD

#### Scenario: Unterminated sequence at the end [auto]
- GIVEN a stream `ok\u{1b}]0;never ended`
- WHEN the stream finishes
- THEN the output is `ok`

#### Scenario: Unterminated OSC cap [auto]
- GIVEN `\u{1b}]0;` followed by 5000 letters and no terminator, then `tail`
- WHEN sanitized
- THEN no ESC is in the output and `tail` is present

#### Scenario: Fuzz invariant [auto]
- GIVEN random byte streams cut at random points
- WHEN sanitized
- THEN the output never contains ESC, a C1 control, a bidi control or a C0 control other than newline and tab, and chunking never changes the output

#### Scenario: Reasoning is sanitized [auto]
- GIVEN thinking deltas `a\u{1b}[31mb`, `c\u{07}d` and `e\u{1b}]0;t\u{07}f`
- WHEN each is sanitized by the same sanitizer rules as answer text
- THEN the outputs are `ab`, `cd` and `ef`

#### Scenario: Reasoning sequence split across chunks [auto]
- GIVEN a thinking stream `x\u{1b}[3` followed by `1my`
- WHEN sanitized chunk by chunk
- THEN the output is `xy`, and an unterminated sequence left in the reasoning stream never alters the answer text that follows (reasoning and answer use separate sanitizer state)

### Requirement: No content in logs
Prompts, messages, system prompts, engine answers and engine reasoning MUST NOT be written to any log, trace, panic message or error text. Error events MUST describe the failure without echoing the user's message.

#### Scenario: Error text does not echo the prompt [auto]
- GIVEN a turn with the message `SECRET-TOKEN-123` whose engine fails
- WHEN the error event and the logs are inspected
- THEN neither contains `SECRET-TOKEN-123`

### Requirement: Real engines
Each shipped adapter MUST stream a reply from the real engine.

#### Scenario: Ollama and OpenAI-compatible servers [manual]
- GIVEN Ollama running with a model and, separately, a running lmstudio, llama.cpp or vLLM
- WHEN `chat` is assigned to it and a message is sent
- THEN the reply streams into the pane and cancel stops it

#### Scenario: Cold model load [manual]
- GIVEN an Ollama model that is not loaded in memory
- WHEN a message is sent
- THEN the pane shows `waiting for the model…` then `loading model…` and no timeout error occurs while it loads

#### Scenario: Live reasoning model [manual]
- GIVEN an Ollama reasoning model (one that streams a `thinking` field) assigned to `chat`
- WHEN a message is sent
- THEN the reasoning streams dimmed in the pane, collapses when the answer starts, and the statusline shows the thinking indicator throughout; a non-reasoning model shows no reasoning line

#### Scenario: CLI engines [manual]
- GIVEN `claude`, `codex` and `gemini` installed and logged in
- WHEN each is assigned to `chat` in turn and a message is sent
- THEN each streams a text reply and no credential file is touched by lazarobox; codex and gemini run jailed

#### Scenario: Jailed claude keeps its login [manual]
- GIVEN a logged-in `claude` under the jail
- WHEN a message is sent
- THEN it answers and its token refresh still works; if the jail breaks claude, claude runs layered-bare and the README says so

#### Scenario: Helper binaries are proven by strace [manual]
- GIVEN a jailed `codex` and a jailed `gemini` turn traced with strace
- WHEN the traced `execve` calls are listed
- THEN every helper binary found is added to the allow-list by exact path (never a directory); if a CLI cannot work without a general shell, its slice is GATED and the user is told
