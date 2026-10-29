# AI Engine Detection Specification (delta: Ollama downloads)

## MODIFIED Requirements

### Requirement: Read-only and localhost-only
Detection MUST send only `GET` requests with no body and no credentials to the 4 catalog endpoints on `127.0.0.1`. It MUST NOT connect to any other host or port. It MUST execute only `<id> --version` for the 4 CLI ids, plus, for codex only, `codex debug models` through the jail (Codex model listing). That run may refresh the catalog from OpenAI with the user's own login and update codex's own cache. Detection never executes a server binary, and MUST NOT open any credential, token or config file of any engine.

This rule describes detection. Three other uses share the client and never happen as part of a detection run:
- the chat requests: POST to the same loopback servers, and the text-only CLI runs, specified in chat-engines;
- the engram requests: GET, POST and DELETE to the memory port (see engram-client), which never touch a catalog endpoint;
- the Ollama pull of ollama-download: one `POST /api/pull` to the Ollama port, sent only after the user confirms a pull.

The client remains bound to `127.0.0.1` for all four uses, and a POST is refused for any path other than the chat paths and `/api/pull`. The engram client is the only user that MAY send a credential, and only `Authorization: Bearer` on a user-confirmed `DELETE` to the memory port, from `ENGRAM_HTTP_TOKEN` in the environment.

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
- THEN only `<id> --version` was executed for each CLI, except that codex was also run as `codex debug models` through the jail helper, and no other engine received any other argument

#### Scenario: The shared client stays loopback-only [auto]
- GIVEN the client asked to connect to a non-loopback host for a GET, a POST or a DELETE
- WHEN the request is made
- THEN no connection is attempted

#### Scenario: Only the allowed POST paths [auto]
- GIVEN the client asked to POST to `127.0.0.1` on `/api/delete` or `/api/push`
- WHEN the request is made
- THEN no connection is attempted, while `/api/chat`, `/v1/chat/completions` and `/api/pull` are allowed

#### Scenario: Detection never touches the memory port [auto]
- GIVEN memory active and a recording fake on the memory port
- WHEN a detection run completes
- THEN the fake recorded no connection from the run, and the detection traffic holds no credential

#### Scenario: Real machine [manual]
- GIVEN Ollama running with models, `claude`, `codex`, `gemini` installed, `opencode` only an alias, an unrelated service on `:8080`
- WHEN detection runs
- THEN ollama is `available · N models`, the three CLIs are `available` with versions, opencode is `not installed`, llamacpp is not `available` (the foreign service is not counted)
