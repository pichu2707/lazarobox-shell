# AI Engine Detection Specification

## Purpose

Finds which AI engines exist on the machine: four local model servers and four agent CLIs. Detection is read-only and local, with one exception: for codex it also runs `codex debug models` through the jail, which may refresh the catalog from OpenAI and update codex's own cache (see Codex model listing). It never sends a prompt. Tags: [auto] = `cargo test` (fake servers on ephemeral ports, fake PATH directories); [manual] = real machine.

## Requirements

### Requirement: Engine catalog
The catalog MUST be fixed and ordered as follows. Servers are probed over HTTP on `127.0.0.1` only, on their catalog port (`ollama` 11434, `lmstudio` 1234, `llamacpp` 8080, `vllm` 8000) unless `[ai.servers.<id>] port` sets another one (see configuration, Server ports). A server's binary name is used only for the PATH lookup and is never executed.

| id | kind | endpoint (GET) | binary on PATH |
|---|---|---|---|
| `ollama` | server | `http://127.0.0.1:11434/api/tags` | `ollama` |
| `lmstudio` | server | `http://127.0.0.1:1234/v1/models` | `lms` |
| `llamacpp` | server | `http://127.0.0.1:8080/v1/models` | `llama-server` |
| `vllm` | server | `http://127.0.0.1:8000/v1/models` | `vllm` |
| `claude` | cli | none | `claude` |
| `codex` | cli | none | `codex` |
| `gemini` | cli | none | `gemini` |
| `opencode` | cli | none | `opencode` |

`/health` MUST NOT be used to decide availability, because its `{"status":"ok"}` body is not specific to llama.cpp.

#### Scenario: A configured port is probed [auto]
- GIVEN `[ai.servers.llamacpp] port` set to the port of a fake server that lists one model owned by `llamacpp`
- WHEN detection runs
- THEN llamacpp is `available` with that model, and the fake recorded one `GET /v1/models`

#### Scenario: Catalog integrity [auto]
- GIVEN the catalog
- WHEN it is validated
- THEN it has exactly the 8 ids above in that order, ids are unique, every server endpoint host is `127.0.0.1`, and every CLI has no endpoint

### Requirement: Status values
Every engine MUST have exactly one status: `detecting` (no result yet), `available`, `not running` (servers only), `not installed` or `unknown` (a detection run failed unexpectedly and produced no result). For a server: a valid endpoint response makes it `available` (even when its binary is not on PATH). Otherwise, if its binary is on PATH, it is `not running`; if not, it is `not installed`. For a CLI: `available` when its binary is on PATH, otherwise `not installed`. The display text MUST be exactly: `detecting…`, `available`, `not running`, `not installed`, `unknown`, with detail appended as ` · <detail>` on `available`: a server shows `<N> models` (`1 model` when N is 1, `0 models` when empty), and a CLI shows its version when known (`available · <version>`). Where the UI marks an engine id that is not available, it uses `(checking)` while the status is `detecting` and `(not detected)` otherwise; a saved model that the engine no longer lists is marked `<name> · not found`.

#### Scenario: Server matrix [auto]
- GIVEN a server engine
- WHEN (valid endpoint, binary absent), (valid endpoint, binary present), (no valid endpoint, binary present), (no valid endpoint, binary absent)
- THEN the status is `available`, `available`, `not running`, `not installed` respectively

#### Scenario: Display text [auto]
- GIVEN ollama returned 2 models, codex has version `0.5.0`, gemini has no version, vllm is not running
- WHEN statuses are formatted
- THEN they read `available · 2 models`, `available · 0.5.0`, `available`, `not running`; and a status with no result yet reads `detecting…`

### Requirement: Server response shapes
A server endpoint response is valid only when ALL hold: the HTTP status is 200, the body is at most 1 MiB, the body parses as a JSON object, and the shape matches. Redirects MUST NOT be followed. Chunked and content-length framing MUST both be accepted.
- `ollama`: the object has a `models` array. Each element that is an object with a string `name` yields a model; other elements are ignored.
- `lmstudio`: the object has a `data` array. Each element that is an object with a string `id` yields a model; other elements are ignored. LM Studio exposes no verified ownership marker, so only this shape is checked (an accepted false-positive risk for another OpenAI-compatible service on `:1234`).
- `llamacpp`, `vllm`: the same `data` shape, AND at least one element MUST be an object whose `owned_by` is the string `"llamacpp"` (llama.cpp) or `"vllm"` (vLLM) respectively. Ports `:8080` and `:8000` are commonly used by unrelated services, so a bare OpenAI-style list is not proof. All elements with a string `id` yield models once the marker is present.

An empty array is valid for `ollama` and `lmstudio` (available, 0 models); it is invalid for `llamacpp` and `vllm`, which need a marker entry. Model order MUST be the response order.

#### Scenario: Valid Ollama [auto]
- GIVEN `:11434/api/tags` answers 200 `{"models":[{"name":"llama3:8b"},{"name":"qwen2.5:7b"}]}`
- WHEN ollama is probed
- THEN it is `available` with models `llama3:8b`, `qwen2.5:7b`

#### Scenario: Valid OpenAI-style list [auto]
- GIVEN `:1234/v1/models` answers 200 `{"object":"list","data":[{"id":"gemma-3-4b"}]}`
- WHEN lmstudio is probed
- THEN it is `available` with model `gemma-3-4b`

#### Scenario: Valid llama.cpp and vLLM [auto]
- GIVEN `:8080/v1/models` answers 200 `{"object":"list","data":[{"id":"m.gguf","owned_by":"llamacpp"}]}` and `:8000/v1/models` answers 200 `{"object":"list","data":[{"id":"qwen","owned_by":"vllm"}]}`
- WHEN llamacpp and vllm are probed
- THEN llamacpp is `available` with model `m.gguf` and vllm is `available` with model `qwen`

#### Scenario: Foreign OpenAI-compatible service [auto]
- GIVEN `:8080/v1/models` answers 200 `{"object":"list","data":[{"id":"gpt-x","owned_by":"localai"}]}`, and `:8000/v1/models` answers the llama.cpp body above
- WHEN llamacpp and vllm are probed and `llama-server` is on PATH and `vllm` is not
- THEN llamacpp is `not running` and vllm is `not installed`, and neither counts the foreign models

#### Scenario: Foreign service answers HTML [auto]
- GIVEN `:8080/v1/models` answers 200 `<html><body>It works</body></html>`
- WHEN llamacpp is probed and `llama-server` is not on PATH
- THEN llamacpp is `not installed`

#### Scenario: Foreign service answers other JSON [auto]
- GIVEN `:8080/v1/models` answers 200 `{"status":"ok"}`, or 200 `{"data":"x"}`, or 200 `{"data":[]}`, or 200 `[]`
- WHEN llamacpp is probed and `llama-server` is on PATH
- THEN llamacpp is `not running`

#### Scenario: Non-200 or oversize [auto]
- GIVEN an endpoint answers 404, 503, 301 with a Location header, or a 2 MiB valid body
- WHEN it is probed
- THEN the response is invalid and no redirect is followed

#### Scenario: Partial entries ignored [auto]
- GIVEN `{"models":[{"name":"a"},{"tag":"b"},3]}`
- WHEN ollama is probed
- THEN it is `available` with exactly one model `a`

#### Scenario: Port closed [auto]
- GIVEN nothing listens on the endpoint port (connection refused)
- WHEN the probe runs
- THEN the result arrives well before the timeout and the status follows the binary rule

### Requirement: CLI detection
A CLI is `available` when a regular file with an execute permission bit exists under its name in a PATH directory (first match wins). Empty and relative PATH entries MUST be skipped, so the working directory is never searched. Shell aliases and functions are NOT detected. The version probe MUST run `<id> --version` directly (no shell) with stdin closed and a timeout of 2 s; on timeout the child MUST be killed. The version is the first non-empty line of stdout, trimmed. A non-zero exit, a timeout or empty output MUST leave the CLI `available` with no version.

#### Scenario: Found with version [auto]
- GIVEN a fake executable `codex` in a PATH directory that prints `codex 0.5.0` on `--version`
- WHEN codex is detected
- THEN it is `available` with version `codex 0.5.0`

#### Scenario: Absent or alias [auto]
- GIVEN no `opencode` file on PATH (even if the user's shell defines an alias)
- WHEN opencode is detected
- THEN it is `not installed`

#### Scenario: Version hangs [auto]
- GIVEN a fake `gemini` that sleeps 30 s on `--version`
- WHEN gemini is detected
- THEN it is `available` with no version within 2.5 s (2 s timeout) and the child process is killed

#### Scenario: Version fails [auto]
- GIVEN a fake `claude` that exits 1 on `--version`
- WHEN claude is detected
- THEN it is `available` with no version

#### Scenario: Relative PATH entry skipped [auto]
- GIVEN PATH contains an empty entry and `.`, and an executable `codex` exists in the working directory
- WHEN codex is detected
- THEN it is `not installed`

### Requirement: Codex model listing
When codex is `available`, detection MUST run `<codex> debug models` once and keep the models that the account can use. The command MAY refresh the catalog from OpenAI with the user's own codex login and update codex's own cache (`models_cache.json` under `$CODEX_HOME` or `~/.codex`), exactly as running codex does; that is how the list reflects the account, and `--bundled` (which skips the refresh) is deliberately not used. Because of that it MUST start the way a codex chat turn starts: through the jail helper (`__jail-exec`) with the codex layout (read-execute on its install, read-write on its state folder only, the usual read-only system folders), with the chat's scrubbed environment, stdin closed, the private working folder and temp folder, and its own process group. codex is `JailPolicy::Required`: when the jail is unavailable, or the layout cannot be jailed, no process is started and codex keeps just `default`. The run is bounded by a timeout of 4 s (the group is killed on timeout) and an output cap of 2 MiB (the real catalog printed about 560 KB with codex-cli 0.162.1; more than the cap is dropped, not truncated). A successful listing MUST be kept in memory (no files) with the time it was read and reused by every detection run for 10 minutes, at startup and on every menu open alike, without starting codex again. The menu's `r` refresh MUST bypass the cache and list again. A failed listing MUST NOT be cached as a success: it falls back to the cached listing while that is younger than 10 minutes (the failure does not extend its age), otherwise to just `default`, and the next run tries again. The `--version` probe and the other engines are not cached. The cache reads its time from an injectable clock. The cache is not keyed by the codex binary or `$CODEX_HOME`, so a switch of account or install shows up after at most 10 minutes or at once with `r`. An `r` pressed while a non-forced run is in flight MUST NOT be dropped: exactly one forced run follows that run, and the footer keeps showing `refreshing…` meanwhile. It MUST run concurrently with the `--version` probe of the same CLI, so the time a CLI adds to a detection is the longer of the two, not their sum. No other CLI is probed this way (gemini has no listing command).

The pure parser MUST read the JSON object `{"models": [...]}`, keep only entries with `visibility == "list"`, order them by `priority` ascending (stable; a missing priority sorts last), and return `(label, slug)` pairs where the label is the sanitized `display_name` (the slug when empty) and the slug is the value saved and passed to the CLI. A slug MUST be rejected unless it is made only of ASCII letters, digits and `._-:/@+`, is not empty, not `default`, does not start with `-` and is at most 128 characters; this rules out whitespace, control, bidi, zero-width and every other Unicode format or separator character. Duplicates are dropped and at most 64 models are kept. The engine state exposes the pairs as `model_labels`, and `models` is `default` followed by the slugs.

On any failure (no jail, an old codex without the subcommand, a non-zero exit, a timeout, too much output, invalid JSON, no listed model) the CLI stays `available` with just `default` and no error is shown.

#### Scenario: Listing parsed [auto]
- GIVEN the fixture `tests/fixtures/codex/debug-models.json` with nine entries, two of them `hide`
- WHEN it is parsed
- THEN seven `(label, slug)` pairs result, ordered by priority, starting with `GPT-6.1-Sol` = `gpt-6.1-sol`

#### Scenario: Invalid entries dropped [auto]
- GIVEN entries whose slug starts with `-`, holds a space, a tab, a newline, a bidi override, a zero-width or line-separator character, a no-break space or any other non-ASCII character, is empty, is `default`, is 129 characters long, is not a string, or repeats an earlier slug, plus a `hide` entry and non-object items
- WHEN parsed
- THEN none of them is returned

#### Scenario: Malformed output [auto]
- GIVEN empty text, `not json`, `[]`, `{}`, `{"models":{}}` or truncated JSON
- WHEN parsed
- THEN the result is empty

#### Scenario: Old codex fails soft [auto]
- GIVEN a fake `codex` that exits 2 on `debug models`
- WHEN codex is detected
- THEN it is `available` with models `default` only

#### Scenario: Listing runs jailed [auto]
- GIVEN a fake `codex` and a recording pass-through helper with the jail available
- WHEN the listing runs
- THEN the helper is called as `__jail-exec` with the codex state folder (or `$CODEX_HOME`) read-write and `codex debug models` after `--`

#### Scenario: No jail, no listing [auto]
- GIVEN the jail is unavailable
- WHEN codex is detected
- THEN codex is not started for the listing and keeps just `default`

#### Scenario: Listing run is contained [auto]
- GIVEN a fake `codex` that prints its cwd, whether stdin is open and `$SSH_AUTH_SOCK`
- WHEN the listing runs
- THEN the cwd is under the private data folder, stdin is at EOF, the variable is scrubbed, and a hanging run is killed with its process group after the timeout

#### Scenario: Listing reused for 10 minutes [auto]
- GIVEN a fake `codex` that counts its `debug models` runs and a fake clock
- WHEN two detection runs happen 9 min 59 s apart
- THEN codex was started for the listing once and both runs show the same models

#### Scenario: Listing re-read after 10 minutes [auto]
- GIVEN a cached listing read 10 minutes ago
- WHEN detection runs
- THEN codex is started again and the window restarts

#### Scenario: Refresh bypasses the cache [auto]
- GIVEN a fresh cached listing
- WHEN the menu's `r` triggers a detection
- THEN codex is started again

#### Scenario: Failure falls back to the cache [auto]
- GIVEN a cached listing 5 minutes old and a codex that now fails
- WHEN a refresh runs
- THEN the cached list is still used, and 5 minutes later (when it expires) the result is just `default`; with nothing cached a failure is retried on the next run

#### Scenario: Listing does not add to the version time [auto]
- GIVEN a fake `codex` whose `--version` and `debug models` each take 1 s
- WHEN codex is detected
- THEN detection finishes in under 1.8 s

#### Scenario: Output over the cap [auto]
- GIVEN a fake `codex` that prints more than the cap
- WHEN the listing runs
- THEN the result is dropped

#### Scenario: Only codex is probed [auto]
- GIVEN fake `codex` and `claude` that both print the fixture
- WHEN both are detected
- THEN codex has seven labeled models and claude has none

### Requirement: Untrusted text is sanitized and bounded
Every string taken from an engine (the first line of `--version` output and each model name from a server reply) is untrusted. Detection MUST strip ANSI CSI and OSC sequences (an OSC ends with BEL or ST), control characters, and bidi and format characters before the text is stored or shown. A version MUST be capped at 40 characters (and trimmed again after the cap), a model name at 128 characters, and a model list at 256 models per engine. A model name that is empty after sanitizing MUST be dropped. A duplicate model name MUST be offered only once in the menu.

#### Scenario: ANSI sequences are stripped [auto]
- GIVEN `--version` prints a first line with CSI colour codes, or a model name contains an OSC title sequence ended by BEL or by ST
- WHEN the text is parsed
- THEN the stored version or name contains none of those sequences or control characters

#### Scenario: Bidi and format characters are dropped [auto]
- GIVEN a model name contains a right-to-left override or another bidi or format character
- WHEN the reply is parsed
- THEN the stored name has those characters removed

#### Scenario: Version is capped [auto]
- GIVEN `--version` prints a first line longer than 40 characters
- WHEN it is parsed
- THEN the version has at most 40 characters and no trailing whitespace

#### Scenario: Model name is capped [auto]
- GIVEN a model name of 129 characters
- WHEN the reply is parsed
- THEN the stored name has 128 characters

#### Scenario: Model list is capped [auto]
- GIVEN a reply that lists 257 models
- WHEN the reply is parsed
- THEN the engine has exactly 256 models

#### Scenario: Names empty after sanitizing are dropped [auto]
- GIVEN a reply whose names are `""`, only an ANSI sequence, only a bidi character, and `ok`
- WHEN the reply is parsed
- THEN the only model is `ok`

#### Scenario: Duplicate names are offered once [auto]
- GIVEN a saved model `a` and an engine that lists `a`, `a`, `b`
- WHEN the Model row is cycled forward twice
- THEN it shows `b` and then `a`, with no repeated choice

### Requirement: HTTP framing is strict
The hand-written HTTP client, for detection probes, chat requests and the engram JSON exchange alike, MUST apply the same strict framing rules. Detection probes and chat requests MUST accept only an HTTP/1.x status line with status 200 as a valid reply; any other status or version is invalid, and a redirect is never followed by any user of the client. The engram JSON exchange (see engram-client) MUST accept any HTTP/1.x status from 200 to 599 and hand status and body to its caller, because engram answers 201 to a save and explains 404 and 409 in the body; a 3xx status is an error and is never followed. For detection probes the body MUST NOT exceed 1 MiB in any framing (Content-Length, chunked or close-delimited); chat streams are bounded by the answer cap of chat-engines instead and are decoded incrementally; the engram JSON exchange is capped at 256 KiB. Chunked framing MUST be strict (hex sizes, CRLF terminators, a final zero chunk). A reply with ambiguous framing MUST be rejected: conflicting Content-Length values, a signed Content-Length, or Content-Length together with Transfer-Encoding. A reply MUST carry at most one Transfer-Encoding header, and its final coding MUST be `chunked` or `identity`.

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

### Requirement: Concurrency and timeouts
All 8 probes MUST run concurrently and in the background; the UI and the input MUST never wait on them. Each server probe MUST finish (success, failure or timeout) within 800 ms, covering connect, request and response; each `--version` run MUST finish or be killed within 2 s, and the codex model listing within 4 s, concurrently with codex's `--version`. The whole run therefore ends within about 4 s, and within about 2 s when codex is absent or has no listing. The UI never waits for it, because detection runs in the background. The results of a run MUST be delivered together, as ONE `Detection` containing all 8 engines, once every probe has finished; a single run never delivers partial results.

#### Scenario: Hanging server [auto]
- GIVEN `:11434` accepts the connection but never answers, and all other engines answer at once
- WHEN detection runs
- THEN exactly one `Detection` with all 8 engines is delivered after the 800 ms server timeout, ollama is `not running` or `not installed` in it, the other seven engines carry their real statuses, and total time is below 2.5 s (no codex listing is involved in that run; with codex available the bound is the 4 s listing timeout)

#### Scenario: UI stays responsive [auto]
- GIVEN detection is in flight with a hanging server
- WHEN key events arrive
- THEN they are handled without waiting for the probe

### Requirement: Triggers and overlapping runs
Detection MUST start at app startup, each time the config menu opens, on the refresh key in the AI section, and each time a chat pane is opened. At most ONE run is in flight at a time (single flight): a trigger that arrives while a run is in flight MUST be a no-op and request nothing. Until the first `Detection` arrives every engine has status `detecting`; while a later run is in flight engines keep their last known status. When a run completes, all engine statuses update at once.

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
Detection MUST send only `GET` requests with no body and no credentials to the 4 server endpoints on `127.0.0.1`, each on its configured port (the catalog port unless `[ai.servers.<id>] port` sets another one). It MUST NOT connect to any other host or port. It MUST execute only `<id> --version` for the 4 CLI ids, plus, for codex only, `codex debug models` through the jail (Codex model listing). That run may refresh the catalog from OpenAI with the user's own login and update codex's own cache. Detection never executes a server binary, and MUST NOT open any credential, token or config file of any engine.

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

### Requirement: Documented limits
The user documentation MUST state that custom endpoints and hosts are not supported (the host is always `127.0.0.1`), how to set a server's port, and that shell aliases are not detected (the binary must be on PATH).

#### Scenario: Docs mention limits [manual]
- GIVEN the README
- WHEN it is read
- THEN it names the limits above

### Requirement: Availability copy per platform (S1 reasons, S4 copy)
Detection MUST keep the refusal reason by platform. On Linux the existing copy is unchanged. On macOS, when `codex` or `gemini` is refused because Seatbelt is unavailable (`sandbox-exec` missing, probe failed) the text MUST say so and MUST NOT say "needs Linux"; before S4 enablement the macOS text MUST NOT claim the CLI is usable. From S4, with an Available probe, `codex` and `gemini` on macOS MUST be `available`. `claude` availability on macOS MUST NOT depend on the jail.

#### Scenario: Seatbelt unavailable [auto]
- GIVEN macOS, `codex` installed and a probe that is Unavailable
- WHEN detection runs
- THEN codex is not usable, the copy names the missing Seatbelt enforcement, does not say "needs Linux", and no codex process is started

#### Scenario: Available after S4 [auto]
- GIVEN macOS, an Available probe, S4 applied, `codex` and `gemini` installed
- WHEN detection runs
- THEN both are `available`

#### Scenario: claude independent of jail [auto]
- GIVEN macOS, an Unavailable probe and `claude` installed
- WHEN detection runs
- THEN claude is `available`

#### Scenario: Linux copy unchanged [auto]
- GIVEN Linux and the existing jail states
- WHEN detection runs
- THEN statuses and copy equal the current ones

### Requirement: Codex model listing under Seatbelt (S3 mechanism, S4 on)
On macOS the `codex debug models` listing MUST run through the same helper path as a chat turn, jailed by Seatbelt, with the same layout, scrubbed environment, timeout and output cap as on Linux. When the jail is unavailable, or the exit is a jail error (including 71), no process runs bare and codex keeps just `default`.

#### Scenario: Listing runs under Seatbelt [auto]
- GIVEN macOS, a fake `codex` and a recording pass-through helper
- WHEN the listing runs
- THEN the helper is called as `__jail-exec` with the codex state folder read-write and `codex debug models` after `--`

#### Scenario: No Seatbelt, no listing [auto]
- GIVEN macOS and an Unavailable probe
- WHEN codex is detected
- THEN codex is not started for the listing and keeps just `default`
