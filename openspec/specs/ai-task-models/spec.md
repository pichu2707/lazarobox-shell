# AI Task Models Specification

## Purpose

Defines which AI tasks exist, how each task gets an engine and a model, and which engines and models the picker offers and with which marks. It is pure data and rules; the screens are in config-menu and the file format is in configuration. Nothing is downloaded and nothing is fetched: the only network access is the localhost detection of ai-engine-detection. Tags: [auto] = `cargo test`; [manual] = real machine.

## Requirements

### Requirement: Fixed task set [S1, W8]
The tasks MUST be fixed and ordered: `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, `simple`. Task ids are lowercase and are the `config.toml` table names. There is no way to add, rename or remove a task. A task assignment is an engine id plus a model name. A task is "assigned" only when its engine is non-empty; a model without an engine is treated as unassigned (the file keeps it).
(Previously: six tasks, `simple` did not exist)

#### Scenario: Task set integrity [auto]
- GIVEN the task table
- WHEN it is validated
- THEN it has exactly `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, `simple` in that order, with unique ids

### Requirement: Fallback to general
A task other than `general` that is not assigned MUST use the `general` assignment as its effective assignment. `general` has no fallback: with no engine it is "not set". Row labels: an unassigned non-general task shows `(default)`; an unset `general` shows `(not set)`; an assigned task shows `<engine> › <model>`, or only `<engine>` when its model is empty.

#### Scenario: Effective assignment [auto]
- GIVEN general is `claude` / `sonnet`, code is `ollama` / `llama3:8b`, chat is unassigned
- WHEN effective assignments are resolved
- THEN code is `ollama` / `llama3:8b` and chat is `claude` / `sonnet`

#### Scenario: Nothing assigned [auto]
- GIVEN no task is assigned
- WHEN effective assignments are resolved
- THEN every task has no effective engine, and the labels are `(not set)` for general and `(default)` for the other four

#### Scenario: Model without engine is unassigned [auto]
- GIVEN code has model `x` and an empty engine
- WHEN its label is built
- THEN it reads `(default)`

### Requirement: Set-all summary
The aggregate `Set all tasks` value MUST be derived from the five EFFECTIVE (resolved) assignments of the draft: when all five resolve to the same value it shows that value (`(not set)` when all resolve to nothing, otherwise `<engine> › <model>`); otherwise it shows `(mixed)`. An unassigned non-general task resolves to general, so choosing `none` for Set all (everything unassigned) reads `(not set)`.

#### Scenario: Aggregate values [auto]
- GIVEN (a) all five are `claude` / `opus`, (b) none assigned, (c) general `claude` / `opus` and the other four unassigned, (d) general `claude` / `opus` and code `ollama` / `llama3:8b`
- WHEN the aggregate is computed
- THEN it reads `claude › opus`, `(not set)`, `claude › opus` and `(mixed)` respectively

### Requirement: Engine choices
The engine list of a picker MUST contain, in this order: the unassign entry (`(default · use general)` for a non-general task, `none` for `general` and `Set all tasks`); then the engines whose status is `available`, in catalog order, each with its status text (see ai-engine-detection); then, if not already listed, every engine id assigned anywhere in the draft (any of the five tasks, for every target) that is not available, with the status text if still `detecting…` and otherwise `(not detected)`. These rows are selectable, so the user can reopen or keep them. Engines that are not available and not assigned MUST NOT be listed. Detection alone MUST NOT change any assignment.

#### Scenario: Engine list [auto]
- GIVEN ollama (2 models) and claude are available, lmstudio is `not running`, and code is assigned to `codex` which is `not installed`
- WHEN the engine list for code is built
- THEN it is `(default · use general)`, `ollama`, `claude`, `codex (not detected)` and never `lmstudio`

#### Scenario: Unassign entry label [auto]
- GIVEN the lists for general and for Set all tasks
- WHEN they are built
- THEN their first entry is `none`

### Requirement: CLI model catalog
For the CLI engines each entry is a `label` shown by the picker and a `value` that is saved and passed to the CLI (the label is display only). `claude` offers `default (CLI picks)` = `default` and then every alias its `/model` listing names (see ai-engine-detection, Claude model listing), in reply order, each as its own label and value (`sonnet`, `opus`, `haiku`, `fable`, `best`, `sonnet[1m]`, `opus[1m]`, `fable[1m]`, `opusplan`), with no version names; when the listing is missing, failed or empty it offers its static list `default (CLI picks)` = `default`, `Opus 5.5` = `opus`, `Sonnet 5.5` = `sonnet`, `Haiku 5.5` = `haiku` and `Fable 5.1` = `fable`, plus the `model list unavailable: <reason>` note when there is a reason; the values are CLI aliases, which always resolve to the newest model of each family. `gemini` offers `default (auto)` = `default` (no `-m` flag, so the CLI runs its own `auto`), `Flash 3.8` = `gemini-3.8-flash`, `Flash 3.5` = `gemini-3.5-flash` and `Flash Lite 3.5` = `gemini-3.5-flash-lite`; these are explicit ids, not the CLI's `flash` and `pro` aliases, which may point at retired 2.5 models. `opencode` offers `default` (label and value). `codex` offers `default (CLI picks)` = `default` and then the models its account lists (see ai-engine-detection, Codex model listing): the label is the `display_name` and the saved value is the `slug`, in the account's priority order; when the listing is missing or failed, only `default (CLI picks)` is offered. Every CLI list ends with the entry `custom…`. CLI entries are selectable and have no mark (no `✓`), because their availability is not checkable. A saved model is matched to an entry by value, so an existing `opus` or `default` shows its label. A model already assigned for that engine and absent from the catalog (for example `gemini-2.5-flash`) MUST NOT be an error: it is listed under its own value before `custom…`, selectable and unmarked. Task rows, the aggregate and the chat statusline keep showing the saved value (`claude › opus`).

#### Scenario: CLI lists [auto]
- GIVEN claude and codex are available
- WHEN their model lists are built
- THEN claude (no listing) reads `default (CLI picks)`, `Opus 5.5`, `Sonnet 5.5`, `Haiku 5.5`, `Fable 5.1`, `custom…`, gemini reads `default (auto)`, `Flash 3.8`, `Flash 3.5`, `Flash Lite 3.5`, `custom…`, and codex reads `default (CLI picks)`, `custom…` (no account list), none with a mark

#### Scenario: Codex account list [auto]
- GIVEN codex is available and its listing holds `GPT-6.1-Sol` = `gpt-6.1-sol` and `GPT-6-Luna` = `gpt-6-luna`
- WHEN the models of codex are listed
- THEN the rows are `default (CLI picks)`, `GPT-6.1-Sol`, `GPT-6-Luna`, `custom…`, and choosing the second saves `gpt-6.1-sol`

#### Scenario: Claude models from its listing [auto]
- GIVEN claude is available and its listing names `sonnet`, `opus`, `haiku`, `fable`, `best`, `sonnet[1m]`, `opus[1m]`, `fable[1m]`, `opusplan`
- WHEN the models of claude are listed
- THEN the rows are `default (CLI picks)`, `sonnet`, `opus`, `haiku`, `fable`, `best`, `sonnet[1m]`, `opus[1m]`, `fable[1m]`, `opusplan`, `custom…`, and each saves its own alias

#### Scenario: Claude falls back to its static list [auto]
- GIVEN claude is available and its listing timed out
- WHEN the models of claude are listed
- THEN the rows are the static claude list, then `model list unavailable: timed out after 5 s`, then `custom…`

#### Scenario: Saved value shows its label [auto]
- GIVEN code is saved as `claude` / `opus` (or `default`)
- WHEN the claude model list is built
- THEN the row labeled `Opus 5.5` (or `default (CLI picks)`) is the current one, and choosing it saves `opus` (or `default`)

#### Scenario: Saved value outside the catalog [auto]
- GIVEN code is saved as `gemini` / `gemini-2.5-flash`
- WHEN the gemini model list is built
- THEN a row `gemini-2.5-flash` is listed as current before `custom…`, with no error

#### Scenario: Assigned custom model listed [auto]
- GIVEN code is assigned `gemini` / `gem-exp`
- WHEN gemini's model list is built for code
- THEN it reads `default`, `gem-exp`, `custom…`, all selectable

### Requirement: Codex model outside the account list
When the codex or claude listing is non-empty, a model of that engine that is assigned or typed as custom, is not `default` and is not in the listing (for claude, a full model ID such as `claude-opus-5-5`) MUST be listed after the account models, selectable, with the warning mark `not in your account's list`; the task row of that assignment MUST end with ` · not in your account's list`, and the picker footer on that row MUST read `not in your account's list · kept as typed, the list may be stale`. The warning never blocks: detection can be stale. When the listing is empty or failed, and for every other CLI engine, no warning is shown.

#### Scenario: Unlisted codex model warned [auto]
- GIVEN codex lists `gpt-6.1-sol` and code is assigned `codex` / `gpt-5.4`
- WHEN the codex models are listed
- THEN `gpt-5.4` appears before `custom…`, selectable and marked `not in your account's list`, and the code task row reads `codex › gpt-5.4 · not in your account's list`

#### Scenario: No listing, no warning [auto]
- GIVEN the codex listing failed and code is assigned `codex` / `gpt-5.4`
- WHEN the codex models are listed
- THEN `gpt-5.4` is unmarked and the task row has no warning

### Requirement: Server model list and marks
For the server engines (`ollama`, `lmstudio`, `llamacpp`, `vllm`) the model list MUST be, in this order: (a) the detected models, in response order, each selectable and marked `✓`; (b) any model assigned in the draft for that engine that is not detected, NOT selectable, marked `download required` and `●` when it is the current assignment of the target (the assignment is kept); (c) the curated static catalog entries that are in neither group, each NOT selectable, drawn muted and marked `download required`. All rows, disabled ones included, can hold the cursor; Enter on a disabled row does nothing. Matching is exact string equality, except that for Ollama a name without a tag equals the same name with `:latest`; a name is listed once and the detected spelling wins. Servers offer no `custom…` entry. A server whose list would be empty shows a single muted non-selectable row `no models detected`. Before the first detection result, server model rows are disabled and marked `checking…`.

#### Scenario: Detected and catalog [auto]
- GIVEN ollama is available with `llama3:8b` and the curated catalog contains `llama3:8b` and `mistral:7b`
- WHEN ollama's list is built
- THEN it is `llama3:8b ✓` (selectable) followed by `mistral:7b download required` (not selectable), and `llama3:8b` appears once

#### Scenario: Tag-less name equals latest [auto]
- GIVEN ollama detects `llama3:latest` and code is assigned `ollama` / `llama3`
- WHEN the list for code is built
- THEN `llama3:latest` appears once, with `✓`, and no `download required` row for `llama3`

#### Scenario: Assigned model no longer downloaded [auto]
- GIVEN code is assigned `ollama` / `old:7b`, and ollama lists only `llama3:8b`
- WHEN the list for code is built
- THEN the order is `llama3:8b ✓`, `old:7b download required ●` (not selectable), then the curated rows, and the assignment of code is unchanged

#### Scenario: Server without models or catalog [auto]
- GIVEN lmstudio is available with `{"data":[]}`
- WHEN its list is built
- THEN the only row is `no models detected`, not selectable

#### Scenario: Server not available [auto]
- GIVEN ollama is `not running` and code is assigned `ollama` / `llama3:8b`
- WHEN the list for code is built
- THEN no row has `✓`, `llama3:8b` is shown muted, `●` and not selectable with no `download required` mark, a muted row `engine not detected` is present, and no curated rows are listed

### Requirement: Curated Ollama catalog
The code MUST ship a static catalog of well-known Ollama model tags, between 8 and 12 entries, with unique non-empty names. Only Ollama has a curated catalog (lmstudio, llamacpp and vllm have none). The catalog MUST be informational: it is never fetched and never refreshed from any network, and the source and the user documentation MUST say it may be stale. It is also the fallback list of the Ollama download flow when the library page cannot be read. lazarobox MUST NOT pull or install any model through a server, except the one `POST /api/pull` of ollama-download, sent after the user confirms it. No chat request MUST ever ask a server to download a model. The only requests lazarobox sends are:
- the localhost detection requests;
- the loopback chat requests of chat-engines, sent only when the user sends a chat message;
- the update check of the updates feature;
- the Hugging Face requests of model-download, sent only while the user is in its download flow;
- the ollama.com and registry.ollama.ai requests and the loopback pull of ollama-download, sent only while the user is in its download flow.

A model is downloaded only by those two flows, after the user confirms it.

#### Scenario: Catalog integrity [auto]
- GIVEN the curated Ollama catalog
- WHEN it is validated
- THEN it has between 8 and 12 entries, all unique and non-empty

#### Scenario: No download path outside the flows [auto]
- GIVEN a download-required model row
- WHEN the cursor is on it and every key is pressed
- THEN no request, command or effect is produced and the assignment is unchanged

#### Scenario: Chat never pulls a model [auto]
- GIVEN `chat` assigned to a server model that is not downloaded
- WHEN a message is sent
- THEN no request is sent to any server and the chat shows the `download required` guidance

#### Scenario: Docs say stale [manual]
- GIVEN the README
- WHEN it is read
- THEN it states that the Ollama catalog is informational and may be out of date, and that a model is downloaded only from the Hugging Face or the Ollama flow, after the user confirms it

### Requirement: Model marks on task rows
Where a task row shows an assigned model of an available server engine, a model not among the detected models MUST carry the suffix ` · download required`; a detected model carries no suffix on the row (`✓` is a picker mark). An assigned engine that is not available (status known) MUST carry the suffix ` (not detected)` after the engine id, and then no model mark is applied. While no detection result exists yet, no suffix or mark is applied. CLI assignments never get a model mark.

#### Scenario: Row marks [auto]
- GIVEN ollama is available with `llama3:8b`; code is `ollama` / `old:7b`, chat is `ollama` / `llama3:8b`, review is `codex` / `default` with codex `not installed`
- WHEN the rows are built
- THEN they read `ollama › old:7b · download required`, `ollama › llama3:8b`, `codex (not detected) › default`

#### Scenario: Before the first detection [auto]
- GIVEN no detection result yet and code is `ollama` / `llama3:8b`
- WHEN the row is built
- THEN it reads `ollama › llama3:8b` with no suffix

### Requirement: Custom model validity
A custom model name (CLI engines only) MUST be trimmed, MUST NOT be empty, MUST NOT contain control characters, and MUST be at most 128 characters; input beyond the cap is not accepted. A name that is empty after trimming is rejected and nothing is assigned.

#### Scenario: Validation [auto]
- GIVEN the inputs `  my-model  `, an empty string, a string of spaces, and 129 characters
- WHEN each is validated
- THEN they give `my-model`, rejected, rejected, and the first 128 characters respectively

### Requirement: Assignment actions
Choosing an engine entry then a selectable model assigns that engine and model to the task; choosing the unassign entry clears the task's engine and model. For `Set all tasks` the same choice is written to all five tasks, `general` included; choosing `none` clears all five, so every non-general task falls back to the empty `general`. Choosing a model for `Set all tasks` writes all five tasks. A non-selectable entry MUST NOT change any assignment.

#### Scenario: Assign one task [auto]
- GIVEN code is unassigned
- WHEN `claude` then `opus` is chosen for code
- THEN code is `claude` / `opus` and the other four tasks are unchanged

#### Scenario: Set all [auto]
- GIVEN tasks are mixed
- WHEN `claude` then `sonnet` is chosen for Set all tasks
- THEN all five tasks are `claude` / `sonnet` and the aggregate reads `claude › sonnet`

#### Scenario: Set all none [auto]
- GIVEN all five are assigned
- WHEN `none` is chosen for Set all tasks
- THEN all five are cleared, general reads `(not set)`, the others read `(default)`, and the aggregate reads `(not set)`

#### Scenario: Unassign a task [auto]
- GIVEN code is `claude` / `opus`
- WHEN `(default · use general)` is chosen for code
- THEN code is cleared and reads `(default)`

### Requirement: Deleting a model in use
A model that is in use MUST NOT be deleted from the menu, and the refusal MUST say who uses it. The check runs when `d` is pressed and again when the delete is confirmed. A model is in use when a task assignment names it, in the menu's draft or in the saved config, with the Ollama tag-less equals `:latest` rule (`<name> is used by task <task> · assign another model first`, or `tasks <a>, <b>`), or when a running chat turn uses it (`<name> is in use by a running chat turn`). A llama.cpp model file is also in use when it is the managed llama-server's model, in the draft or in the saved config (`<file> is the model of the managed llama-server · choose another one for it first`), or when a running llama-server lists it (`<file> is loaded by the running llama-server`). The runtime refuses again any file that a task assignment or the managed model names under another spelling (a link, `..`), comparing the files with links followed. Assignments are never changed by the refusal: the user reassigns first.

#### Scenario: The managed model [auto]
- GIVEN the managed llama-server's model, in the draft or the saved config
- WHEN `d` or the confirming `y` is pressed on its row
- THEN the delete is refused, the notice names the managed llama-server, and nothing is sent

#### Scenario: The draft and the saved config both count [auto]
- GIVEN a model assigned to `chat` in the saved config but not in the draft, or the other way round
- WHEN `d` is pressed on its row
- THEN the delete is refused and the notice names `chat`, and nothing is sent

#### Scenario: A running turn [auto]
- GIVEN a chat turn running on the model
- WHEN `d` is pressed on its row
- THEN the delete is refused and the notice says a running chat turn uses it

#### Scenario: In use after the confirmation opened [auto]
- GIVEN the confirmation of a model, drawn
- WHEN a task or a turn starts using it and `y` is pressed
- THEN nothing is sent, the screen closes and the notice names who uses it

### Requirement: Runtime use of every task assignment [S1]
Each task MUST be used at runtime through its effective assignment (own assignment, else `general`; see Fallback to general), read from the current configuration on every use, so a saved change applies from the next message or the next worker spawn. A chat turn uses `chat`, except in an orchestrating pane (global gate on and the pane's ORCH mode on), where the turn uses `orchestrate`; each orchestrator worker uses the assignment of its kind (`general`, `code`, `chat`, `review`, `summarize`). The model MUST be validated against detection before use: for a server engine the model MUST be non-empty and among the detected models (exact match, with the Ollama tag-less equals `:latest` rule); otherwise the chat shows the model guidance (or the worker is `refused`) and nothing is sent. For a CLI engine the model is not validated (empty and `default` mean the CLI's own default). The runtime MUST NOT change any assignment on its own, MUST NOT pick another engine when the effective one is unusable, and MUST NOT write the config; only a user choice in the menu changes an assignment. The picker opened from a worker failure alert does not change any assignment: its choice is a run-scoped override (see agent-orchestrator) that is never written to the draft or the file.

#### Scenario: Effective assignment used [auto]
- GIVEN `general` is `claude` / `sonnet` and `chat` is `ollama` / `llama3:8b`, both available
- WHEN a chat message is sent
- THEN the turn goes to ollama with `llama3:8b`

#### Scenario: Fallback to general [auto]
- GIVEN `chat` is unassigned and `general` is `claude` / `sonnet`, claude available
- WHEN a chat message is sent
- THEN the turn goes to claude with `sonnet`

#### Scenario: Server model must be detected [auto]
- GIVEN `chat` is `lmstudio` / `m1` and lmstudio lists only `m2`
- WHEN a chat message is sent
- THEN nothing is sent and the guidance names `m1` and `download required`

#### Scenario: Empty server model [auto]
- GIVEN `chat` is `ollama` with an empty model
- WHEN a chat message is sent
- THEN nothing is sent and the guidance says no model is chosen

#### Scenario: CLI model not validated [auto]
- GIVEN `chat` is `claude` / `custom-x` and claude is available
- WHEN a chat message is sent
- THEN the turn is started with that model name

#### Scenario: Assignment never changed [auto]
- GIVEN `chat` is `ollama` / `llama3:8b` and ollama is `not running`
- WHEN a chat message is attempted
- THEN the assignments are unchanged and no other engine is contacted

#### Scenario: Saved change applies to the next message [auto]
- GIVEN an open chat and the menu saves `chat` as `claude` / `opus`
- WHEN the next message is sent
- THEN the turn goes to claude with `opus`

#### Scenario: Worker kind uses its own row [auto]
- GIVEN an orchestrating pane, `review` is `codex` / `default` and `code` is `claude` / `sonnet`
- WHEN a `review` worker and then a `code` worker are spawned
- THEN they run on codex / `default` and claude / `sonnet` respectively

#### Scenario: Summarize row is now used [auto]
- GIVEN `summarize` is `claude` / `haiku`
- WHEN a `summarize` worker is spawned
- THEN it runs on claude / `haiku`

### Requirement: Tool marks on installed Ollama models
In the Ollama model list, each detected model (the `✓` rows) MUST carry a tool mark next to `✓`, decided only by its recorded size and capability (see ai-engine-detection, Ollama model size and tool capability):
- capability `tools` and size 3B or more: the mark `🔧`, so the row reads `<name> ✓ 🔧`;
- capability `tools` and size under 3B: the mark `tools: unreliable`, so the row reads `<name> ✓ tools: unreliable`;
- capability `no tools`, capability `unknown`, or size `unknown`: no tool mark. Unknown never produces a mark.

Exactly 3B counts as 3B or more. The mark describes the model, not the configuration: it MUST show the same when `[ai.tools] enabled = false`. The mark is display only: it MUST NOT change selectability, the assignment, the saved value or the task row text. Rows not detected (`download required`, curated catalog) get no tool mark. A mark too long for the row is clipped by display width and never wraps or misaligns the row.

#### Scenario: Tools and large enough [auto]
- GIVEN ollama detects `llama3:8b` with capability `tools` and size 8B
- WHEN the list is built
- THEN the row reads `llama3:8b ✓ 🔧`

#### Scenario: Tools but small [auto]
- GIVEN ollama detects `tiny:1b` with capability `tools` and size 1B
- WHEN the list is built
- THEN the row reads `tiny:1b ✓ tools: unreliable`

#### Scenario: Exactly 3B [auto]
- GIVEN ollama detects `mid:3b` with capability `tools` and size 3B
- WHEN the list is built
- THEN the row reads `mid:3b ✓ 🔧`

#### Scenario: No tools declared [auto]
- GIVEN ollama detects `gemma:7b` with capability `no tools`
- WHEN the list is built
- THEN the row reads `gemma:7b ✓` with no tool mark

#### Scenario: Unknown capability or size [auto]
- GIVEN ollama detects `a:7b` with capability `unknown`, and `b:latest` with capability `tools` and size `unknown`
- WHEN the list is built
- THEN both rows read `✓` with no tool mark

#### Scenario: Tools setting off [auto]
- GIVEN `[ai.tools] enabled = false` and ollama detects `llama3:8b` with capability `tools` and size 8B
- WHEN the list is built
- THEN the row still reads `llama3:8b ✓ 🔧`

#### Scenario: Mark does not change behaviour [auto]
- GIVEN a marked row
- WHEN Enter is pressed on it
- THEN the model is assigned exactly as for an unmarked `✓` row

#### Scenario: Not detected rows [auto]
- GIVEN code is assigned `ollama` / `old:7b` (not detected) and the curated catalog holds `mistral:7b`
- WHEN the list is built
- THEN `old:7b` and `mistral:7b` carry only `download required` and no tool mark

### Requirement: Tool mark on CLI engine rows
In the engine list of a picker, the engine rows `claude`, `codex` and `gemini` MUST carry the tool mark `🔧`, because lazarobox gives those engines its tools (the engines in the chat tool gate, `MCP_VERIFIED`). The mark follows that gate, not the configuration: it MUST show whether `[ai.tools] enabled` is true or false. It is shown together with the status text, which stays unchanged and first, with the mark after it. The `ollama` engine row MUST carry no tool mark, because only its models carry marks. The rows of `opencode`, `lmstudio`, `llamacpp` and `vllm` carry no tool mark. The CLI model rows (`Opus 5.5`, `default`, `custom…` and so on) MUST carry no tool mark. An engine listed as `(not detected)` keeps its text and gets no tool mark. A mark is never wrong: an engine the jail refuses (`refused · …`) and a `claude` that shows `update needed` cannot run tools, so their rows carry no tool mark.

#### Scenario: CLI engine rows [auto]
- GIVEN claude, codex, gemini, opencode and ollama are available
- WHEN the engine list is built
- THEN the `claude`, `codex` and `gemini` rows carry `🔧`, and the `opencode` and `ollama` rows carry no tool mark

#### Scenario: Shown with tools off [auto]
- GIVEN `[ai.tools] enabled = false` and claude is available
- WHEN the engine list is built
- THEN the `claude` row still carries `🔧`

#### Scenario: CLI model rows unmarked [auto]
- GIVEN claude is available
- WHEN its model list is built
- THEN no row carries a tool mark

#### Scenario: Refused or outdated engine [auto]
- GIVEN `codex` is refused by the jail, or `claude` is older than the minimum version
- WHEN the engine list is built
- THEN the row keeps its status text (`refused · …`, `update needed: …`) and carries no tool mark

#### Scenario: Not detected engine [auto]
- GIVEN code is assigned `codex`, which is `not installed`
- WHEN the engine list is built
- THEN the row reads `codex (not detected)` with no tool mark

### Requirement: Tool mark text fallback
Unless `[ai] tool_mark` overrides it (see the configuration spec), the tool mark MUST render as the emoji `🔧` only when the terminal can show it. When `TERM=linux`, or the locale is not UTF-8, every place that would show `🔧` MUST show the text `tools` instead. `tools: unreliable` is plain text and does not change. The choice MUST be made in one place, so every screen (engine rows, installed model rows, Ollama download screens) agrees. The fallback MUST NOT change selectability or alignment: rows stay within their width.

#### Scenario: Linux console [auto]
- GIVEN `TERM=linux` and a model with capability `tools` and size 8B
- WHEN the row is rendered
- THEN it reads `<name> ✓ tools`

#### Scenario: Non-UTF-8 locale [auto]
- GIVEN `TERM=xterm-256color` and a locale that is not UTF-8
- WHEN an engine row for `claude` is rendered
- THEN it carries `tools` and no `🔧`

#### Scenario: Normal terminal [auto]
- GIVEN `TERM=xterm-256color` and a UTF-8 locale
- WHEN the same rows are rendered
- THEN they carry `🔧`

#### Scenario: Real machine [manual]
- GIVEN a real Ollama with a tool model of 3B or more, a tool model under 3B and a model without tools
- WHEN the model picker is opened on a UTF-8 terminal and then on `TERM=linux`
- THEN the first shows `🔧`, the second `tools: unreliable`, the third no mark, and the console shows `tools` in place of `🔧`

### Requirement: Orchestrate fallback and recommendation [S1]
`orchestrate` follows Fallback to general: unassigned, it uses the `general` assignment, and its row shows `(default)`. Ollama (and the other server engines) MUST remain selectable for `orchestrate` but MUST be marked `not recommended` on that row's engine list and on the task row (` · not recommended`), because small local models cannot orchestrate. The mark MUST NOT block, change or clear an assignment.

#### Scenario: Orchestrate falls back [auto]
- GIVEN `orchestrate` unassigned and `general` is `claude` / `opus`
- WHEN effective assignments are resolved
- THEN `orchestrate` is `claude` / `opus`

#### Scenario: Ollama marked not recommended [auto]
- GIVEN `orchestrate` is `ollama` / `llama3:8b` (available)
- WHEN the row and the engine list for `orchestrate` are built
- THEN the row reads `ollama › llama3:8b · not recommended`, the `ollama` entry shows `not recommended`, both are selectable, and the assignment is unchanged

#### Scenario: Other tasks unmarked [auto]
- GIVEN `code` is `ollama` / `llama3:8b`
- WHEN the row is built
- THEN no `not recommended` mark is shown

### Requirement: Set all excludes Orchestrate [S1]
`Set all tasks` MUST write, clear and summarize the five tasks `general`, `code`, `chat`, `review` and `summarize` only, and MUST NOT read or write `orchestrate`.

#### Scenario: Set all leaves the orchestrator alone [auto]
- GIVEN `orchestrate` is `claude` / `opus`
- WHEN `codex` / `default` is chosen for Set all tasks
- THEN the five tasks are `codex` / `default` and `orchestrate` is still `claude` / `opus`, and the aggregate ignores `orchestrate`

### Requirement: Simple fallback and use [W8]
`simple` follows Fallback to general: unassigned, it uses the `general` assignment and its row shows `(default)`, which MUST be visible. If the effective assignment is a cloud engine, the worker is not a cheap author, and no notice other than the visible `(default)` row is required, because the menu chose it. An orchestrator worker of kind `simple` MUST run on the effective `simple` assignment read at spawn time. The runtime MUST NOT pick another engine or model, MUST NOT escalate to the cloud, and MUST NOT write the config. The server engines are not marked `not recommended` for `simple`.

#### Scenario: Simple falls back [W8, auto]
- GIVEN `simple` unassigned and `general` is `claude` / `sonnet`
- WHEN a `simple` worker is spawned
- THEN it runs on claude / `sonnet` and the row reads `(default)`

#### Scenario: Simple on a local model [W8, auto]
- GIVEN `simple` is `ollama` / `llama3:8b` (available)
- WHEN a `simple` worker is spawned
- THEN it runs on ollama / `llama3:8b`, and no other engine is contacted

#### Scenario: Unusable simple engine [W8, auto]
- GIVEN `simple` is `ollama` with a model that is not downloaded
- WHEN a `simple` worker is spawned
- THEN it ends `refused` naming the model and `download required`, the assignments are unchanged and no other engine is contacted

### Requirement: Set all excludes Simple [W8]
`Set all tasks` MUST write, clear and summarize the five tasks `general`, `code`, `chat`, `review` and `summarize` only, and MUST NOT read or write `simple` (as for `orchestrate`).

#### Scenario: Set all leaves simple alone [W8, auto]
- GIVEN `simple` is `ollama` / `llama3:8b`
- WHEN `codex` / `default` is chosen for Set all tasks
- THEN the five tasks are `codex` / `default`, `simple` is still `ollama` / `llama3:8b`, and the aggregate ignores `simple`
