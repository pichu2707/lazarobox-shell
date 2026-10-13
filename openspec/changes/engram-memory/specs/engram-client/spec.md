# Engram Client Specification

## Purpose

How lazarobox talks to a local `engram serve` (v3.4.0 HTTP API): the loopback JSON exchange, the worker that keeps every call off the render thread, health and version detection, project resolution, the per-conversation session, saving notes, fetching context, deleting notes, and the user-confirmed start of `engram serve`. What the user sees and when memory is on is in chat-memory. The client is a port: a pure command/reply vocabulary in core and a thin adapter in runtime, shaped so that future model tools (`mem_save`, `mem_search`) map onto the same commands without rework. Tags: [auto] = `cargo test` (fake engram servers on ephemeral ports, recorded HTTP fixtures, fake `engram` executables on a fake PATH); [manual] = real machine with engram 3.4.0.

Terms: "memory port" = the configured loopback port (see configuration). "Engram project" = the project name returned by engram for a directory. "Note" = one engram observation created by lazarobox. "Kind" = the closed category of a note (see chat-memory). "Ready" = the server answered the health probe as a 3.x engram.

## ADDED Requirements

### Requirement: Loopback JSON exchange
The client MUST send `GET`, `POST` and `DELETE` requests, HTTP/1.1, with a JSON body only on POST, only to `127.0.0.1` on the memory port. It MUST refuse any other host or address (no connection attempted), MUST NOT follow redirects, MUST NOT send `Cookie`, and MUST NOT read any credential or config file of engram. The only credential it MAY send is `Authorization: Bearer <value of ENGRAM_HTTP_TOKEN from lazarobox's own environment>`, and only on `DELETE` (see Deleting notes); the token MUST NOT be read from any file, stored, logged or put in an error text. Query values MUST be percent-encoded so that no request line or header can contain a raw control character, space or `&` coming from a path or a title. Unlike detection and chat streams, the exchange MUST return any HTTP/1.x status (200 to 599) with its body to the caller, because engram answers 201 to a save and explains 404 and 409 in the body. Framing rules are as strict as ai-engine-detection (conflicting or signed Content-Length, Content-Length with Transfer-Encoding and malformed chunking are rejected), and the body MUST NOT exceed 256 KiB in any framing; a larger body is an error.

#### Scenario: Only loopback [auto]
- GIVEN an endpoint host that is not `127.0.0.1`
- WHEN any request is made
- THEN no connection is attempted and the call fails

#### Scenario: Statuses are returned, redirects are not followed [auto]
- GIVEN fake replies 201 with a JSON body, 404 with a JSON body, and 307 with a Location
- WHEN each is read
- THEN the first two return status and body to the caller, and the third is an error whose Location is never requested

#### Scenario: Framing and cap [auto]
- GIVEN a reply with Content-Length and Transfer-Encoding together, and another with a 256 KiB plus one byte body
- WHEN each is read
- THEN both are rejected

#### Scenario: Query values are encoded [auto]
- GIVEN a cwd `/tmp/a b&c\r\nX: y`
- WHEN the project request is built
- THEN the recorded request line has a single query value with the characters percent-encoded and no injected header

#### Scenario: Credential only on DELETE [auto]
- GIVEN `ENGRAM_HTTP_TOKEN=tok-123` in the environment and a recording server
- WHEN health, project, context, session, save and delete calls are made
- THEN `Authorization: Bearer tok-123` appears on the delete request only, and no `Cookie` header appears anywhere

### Requirement: Worker, command channel and bounded calls
All engram I/O MUST run on a dedicated worker, never on the render thread. The chat state sends memory commands as effects and receives replies as events, the same shape as the conversation store thread. Commands MUST be executed one at a time in arrival order (so a session is always registered before the save that needs it) from a bounded queue of 64; when the queue is full a command is answered at once as unavailable (`busy`). Every call MUST have a connect timeout of 200 ms and a total timeout of 300 ms for `/health`, 800 ms for reads (`/project/current`, `/context`) and 2 s for writes (sessions, observations, delete); a timeout MUST become a reply of kind `did not answer`, never a hang, so a silent engram delays the queue by at most 2 s per command. A slow, silent or closed engram MUST NOT delay key handling, drawing, or any chat stream. A reply for a pane that no longer exists MUST be dropped silently. With memory disabled or not acknowledged (see chat-memory) the worker MUST NOT be started and no request MUST be made. The token is read from the environment by the runtime only; chat state knows only whether a token is present.

#### Scenario: Silent server does not block the UI [auto]
- GIVEN a fake engram that accepts connections and never answers, and a chat stream running
- WHEN a write call is issued
- THEN keys are handled and stream deltas are drawn meanwhile, and the call ends with `did not answer` within 2 s

#### Scenario: Order and full queue [auto]
- GIVEN a scripted slow transport, a register and a save queued, and then 70 further commands
- WHEN the queue drains
- THEN the register ran before the save, and the commands over the queue size were answered `busy` at once

#### Scenario: Connect refused [auto]
- GIVEN nothing listening on the memory port
- WHEN a call is made
- THEN the reply is `down` at once and nothing panics

#### Scenario: Stale reply dropped [auto]
- GIVEN a context call in flight and its chat pane closed
- WHEN the reply arrives
- THEN it is ignored with no effect and no panic

#### Scenario: Disabled means no traffic [auto]
- GIVEN memory disabled and a recording fake on the memory port
- WHEN the app runs, a chat is opened and messages are exchanged
- THEN the fake records no connection

### Requirement: Health and version detection
Before the first memory action of a run the worker MUST probe `GET /health`. The result MUST be exactly one of: ready (HTTP 200, JSON with `service` equal to `engram`, and an `instance_id` that matches `[0-9a-f-]{1,64}` and a `capabilities` object); old server (an engram reply without `instance_id` or `capabilities`, or whose `instance_id` does not match that pattern, such as the 1.x `version` `0.1.0`); not engram (a reply that is not JSON, has another `service`, or has a non-200 status); down (connect failure or timeout). The result is cached for the run. Any transport failure MUST set the state to down; while down, commands are answered unavailable at once, and a new probe runs only on a memory action and only if at least 10 s passed since the last probe (never in a loop or a timer). A changed `instance_id` MUST clear the project cache and the set of registered sessions. The unavailable reasons are `engram is not running`, `old engram server on :<port>, restart it with 3.x` and `port <port> is not engram`.

#### Scenario: Ready 3.x [auto]
- GIVEN a recorded 3.4.0 health reply with `instance_id` and `capabilities`
- WHEN probed
- THEN the result is ready

#### Scenario: Old and foreign servers [auto]
- GIVEN the recorded 1.x reply `{"service":"engram","status":"ok","version":"0.1.0"}`, a reply of `<html>`, and a JSON reply with `service` `other`
- WHEN each is probed
- THEN they are old server, not engram and not engram

#### Scenario: Cached, then probed again after a failure [auto]
- GIVEN a ready result and two successful calls
- WHEN a call fails with a connection error, a memory action starts 5 s later, and another 11 s after the failure (injected clock)
- THEN the first two used the cache (one health request), the action at 5 s is answered unavailable with no request, and the one at 11 s sends a new health request

#### Scenario: Server replaced [auto]
- GIVEN a registered session and a new health reply with another `instance_id`
- WHEN the next save runs
- THEN the project is resolved again and the session is registered again

### Requirement: Project resolution
The engram project MUST come from `GET /project/current?cwd=<origin cwd of the chat pane>`, and every later request of that chat (session, save, context, delete) MUST pass that project explicitly. The ai-chat project tag is not used for engram. engram answers 200 for every cwd, so the reply MUST be classified by its `project_source` field and never by HTTP status. Memory is available ONLY for git projects: a `git_*` source (such as `git_remote`) with a non-empty `project` resolves the project. A source of `dir_basename` or `ambiguous`, an empty `project`, an unknown source or a reply without those fields MUST make memory unavailable for that chat with the system line `memory needs a git project`. An unknown origin cwd MUST give the same unavailable result without a request. A 404 `unknown_project` from `GET /context` is not an error: it means no memory yet. lazarobox MUST NEVER guess, derive or default a project name, and there is no project override in this change.

#### Scenario: Project from engram [auto]
- GIVEN a fake answering `{"project":"lazarobox-shell"}` for cwd `/work/lazarobox`
- WHEN the chat resolves its project
- THEN later requests carry `project=lazarobox-shell`, and the ai-chat tag of the conversation is unchanged

#### Scenario: Only git projects [auto]
- GIVEN fakes answering 200 with `project_source` `dir_basename` (project `tmp`), 200 `ambiguous` with `project:""`, and 200 `git_remote`
- WHEN each is resolved
- THEN the first two make memory unavailable for that chat with the line `memory needs a git project` and no save, session or context request is ever sent, and the third resolves

#### Scenario: Context of an unknown project [auto]
- GIVEN a fake answering 404 `unknown_project` to `GET /context`
- WHEN the context is fetched
- THEN there is no block and no unavailable line (no memory yet)

#### Scenario: No cwd [auto]
- GIVEN a chat whose origin cwd is unknown
- WHEN the project is resolved
- THEN memory is unavailable and no request is made

#### Scenario: Different chats, different projects [auto]
- GIVEN two chat panes opened from directories that engram maps to `a` and `b`
- WHEN each saves a note
- THEN each request carries its own project

### Requirement: Session lifecycle
There MUST be one engram session per conversation with the id `lazarobox-<32 lowercase hex>` (16 random bytes), created on the first write of that conversation and stored with the conversation (see chat-history, memory link tables) so a resumed conversation reuses it. It MUST be registered (`POST /sessions` with the id, the project and the origin cwd as directory) before the first write, once per server instance, and never at chat open, so a chat that never writes creates no session. Registering a session that already exists, or one that was ended, MUST be treated as resuming it and MUST NOT fail the write; this is unverified against 3.4.0 (open question E-3). When the conversation closes (pane closed, new conversation, or app quit) and a session was registered in this run, the worker MUST send `POST /sessions/{id}/end` with a summary: the approved summary when there is one (see chat-memory), otherwise `Notes: <title>; <title>` listing the titles of the notes saved in this conversation (at most 1,000 characters). The end call is best effort within the call timeout and MUST NOT delay quit by more than 1 s in total.

#### Scenario: Registered on first write only [auto]
- GIVEN a ready memory and a conversation that has not written
- WHEN the chat opens, and later a first note is saved
- THEN no session request exists before the save, and exactly one `POST /sessions` with an id matching `lazarobox-[0-9a-f]{32}` precedes the first observation

#### Scenario: Not registered twice [auto]
- GIVEN a registered session
- WHEN a second note is saved
- THEN no second registration is sent

#### Scenario: Resume reuses the id [auto]
- GIVEN a stored conversation with a session id, resumed, and a fake that answers a repeated registration with the existing session
- WHEN it saves a note
- THEN the same session id is used, the note is saved and no error is shown

#### Scenario: End with the approved summary [auto]
- GIVEN a registered session and an approved summary
- WHEN the pane is closed
- THEN `POST /sessions/<id>/end` carries that summary

#### Scenario: End with the note titles [auto]
- GIVEN a registered session with notes titled `A` and `B` and no approved summary
- WHEN the conversation closes
- THEN the end summary is `Notes: A; B`

#### Scenario: Summary sent once, latest approval wins [auto]
- GIVEN a registered session and summaries `first` then `second` approved in the same conversation
- WHEN the pane closes
- THEN no end request was sent at approval time, and exactly one `POST /sessions/<id>/end` carries `second`

#### Scenario: Chat without writes sends nothing at close [auto]
- GIVEN a conversation that read context but saved nothing
- WHEN it closes
- THEN no end request is sent

#### Scenario: Quit is not held up [auto]
- GIVEN a registered session and a fake engram that never answers
- WHEN the app quits
- THEN it exits within 1 s

### Requirement: Saving a note
A save MUST be `POST /observations` with the session id, the project, `type` equal to the kind (category), the title, the content (at most 50,000 bytes, cut on a character boundary), `scope` equal to `project` (always; there is no other scope in this change), and `topic_key` equal to `chat/<kind>/<slug(title)>` (see chat-memory). It MUST NOT request prompt capture. Only content that already passed the redaction pass of chat-memory MAY be sent. A 201 reply yields the observation id, which is recorded as a note link in chat.db (see chat-history). Any other status MUST become a short failure reply (`could not save note: <sanitized reason of at most 120 characters>`) that keeps the user's text (see chat-memory), and a `session_project_mismatch` MUST NOT be retried with another project.

#### Scenario: Request body [auto]
- GIVEN a ready memory, category `decision` and title `Use Axum`
- WHEN a note is saved
- THEN the recorded body has `type` `decision`, `scope` `project`, `topic_key` `chat/decision/use-axum`, the project and session ids, and the redacted content, and no other fields except those engram requires

#### Scenario: Same topic upserts [auto]
- GIVEN a fake that answers the same id and a higher revision for the same topic_key
- WHEN the same category and title are saved twice with different content
- THEN both calls succeed and one link exists for that id

#### Scenario: Failure is short and sanitized [auto]
- GIVEN a fake answering 400 `session_project_mismatch` with a long body containing an escape sequence
- WHEN a note is saved
- THEN the failure text is at most 120 characters and contains no control or escape character

#### Scenario: Real engram [manual]
- GIVEN engram 3.4.0 running
- WHEN a note is saved and then saved again with another content
- THEN engram shows one observation with revision count 2 under the `chat/` topic_key

### Requirement: Fetching context
At chat open (new or resumed) the worker MUST send `GET /context?project=<P>&scope=project&compact=true&observations=8&sessions=2&prompts=0&max_bytes=2000` and read `{"context": "<markdown>"}`. Detail beyond that is fetched only when a model tool asks for it (out of scope here); there MUST be no search per turn and no other automatic read. Whatever size the server returns, the formatter keeps at most 2000 bytes (see chat-memory).

#### Scenario: One context request per conversation view [auto]
- GIVEN a ready memory
- WHEN a conversation view opens and three messages are exchanged
- THEN exactly one context request is recorded and no search request

#### Scenario: Server ignores the byte limit [auto]
- GIVEN a fake returning 50,000 bytes of context
- WHEN it is fetched
- THEN the reply is accepted and the injected text is capped by the formatter

### Requirement: Deleting notes
A note MUST be deleted only through `DELETE /observations/{id}?expected_project=<P>` with the bearer credential of Loopback JSON exchange, and only after the user confirmed it (see chat-history). A 2xx, or a 404 whose body `error` is `observation not found`, counts as deleted; any other 404 is a failure. A 401 or 403 becomes `engram refused the delete (check ENGRAM_HTTP_TOKEN)`. Notes are deleted one by one, a failure does not stop the others, and the reply reports how many were deleted.

#### Scenario: Delete request [auto]
- GIVEN `ENGRAM_HTTP_TOKEN=tok` and a note link with id 7 in project `p`
- WHEN the user-confirmed delete runs
- THEN the recorded request is `DELETE /observations/7?expected_project=p` with `Authorization: Bearer tok`

#### Scenario: Already gone and refused [auto]
- GIVEN three notes where the fake answers 204, 404 `observation not found` and 403
- WHEN they are deleted
- THEN the reply reports 2 deleted of 3 and the refusal text

#### Scenario: Never deletes on its own [auto]
- GIVEN any run of the app with memory ready, including conversation retention eviction
- WHEN the recorded traffic is inspected
- THEN a `DELETE` appears only after an explicit user confirmation

### Requirement: Offer to start the server
When memory is enabled and acknowledged and the health result is `down`, lazarobox MUST offer to start `engram serve`. The offer is made at most once per run, when the first conversation view of the run opens or when the user presses a memory key while the state is down. The offer shows the absolute path of the binary that would run, so the user sees which binary it is: `engram is not running. Start "<absolute path> serve <port>"? y start · n not now` (for example `Start "/home/linuxbrew/.linuxbrew/bin/engram serve 7437"?`), and nothing is spawned until the user confirms with an unmodified lowercase `y` pressed with an empty composer; `n` or Esc (also with an empty composer) declines, and declining is remembered for the run. Any other key, a repeat, a release or a modifier key on its own is not an answer: it goes to the composer as usual and the offer stays pending and visible, and with text in the composer every key is for the composer. The binary is resolved from PATH to an absolute path by lazarobox itself and its `version` output MUST parse as major 3 or higher before the offer is made. No offer MUST be made for an old server, for a server that is not engram, when the binary is not on PATH (the reason `engram not found on PATH` is shown instead), when the binary on PATH is 1.x (the line `engram on PATH is 1.x; upgrade it` is shown), or when the project is unknown or ambiguous. A `version` output that cannot be parsed (or a binary whose version command fails or times out) counts as no binary and shows `engram not found on PATH`. A binary path that is not valid UTF-8, or that the sanitizer would change or that holds a control character, MUST NOT be offered and is treated as not found: the path shown MUST equal the path that would run. The offer is emitted by the worker once per run, at the first probe that finds nothing running (a refused connection), and not again; the chat view (M6) MUST hold it until a conversation view opens or a memory key is pressed, and show it then. Only a `y` key press confirms (a key release or a repeat of a held key neither confirms nor declines). The binary lookup is bounded at about 3 s (the `version` helper has its own 2 s deadline).

#### Scenario: Offer on a down server [auto]
- GIVEN memory enabled, nothing on the memory port and a fake `engram` on PATH reporting version 3.4.0
- WHEN the first conversation view opens
- THEN the prompt `engram is not running. Start "<absolute path of the fake> serve <port>"? y start · n not now` is shown (the path is absolute, not the bare word `engram`) and no process has been started

#### Scenario: Declined or other key [auto]
- GIVEN the offer shown
- WHEN `n`, Esc, `x` or `Y` is pressed
- THEN nothing is spawned, memory stays unavailable for the run and no second offer is made

#### Scenario: No offer for old, foreign or missing [auto]
- GIVEN an old 1.x server, a non-engram service, a down port with no `engram` on PATH, and a down port with a fake `engram` reporting 1.20
- WHEN the first conversation view opens in each case
- THEN no offer is made and the matching text (`old engram server on :<port>, restart it with 3.x`, `port <port> is not engram`, `engram not found on PATH`, `engram on PATH is 1.x; upgrade it`) is shown

#### Scenario: Offer once per run [auto]
- GIVEN the offer declined in one pane
- WHEN another chat pane opens and a memory key is pressed
- THEN no second offer is shown

### Requirement: Spawn, port race and lifecycle
After confirmation lazarobox MUST probe `GET /health` again, and if a ready server answers it MUST adopt it and spawn nothing. Otherwise it MUST run, as `serve <port>`, the exact absolute path shown in the offer (never a path resolved again) and only while that path is still the same file (device, inode, size and modification time recorded when it was found and compared at confirmation; a mismatch spawns nothing and shows `engram on PATH changed; not started`). A confirmation with no offer pending spawns nothing and shows `could not start engram: nothing was offered`. Each argument is a separate argv element, no shell, no stdin, in its own session (`setsid`), without adding or changing `ENGRAM_DATA_DIR` (so engram uses the user's own data directory) and with `ENGRAM_PORT` removed from its environment (the argument is authoritative), and with stdout and stderr appended to `engram-serve.log` under `$XDG_STATE_HOME/lazarobox/` (fallback `$HOME/.local/state/lazarobox/`), created with mode 0700 for the directory and 0600 for the file, never through a symlink (neither the directory nor the file), never blocking on a FIFO, only in a directory and file owned by the user (a looser mode on an existing directory or file is tightened, a foreign owner is refused, as for the chat store), and truncated when over 1 MiB. Reading the log for the failure line obeys the same rules. It MUST then probe health every 100 ms for up to 3 s (the whole start therefore ends within about 3 s plus one final probe deadline of 300 ms). If the child exits first, it MUST be reaped and, if the port answers with a ready server (another process won the race), that server is adopted; otherwise memory is unavailable with the exit reason. lazarobox MUST spawn at most once per run. It MUST NEVER signal or kill a server, including the one it started, not on pane close and not on quit, and it MUST show `started engram serve (pid N); it keeps running after lazarobox quits` once the server answers. When nothing answers, the line is `could not start engram` followed by the sanitized last log line (at most 120 characters). When the spawn itself fails, the line is `could not start engram` followed by a short sanitized lowercase hint of the OS error (for example `could not start engram: permission denied`), bounded like the log line.

#### Scenario: Binary replaced after the offer [auto]
- GIVEN a confirmed offer for a fake `engram` that was then replaced or rewritten at the same path
- WHEN the start runs
- THEN nothing is spawned and the line is `memory unavailable: engram on PATH changed; not started`

#### Scenario: Log path is a FIFO or a symlink [auto]
- GIVEN a FIFO or a symlink at `engram-serve.log`, or a symlinked or foreign-owned log directory
- WHEN the start runs
- THEN nothing is spawned, nothing blocks, and no file outside the log directory is touched

#### Scenario: Argv and environment [auto]
- GIVEN a fake `engram` that records argv and environment, and a confirmed offer
- WHEN it is spawned
- THEN argv is exactly `serve <port>`, no shell is involved, `ENGRAM_DATA_DIR` is not added or changed, `ENGRAM_PORT` is absent, stdin is `/dev/null`, and the child is in its own session

#### Scenario: Server appears between offer and spawn [auto]
- GIVEN a confirmed offer and a ready fake that starts listening before the second probe
- WHEN the second probe runs
- THEN nothing is spawned and the server is adopted

#### Scenario: Lost the race [auto]
- GIVEN a spawned fake that exits with a bind error while a ready server now answers
- WHEN the child exit is detected
- THEN the child is reaped, the server is adopted and no second spawn happens

#### Scenario: Spawn fails and nothing answers [auto]
- GIVEN a fake `engram` that exits 1 and no server
- WHEN the offer is confirmed
- THEN memory is unavailable with the exit reason, the chat works, and no further spawn is attempted in the run

#### Scenario: Log file modes [auto]
- GIVEN a permissive umask
- WHEN the server is started
- THEN the log directory is mode 0700 and `engram-serve.log` is mode 0600 and receives the child's output

#### Scenario: Never killed [auto]
- GIVEN a server that lazarobox started
- WHEN the pane closes and the app quits
- THEN no signal is sent to the server's pid and the PID line was shown

#### Scenario: Real start and survival [manual]
- GIVEN no engram server and engram 3.4.0 on PATH
- WHEN the offer is confirmed and lazarobox is quit
- THEN `engram serve` keeps running, answers `/health` with `instance_id`, and the log holds its output

### Requirement: No content in logs or errors
Note titles, note content, summaries, injected context and the token MUST NOT be written to any lazarobox log, trace, panic message or error text. Failure replies describe the failure without echoing the user's text.

#### Scenario: Errors do not echo content [auto]
- GIVEN a save of content `SECRET-NOTE-456` that engram rejects with 500
- WHEN the failure text and any log are inspected
- THEN neither contains `SECRET-NOTE-456`
