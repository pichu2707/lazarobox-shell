# Delta for AI Engine Detection

Tags: [auto] = `cargo test` (fake servers on ephemeral ports, fake PATH directories); [manual] = real machine. The "read-only" rule of this spec now applies to detection only; the shared hand-written loopback client also serves chat requests (POST, see chat-engines). The catalog, status values, response shapes, CLI detection, sanitizing, concurrency and documented-limits requirements are unchanged. The Purpose statement "Detection is read-only and local. It never sends a prompt" still holds for detection.

## MODIFIED Requirements

### Requirement: HTTP framing is strict
The hand-written HTTP client, for detection probes and for chat requests alike, MUST accept only an HTTP/1.x status line with status 200 as a valid reply; any other status or version is invalid and a redirect is never followed. For detection probes the body MUST NOT exceed 1 MiB in any framing (Content-Length, chunked or close-delimited); chat streams are bounded by the answer cap of chat-engines instead and are decoded incrementally. Chunked framing MUST be strict (hex sizes, CRLF terminators, a final zero chunk). A reply with ambiguous framing MUST be rejected: conflicting Content-Length values, a signed Content-Length, or Content-Length together with Transfer-Encoding. A reply MUST carry at most one Transfer-Encoding header, and its final coding MUST be `chunked` or `identity`.
(Previously: stated for the detection client only, with the 1 MiB cap on every body)

#### Scenario: Only HTTP/1.x status 200 is accepted [auto]
- GIVEN replies with an HTTP/2 or malformed status line, a signed status, or status 301
- WHEN each is parsed
- THEN none is a valid reply and no redirect is followed

#### Scenario: Body over the cap is rejected in every framing [auto]
- GIVEN a detection body of 1 MiB plus one byte framed by Content-Length, by chunks (one or several) or by close
- WHEN it is parsed
- THEN it is rejected, while a chunked body exactly at the cap is accepted

#### Scenario: Chunked framing is strict [auto]
- GIVEN a chunked body with a bad size, a missing terminator or no final zero chunk
- WHEN it is parsed
- THEN it is rejected, and a well-formed chunked body is decoded

#### Scenario: Ambiguous framing is rejected [auto]
- GIVEN replies with `Content-Length: 2` and `Content-Length: 3`, with `Content-Length: 2` and `Transfer-Encoding: chunked`, or with `Content-Length: +2`
- WHEN each is parsed
- THEN each is rejected, while two equal Content-Length values are accepted

#### Scenario: Transfer-Encoding rules [auto]
- GIVEN replies with two Transfer-Encoding headers, with `chunked, gzip`, with `gzip` alone, and with an empty value
- WHEN each is parsed
- THEN each is rejected, while `gzip, chunked` and `identity` are accepted

#### Scenario: Chat replies follow the same framing rules [auto]
- GIVEN a chat reply with `Content-Length` and `Transfer-Encoding: chunked`, and another with status 301
- WHEN each is read by the chat client
- THEN both are rejected and no redirect is followed

#### Scenario: A long chat stream is not capped at 1 MiB framing [auto]
- GIVEN a well-formed chunked chat stream totalling 512 KiB
- WHEN it is read by the chat client
- THEN it is decoded incrementally without the detection body cap

### Requirement: Triggers and overlapping runs
Detection MUST start at app startup, each time the config menu opens, on the refresh key in the AI section, and each time a chat pane is opened. At most ONE run is in flight at a time (single flight): a trigger that arrives while a run is in flight MUST be a no-op and request nothing. Until the first `Detection` arrives every engine has status `detecting`; while a later run is in flight engines keep their last known status. When a run completes, all engine statuses update at once.
(Previously: three triggers, with no trigger from opening a chat pane)

#### Scenario: Four triggers [auto]
- GIVEN the app starts, and each earlier run has completed before the next trigger
- WHEN the app starts, then the menu is opened, then `r` is pressed on an AI row, then a chat pane is opened
- THEN exactly one new detection run is requested each time

#### Scenario: Trigger while a run is in flight is a no-op [auto]
- GIVEN a run is in flight (requested, `Detection` not yet delivered)
- WHEN the menu is opened, or `r` is pressed on an AI row, or startup is requested again, or a chat pane is opened
- THEN no new run is requested

#### Scenario: Completion re-arms the triggers [auto]
- GIVEN a run in flight and a trigger that was ignored
- WHEN the run's `Detection` is delivered and then a trigger arrives
- THEN exactly one new run is requested

#### Scenario: Engines show detecting until the result [auto]
- GIVEN the app started and the first run is in flight
- WHEN statuses are read, then the `Detection` arrives
- THEN every engine reads `detecting…` before it, and all 8 engines show their real statuses at once after it

#### Scenario: Failed run still answers [auto]
- GIVEN a run whose probe task panics
- WHEN the run ends
- THEN an empty `Detection` is delivered, every engine reads `unknown`, and the next trigger requests a new run (the single-flight flag is cleared)

### Requirement: Read-only and localhost-only
Detection MUST send only `GET` requests with no body and no credentials to the 4 catalog endpoints on `127.0.0.1`. It MUST NOT connect to any other host or port. It MUST execute only `<id> --version` for the 4 CLI ids, never a server binary, and MUST NOT open any credential, token or config file of any engine. This rule describes detection: the chat requests (POST to the same loopback servers, and the text-only CLI runs) are specified in chat-engines and never happen as part of a detection run. The client remains bound to `127.0.0.1` for both uses.
(Previously: stated for the whole client, which had no POST)

#### Scenario: Traffic audit [auto]
- GIVEN a recording fake server per endpoint
- WHEN a full detection run completes
- THEN each recorded request is `GET` of the catalog path with no `Authorization`/`Cookie` header and no body, and nothing else connected

#### Scenario: Detection stays GET-only with chat present [auto]
- GIVEN a chat pane open and recording fake servers
- WHEN detection runs with no message sent
- THEN no `POST` is recorded and no prompt is sent to any server

#### Scenario: Chat never runs during detection [auto]
- GIVEN fake CLIs that record their argv
- WHEN a detection run completes
- THEN only `<id> --version` was executed for each CLI

#### Scenario: The shared client stays loopback-only [auto]
- GIVEN the client asked to connect to a non-loopback host for a GET or a POST
- WHEN the request is made
- THEN no connection is attempted

#### Scenario: Real machine [manual]
- GIVEN Ollama running with models, `claude`, `codex`, `gemini` installed, `opencode` only an alias, an unrelated service on `:8080`
- WHEN detection runs
- THEN ollama is `available · N models`, the three CLIs are `available` with versions, opencode is `not installed`, llamacpp is not `available` (the foreign service is not counted)
