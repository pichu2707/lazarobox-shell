# Proposal: AI Engines (Phase 1: detection and selection)

## Intent

Lazarobox is going to become an AI harness that is managed entirely from inside lazarobox, so the user never has to open another CLI to set it up. Before any chat or agent work can happen, lazarobox has to know which AI engines exist on the machine and which one the user wants to use. This change is phase 1. It detects local model servers and installed agent CLIs, and it lets the user pick a default engine and model from the config menu. Local engines come first so the user can test at no cost.

## Roadmap (only P1 is in this change)

| Phase | Content |
|---|---|
| **P1** | Detect engines, add the AI menu section, persist the choice (this change) |
| P2 | Chat pane that streams from a local model (Ollama) |
| P3 | Agent loop with tools and approvals. Reads run without asking. Writes and exec need approval, with an "allow for this session" option. The agent may open its own panes. Writing into the user's existing panes needs approval on every call |
| P4 | Headless CLI engines (for example `claude -p` with JSON output). They use the user's own login, and lazarobox never touches their credentials |
| P5 | Cloud APIs. OAuth2 where the provider supports it for third-party apps, otherwise an API key from an env var or the OS keyring. Keys are never stored in plain text in `config.toml` |
| P6 | Orchestrator: the user picks agent, engine and model for each task group |
| Later | MCP |

## Decisions (made by the orchestrator in automatic mode)

- D1: The engine catalog is fixed:
  - Local servers: `ollama` (`127.0.0.1:11434/api/tags`), `lmstudio` (`:1234/v1/models`), `llamacpp` (`:8080/v1/models` or `/health`), `vllm` (`:8000/v1/models`).
  - CLIs: `claude`, `codex`, `gemini`, `opencode`.
- D2: A server is **available** only when its response has the expected JSON shape. An open port that answers with anything else counts as **not running**. This covers the unknown service on `:8080`.
- D3: A CLI is **available** when the executable is on `PATH`, and **not installed** when it is not. Its version comes from `--version`, run with no shell, stdin closed and a short timeout. If the version fails, the CLI is still available but has no version. Shell aliases are NOT detected, so `opencode` defined as an alias shows as not installed.
- D4: Detection runs in the background at startup and again on refresh (a key in the AI section). The probes run concurrently with short timeouts (design sets the values) and never block the UI or the input.
- D5: Detection is read-only. It sends no traffic except to `127.0.0.1`, it reads no credential or token files, and it only runs `--version`.
- D6: The new AI section has an **Engine** row and a **Model** row, and their choices are built from the detection result:
  - The Model row lists models only for local servers. For CLIs it shows `default`.
  - Changing the engine resets the model to the first model listed, or to none.
  - Each engine shows its status.
- D7: `config.toml` gets `[ai] engine = "<id>"` and `model = "<name>"`, and nothing else. No secrets go there. If the saved engine is not detected at startup, the value is kept and shown as not detected. It is never erased.
- D8: An `AiConfig` struct holds strings and is kept separate from `Config` (which is `Copy`). The menu learns about dynamic choice lists, while the static Settings rows stay the same.

## Scope

### In Scope
- Engine catalog, response-shape parsers, PATH lookup, version probe, background detection with refresh.
- AI menu section with dynamic choices and status, plus persistence of `[ai]` that keeps everything else in the file intact.

### Out of Scope
- Chat UI, sending prompts, agent loop, tools, approvals, headless CLI execution, OAuth or keys, the orchestrator and MCP (P2 to P6).
- Probing non-default ports or remote hosts. Detecting shell aliases.

## Capabilities

### New Capabilities
- `ai-engine-detection`: the catalog, the probes, the status values (available, not running, not installed), timeouts, refresh, and the read-only and localhost-only guarantees.

### Modified Capabilities
- `config-menu`: adds the AI section and choice lists built from the detection result. Removes "the only section is Settings" and the rule that every allowed value is static.
- `config-persistence`: adds `ai.engine` and `ai.model` to the owned keys.
- `configuration`: adds the `[ai]` schema and its defaults (no engine).

## Approach

- `src/core/ai/` is pure: the catalog, the parsers for `/api/tags` and `/v1/models`, `AiConfig`, and the reducer that turns probe results into engine states.
- The runtime runs the probes as tokio tasks and sends the results back as `AppEvent`s through the existing mpsc channel. The app asks for a refresh with a new `Effect`.
- The HTTP client is restricted to `127.0.0.1`. Design decides between `reqwest`, which is declared but unused today, and a minimal hand-written HTTP/1.1 GET over `tokio::net`.

## Affected Areas

| Area | Impact |
|---|---|
| `src/core/ai/` | New: catalog, parsers, `AiConfig`, state |
| `src/core/config.rs` | Modified: parse `[ai]` into `AiConfig` |
| `src/core/menu.rs` | Modified: dynamic sections and choices, AI section |
| `src/app.rs`, `src/runtime.rs` | Modified: detection Effect and AppEvent, probe tasks |
| `src/ui/` (menu render) | Modified: AI rows and status |
| config save path | Modified: owned `ai.*` keys |

## Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| Something else is listening on the port, such as `:8080` | High | Check the response shape (D2), with tests on foreign responses |
| A CLI `--version` hangs or is slow | Med | Timeout, kill on timeout, stdin closed |
| Menu refactor breaks Settings behavior | Med | Existing menu tests stay green, and the static rows are unchanged |
| Subscription logins cannot be reused by third-party apps | High (P4/P5) | Headless CLI approach in P4. Verify OAuth support for each provider in P5 |
| Running PATH binaries is an execution surface | Low | Only `--version`, no shell, fixed names |

## Rollback Plan

Revert the merge of the tracker `feat/ai-engines` into main. An `[ai]` table left in `config.toml` is harmless, because unknown tables are already ignored and preserved.

## Delivery Slices

Feature-branch chain on `feat/ai-engines`, with a 400-line budget per PR and strict TDD.

| # | Content | Lines |
|---|---|---|
| AE1 | Pure core: catalog, parsers, `AiConfig` parsing | ~300 |
| AE2 | Runtime probes, PATH and version, Effect and AppEvent, refresh | ~350 |
| AE3 | Dynamic menu, AI section, `[ai]` persistence | ~380 |

## Success Criteria

- [ ] `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check` pass on every slice.
- [ ] On the user's machine:
  - [ ] Ollama is shown as available, with its models.
  - [ ] `claude`, `codex` and `gemini` are shown as available, with versions.
  - [ ] `opencode` is shown as not installed.
  - [ ] `:8080` is not shown as llama.cpp unless it really is llama.cpp.
- [ ] The UI stays responsive while the probes run, and refresh updates the statuses.
- [ ] Saving writes only `ai.engine` and `ai.model`, and the rest of the file is preserved.
- [ ] No probe goes anywhere except `127.0.0.1`, and no credential file is opened.

## Proposal question round

The user could not be asked. These assumptions need review:
1. Should local servers that are installed but not running (for example the `ollama` binary is on PATH but nothing is listening) show **not running** rather than **not installed**? (Assumed: servers are only probed over HTTP, so they show not running.)
2. Is `default` acceptable for the CLI Model row in P1, or should known CLI model names be listed?
3. Should refresh also run automatically every time the menu opens, or only at startup and on the refresh key?
4. Should users be able to add custom endpoints or ports in `config.toml` in P1, or is that deferred? (Assumed: deferred.)
5. Should aliases like `opencode` be resolved later by asking the user's shell, or should the user add the binary to PATH?
