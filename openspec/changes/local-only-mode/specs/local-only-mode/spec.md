# Local-Only Mode Specification

## Purpose

One user setting, `[ai] local_only`, that keeps the AI work of lazarobox on the machine: while it is on, no cloud AI engine is used for any task, the outbound calls that lazarobox makes on its own or that a model starts are off, and the state is visible. It is selectable by the user, like every setting: it lives in the user's own `config.toml` and the menu turns it on and off. Locking it is a separate goal (#659); this capability only keeps that possible. Part of goal #657. Slice tags: [S1] key and engine refusal, [S2] menu, [S3] Ollama cloud models, [S4] work in flight, [S5] auto and model calls, [S6] MCP servers. Scenario tags: [auto] = `cargo test` (pure config, fake detection, fake CLIs, `TestBackend`); [manual] = a real machine. Items that depend on an open question of the proposal name it (Q1-Q6) and follow its recommendation.

Terms: "the mode" = `local_only` on. "Cloud engine" = an engine of kind CLI (`claude`, `codex`, `gemini`, `opencode`) and, from S3 (Q1), an Ollama model that `forwards_off_machine` judges as served by Ollama's cloud (a name ending in `-cloud` or the tag `cloud`, ASCII case-insensitive). "Local engine" = a server engine (`ollama`, `lmstudio`, `llamacpp`, `vllm`) that answers on `127.0.0.1`, with a model that is not a cloud model. "Task" = `general`, `code`, `chat`, `review`, `summarize`, `orchestrate` (see ai-task-models). "Auto call" = a network call lazarobox starts on its own; "model call" = one a model's tool call starts; "user call" = one an explicit user action starts.

## Requirements

### Requirement: The switch is the user's [S1]
The mode MUST be read from `[ai] local_only` (see configuration, The local-only key) and MUST be changed only by the user: by the `Local only` menu row (see config-menu) or by editing the file. lazarobox MUST NOT turn it on or off by itself, and MUST NOT change any task assignment when it changes. The rest of the code MUST read the mode through one accessor, so a later system policy (#659) can decide its value without changing the rules below. No refusal text MUST tell the user how to turn the mode off.

#### Scenario: Off by default [auto]
- GIVEN a file without `local_only`
- WHEN the config is read
- THEN the mode is off and every engine behaves as today

#### Scenario: Assignments are untouched [auto]
- GIVEN `general` is `claude` / `sonnet` and the mode is turned on from the menu and saved
- WHEN the file is read back
- THEN `[ai] engine = "claude"` and `model = "sonnet"` are byte-identical and only `local_only = true` was added

### Requirement: Cloud engines are refused for every task [S1]
While the mode is on, the availability of any task (see chat-engines, Availability by task) whose effective engine is a cloud engine MUST be the result "refused by local only", checked right after "unassigned" and before every other result, so an engine with no adapter (`opencode`) is also refused by local only. The refusal MUST apply to: chat turns, the ORCH coordinator turn, compaction, the memory summary (`Prefix s`), every worker spawn, retry, relaunch and run-scoped override, and the resume of an interrupted run. It MUST NOT select another engine, MUST NOT fall back from a refused task to `general` or `chat`, and MUST NOT contact the refused engine. A local engine MUST behave exactly as with the mode off.

#### Scenario: Chat on a cloud CLI [auto]
- GIVEN the mode on, `chat` is `claude` / `sonnet` and claude is available
- WHEN a message is sent
- THEN no claude process starts, the message stays in the composer and the guidance reads `claude: cloud engine, not used while local only is on: pick a local engine in the menu (Ctrl+Space m)`

#### Scenario: Every task [auto]
- GIVEN the mode on and, one at a time, each task assigned to each of `claude`, `codex`, `gemini`, `opencode`
- WHEN availability is computed for that task
- THEN it is refused by local only, naming that engine

#### Scenario: Inherited from general [auto]
- GIVEN the mode on, `chat` unassigned and `general` `codex` / `default`
- WHEN availability is computed for `chat`
- THEN it is refused by local only for `codex`, and no other engine is used

#### Scenario: Compaction and summary [auto]
- GIVEN the mode on, memory on, `chat` `ollama` / `llama3:8b` (available) and `summarize` `gemini`
- WHEN a chat turn ends with compaction due, and then `Prefix s` is pressed
- THEN no compaction starts (`summarize` is set, so it is the compaction engine, and it is refused; the truncation marker stands), and the summary runs on `ollama`, the `chat` engine

#### Scenario: Summary on a cloud chat engine [auto]
- GIVEN the mode on, memory on and `chat` `claude` / `sonnet`
- WHEN `Prefix s` is pressed after an answer
- THEN nothing is sent and the notice is the claude guidance

#### Scenario: Orchestrator [auto]
- GIVEN the mode on, `orchestrate` `claude`, and `code` `codex`, `review` `ollama` / `qwen3:14b`
- WHEN a message is sent in an ORCH pane, and in another run a coordinator on `ollama` spawns a `code` and a `review` worker
- THEN the ORCH turn shows the claude guidance; the `code` worker is refused with the codex guidance as its reason and the `review` worker runs

#### Scenario: Local engines unchanged [auto]
- GIVEN the mode on and every task on `ollama` / `llama3:8b`, available
- WHEN each task runs
- THEN the requests are byte-identical to the ones with the mode off

### Requirement: Ollama cloud models are refused [S3]
While the mode is on, an `ollama` task whose model is a cloud model (Q1, recommended option a) MUST be refused by local only like a cloud CLI, with the guidance `ollama: <model> runs in Ollama's cloud, not used while local only is on: pick a local model in the menu (Ctrl+Space m)`. An `ollama` task with no model keeps its existing "no model" result. The rule MUST be the one already used by the GitHub tools (`forwards_off_machine`), and MUST NOT be applied to another engine by name.

#### Scenario: A cloud model [auto]
- GIVEN the mode on and `chat` `ollama` / `gpt-oss:120b-cloud`, listed by Ollama
- WHEN a message is sent
- THEN nothing is sent to Ollama and the guidance names `gpt-oss:120b-cloud`

#### Scenario: Tagged cloud and case [auto]
- GIVEN the mode on and, one at a time, the models `qwen3-coder:cloud`, `GLM-4.6-CLOUD`, `llama3:8b`
- WHEN availability is computed
- THEN the first two are refused and `llama3:8b` is ready

#### Scenario: Not by name on other engines [auto]
- GIVEN the mode on and `chat` `llamacpp` with a model file named `x-cloud.gguf`
- WHEN availability is computed
- THEN it is not refused by local only

### Requirement: The pickers show no cloud engine [S2, S3]
While the mode is on, the task picker MUST NOT list a cloud CLI, and when at least one is detected or assigned it MUST end the engine list with one disabled note row `cloud engines hidden: local only is on`. From S3, the Ollama model level MUST NOT list a cloud model, and when it hides one it MUST end with the disabled note row `Ollama cloud models hidden: local only is on`. A task whose effective engine is refused MUST show the mark ` · off: local only` after its label in the Models tab (see config-menu, Models tab). With the mode off, the picker and the rows MUST be as today.

#### Scenario: Picker without CLIs [auto, TestBackend + insta]
- GIVEN the mode on, ollama and claude available
- WHEN the picker of `code` opens
- THEN it lists `(default · use general)` and `ollama`, then the note row, and no `claude` row

#### Scenario: Row mark [auto]
- GIVEN the mode on and `general` `claude` / `sonnet`
- WHEN the Models tab is rendered
- THEN `general` reads `claude › sonnet · off: local only` and every task that inherits it ends with ` · off: local only`

### Requirement: Turning it on stops cloud work in flight [S4]
When a menu save turns the mode on, every chat turn and compaction run streaming on an engine that is now refused MUST be cancelled (see chat-engines, Cancel releases the engine), and the chat MUST add the transcript notice `local only is on: the <engine> turn was stopped` (a notice, never sent to a model). Every running worker whose engine is now refused MUST be cancelled with the reason `stopped: local only is on`, and queued workers are refused when they would start. Work on local engines MUST NOT be touched. A hand edit of the file applies at the next start. Turning the mode off MUST restart nothing.

#### Scenario: A streaming cloud turn [auto]
- GIVEN a chat streaming on `claude` and another on `ollama`
- WHEN the mode is turned on and saved
- THEN the claude turn is cancelled with the notice, and the ollama turn goes on

#### Scenario: A run with a cloud worker [auto]
- GIVEN a run with a running `codex` worker and a running `ollama` worker
- WHEN the mode is turned on and saved
- THEN the codex worker ends cancelled with `stopped: local only is on` and the ollama worker goes on

### Requirement: Stored data stays where it is [S1]
The mode MUST NOT delete, rewrite or hide saved conversations, run records or usage rows that name a cloud engine. A saved conversation reopened while the mode is on continues on the current `chat` engine, refused if it is a cloud engine. An interrupted run is resumed only when the `orchestrate` engine is local.

#### Scenario: Reopen an old conversation [auto]
- GIVEN a saved conversation whose messages were answered by `claude`, the mode on and `chat` `ollama` / `llama3:8b`
- WHEN it is reopened and a message is sent
- THEN the history goes to ollama only and the stored rows are unchanged

### Requirement: Auto and model calls are off; user calls stay [S5]
While the mode is on:
- the update check MUST NOT run at start or on `r` on the Updates row, and the newer llama-server build check MUST NOT run, as if `[updates] check = false`; the Updates row value MUST end with ` · checks off: local only`, and `r` there MUST show `update checks are off while local only is on`;
- detection MUST NOT run the codex or claude model listing (`codex debug models`, `claude -p /model`); the version probe stays;
- the GitHub read tools MUST NOT be offered to any engine (Q4, recommended option a), `/github` MUST answer `GitHub tools are off while local only is on` and grant nothing, and the GitHub row of the Tools tab MUST read `off: local only`;
- the saved `[updates] check` and `[ai.tools] github` values MUST NOT be rewritten.

These user calls MUST stay as they are: the update install pane, Hugging Face search, downloads and token, the Ollama library and `ollama pull`, the llama-server install wizard, the Feedback form send (with its similar-issue search), and the browser opener (Q4).

#### Scenario: No update check [auto]
- GIVEN the mode on, `[updates] check` unset and a stale update cache
- WHEN the app starts and `r` is pressed on the Updates row
- THEN no request reaches the fake crates.io and the footer shows the text above

#### Scenario: No listing [auto]
- GIVEN the mode on and fake `codex` and `claude` on PATH that record their argv
- WHEN detection runs, forced
- THEN each was run for its version only

#### Scenario: GitHub tools off [auto]
- GIVEN the mode on, `[ai.tools] github = true` and a chat on `ollama`
- WHEN the tool list is built and `/github` is sent
- THEN no `gh_` read tool is listed, `/github` shows the text above and no `gh` process starts

#### Scenario: User calls stay [auto]
- GIVEN the mode on
- WHEN a Hugging Face search runs and the Feedback form is sent
- THEN both reach their fake services as with the mode off

### Requirement: MCP servers stay off unless declared local [S6]
While the mode is on (Q3, recommended option b), a user MCP server MUST be started only when its `[[ai.mcp.servers]]` entry has `local = true`. Each other enabled server of the chat's active list MUST NOT be started, and the chat MUST add, once per set, the notice `<name>: MCP server not started: local only is on (set local = true on its entry if it never calls out)`. `local` is the user's statement; lazarobox MUST NOT check it. A catalog connector (see the mcp-connectors change) MUST NOT carry `local = true` in its snippet. With the mode off, `local` changes nothing.

#### Scenario: One local, one not [auto]
- GIVEN the mode on and enabled servers `fs` with `local = true` and `ga4` without it
- WHEN a chat set starts
- THEN `fs` starts, `ga4` does not, and the notice names `ga4`

#### Scenario: Mode off [auto]
- GIVEN the mode off and the same two servers
- WHEN a chat set starts
- THEN both start, as today

### Requirement: What counts as local is stated [S1, each slice]
The README MUST say, in the section "🛡️ Tus datos: qué sale de tu máquina", from S1: that the mode is a choice the user makes in their own `config.toml` and menu, not a lock, and that a locked version is planned (#659); from which version the mode is enforced, and that an older build reports `ai.local_only` as an unknown key without enforcing it; that local means an engine that answers on `127.0.0.1` of this machine, and that a port forwarded to another machine (an SSH tunnel, a proxy) sends the data there without lazarobox seeing it (Q2); and what stays on in the mode (the user calls above, commands the user approves, the user's own shell). Each later slice adds what it ships: Ollama cloud models refused (S3), the calls turned off (S5), MCP servers and `local` (S6). The table of "Seguridad / modelo de amenazas" gains a row for the mode.

#### Scenario: README states the limits [manual]
- GIVEN the README after S1
- WHEN the section is read
- THEN it holds each S1 item above, in Spanish, and the "Planificado" list no longer lists #657 (it keeps #658, #659 and #570)
