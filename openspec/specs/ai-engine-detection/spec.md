# AI Engine Detection Specification

## Purpose

Finds which AI engines exist on the machine: four local model servers and four agent CLIs. Detection is read-only and local. It never sends a prompt. Tags: [auto] = `cargo test` (fake servers on ephemeral ports, fake PATH directories); [manual] = real machine.

## Requirements

### Requirement: Engine catalog
The catalog MUST be fixed and ordered as follows. Servers are probed over HTTP on `127.0.0.1` only. A server's binary name is used only for the PATH lookup and is never executed.

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
The hand-written HTTP client MUST accept only an HTTP/1.x status line with status 200 as a valid reply; any other status or version is invalid and a redirect is never followed. The body MUST NOT exceed 1 MiB in any framing (Content-Length, chunked or close-delimited). Chunked framing MUST be strict (hex sizes, CRLF terminators, a final zero chunk). A reply with ambiguous framing MUST be rejected: conflicting Content-Length values, a signed Content-Length, or Content-Length together with Transfer-Encoding. A reply MUST carry at most one Transfer-Encoding header, and its final coding MUST be `chunked` or `identity`.

#### Scenario: Only HTTP/1.x status 200 is accepted [auto]
- GIVEN replies with an HTTP/2 or malformed status line, a signed status, or status 301
- WHEN each is parsed
- THEN none is a valid reply and no redirect is followed

#### Scenario: Body over the cap is rejected in every framing [auto]
- GIVEN a body of 1 MiB plus one byte framed by Content-Length, by chunks (one or several) or by close
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

### Requirement: Concurrency and timeouts
All 8 probes MUST run concurrently and in the background; the UI and the input MUST never wait on them. Each server probe MUST finish (success, failure or timeout) within 800 ms, covering connect, request and response; each `--version` run MUST finish or be killed within 2 s. The whole run therefore ends within about 2 s. The results of a run MUST be delivered together, as ONE `Detection` containing all 8 engines, once every probe has finished; a single run never delivers partial results.

#### Scenario: Hanging server [auto]
- GIVEN `:11434` accepts the connection but never answers, and all other engines answer at once
- WHEN detection runs
- THEN exactly one `Detection` with all 8 engines is delivered after the 800 ms server timeout, ollama is `not running` or `not installed` in it, the other seven engines carry their real statuses, and total time is below 2.5 s

#### Scenario: UI stays responsive [auto]
- GIVEN detection is in flight with a hanging server
- WHEN key events arrive
- THEN they are handled without waiting for the probe

### Requirement: Triggers and overlapping runs
Detection MUST start at app startup, each time the config menu opens, and on the refresh key in the AI section. At most ONE run is in flight at a time (single flight): a trigger that arrives while a run is in flight MUST be a no-op and request nothing. Until the first `Detection` arrives every engine has status `detecting`; while a later run is in flight engines keep their last known status. When a run completes, all engine statuses update at once.

#### Scenario: Three triggers [auto]
- GIVEN the app starts, and each earlier run has completed before the next trigger
- WHEN the app starts, then the menu is opened, then `r` is pressed on an AI row
- THEN exactly one new detection run is requested each time

#### Scenario: Trigger while a run is in flight is a no-op [auto]
- GIVEN a run is in flight (requested, `Detection` not yet delivered)
- WHEN the menu is opened, or `r` is pressed on an AI row, or startup is requested again
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
Detection MUST send only `GET` requests with no body and no credentials to the 4 catalog endpoints on `127.0.0.1`. It MUST NOT connect to any other host or port. It MUST execute only `<id> --version` for the 4 CLI ids, never a server binary, and MUST NOT open any credential, token or config file of any engine.

#### Scenario: Traffic audit [auto]
- GIVEN a recording fake server per endpoint
- WHEN a full detection run completes
- THEN each recorded request is `GET` of the catalog path with no `Authorization`/`Cookie` header and no body, and nothing else connected

#### Scenario: Real machine [manual]
- GIVEN Ollama running with models, `claude`, `codex`, `gemini` installed, `opencode` only an alias, an unrelated service on `:8080`
- WHEN detection runs
- THEN ollama is `available · N models`, the three CLIs are `available` with versions, opencode is `not installed`, llamacpp is not `available` (the foreign service is not counted)

### Requirement: Documented limits
The user documentation MUST state that custom endpoints and ports are not supported in this phase and that shell aliases are not detected (the binary must be on PATH).

#### Scenario: Docs mention limits [manual]
- GIVEN the README
- WHEN it is read
- THEN it names both limits
