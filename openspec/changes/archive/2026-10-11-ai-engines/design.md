# Design: AI Engines (Phase 1: detection and selection)

## Technical Approach

The design keeps the functional core and imperative shell split that config-menu and copy-selection already use.

- **Pure core (`core::ai`)** owns the engine catalog, the status model, the response-shape parsers (Ollama `/api/tags`, OpenAI-compatible `/v1/models`), the `--version` output parser, the HTTP/1.x response parser, the reducer that turns raw probe facts into a `Detection`, and `AiConfig`. It does no IO.
- **Runtime (`runtime::ai_detect`)** only gathers raw facts: a PATH lookup, a loopback HTTP GET and a `--version` run, each with a hard timeout. It runs as a background tokio task and sends one `Detection` back through a dedicated channel.
- **App** owns `ai: AiConfig`, the last `Detection`, and a single-flight `detecting` flag. It emits `Effect::DetectEngines` and consumes `AppEvent::EnginesDetected(Detection)`.
- **Menu** gains an optional, dynamic AI section, attached with `MenuState::with_ai`. `MenuState::open(&Config)` is unchanged, so every existing Settings test and snapshot stays byte-identical.

ADRs use the `AE-` prefix. The text is over the 800-word budget on purpose, because the orchestrator asked for 7 decision areas.

## Architecture Decisions

| # | Topic | Options | Decision / rationale |
|---|---|---|---|
| AE-1 | Module layout | everything in runtime; **pure `core::ai` + thin `runtime::ai_detect`** | **Split.** The runtime returns raw facts (`Probe::Server { id, on_path, reply: Option<(u16, Vec<u8>)> }`, `Probe::Cli { id, path: Option<PathBuf>, stdout: Option<Vec<u8>> }`). Then `ai::reduce(&[Probe]) -> Detection` classifies them. Every rule (shape checks, the status table, version trimming) is then a plain `#[test]` with byte fixtures. That includes the foreign `:8080` service. Files: `core/ai/mod.rs` (catalog, types, reduce), `core/ai/parse.rs`, `core/ai/http.rs`, `core/ai/config.rs`, `core/ai/menu.rs` (the AI menu logic, with tests in `core/menu/ai_tests.rs`). |
| AE-2 | Status rules | — | Servers: a valid shape gives `Available { models }`. Otherwise the binary on PATH gives `NotRunning`, and no binary gives `NotInstalled` (orchestrator ruling). The PATH binaries are `ollama`, `lms` (LM Studio), `llama-server` and `vllm`, and the server binaries are **never executed**. CLIs: on PATH gives `Available { version }`, where `version` is `None` if the probe fails. Not on PATH gives `NotInstalled`. Models: a server lists what the server reports, and a CLI lists `["default"]`. |
| AE-3 | Shape checks | generic `/v1/models` for all; `/health`; **owner marker where available** | HTTP 200 is required. Ollama: the object has `models: [{name: str}, ..]`, and an empty list is valid. OpenAI: the object has `data: [{id: str}, ..]`; an empty list is valid for Ollama and LM Studio. LM Studio has no verified ownership marker, so only the shape is checked (accepted false-positive risk on `:1234`). **llama.cpp and vLLM also need at least one entry with `owned_by` set to `"llamacpp"` or `"vllm"` respectively**, so an empty `data` list is invalid for them and a foreign OpenAI-compatible service on `:8080` or `:8000` (any other `owned_by`) is not counted `:8080` (LocalAI and others) and `:8000` (dev servers) are contested ports, and a bare OpenAI list there is not proof. **`/health` is not used**: `{"status":"ok"}` is generic and would bring back the false positive that D2 exists to prevent. Parsing uses `serde_json::Value`, which is declared and in the lockfile, so no new crate is added. |
| AE-4 | HTTP client | `reqwest 0.12` (declared, unused; default features pull `native-tls`/`openssl`, `hyper`, `h2`); **hand-written GET over `tokio::net::TcpStream`** | **Hand-written.** `http_get(port: u16, path: &'static str)` connects only to `SocketAddr::from(([127,0,0,1], port))`. Because there is no host parameter, **D5 holds by construction**. It sends `GET {path} HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\nConnection: close\r\n\r\n` and reads to EOF, capped at 1 MiB. HTTP/1.0 makes Go (Ollama), Node (LM Studio) and h11 (vLLM) reply with Content-Length or a close-delimited body. The pure `http::parse_response` still decodes `chunked` as a defence (about 25 lines). The whole client is about 80 lines with no TLS, and it is tested against a loopback `TcpListener`. reqwest stays declared for P5 (cloud TLS). P1 gains nothing from it and would link a TLS stack for plain loopback calls. Tradeoff: we own a tiny parser (keep-alive, redirects and TLS are deliberately unsupported). |
| AE-5 | Probes and timeouts | — | `find_on_path(name, PATH)` uses `env::split_paths`, skips empty and relative entries, and requires a regular file with an exec bit. `version(abs_path)` runs `tokio::process::Command` on the **resolved absolute path** with the arg `--version`, no shell, `stdin(null)`, `stderr(null)`, `stdout(piped)` and and `process_group(0)` (the child leads its own group), inside `timeout(VERSION_TIMEOUT = 2 s, ...)`. On a timeout or when the future is dropped, a `GroupKill` guard kills the whole group with `killpg`. On the normal-exit path, `waitid(WNOWAIT)` observes the exit while the child is still unreaped, and the group is killed before the reap, so a pid cannot be reused first. Output is read up to a 64 KiB cap, and a spawn that fails with ETXTBSY is retried (max 4). `libc` is already a dependency, so no crate is added. A non-zero exit gives no version. The pure `parse_version` takes the first non-empty stdout line, sanitized, trimmed and capped at 40 characters (see Review-driven changes). `HTTP_TIMEOUT = 800 ms` covers connect, write and read. All 8 probes run concurrently (`futures::future::join_all`), so the worst case is about 2 s (the 800 ms HTTP probes end earlier; the spec bound of 2 s for `--version` wins over the earlier 3 s). PATH is read once per detection. |
| AE-6 | Async integration | spawn inside `App`; **injected detector in `Panes`, dedicated channel** | `Effect::DetectEngines` → `Panes::apply` calls `(self.detect)()`. This is a `Box<dyn FnMut()>` set by `Panes::with_detector`, and the default is a no-op, so existing `Panes` tests are untouched. In production it calls `spawn_detection(ai_tx, path)`: an inner task runs `detect_all`, and an outer task awaits it and sends `inner.await.unwrap_or_default()`. A panicked probe therefore still answers, with an empty `Detection` whose statuses show as `unknown`. `run_loop` gets an `ai_rx` select arm → `step(app, AppEvent::EnginesDetected(d))`. **Single flight in App:** `request_detection()` emits the effect only when `!detecting` and then sets the flag. `EnginesDetected` clears it. Requests made while a detection is in flight are dropped, because the running one is at most about 2 s old. Triggers: startup (`run()` applies `app.request_detection()` before the loop), menu open, and `r`. The UI thread never waits. |
| AE-7 | `AiConfig` and parsing | add strings to `Config`; **separate non-`Copy` struct** | `AiConfig { engine: Option<String>, model: Option<String> }` (`Clone, Eq, Default`). `Config` stays `Copy`. `ai::config::read(&Table, &mut problems)` is called from `Config::parse`, and `Parsed` gains `ai`. A missing key or `""` gives `None`. A non-string value gives `ai.engine: expected a string`, and an `[ai]` that is not a table gives `ai: expected a table`, both through the existing notice path. Unknown ids are **kept verbatim** (D7). `Config::load` keeps its signature as a wrapper over the new `Config::load_with_ai(read) -> (Config, AiConfig, Option<String>)`, so existing tests do not change. The runtime calls `load_with_ai` and then `App::with_ai`. |
| AE-8 | Persisting owned strings | `String` fields; separate `AiEdit` path in `apply_edit`; **`value: Cow<'static, str>`** | **Cow.** `table` and `key` stay `&'static` (`"ai"`, `"engine"`, `"model"`), so `EditError` is unchanged. Only `value` is now owned or borrowed. `Cow::Borrowed` is const-compatible, so `const STATUSLINE_TOP` and `const fn edit` still compile. `ConfigEdit` loses `Copy` and keeps `Clone`. The ripple is about 8 lines: `Value::from(edit.value.as_ref())` and `.into()` at the menu and test sites. `None` is written as `""`, so no key removal is needed. Writing goes through the same atomic `save_config`. |
| AE-9 | Menu model | rewrite `SECTIONS` dynamically; **static Settings + optional `AiMenu`** | `MenuState { .., ai: Option<AiMenu> }` where `AiMenu { original: AiConfig, draft: AiConfig, engines: Option<Detection> }` (in `core/ai/menu.rs`). `open(&Config)` stays the same and gives no AI section. `with_ai(ai, engines)` attaches it (two arguments; the pending state is derived from `engines: None`). The selection index covers the static choices and then the 2 AI choices (Engine, Model). The AI section always has **a fixed 3 rows** (Engine, Model, Status), so a detection update (`set_engines`) never moves the selection. The selection index covers the static choices and then only Engine and Model; the Status row is display-only and, like the disabled Mouse row, is skipped by navigation (wrap included). `Row` changes `value: Option<&'static str>` to `Option<Cow<'static, str>>` and loses `Copy`. The test helper `row()` converts, so the existing assertions keep working. |
| AE-10 | AI rows | — | **Engine** choices, in order: `none`; the catalog ids whose status is `Available` (catalog order); then the saved id and the current draft id when not already offered (not available, or not in the catalog). Unavailable catalog engines are not offered as choices. The value is `none` or the id, with the suffix `(checking)` while its status is `detecting…` (no result yet) and `(not detected)` otherwise. Changing the engine resets the model to the engine's first model, or to `None`; `none` clears both. **Model** cycles the engine's models (a CLI offers `["default"]`). A non-empty draft model not in the list is appended as `name · not found`, so it is shown and never silently erased. With no models and an empty draft it shows `none`, cannot be cycled, and returns `None`. **Status** is a display-only row (muted, skipped by navigation) that shows the draft engine's status text: `detecting…`, `available · N models` (`1 model`, `0 models`), `available · <version>`, `available`, `not running`, `not installed`, `unknown` (a failed detection, see AE-6), `not detected` (a saved id not in the catalog) or `-` (no engine). `r` (Press only, selection on an AI row, i.e. Engine or Model) returns `MenuCommand::Refresh`. On `EnginesDetected` the draft is never changed. Only the choices and labels are rebuilt. |
| AE-11 | Save and cancel | — | `edits()` returns the static edits followed by `ai.engine` and `ai.model` (always both, `None` as `""`) when either differs from the original. On `ConfigSaved` the App copies `menu.ai_draft()` into `App.ai` before `close_menu`. Cancel discards the draft. AI changes have no live preview, so there is nothing to revert. |
| AE-12 | Popup geometry | — | Height = `rows + 4` (borders, blank row, footer). Width is 62 (`WANT_WIDTH_AI`) with the AI section and 51 (`WANT_WIDTH`) without it. Without AI the popup is still 51x8, so the Settings-only snapshots are unchanged. Rows below `inner.bottom()` are clipped instead of indexed out of bounds. `fits` becomes `width ≥ 24 && height ≥ rows + 3`, otherwise the one-line fallback is drawn. The hint gains `r refresh` (between `h/l change` and `Enter save`) only while the selection is on an AI row. Long model names are clipped with `…` by the existing `clip`. |

## Review-driven changes

Adversarial reviews (observations 2933, 2937, 2939, 2941) and the PRs #77, #82, #83, #84, #88 and #89 changed the delivered code beyond the ADRs above. The ADRs now describe it, and these points are the summary.

- **Process-group kill (AE-5):** `version()` runs the child in its own group, kills the group on timeout or cancellation (`GroupKill`), and uses `waitid(WNOWAIT)` so the kill happens before the reap. 64 KiB output cap, ETXTBSY retry (max 4). Known residual: a child that calls `setsid` leaves the group.
- **Sanitizing and bounds (AE-3, AE-5):** ANSI CSI/OSC, control and bidi/format characters are stripped from versions and model names; version cap 40, name cap 128, list cap 256; empty names dropped. Specified in ai-engine-detection.
- **Strict HTTP framing (AE-4):** HTTP/1.x status 200 only, 1 MiB cap in every framing, strict chunked, ambiguous Content-Length/Transfer-Encoding rejected, at most one Transfer-Encoding header ending in `chunked` or `identity`. The request is still HTTP/1.0.
- **Menu (AE-9, AE-12):** `with_ai(ai, engines)` takes two arguments; the logic lives in `core/ai/menu.rs`; the AI popup is 62 wide, the Settings-only popup 51.
- **Startup (AE-6):** `startup_effects(&mut App)` and `app_from_config` make the startup trigger and the saved-choice carry-over unit-testable.
- **Tests:** `tests/version_stdin.rs` is a separate integration binary because it swaps fd 0 for the whole process.
- **Known gap (carried):** before the first detection answer, Enter with an unchanged draft closes the menu silently (verify W2). The AI selector change owns the fix.

## Data Flow

```
startup / menu open / `r` ─> App::request_detection ─(!detecting)─> Effect::DetectEngines
Panes::apply ─> detector() ─> tokio task: detect_all
   PATH lookup ─┐   http_get(127.0.0.1:port) ─┐   version(abs, 2s, kill) ─┐
                └──────── join_all ───────────┴───────────────────────────┘
                     Vec<Probe> ─> ai::reduce ─> Detection ─> ai_tx
run_loop select! ai_rx ─> AppEvent::EnginesDetected ─> App.engines, detecting=false
                                                   └─> menu.set_engines (in place)
Enter ─> MenuState::edits (static + ai.*) ─> SaveConfig ─> apply_edit ─> atomic write
```

## File Changes

| File | Action | Description |
|---|---|---|
| `src/core/ai/mod.rs` | Create | `EngineId`, `CATALOG`, `Status`, `EngineState`, `Detection`, `Probe`, `reduce` |
| `src/core/ai/parse.rs` | Create | `ollama_tags`, `openai_models(owner)`, `parse_version` |
| `src/core/ai/http.rs` | Create | `request(port, path)`, `parse_response` (status, Content-Length, close, chunked) |
| `src/core/ai/config.rs` | Create | `AiConfig`, `read(&Table, &mut problems)` |
| `src/core/mod.rs` | Modify | `pub mod ai` |
| `src/core/config.rs` | Modify | `Parsed.ai`, `load_with_ai`, `ConfigEdit.value: Cow` |
| `src/core/config_edit.rs` | Modify | `as_ref()` on the value; test helper uses `Cow::Borrowed` |
| `src/core/ai/menu.rs` | Create | `AiMenu`: draft, choices, status text, `edits`, refresh |
| `tests/version_stdin.rs` | Create | integration binary: `version()` with fd 0 swapped process-wide |
| `src/core/menu.rs` | Modify | `Row` with Cow, `AiMenu`, `with_ai`, `set_engines`, AI rows, cycling, `Refresh`, AI edits |
| `src/runtime/ai_detect.rs` | Create | `http_get`, `find_on_path`, `version`, `detect_all`, `spawn_detection` |
| `src/runtime.rs` | Modify | `startup_effects(app)` and `app_from_config` helpers (testable startup), `ai_rx` arm, `with_detector`, `DetectEngines` arm, startup request, `load_with_ai` |
| `src/runtime/config_write.rs` | Modify | const test edit uses `Cow::Borrowed` |
| `src/app.rs` | Modify | `ai`, `engines`, `detecting`, `with_ai`, `request_detection`, Effect/AppEvent variants, menu wiring |
| `src/ui/components/menu.rs` | Modify | dynamic height, row clipping, `fits(rows)`, hint, AI snapshots |

## Interfaces / Contracts

```rust
// core::ai (pure)
pub enum Status { Available, NotRunning, NotInstalled }
pub struct EngineState { pub id: &'static str, pub status: Status,
    pub version: Option<String>, pub models: Vec<String> }
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Detection { pub engines: Vec<EngineState> } // catalog order; empty = failed
pub enum Probe { Server { id: &'static str, on_path: bool, reply: Option<(u16, Vec<u8>)> },
                 Cli { id: &'static str, path: Option<PathBuf>, stdout: Option<Vec<u8>> } }
pub fn reduce(probes: &[Probe]) -> Detection;
pub struct AiConfig { pub engine: Option<String>, pub model: Option<String> }
// config: pub struct ConfigEdit { table: &'static str, key: &'static str, value: Cow<'static, str> }
// app: Effect::DetectEngines; AppEvent::EnginesDetected(Detection); MenuCommand::Refresh
// runtime: async fn http_get(port: u16, path: &'static str) -> Option<(u16, Vec<u8>)>
```

## Testing Strategy (strict TDD; `cargo test`, `clippy --all-targets`, `fmt --check` on every slice)

| Layer | What | Approach |
|---|---|---|
| Core ai | each shape: valid, empty list, missing field, wrong type, HTML, foreign `{"status":"ok"}`, OpenAI list without the owner marker on llamacpp/vllm, non-200; status table (shape × on_path); `parse_version` (blank lines, CRLF, cap 40/41); HTTP parse (Content-Length short/exact, close-delimited, chunked, malformed) | byte fixtures, plain `#[test]` |
| Config | `[ai]` read, `""` → None, wrong types → problem and default, unknown id kept, Settings unaffected; save round-trip with `ai.*` | existing patterns |
| Runtime | `http_get` against a `127.0.0.1:0` listener: a reply, silence (returns within the timeout plus slack), over the cap, refused; `find_on_path` in a tempdir (no exec bit, a directory, relative entry); `version` with tempdir scripts: echo, `sleep 10` killed in time, `read x` proves stdin is closed, exit 1 → None | `#[tokio::test]`, no external network |
| App | single flight (two requests → one effect), clear on result, triggers (startup, open, `r`), the menu updated in place with the selection kept, save copies the draft, cancel discards it | `update` |
| Menu/UI | engine/model cycling and reset, `not found` kept, saved unknown id, Refresh only on AI rows and Press, Settings snapshots unchanged, new AI snapshots, no panic at any size | `TestBackend` + insta |

## Migration / Rollout

No migration is needed: an existing `config.toml` without `[ai]` behaves exactly as before. Feature-branch chain on `feat/ai-engines`, 400-line budget per slice.

| # | Content | Est. | Depends |
|---|---|---|---|
| AE1 | `core::ai`: catalog, types, parsers, `parse_version`, `http::parse_response`, `reduce` | ~360 | — |
| AE2 | `runtime::ai_detect` probes; Effect/AppEvent; App single flight; runtime channel, detector and startup trigger | ~370 | AE1 |
| AE3 | `AiConfig` + `[ai]` parse; `ConfigEdit` Cow; `Row` Cow; `AiMenu` (rows, cycling, Refresh, edits) | ~380 | AE1 |
| AE4 | App menu wiring (`with_ai`, open/`r` triggers, set_engines, save copy), runtime `load_with_ai`, popup geometry and snapshots | ~220 | AE2, AE3 |

The 3-slice plan in the proposal put about 470 lines in AE3, which is over budget. The split into AE3 and AE4 is therefore planned up front, not kept as a fallback. If AE2 goes over, move the App/runtime wiring into AE4, so AE2 contains only the probes.

## Risks

- The owner markers (`owned_by`) depend on llama.cpp and vLLM versions. A very old llama-server would show as `not running` (a false negative, which is safer than a false positive).
- `lms` on PATH is an imperfect sign that LM Studio is installed. Without it, a stopped LM Studio shows as `not installed`.
- A slow `claude --version` (Node cold start) can hit the 2 s timeout, and the CLI then shows `available` with no version. The UI is never blocked, but the status shows `detecting…` until then.
- A probe that panics is reported as `unknown` (AE-6) and never leaves the single-flight flag stuck.

## Open Questions

- [x] Resolved by orchestrator: `/health` is never used (AE-3); `r` works only with the selection on an AI row, Press only (AE-10); `none` is written as `""` for both keys (AE-8, AE-11).
- [x] Resolved by orchestrator: the spec is aligned to the design. One `Detection` after all probes finish (worst case about 2 s), single flight with triggers as no-ops while in flight, `detecting…` until the result, all statuses update at once (AE-6).
