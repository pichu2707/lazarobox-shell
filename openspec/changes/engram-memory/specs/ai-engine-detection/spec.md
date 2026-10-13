# Delta for AI Engine Detection

Tags: [auto] = `cargo test` (fake servers on ephemeral ports); [manual] = real machine. Builds on the ai-chat delta of this spec. The shared hand-written loopback client now has a third user: the engram client (see engram-client), which reads small JSON replies of any status. Detection and chat streams are unchanged.

## MODIFIED Requirements

### Requirement: HTTP framing is strict
The hand-written HTTP client, for detection probes, chat requests and the engram JSON exchange alike, MUST apply the same strict framing rules. Detection probes and chat requests MUST accept only an HTTP/1.x status line with status 200 as a valid reply; any other status or version is invalid, and a redirect is never followed by any user of the client. The engram JSON exchange (see engram-client) MUST accept any HTTP/1.x status from 200 to 599 and hand status and body to its caller, because engram answers 201 to a save and explains 404 and 409 in the body; a 3xx status is an error and is never followed. For detection probes the body MUST NOT exceed 1 MiB in any framing (Content-Length, chunked or close-delimited); chat streams are bounded by the answer cap of chat-engines instead and are decoded incrementally; the engram JSON exchange is capped at 256 KiB. Chunked framing MUST be strict (hex sizes, CRLF terminators, a final zero chunk). A reply with ambiguous framing MUST be rejected: conflicting Content-Length values, a signed Content-Length, or Content-Length together with Transfer-Encoding. A reply MUST carry at most one Transfer-Encoding header, and its final coding MUST be `chunked` or `identity`.
(Previously: stated for detection probes and chat requests only, with no engram JSON exchange)

#### Scenario: Only HTTP/1.x status 200 is accepted [auto]
- GIVEN replies with an HTTP/2 or malformed status line, a signed status, or status 301
- WHEN each is parsed as a detection probe or a chat reply
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

#### Scenario: Engram JSON exchange reads 201 and 4xx [auto]
- GIVEN replies 201 with `{"id":7}`, 404 with `{"error":"unknown_project"}` and 301 with a Location
- WHEN each is read by the engram JSON exchange
- THEN the first two return status and body, and the third is an error whose Location is never requested

#### Scenario: Engram JSON exchange keeps the framing rules [auto]
- GIVEN a reply with `Content-Length: 2` and `Transfer-Encoding: chunked`, and a 256 KiB plus one byte body
- WHEN each is read by the engram JSON exchange
- THEN both are rejected

### Requirement: Read-only and localhost-only
Detection MUST send only `GET` requests with no body and no credentials to the 4 catalog endpoints on `127.0.0.1`. It MUST NOT connect to any other host or port. It MUST execute only `<id> --version` for the 4 CLI ids, never a server binary, and MUST NOT open any credential, token or config file of any engine. This rule describes detection: the chat requests (POST to the same loopback servers, and the text-only CLI runs) are specified in chat-engines and never happen as part of a detection run, and the engram requests (GET, POST and DELETE to the memory port, see engram-client) are never made by a detection run and never touch a catalog endpoint. The client remains bound to `127.0.0.1` for all three uses. The engram client is the only user that MAY send a credential, and only `Authorization: Bearer` on a user-confirmed `DELETE` to the memory port, from `ENGRAM_HTTP_TOKEN` in the environment.
(Previously: stated for detection and chat only, with no engram requests and no credential exception)

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
- GIVEN the client asked to connect to a non-loopback host for a GET, a POST or a DELETE
- WHEN the request is made
- THEN no connection is attempted

#### Scenario: Detection never touches the memory port [auto]
- GIVEN memory active and a recording fake on the memory port
- WHEN a detection run completes
- THEN the fake recorded no connection from the run, and the detection traffic holds no credential

#### Scenario: Real machine [manual]
- GIVEN Ollama running with models, `claude`, `codex`, `gemini` installed, `opencode` only an alias, an unrelated service on `:8080`
- WHEN detection runs
- THEN ollama is `available · N models`, the three CLIs are `available` with versions, opencode is `not installed`, llamacpp is not `available` (the foreign service is not counted)
