# Delta for ai-engine-detection

## ADDED Requirements

### Requirement: Ollama model size and tool capability
For every model Ollama lists, detection MUST record its parameter size (from `details.parameter_size` of `GET /api/tags`) and a tri-state tool capability: `tools`, `no tools` or `unknown`. The capability MUST come from the `capabilities` list of a `POST /api/show` with body `{"model": "<name>"}` for that model, never from the `capabilities` of `/api/tags` (verified on Ollama 0.30.10: tags can omit `tools` while show reports it). `tools` means the list contains `tools`; `no tools` means the list was read and does not contain it; `unknown` means anything else (probe timeout, connection error, non-200 reply, oversized or malformed body, missing `capabilities` field as on an older Ollama, call limit reached). A failed probe MUST NOT fail the detection run, MUST NOT change the engine status or the model list, and MUST NOT delay the run beyond a fixed total bound. A size that is missing or unparsable is `unknown`. The probe MUST be bounded by a per-call timeout, a response body cap, a limit on parallel calls and a limit on total calls per run. The result MUST be cached in memory by model digest, so an unchanged model is not probed again on later runs; the cache is never written to disk.

#### Scenario: Tools declared [auto]
- GIVEN a fake Ollama listing `llama3:8b` (`8.0B`) whose `/api/show` reply has `capabilities` `["completion","tools"]`
- WHEN a detection run completes
- THEN the model is recorded with size 8B and capability `tools`

#### Scenario: Tools not declared [auto]
- GIVEN a fake Ollama whose `/api/show` reply for `gemma:2b` has `capabilities` `["completion"]`
- WHEN a detection run completes
- THEN the model is recorded with capability `no tools`

#### Scenario: Old Ollama without capabilities [auto]
- GIVEN a fake Ollama whose `/api/show` reply has no `capabilities` field
- WHEN a detection run completes
- THEN the model capability is `unknown` and the engine status and model list are unchanged

#### Scenario: Probe timeout or failure [auto]
- GIVEN a fake Ollama whose `/api/show` never answers for one model, returns 500 for a second and returns a malformed body for a third
- WHEN a detection run completes
- THEN the engine is still `available · N models`, the three capabilities are `unknown`, and the run ended within the fixed total bound

#### Scenario: Oversized reply [auto]
- GIVEN a fake Ollama whose `/api/show` reply is larger than the body cap
- WHEN a detection run completes
- THEN that model capability is `unknown` and no more than the cap was read

#### Scenario: Call limits [auto]
- GIVEN a fake Ollama listing more models than the total call limit
- WHEN a detection run completes
- THEN no more `/api/show` calls than the total limit were recorded, never more than the parallel limit at once, and the remaining models are `unknown`

#### Scenario: Digest cache [auto]
- GIVEN a completed detection run and the same models and digests on the next run
- WHEN the next run completes
- THEN no `/api/show` call is recorded for the unchanged digests, and a model with a new digest is probed

#### Scenario: Size missing [auto]
- GIVEN a model whose `/api/tags` entry has no `details.parameter_size`
- WHEN a detection run completes
- THEN its size is `unknown`

## MODIFIED Requirements

### Requirement: Read-only and localhost-only
Detection MUST send only `GET` requests with no body and no credentials to the 4 server endpoints on `127.0.0.1`, each on its configured port (the catalog port unless `[ai.servers.<id>] port` sets another one), with one exception: for Ollama it MAY also send `POST /api/show` (see Ollama model size and tool capability). That POST carries only the model name as a JSON body, no credentials, goes to the Ollama loopback port only, and is bounded by a timeout, a body cap, a parallel call limit, a total call limit and the digest cache. Detection MUST NOT send any other POST. It MUST NOT connect to any other host or port. It MUST execute only `<id> --version` for the 4 CLI ids, plus, for codex only, `codex debug models` through the jail (Codex model listing). That run may refresh the catalog from OpenAI with the user's own login and update codex's own cache. Detection never executes a server binary, and MUST NOT open any credential, token or config file of any engine.

This rule describes detection. Three other uses share the client and never happen as part of a detection run:
- the chat requests: POST to the same loopback servers, and the text-only CLI runs, specified in chat-engines;
- the engram requests: GET, POST and DELETE to the memory port (see engram-client), which never touch a catalog endpoint;
- the Ollama pull of ollama-download: one `POST /api/pull` to the Ollama port, sent only after the user confirms a pull.

The client remains bound to `127.0.0.1` for all four uses, and a POST is refused for any path other than the chat paths, `/api/pull` and `/api/show`. The engram client is the only user that MAY send a credential, and only `Authorization: Bearer` on a user-confirmed `DELETE` to the memory port, from `ENGRAM_HTTP_TOKEN` in the environment.

#### Scenario: Traffic audit [auto]
- GIVEN a recording fake server per endpoint
- WHEN a full detection run completes
- THEN each recorded request is either a `GET` of the catalog path or a `POST /api/show` to the Ollama fake, none has an `Authorization`/`Cookie` header, every `GET` has no body, every `POST` body holds only a model name, and nothing else connected

#### Scenario: Detection POSTs only api/show [auto]
- GIVEN a recording fake Ollama with models and no chat message sent
- WHEN detection runs
- THEN the only `POST` path recorded is `/api/show`, only on the Ollama port, within the call limits, and no prompt is sent to any server

#### Scenario: Detection stays GET-only with chat present [auto]
- GIVEN a chat pane open and recording fake servers, Ollama among them
- WHEN detection runs with no message sent
- THEN the only `POST` recorded is `POST /api/show` on the Ollama fake, no `POST` is recorded on any other server, and no prompt is sent to any server

#### Scenario: Detection stays GET-only for the other engines [auto]
- GIVEN recording fakes for lmstudio, llamacpp and vllm
- WHEN detection runs
- THEN no `POST` is recorded on any of them

#### Scenario: Chat never runs during detection [auto]
- GIVEN fake CLIs that record their argv
- WHEN a detection run completes
- THEN only `<id> --version` was executed for each CLI, except that codex was also run as `codex debug models` through the jail helper, and no other engine received any other argument

#### Scenario: The shared client stays loopback-only [auto]
- GIVEN the client asked to connect to a non-loopback host for a GET, a POST (including `/api/show`) or a DELETE
- WHEN the request is made
- THEN no connection is attempted

#### Scenario: Only the allowed POST paths [auto]
- GIVEN the client asked to POST to `127.0.0.1` on `/api/delete` or `/api/push`
- WHEN the request is made
- THEN no connection is attempted, while `/api/chat`, `/v1/chat/completions`, `/api/pull` and `/api/show` are allowed

#### Scenario: Detection never touches the memory port [auto]
- GIVEN memory active and a recording fake on the memory port
- WHEN a detection run completes
- THEN the fake recorded no connection from the run, and the detection traffic holds no credential

#### Scenario: Real machine [manual]
- GIVEN Ollama running with models, `claude`, `codex`, `gemini` installed, `opencode` only an alias, an unrelated service on `:8080`
- WHEN detection runs
- THEN ollama is `available · N models`, the three CLIs are `available` with versions, opencode is `not installed`, llamacpp is not `available` (the foreign service is not counted), and the picker opens without a visible delay
