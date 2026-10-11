# Tasks: AI Engines (Phase 1: detection and selection)

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | ~1,250 total (range 1,100-1,700, tests ~1.5x code), 4 PRs of 260-380 lines; up to 8 PRs if the fallbacks fire |
| Per-PR estimate | AE1 ~360, AE2 ~370, AE3 ~380, AE4 ~260 |
| 400-line budget risk | High (AE1, AE2 and AE3 sit above 350; the previous change overran tests by 30-60%) |
| Chained PRs recommended | Yes |
| Suggested split | AE1 -> AE2 -> AE3 -> AE4 (fallbacks AE1a/b, AE2a/b, AE3a/b below) |
| Delivery strategy | auto-chain |
| Chain strategy | feature-branch-chain |

Decision needed before apply: No
Chained PRs recommended: Yes
Chain strategy: feature-branch-chain
400-line budget risk: High

Tracker `feat/ai-engines` equals main `a471e26`. The planning branch `feat/ai-engines-plan` (spec, design, tasks) is PR #1 into the tracker. AE1 targets `feat/ai-engines-plan`; each later slice targets the previous slice branch; only the tracker merges to main. Tests are never trimmed to fit a budget; a slice that passes 400 during apply splits at its fallback line.

Fallback splits (apply only if the slice exceeds 400 during apply; stubs are split along the same line):
- **AE1** (~360): AE1a `...-01a-parsers` = AE1.1 (parse/http stubs), AE1.4-AE1.11 (`ollama_tags`, `openai_models`, `parse_version`, `parse_response`); AE1b `...-01b-reduce` = catalog, `reduce`, `status_text` (AE1.1 rest, AE1.2-AE1.3, AE1.12-AE1.17).
- **AE2** (~370): AE2a `...-02a-probes` = AE2.1 (probe stubs), AE2.2-AE2.13 (`ai_detect.rs` only); AE2b `...-02b-app-flight` = AE2.1 (App/Panes stubs), AE2.14-AE2.17.
- **AE3** (~380): AE3a `...-03a-config-cow` = AE3.1 (config part), AE3.2-AE3.6; AE3b `...-03b-ai-menu` = AE3.1 (menu part), AE3.7-AE3.18.
- **AE4** (~260): no fallback needed; if it grows, move AE4.9-AE4.10 (persistence) into AE4b.

### Suggested Work Units (merge BOTTOM-UP into the tracker `feat/ai-engines`)

| Slice | Branch | Base | Est. lines | Content |
|---|---|---|---|---|
| AE1 | `feat/ai-engines-01-core-ai` | `feat/ai-engines-plan` | ~360 | `core::ai`: catalog, types, parsers, `parse_version`, `http::parse_response`, `reduce`, `status_text` |
| AE2 | `feat/ai-engines-02-probes-flight` | AE1 branch | ~370 | `runtime::ai_detect` probes, `Effect`/`AppEvent`, App single flight, `Panes` detector, `ai_rx` arm, startup trigger |
| AE3 | `feat/ai-engines-03-config-menu-model` | AE2 branch | ~380 | `ConfigEdit`/`Row` Cow, `AiConfig` + `[ai]` parse, `AiMenu` (rows, cycling, navigation, Refresh, edits) |
| AE4 | `feat/ai-engines-04-wiring-ui` | AE3 branch | ~260 | App menu wiring, triggers, save/cancel, persistence checks, popup geometry, snapshots, README limits |

Every PR must be green: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`. Strict TDD: RED is an assertion-level failing test against compiling stubs (stub returns a wrong or default value, so the failure is a failed assertion, not a compile error); then GREEN, then REFACTOR. All assertions use exact equality (`assert_eq!` on whole values, strings, byte vectors, styles); never `.all`, `.any` or `contains` to loosen a check. Characterization tests that pass immediately are labelled. No new crates (tokio `full`, futures, serde_json, tempfile, insta already declared). Tests never touch the real catalog ports: runtime tests use `detect_with` and `http_get` on ephemeral `127.0.0.1:0` listeners. Each PR states start/end, parent, follow-up, out of scope and a dependency diagram with the current PR marked. Work-unit commits, conventional commits, no AI attribution, no cross-slice commits.

## AE1: Pure `core::ai` (catalog, parsers, reduce)

- [x] AE1.1 Stubs (compile only): `src/core/ai/mod.rs` with `CATALOG`, `Status`, `EngineState`, `Detection` (Default), `Probe`, `reduce` (returns `Detection::default()`), `status_text(Option<&Detection>, &str) -> String` (returns `""`); `parse.rs` (`ollama_tags`, `openai_models`, `parse_version` return `None`); `http.rs` (`parse_response` returns `None`); `pub mod ai` in `src/core/mod.rs`.
- [x] AE1.2 RED: catalog is exactly the 8 ids in order, unique, every server host `127.0.0.1`, every CLI has no endpoint, binaries `ollama lms llama-server vllm claude codex gemini opencode` [ai-engine-detection: Catalog integrity].
- [x] AE1.3 GREEN: `CATALOG` data (AE-1).
- [x] AE1.4 RED: `ollama_tags`: two models in order; `{"models":[]}` gives `Some(vec![])`; `[{"name":"a"},{"tag":"b"},3]` gives exactly `["a"]`; `{"status":"ok"}`, `[]`, `{"models":"x"}`, HTML give `None` [Valid Ollama, Partial entries ignored, Foreign service answers other JSON (part)].
- [x] AE1.5 GREEN: `ollama_tags` over `serde_json::Value` (AE-3).
- [x] AE1.6 RED: `openai_models(body, owner)`: lmstudio (`None` owner) `gemma-3-4b`, empty `data` valid; with owner `llamacpp`/`vllm` marker required, empty `data` invalid, `owned_by:"localai"` invalid, marker present yields every string-`id` entry; `{"status":"ok"}`, `{"data":"x"}`, `{"data":[]}`, `[]`, HTML give `None` [Valid OpenAI-style list, Valid llama.cpp and vLLM, Foreign OpenAI-compatible service, Foreign service answers HTML, Foreign service answers other JSON].
- [x] AE1.7 GREEN: `openai_models` with the owner check (AE-3).
- [x] AE1.8 RED: `parse_version`: `codex 0.5.0\n` gives `codex 0.5.0`; leading blank lines and CRLF skipped; exactly 40 chars kept, 41 capped to 40; empty and whitespace-only give `None` [Found with version (parse part), Version fails (empty output)].
- [x] AE1.9 GREEN: `parse_version` (AE-5).
- [x] AE1.10 RED: `http::parse_response`: Content-Length exact; short body gives `None`; close-delimited; chunked; case-insensitive headers; `301` with `Location` returned as `(301, body)`, not followed; malformed status line gives `None`; 2 MiB body gives `None` [Non-200 or oversize (framing part)].
- [x] AE1.11 GREEN: `parse_response` (AE-4, 1 MiB cap).
- [x] AE1.12 RED: `reduce` servers: table of (valid, binary) x4 gives `available`, `available`, `not running`, `not installed` with exact `EngineState` and models; reply `None` follows the binary rule; 404, 503, 301, 2 MiB valid body are invalid; `:8080` foreign `localai` with `llama-server` on PATH and `vllm` absent gives llamacpp `NotRunning`, vllm `NotInstalled`; HTML without binary gives `NotInstalled`; `{"status":"ok"}`, `{"data":"x"}`, `{"data":[]}`, `[]` with binary give `NotRunning` [Server matrix, Non-200 or oversize, Port closed (reduce part), Foreign OpenAI-compatible service, Foreign service answers HTML, Foreign service answers other JSON].
- [x] AE1.13 GREEN: server branch of `reduce` (AE-2); output in catalog order.
- [x] AE1.14 RED: `reduce` CLIs: path + `codex 0.5.0` gives `Available`, version `Some("codex 0.5.0")`, models `["default"]`; path `None` gives `NotInstalled` (alias case); stdout `None` or empty gives `Available` with no version; result always 8 entries in catalog order [Found with version, Absent or alias, Version fails].
- [x] AE1.15 GREEN: CLI branch of `reduce`.
- [x] AE1.16 RED: `status_text`: `available · 2 models`, `available · 1 model`, `available · 0 models`, `available · 0.5.0`, `available`, `not running`, `not installed`; `None` detection gives `detecting…`; `Some(empty)` gives `unknown`; id outside the catalog gives `not detected` [Display text, Engines show detecting until the result (display part), Failed run still answers (display part)].
- [x] AE1.17 GREEN: `status_text`.
- [x] AE1.18 Final: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

## AE2: Probes, App single flight, runtime wiring

- [x] AE2.1 Stubs (compile only): `src/runtime/ai_detect.rs` with `HTTP_TIMEOUT`, `VERSION_TIMEOUT`, `http_get(port, path) -> Option<(u16, Vec<u8>)>` (`None`), `find_on_path(name, &OsStr)` (`None`), `version(&Path)` (`None`), `detect_with(&[Target], &OsStr) -> Detection` and `detect_all` (`Detection::default()`), `spawn_detection_of(tx, fut)` (sends nothing), `spawn_detection`; `Effect::DetectEngines`, `AppEvent::EnginesDetected(Detection)`, `App.engines: Option<Detection>`, `detecting`, `request_detection() -> Vec<Effect>` (`vec![]`); `Panes::with_detector` storing a no-op `Box<dyn FnMut()>`.
- [x] AE2.2 RED: `http_get` on a `127.0.0.1:0` listener: reply `200` with body gives `Some((200, body))`; chunked reply decoded; the recorded request bytes equal exactly `GET /api/tags HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\nConnection: close\r\n\r\n` (no Authorization, no Cookie, no body) [Traffic audit (request part), Valid Ollama].
- [x] AE2.3 GREEN: `http_get` (AE-4).
- [x] AE2.4 RED: `http_get` silence (accepts, never answers) returns `None` in under 1.2 s; over-cap reply (more than 1 MiB) gives `None`; refused port gives `None` in under 400 ms; a 301 is returned, not followed [Port closed, Non-200 or oversize, Hanging server (probe part)].
- [x] AE2.5 GREEN: timeout around connect/write/read and the 1 MiB read cap.
- [x] AE2.6 RED: `find_on_path` in tempdirs: executable file found; first match wins across two dirs; no exec bit gives `None`; a directory named `codex` gives `None`; empty and `.` entries skipped even with an executable `codex` in the cwd (cwd changed under a static `Mutex`, restored after; no other test touches cwd) [Absent or alias, Relative PATH entry skipped].
- [x] AE2.7 GREEN: `find_on_path` (AE-5).
- [x] AE2.8 RED: `version(abs_path)` with tempdir scripts: echo gives `Some(b"codex 0.5.0\n")`; `sleep 30` gives `None` within 2.5 s and the recorded child pid no longer exists; exit 1 gives `None`; `read x; echo done` gives `Some(b"done\n")` proving stdin is closed [Found with version, Version hangs, Version fails].
- [x] AE2.9 GREEN: `version` with `kill_on_drop`, `stdin(null)`, `stderr(null)`, 2 s timeout.
- [x] AE2.10 RED: `detect_with` on ephemeral fake servers plus a fake PATH dir: exact 8-entry `Detection` (ollama 2 models, lmstudio, llamacpp with marker, vllm foreign `NotRunning`, codex version, gemini hanging, claude exit 1, opencode absent); every recorded request is exactly the catalog GET with one connection per server and an extra unrelated listener sees zero connections; a fake `ollama` script on PATH is never executed (marker file absent); a hanging `:11434` gives ONE `Detection` after the 800 ms timeout, ollama `NotRunning`, the other seven real, total under 2.5 s [Traffic audit, Hanging server, Version hangs, Valid llama.cpp and vLLM, Valid OpenAI-style list].
- [x] AE2.11 GREEN: `detect_with` (concurrent `join_all`, PATH read once, `reduce`) and `detect_all` over the catalog ports (AE-5).
- [x] AE2.12 RED: `spawn_detection_of`: a normal future's `Detection` arrives on the channel; a panicking future still sends `Detection::default()`; the call returns in under 100 ms while a hanging future runs [Failed run still answers, UI stays responsive (runtime part)].
- [x] AE2.13 GREEN: inner/outer task with `unwrap_or_default` (AE-6).
- [x] AE2.14 RED: App flight via `update`: `request_detection()` gives `[DetectEngines]`, a second call gives `[]`; `EnginesDetected(d)` stores `Some(d)`, clears the flag, and the next call gives `[DetectEngines]` again; before the first result `status_text(app.engines)` is `detecting…` for all 8, after it the real statuses at once; `EnginesDetected(Detection::default())` also clears the flag and reads `unknown`; a key event while `detecting` emits its `WritePty` immediately [Three triggers (startup), Trigger while a run is in flight is a no-op, Completion re-arms the triggers, Engines show detecting until the result, Failed run still answers a run, UI stays responsive].
- [x] AE2.15 GREEN: `request_detection`, `EnginesDetected` handling (AE-6).
- [x] AE2.16 RED: `Panes::apply(Effect::DetectEngines)` calls the injected detector exactly once (counter); the default detector does nothing and does not panic; `step(app, EnginesDetected(d))` updates the app and returns no `WritePty` [Three triggers (startup glue), Trigger while a run is in flight (runtime part)].
- [x] AE2.17 GREEN: `with_detector`, `DetectEngines` arm in `Panes::apply`, `ai_rx` arm in `run_loop`, `run()` applies `request_detection()` before the loop, production detector calls `spawn_detection`.
- [x] AE2.18 Final: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

## AE3: Config, Cow refactor and AI menu model

- [x] AE3.1 (config part in AE3a, menu part in AE3b) Stubs (compile only): `src/core/ai/config.rs` (`AiConfig` with `Default`, `read` returns default); `Config::load_with_ai` wrapping `load`; `MenuState::with_ai`, `AiMenu`, `set_engines`, `MenuCommand::Refresh`, `ai_draft()` returning defaults.
- [x] AE3.2 Characterization (expected to pass immediately): switch `ConfigEdit.value` and `Row.value` to `Cow<'static, str>` (AE-8, AE-9) in `src/core/config.rs`, `config_edit.rs`, `menu.rs`, `src/runtime/config_write.rs`; every existing config, config_edit, menu, config_write test and Settings snapshot stays green and unchanged.
- [x] AE3.3 RED: `ai::config::read`: `engine = "ollama"`, `model = "llama3:8b"` parsed with no problem; `future-engine` kept verbatim; `""` gives `None`; `engine = 3` gives `engine: None`, model kept, problem exactly `ai.engine: expected a string`; non-table `ai` gives `ai: expected a table` [configuration: AI keys parsed, Unknown engine id kept, Empty string is unset, Invalid AI types fall back per key, Non-table ai].
- [x] AE3.4 GREEN: `ai::config::read` (AE-7).
- [x] AE3.5 RED: `Config::parse`/`load_with_ai`: empty file gives defaults and unset AI; `[ai]` plus `[tabbar] position = "bottom"` and `engine = 3` gives tabbar bottom and notice exactly `config: ai.engine: expected a string`; `ai = 3` gives `config: ai: expected a table`; AI problems count in `config (N problems): ...`; characterization of the existing notices (missing file, per-key fallback, several keys, unparseable, mouse reserved, unknown keys, each value) [configuration: Defaults, Each value, Unknown keys ignored, Missing file, Invalid value falls back per key, Several invalid keys, Unparseable file, Mouse reserved, AI keys parsed, Invalid AI types, Non-table ai].
- [x] AE3.6 GREEN: `Parsed.ai`, `Config::parse` calls `ai::config::read`, `load_with_ai` (AE-7); `Config` stays `Copy`.
- [x] AE3.7 RED: engine choices and cycling on `MenuState` with a `Detection` fixture: ollama, claude, codex available and lmstudio `NotRunning`, draft `ollama`, `l` x3 then x1 gives `claude`, `codex`, `none`, `ollama`; saved `codex` `NotInstalled` shows `codex (not detected)`, choices `none ollama codex`, `l` gives `none`; engines `None` with saved `claude` shows `claude (checking)`; no engines and nothing saved shows `none` and `l` changes nothing [config-menu: Cycle engines, Saved engine not available, Pending detection, No engines at all].
- [x] AE3.8 GREEN: engine rows and choices (AE-10).
- [x] AE3.9 RED: model choices: engine change to `claude` gives `default`, back to `ollama` gives `llama3:8b`; `none` clears engine and model, Model row `none`; `{"models":[]}` gives `none` and `l` changes nothing; saved `old:7b` shows `old:7b · not found`, draft still `old:7b`, then `l` shows `llama3:8b` [Engine change resets model, Engine none clears model, Server without models, Saved model no longer listed].
- [x] AE3.10 GREEN: model rows, reset rules, appended `not found` choice (AE-10).
- [x] AE3.11 RED: `Status` row text follows the draft engine: `not running` (saved lmstudio), `-` (none), `available · 2.1.0` (claude); `detecting…` while pending; unknown saved id gives `not detected`; the AI section always has exactly three rows (Engine, Model, Status) before and after `set_engines`, Status flagged muted/display-only [Status follows the draft engine, AI rows rendered (row part), Pending detection].
- [x] AE3.12 GREEN: Status row via `status_text` (AE-10).
- [x] AE3.13 RED: navigation over static rows + Engine + Model: `j` then `k` returns; wrap in both directions; Tab bar to Engine skipping Mouse and back; Engine `j j` gives Model then Statusline position (never Status); `k` from Statusline position gives Model then Engine; Repeat `j` moves like Press [Move down and up, Wrap, Disabled row is skipped, Status row is skipped and navigation wraps, Repeat moves].
- [x] AE3.14 GREEN: selection indices over static + 2 AI choices (AE-9).
- [x] AE3.15 RED: `set_engines` in place keeps selection and draft: ollama `detecting…` then `available · 2 models` with Model selected; a refresh returning `NotRunning` leaves draft `ollama`, Engine row `ollama (not detected)`, Status `not running`; `r` Press on Engine or Model gives `MenuCommand::Refresh`; on Statusline row, as Repeat, and `R` it gives no command [Result arrives while open (menu part), Engine disappears while open (menu part), Refresh on AI row (menu part), Refresh elsewhere or repeated (menu part)].
- [x] AE3.16 GREEN: `set_engines`, `Refresh` mapping (Press only).
- [x] AE3.17 RED: `edits()` appends `ai.engine` and `ai.model` (both, `None` as `""`) only when either differs, and none when equal; draft restored after Esc and reopen; unchanged AI plus a position change yields only the position edit; descriptor integrity extended: dynamic items have a choice source, display-only item has a value source and no key path, no duplicate key paths, non-empty labels [Esc reverts AI (menu part), Unchanged AI draft (menu part), Descriptor integrity, Collision ignored when AI unchanged (edit part), Undetected saved engine survives (edit part)].
- [x] AE3.18 GREEN: AI edits (AE-11) and item sources in the descriptor table.
- [x] AE3.19 Final: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

## AE4: App wiring, persistence, popup geometry

- [x] AE4.1 Stubs (compile only): `App::with_ai(AiConfig)`, `App::ai()`, `open_menu` still without AI, `ConfigSaved` does not copy; `Config::load_with_ai` used in `src/runtime.rs` returning the AI as default.
- [x] AE4.2 Characterization (expected to pass immediately): menus without AI keep the existing Settings snapshots byte-identical; Unknown key (`x`, Tab, Ctrl+Space) and Paste ignored emit no `WritePty` and keep MENU; Settings footer hint; error replaces hint; existing `config_edit` tests for comments/order, missing keys, mouse untouched stay green [config-menu: Settings rows (old), Unknown key, Paste ignored, Footer hint, Error replaces hint; config-persistence: Comments and order preserved, Missing keys are created, Mouse untouched].
- [x] AE4.3 RED: triggers through `update`: opening the menu gives exactly `[DetectEngines]` when idle and `[]` while a run is in flight; `r` on Engine gives exactly one `DetectEngines`, selection and draft unchanged, no `WritePty`; `r` on Statusline position, a Repeat of `r`, and `R` give none [Three triggers (menu, `r`), Trigger while a run is in flight is a no-op, Refresh on AI row, Refresh elsewhere or repeated].
- [x] AE4.4 GREEN: `open_menu` builds `with_ai(self.ai, self.engines, self.detecting)` and calls `request_detection`; `MenuCommand::Refresh` calls `request_detection`.
- [x] AE4.5 RED: `EnginesDetected` while the menu is open updates Status/choices in place, Model stays selected, draft unchanged, no `WritePty`; `ollama` turning `NotRunning` shows `ollama (not detected)` and Status `not running`; a result while closed is what the next open shows; opening the menu emits no `WritePty` [Result arrives while open, Engine disappears while open].
- [x] AE4.6 GREEN: forward `EnginesDetected` to `menu.set_engines`.
- [x] AE4.7 RED: save/cancel: `ConfigSaved` copies `menu.ai_draft()` into `App.ai` before closing and the values read `claude`/`default`; Esc then reopen shows `ollama`; Enter with an unchanged draft closes with no `SaveConfig` effect; AI changes emit no `ResizePty`; `App::with_ai(load_with_ai(..).1)` carries the file's AI values [Save closes with AI values, Esc reverts AI, Unchanged AI draft].
- [x] AE4.8 GREEN: `with_ai`, draft copy on `ConfigSaved`, runtime uses `load_with_ai` then `App::with_ai` (AE-7, AE-11).
- [x] AE4.9 RED (characterization where it already passes): `apply_edit` with AI edits: `# mine\n[statusline]\nposition = "bottom"\n` + `ollama`/`llama3:8b` ends exactly with `[ai]\nengine = "ollama"\nmodel = "llama3:8b"\n` and earlier bytes unchanged; in-place update keeps the comment and order; `none` writes `engine = ""` and `model = ""` and `Config::parse` gives unset; draft `a"b\c` parses back identically; a position-only save adds no `[ai]`; saved `codex` with only a position change keeps `engine = "codex"`; round trip through `Config::parse` equals the draft with no notice; `ai = 3` + changed engine gives byte-identical file and footer exactly `config.toml: ai is not a table; fix it before saving`; `[ai.engine]` gives `config.toml: ai.engine is a table; fix it before saving` (and the model variant); `engine = 3` is replaced by `engine = "ollama"`; `ai = 3` + unchanged AI + changed position saves the position and leaves `ai = 3` [config-persistence: ai is a scalar, engine holds a table, Scalar of wrong type replaced, Collision ignored when AI unchanged, Saved file parses back, AI table created, Existing AI keys updated in place, No engine saved as empty string, Special characters escaped, AI untouched when unchanged, Undetected saved engine survives].
- [x] AE4.10 GREEN: adjust `apply_edit`/error mapping only where AE4.9 fails (the generic table/key code is expected to cover most cases).
- [x] AE4.11 RED: `src/ui/components/menu.rs` via `TestBackend` + insta: normal terminal with ollama 2 models and draft `ollama`/`llama3:8b` shows title `AI` and exactly three rows (Engine `ollama`, Model `llama3:8b`, Status `available · 2 models`), Status cells muted with the same exact style as the Mouse row, no per-engine list; Settings title and rows with the AI section after them; footer exactly `j/k move · h/l change · r refresh · Enter save · Esc cancel` on Engine and the old hint on Settings rows; height `rows + 4` (8 without AI); small sizes (`fits` = width >= 24 and height >= rows + 3) draw the one-line fallback; every size 0..=40 x 0..=20 never panics; a 60-char model name is clipped with `…` [AI rows rendered, Settings rows, Footer hint, Footer hint on AI row].
- [x] AE4.12 GREEN: dynamic height, row clipping, `fits(rows)`, `r refresh` hint on AI rows (AE-12); accept only the new AI snapshots with `cargo insta review`.
- [x] AE4.13 Docs: README states custom endpoints and ports are not supported in this phase and shell aliases are not detected (the binary must be on PATH) [Docs mention limits (text part)].
- [x] AE4.14 Final: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

## Verify prep: manual checks on the user's machine

Run after AE4 is merged into the tracker, on a build of the tracker branch.

- [ ] VP.1 [manual] Real machine: Ollama listening on 11434, `claude`/`codex`/`gemini` on PATH, `opencode` defined only as a shell alias, an unrelated service on `:8080`; the AI menu shows ollama `available · N models`, the three CLIs `available` with versions, opencode `not installed`, llamacpp not `available` (the foreign service is not counted) [ai-engine-detection: Real machine].
- [ ] VP.2 [manual] AI menu in Kitty: open the menu, change Engine to `claude`, press `r`, Enter, restart; statuses match the machine, the UI never freezes, `config.toml` gains `[ai]` with `engine = "claude"` and `model = "default"`, the menu shows `claude` after restart [config-menu: AI menu in Kitty].
- [ ] VP.3 [manual] README names both limits (custom endpoints/ports unsupported, aliases not detected) [ai-engine-detection: Docs mention limits].
- [ ] VP.4 [manual] Positions in Kitty regression: each of the four statusline/tabbar combinations with one and three tabs still lays out correctly with the new `[ai]` code in place [configuration: Positions in Kitty].
- [ ] VP.5 Verify prep: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` on the tracker; confirm every row of the coverage tables has a passing test or a ticked [manual] item; hand the tracker to `sdd-verify`.
- [ ] VP.6 Archive prep: the Purpose lines of the main `config-menu` and `configuration` specs MUST be edited by hand at archive, as noted in the deltas (`config-persistence` needs none).

## Spec Coverage Table

Every scenario in the four deltas maps to at least one task.

### ai-engine-detection (27)

| Scenario | Tasks |
|---|---|
| Catalog integrity | AE1.2, AE1.3 |
| Server matrix | AE1.12 |
| Display text | AE1.16 |
| Valid Ollama | AE1.4, AE2.2, AE2.10 |
| Valid OpenAI-style list | AE1.6, AE2.10 |
| Valid llama.cpp and vLLM | AE1.6, AE2.10 |
| Foreign OpenAI-compatible service | AE1.6, AE1.12 |
| Foreign service answers HTML | AE1.6, AE1.12 |
| Foreign service answers other JSON | AE1.4, AE1.6, AE1.12 |
| Non-200 or oversize | AE1.10, AE1.12, AE2.4 |
| Partial entries ignored | AE1.4 |
| Port closed | AE1.12, AE2.4 |
| Found with version | AE1.8, AE1.14, AE2.8, AE2.10 |
| Absent or alias | AE1.14, AE2.6 |
| Version hangs | AE2.8, AE2.10 |
| Version fails | AE1.8, AE1.14, AE2.8 |
| Relative PATH entry skipped | AE2.6 |
| Hanging server | AE2.4, AE2.10 |
| UI stays responsive | AE2.12, AE2.14 |
| Three triggers | AE2.14, AE2.16, AE4.3 |
| Trigger while a run is in flight is a no-op | AE2.14, AE2.16, AE4.3 |
| Completion re-arms the triggers | AE2.14 |
| Engines show detecting until the result | AE1.16, AE2.14 |
| Failed run still answers | AE1.16, AE2.12, AE2.14 |
| Traffic audit | AE2.2, AE2.10 |
| Real machine [manual] | VP.1 |
| Docs mention limits [manual] | AE4.13, VP.3 |

### config-menu (30)

| Scenario | Tasks |
|---|---|
| AI rows rendered | AE3.11, AE4.11 |
| Status follows the draft engine | AE3.11 |
| Cycle engines | AE3.7 |
| Engine change resets model | AE3.9 |
| Engine none clears model | AE3.9 |
| Server without models | AE3.9 |
| Saved model no longer listed | AE3.9 |
| Saved engine not available | AE3.7 |
| Pending detection | AE3.7, AE3.11 |
| No engines at all | AE3.7 |
| Result arrives while open | AE3.15, AE4.5 |
| Engine disappears while open | AE3.15, AE4.5 |
| Refresh on AI row | AE3.15, AE4.3 |
| Refresh elsewhere or repeated | AE3.15, AE4.3 |
| Esc reverts AI | AE3.17, AE4.7 |
| Save closes with AI values | AE4.7 |
| Unchanged AI draft | AE3.17, AE4.7 |
| AI menu in Kitty [manual] | VP.2 |
| Descriptor integrity | AE3.17 |
| Settings rows | AE4.2 (characterization), AE4.11 |
| Move down and up | AE3.13 |
| Wrap | AE3.13 |
| Disabled row is skipped | AE3.13 |
| Status row is skipped and navigation wraps | AE3.13 |
| Repeat moves | AE3.13 |
| Unknown key | AE4.2 (characterization) |
| Paste ignored | AE4.2 (characterization) |
| Footer hint | AE4.2, AE4.11 |
| Footer hint on AI row | AE4.11 |
| Error replaces hint | AE4.2 (characterization), AE4.9 |

### configuration (14)

| Scenario | Tasks |
|---|---|
| Defaults | AE3.5 |
| Each value | AE3.5 (characterization) |
| Unknown keys ignored | AE3.5 (characterization) |
| AI keys parsed | AE3.3, AE3.5 |
| Unknown engine id kept | AE3.3 |
| Empty string is unset | AE3.3 |
| Missing file | AE3.5 (characterization) |
| Invalid value falls back per key | AE3.5 (characterization) |
| Several invalid keys | AE3.5 (characterization) |
| Unparseable file | AE3.5 (characterization) |
| Mouse reserved | AE3.5 (characterization) |
| Invalid AI types fall back per key | AE3.3, AE3.5 |
| Non-table ai | AE3.3, AE3.5 |
| Positions in Kitty [manual] | VP.4 |

### config-persistence (14)

| Scenario | Tasks |
|---|---|
| ai is a scalar | AE4.9 |
| engine holds a table | AE4.9 |
| Scalar of wrong type replaced | AE4.9 |
| Collision ignored when AI unchanged | AE3.17, AE4.9 |
| Comments and order preserved | AE4.2 (characterization), AE4.9 |
| Missing keys are created | AE4.2 (characterization) |
| Mouse untouched | AE4.2 (characterization) |
| Saved file parses back | AE4.9 |
| AI table created | AE4.9 |
| Existing AI keys updated in place | AE4.9 |
| No engine saved as empty string | AE4.9 |
| Special characters escaped | AE4.9 |
| AI untouched when unchanged | AE3.17, AE4.9 |
| Undetected saved engine survives | AE3.17, AE4.9 |

## Deferred (out of scope)

- Cloud engines, API keys, TLS and `reqwest` use (P5); custom endpoints and ports.
- Sending prompts, streaming, per-pane engine choice, shell alias detection.
- Per-engine incremental delivery (one `Detection` per run by design, AE-6).
