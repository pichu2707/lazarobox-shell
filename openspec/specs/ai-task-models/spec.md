# AI Task Models Specification

## Purpose

Defines which AI tasks exist, how each task gets an engine and a model, and which engines and models the picker offers and with which marks. It is pure data and rules; the screens are in config-menu and the file format is in configuration. Nothing is downloaded and nothing is fetched: the only network access is the localhost detection of ai-engine-detection. Tags: [auto] = `cargo test`; [manual] = real machine.

## Requirements

### Requirement: Fixed task set
The tasks MUST be fixed and ordered: `general`, `code`, `chat`, `review`, `summarize`. Task ids are lowercase and are the `config.toml` table names. There is no way to add, rename or remove a task. A task assignment is an engine id plus a model name. A task is "assigned" only when its engine is non-empty; a model without an engine is treated as unassigned (the file keeps it).

#### Scenario: Task set integrity [auto]
- GIVEN the task table
- WHEN it is validated
- THEN it has exactly `general`, `code`, `chat`, `review`, `summarize` in that order, with unique ids

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
For the CLI engines the model list MUST be a catalog (static) and, for `opencode`, the models its own listing gave at detection (see ai-engine-detection), and each entry is a `label` shown by the picker and a `value` that is saved and passed to the CLI (the label is display only). `claude` offers `default (CLI picks)` = `default`, `Opus 5.5` = `opus`, `Sonnet 5.5` = `sonnet`, `Haiku 5.5` = `haiku` and `Fable 5.1` = `fable`; the values are CLI aliases, which always resolve to the newest model of each family. `gemini` offers `default (auto)` = `default` (no `-m` flag, so the CLI runs its own `auto`), `Flash 3.8` = `gemini-3.8-flash`, `Flash 3.5` = `gemini-3.5-flash` and `Flash Lite 3.5` = `gemini-3.5-flash-lite`; these are explicit ids, not the CLI's `flash` and `pro` aliases, which may point at retired 2.5 models. `codex` offers `default` (label and value). `opencode` offers `default` (label and value), then every model its listing gave (`provider/model` ids such as `openai/gpt-5.4`, label and value the same, in the CLI's order); without a listing it offers just `default`. Every CLI list ends with the entry `custom…`. CLI entries are selectable and have no mark (no `✓`), because their availability is not checkable. A saved model is matched to an entry by value, so an existing `opus` or `default` shows its label. A model already assigned for that engine and absent from the catalog and from the listing (for example `gemini-2.5-flash`) MUST NOT be an error: it is listed under its own value before `custom…`, selectable and unmarked. Task rows, the aggregate and the chat statusline keep showing the saved value (`claude › opus`).

#### Scenario: CLI lists [auto]
- GIVEN claude and codex are available
- WHEN their model lists are built
- THEN claude reads `default (CLI picks)`, `Opus 5.5`, `Sonnet 5.5`, `Haiku 5.5`, `Fable 5.1`, `custom…`, gemini reads `default (auto)`, `Flash 3.8`, `Flash 3.5`, `Flash Lite 3.5`, `custom…`, and codex reads `default`, `custom…`, none with a mark

#### Scenario: opencode lists its detected models [auto]
- GIVEN opencode is available with the detected models `default`, `openai/gpt-5.4` and `opencode/big-pickle`
- WHEN its model list is built
- THEN it reads `default`, `openai/gpt-5.4`, `opencode/big-pickle`, `custom…`, none with a mark; with only `default` detected it reads `default`, `custom…`; a saved model absent from the listing is listed before `custom…`, and one present in it is the single current row

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
The code MUST ship a static catalog of well-known Ollama model tags, between 8 and 12 entries, with unique non-empty names. Only Ollama has a curated catalog (lmstudio, llamacpp and vllm have none). The catalog MUST be informational: it is never fetched, never refreshed from any network, and the source and the user documentation MUST say it may be stale. lazarobox MUST NOT download, pull or install any model, and MUST NOT send any request other than the localhost detection requests and the loopback chat requests of chat-engines (sent only when the user sends a chat message). No chat request MUST ever ask a server to download a model.

#### Scenario: Catalog integrity [auto]
- GIVEN the curated Ollama catalog
- WHEN it is validated
- THEN it has between 8 and 12 entries, all unique and non-empty

#### Scenario: No download path [auto]
- GIVEN a download-required model
- WHEN the cursor is on it and every key is pressed
- THEN no request, command or effect is produced and the assignment is unchanged

#### Scenario: Chat never pulls a model [auto]
- GIVEN `chat` assigned to a server model that is not downloaded
- WHEN a message is sent
- THEN no request is sent to any server and the chat shows the `download required` guidance

#### Scenario: Docs say stale [manual]
- GIVEN the README
- WHEN it is read
- THEN it states that the Ollama catalog is informational, may be out of date, and that nothing is downloaded by lazarobox

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

### Requirement: Runtime use of the chat assignment
The chat MUST use the effective assignment of the `chat` task (own assignment, else `general`; see Fallback to general), read from the current configuration on every send, so a saved change applies from the next message. The model MUST be validated against detection before a send: for a server engine the model MUST be non-empty and among the detected models (exact match, with the Ollama tag-less equals `:latest` rule); otherwise the chat shows the model guidance and sends nothing. For a CLI engine the model is not validated (empty and `default` mean the CLI's own default). The chat MUST NOT change any assignment, MUST NOT pick another engine when the effective one is unusable, and MUST NOT write the config.

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
