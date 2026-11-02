# Configuration Specification

## Purpose

A user config file read once at startup. It places the statusline and the tab bar, and records the AI engine and model of the general task and of each AI task. Tags: [auto] = `cargo test`; [manual] = real Kitty.

## Requirements

### Requirement: File location
The config MUST be read from `$XDG_CONFIG_HOME/lazarobox/config.toml`. When `XDG_CONFIG_HOME` is unset, empty or relative, it MUST be read from `$HOME/.config/lazarobox/config.toml`, but only when `HOME` is absolute; an empty or relative `HOME` is treated as absent. The file MUST be read once at startup; there is no hot reload.

#### Scenario: Path resolution [auto]
- GIVEN `XDG_CONFIG_HOME=/x` and `HOME=/h`
- WHEN the path is resolved
- THEN it is `/x/lazarobox/config.toml`; without a usable XDG value it is `/h/.config/lazarobox/config.toml`; without either there is no path

#### Scenario: Relative or empty HOME is ignored [auto]
- GIVEN `XDG_CONFIG_HOME` is unusable and `HOME` is empty or relative
- WHEN the path is resolved
- THEN there is no path and the defaults apply (nothing is read relative to the working directory)

### Requirement: Schema and defaults
The file is TOML with `[statusline] position` and `[tabbar] position`, each `"top"` or `"bottom"`, a top-level `mouse` boolean, `[ai] engine` and `[ai] model`, each a string (the assignment of the `general` task), `[ai] profile`, a string that is `"analyst"` or `"programmer"`, and for each of the tasks `code`, `chat`, `review` and `summarize` a table `[ai.tasks.<task>]` with `engine` and `model`, each a string. Defaults: statusline `"bottom"`, tabbar `"top"`, `mouse = true`, profile `"programmer"`, and every `engine` and `model` unset (no engine, no model). The `mouse` key controls host mouse capture and the wheel (see mouse-input) and takes effect at startup and when changed from the config menu. An empty string equals unset (for `profile`, the default), and an unassigned task falls back to `general` (see ai-task-models). Any key may be omitted. Unknown keys and tables MUST be ignored, including `[ai.tasks.general]` (ignored and preserved: `[ai]` is the general task) and any unknown task name under `ai.tasks`. All four position combinations are valid. Any string is valid for every `engine` and `model`, including an id that is not in the engine catalog; it MUST be kept as written and MUST NOT raise a notice. No secret is part of the schema. The profile is not an engine key: only `analyst` and `programmer` are valid (see Profile key validation).

#### Scenario: Defaults [auto]
- GIVEN an empty file
- WHEN it is parsed
- THEN statusline is bottom, tabbar is top, mouse is true, the profile is `programmer`, and ai engine and model are unset

#### Scenario: Mouse false [auto]
- GIVEN `mouse = false`
- WHEN it is parsed
- THEN mouse is false and no notice is produced

#### Scenario: Mouse true takes effect [auto]
- GIVEN `mouse = true`
- WHEN the app starts
- THEN mouse capture is enabled (see mouse-input) and no notice is produced

#### Scenario: Each value [auto]
- GIVEN a file that sets one position
- WHEN it is parsed
- THEN that position changes and the other keeps its default

#### Scenario: Unknown keys ignored [auto]
- GIVEN a file with unknown keys and tables next to valid ones
- WHEN it is parsed
- THEN the valid keys apply and no error is raised

#### Scenario: AI keys parsed [auto]
- GIVEN `[ai]` with `engine = "ollama"` and `model = "llama3:8b"`
- WHEN it is parsed
- THEN ai engine is `ollama`, ai model is `llama3:8b`, and no notice is produced

#### Scenario: Profile parsed [auto]
- GIVEN `[ai]` with `profile = "programmer"`
- WHEN it is parsed
- THEN the profile is `programmer` and no notice is produced

#### Scenario: Empty profile is the default [auto]
- GIVEN `[ai]` with `profile = ""`
- WHEN it is parsed
- THEN the profile is `programmer` and no notice is produced

#### Scenario: Unknown engine id kept [auto]
- GIVEN `[ai]` with `engine = "future-engine"`
- WHEN it is parsed
- THEN ai engine is `future-engine` and no notice is produced

#### Scenario: Empty string is unset [auto]
- GIVEN `[ai]` with `engine = ""` and `model = ""`
- WHEN it is parsed
- THEN ai engine and model are unset

#### Scenario: Task tables parsed [auto]
- GIVEN `[ai]` with `engine = "claude"` and `model = "sonnet"`, and `[ai.tasks.code]` with `engine = "ollama"` and `model = "llama3:8b"`
- WHEN it is parsed
- THEN general is `claude` / `sonnet`, code is `ollama` / `llama3:8b`, chat, review and summarize are unset, and no notice is produced

#### Scenario: Empty task string is unset [auto]
- GIVEN `[ai.tasks.chat]` with `engine = ""` and `model = ""`
- WHEN it is parsed
- THEN chat is unset and falls back to general

#### Scenario: General task table and unknown tasks ignored [auto]
- GIVEN `[ai.tasks.general]` with `engine = "codex"`, `[ai.tasks.poetry]` with `engine = "claude"`, and `[ai]` with `engine = "ollama"`
- WHEN it is parsed
- THEN general is `ollama`, no task named `poetry` exists, and no notice is produced

#### Scenario: Unknown task engine kept [auto]
- GIVEN `[ai.tasks.review]` with `engine = "future-engine"`
- WHEN it is parsed
- THEN review's engine is `future-engine` and no notice is produced

### Requirement: Fallback and startup notice
A missing file MUST silently give the defaults. An unreadable file or unparseable TOML (a syntax error) MUST give all the defaults and a startup notice `config: <short error>` (one line) shown through the existing statusline notice until the next key.

An invalid value or type MUST fall back to the default for THAT KEY ONLY; every other key in the file is respected. The notice names the failing key, for example `config: statusline.position: expected "top" or "bottom"`; when several keys fail it reports the count and the first: `config (N problems): <first problem>`. A non-table section (`statusline = 3`) reports `statusline: expected a table`, and a non-boolean `mouse` reports `mouse: expected true or false` (the key falls back to the default `true`). A non-string `ai.engine` or `ai.model` reports `ai.engine: expected a string` or `ai.model: expected a string`, and a non-table `ai` reports `ai: expected a table`. A non-table `ai.tasks` reports `ai.tasks: expected a table` (all tasks fall back to unset), a non-table `ai.tasks.<task>` reports `ai.tasks.<task>: expected a table` (that task is unset), and a non-string task key reports `ai.tasks.<task>.engine: expected a string` or `ai.tasks.<task>.model: expected a string` (only that key falls back to unset). Problems under `ai.tasks.general` and under unknown task names are not reported.

`mouse = true` and `mouse = false` MUST both be accepted with no notice. The notice `config: mouse is not supported yet` MUST NOT exist any more.

#### Scenario: Missing file [auto]
- GIVEN no config file
- WHEN the app starts
- THEN the defaults apply (mouse capture on) and no notice shows

#### Scenario: Invalid value falls back per key [auto]
- GIVEN `[statusline] position = "middle"` and `[tabbar] position = "bottom"` and `mouse = true`
- WHEN the app starts
- THEN the statusline is at the default (bottom), the tab bar is at the bottom, mouse is true, and the notice is `config: statusline.position: expected "top" or "bottom"`

#### Scenario: Several invalid keys [auto]
- GIVEN invalid values for `mouse`, `statusline.position` and `tabbar.position`
- WHEN the app starts
- THEN all three use their defaults (mouse true) and the notice is `config (3 problems): ` followed by the first key's problem

#### Scenario: Invalid mouse type [auto]
- GIVEN `mouse = "yes"`
- WHEN the app starts
- THEN mouse is true (the default), capture is enabled and the notice is `config: mouse: expected true or false`

#### Scenario: Unparseable file [auto]
- GIVEN a file with a TOML syntax error
- WHEN the app starts
- THEN all defaults apply and the notice is `config: <short parse error>`

#### Scenario: No mouse notice [auto]
- GIVEN `mouse = true`, and in another case `mouse = false`
- WHEN the app starts
- THEN no notice is produced, and the text `mouse is not supported yet` appears nowhere

#### Scenario: Invalid AI types fall back per key [auto]
- GIVEN `[ai]` with `engine = 3` and `model = "llama3:8b"`, and `[tabbar] position = "bottom"`
- WHEN the app starts
- THEN ai engine is unset, ai model is `llama3:8b`, the tab bar is at the bottom, and the notice is `config: ai.engine: expected a string`

#### Scenario: Non-table ai [auto]
- GIVEN a file with `ai = 3`
- WHEN the app starts
- THEN ai engine and model are unset and the notice is `config: ai: expected a table`

#### Scenario: Non-table ai.tasks [auto]
- GIVEN a file with `[ai]` `engine = "claude"` and `tasks = 3`
- WHEN the app starts
- THEN general is `claude`, every other task is unset, and the notice is `config: ai.tasks: expected a table`

#### Scenario: Non-table task [auto]
- GIVEN `[ai.tasks]` with `code = 3` and `[ai.tasks.chat]` with `engine = "codex"`
- WHEN the app starts
- THEN code is unset, chat is `codex`, and the notice is `config: ai.tasks.code: expected a table`

#### Scenario: Invalid task key type falls back per key [auto]
- GIVEN `[ai.tasks.code]` with `engine = 5` and `model = "opus"`
- WHEN the app starts
- THEN code's engine is unset, code's model is `opus`, and the notice is `config: ai.tasks.code.engine: expected a string`

#### Scenario: Several task problems counted [auto]
- GIVEN `ai.tasks.code.engine = 5` and `ai.tasks.chat.model = 6`
- WHEN the app starts
- THEN the notice is `config (2 problems): ai.tasks.code.engine: expected a string`

#### Scenario: Positions in Kitty [manual]
- GIVEN each of the four combinations in `~/.config/lazarobox/config.toml`
- WHEN the app runs with one and with three tabs
- THEN the bars sit where configured and nvim fills the rest

### Requirement: Profile key validation
An `[ai] profile` that is not a string MUST fall back to the default for that key only and report `ai.profile: expected a string`. A string other than `analyst`, `programmer` or the empty string (case-sensitive, no trimming) MUST fall back to the default and report `ai.profile: expected "analyst" or "programmer"`. These problems follow the existing startup-notice rules: the notice is `config: <problem>`, several problems give `config (N problems): <first problem>`, and the other keys of the file are respected. A valid profile never raises a notice.

#### Scenario: Unknown profile value [auto]
- GIVEN `[ai]` with `profile = "wizard"` and `engine = "ollama"`
- WHEN the app starts
- THEN the profile is `programmer`, ai engine is `ollama` and the notice is `config: ai.profile: expected "analyst" or "programmer"`

#### Scenario: Case and spaces are not forgiven [auto]
- GIVEN `profile = "Analyst"`, and in another file `profile = " analyst"` (the default is `programmer`)
- WHEN each is parsed
- THEN both give the default profile and the unknown-value notice

#### Scenario: Non-string profile [auto]
- GIVEN `[ai]` with `profile = 3`
- WHEN the app starts
- THEN the profile is `programmer` and the notice is `config: ai.profile: expected a string`

#### Scenario: Counted with other problems [auto]
- GIVEN `[ai]` with `profile = 3` and `[statusline] position = "middle"`
- WHEN the app starts
- THEN the notice starts with `config (2 problems): ` and both keys use their defaults

#### Scenario: Non-table ai hides the profile [auto]
- GIVEN a file with `ai = 3`
- WHEN the app starts
- THEN the profile is `programmer` and the only notice is `config: ai: expected a table`

### Requirement: Memory keys
The file MAY hold a table `[ai.memory]` with `enabled` (boolean, default `false`), `port` (integer from 1 to 65535, default the value of the `ENGRAM_PORT` environment variable when that is a valid port, else `7437`) and `acknowledged` (boolean, default `false`). Any key may be omitted. Unknown keys under `ai.memory` MUST be ignored. A wrong type or out-of-range value MUST fall back to the default for that key only and report a problem under the existing startup-notice rules (`config: <problem>`, or `config (N problems): <first problem>`), with the problems `ai.memory.enabled: expected a boolean`, `ai.memory.acknowledged: expected a boolean`, `ai.memory.port: expected an integer from 1 to 65535`, and, when `ai.memory` is not a table, `ai.memory: expected a table`. Memory is active only when `enabled` and `acknowledged` are both true (see chat-memory). The token is never part of the schema: `ENGRAM_HTTP_TOKEN` is read from the environment only and no secret is ever read from or written to the file.

#### Scenario: Defaults [auto]
- GIVEN a file with no `[ai.memory]`
- WHEN it is parsed
- THEN enabled is false, acknowledged is false, the port is 7437 and no notice is produced

#### Scenario: Values parsed [auto]
- GIVEN `[ai.memory]` with `enabled = true`, `acknowledged = true` and `port = 8123`
- WHEN it is parsed
- THEN memory is active on port 8123 and no notice is produced

#### Scenario: Enabled without acknowledgement is inactive [auto]
- GIVEN `[ai.memory]` with `enabled = true` only
- WHEN it is parsed
- THEN memory is not active

#### Scenario: Port default from the environment [auto]
- GIVEN no `port` key and `ENGRAM_PORT=8123`, and in another run `ENGRAM_PORT=abc`
- WHEN each is parsed
- THEN the port is 8123, and 7437 for the invalid value, and a `port` key always wins

#### Scenario: Wrong types and range [auto]
- GIVEN `enabled = "yes"`, `port = 0`, `port = 70000`, `port = "7437"` and `acknowledged = 1` in separate files
- WHEN each is parsed
- THEN each key falls back to its default and the notice names the matching problem

#### Scenario: Counted with other problems [auto]
- GIVEN `port = 0` and `[statusline] position = "middle"`
- WHEN the app starts
- THEN the notice starts with `config (2 problems): ` and both keys use their defaults

#### Scenario: Non-table ai.memory [auto]
- GIVEN a file with `[ai]` holding `memory = 3`
- WHEN the app starts
- THEN memory uses the defaults and the notice is `config: ai.memory: expected a table`

#### Scenario: Unknown keys ignored [auto]
- GIVEN `[ai.memory]` with `endpoint = "http://x"` next to valid keys
- WHEN it is parsed
- THEN the valid keys apply and no notice is produced

### Requirement: The notify flag
`[ai] notify` is a boolean. A missing key is `false`. A value that is not a boolean falls back to `false` and produces the problem `ai.notify: expected a boolean`, which follows the `ai.context` problem in the notice order. It is independent of every other `[ai]` key.

#### Scenario: Default [auto]
- GIVEN no `[ai]` table, or one without `notify`
- WHEN the config is read
- THEN notify is `false` and there is no problem

#### Scenario: Explicit [auto]
- GIVEN `[ai]\nnotify = true`
- WHEN the config is read
- THEN notify is `true` and there is no problem

#### Scenario: Wrong type [auto]
- GIVEN `[ai]\nnotify = "yes"` (or an integer, or a table)
- WHEN the config is read
- THEN notify is `false` and the problem is `ai.notify: expected a boolean`

### Requirement: Managed llama-server
`[ai.servers.llamacpp]` MAY hold `managed` (boolean, default `false`) and `model` (string, the path of the model file `llama-server` loads, used as written: it SHOULD be absolute, since `~` is not expanded and a relative path is read from lazarobox's working directory). With `managed = true` and a non-empty `model`, lazarobox manages the server (see chat-engines), and llamacpp's port defaults to 39281 instead of 8080; `port` still overrides it. Under any other server id, `managed` and `model` MUST be ignored like any unknown key. A wrong value MUST leave the server unmanaged and report one problem under the existing startup-notice rules: `ai.servers.llamacpp.managed: expected true or false`, `ai.servers.llamacpp.model: expected a path to a model file` (also when `managed` is off), or `ai.servers.llamacpp.model: required when managed = true` (missing or empty).

#### Scenario: A managed llama-server is opt-in [auto]
- GIVEN no `managed`, `managed = false`, or a `model` without `managed`
- WHEN it is parsed
- THEN nothing is managed, llamacpp keeps its port and no notice is produced

#### Scenario: A managed llama-server has its own port [auto]
- GIVEN `managed = true` and `model = "/m/q.gguf"`
- WHEN it is parsed
- THEN the server is managed with that model on port 39281, or on `port` when it is set

#### Scenario: A wrong managed value turns it off with its problem [auto]
- GIVEN `managed = "yes"`, `managed = true` without a model or with `model = ""`, or `model = 1`
- WHEN it is parsed
- THEN nothing is managed, llamacpp keeps port 8080 and the notice names the matching problem

### Requirement: Server ports
The file MAY hold one table `[ai.servers.<id>]` per server engine (`ollama`, `lmstudio`, `llamacpp`, `vllm`) with `port` (integer from 1 to 65535, default the catalog port). Detection and chat both use it, always on `127.0.0.1`. Unknown ids and unknown keys under `ai.servers` MUST be ignored. A wrong type or out-of-range value MUST fall back to the catalog port for that server only and report a problem under the existing startup-notice rules, with the problems `ai.servers.<id>.port: expected an integer from 1 to 65535`, `ai.servers.<id>: expected a table` when the server entry is not a table, and `ai.servers: expected a table` when `ai.servers` is not a table. The menu MUST NOT write `ai.servers` and every save MUST keep it byte for byte. The ports are read at startup.

#### Scenario: Server ports default to the catalog [auto]
- GIVEN a file with no `[ai.servers]`, or an empty one
- WHEN it is parsed
- THEN the ports are 11434, 1234, 8080 and 8000 and no notice is produced

#### Scenario: Server ports are read [auto]
- GIVEN `[ai.servers.llamacpp] port = 39080` and `[ai.servers.ollama] port = 11500`
- WHEN it is parsed
- THEN those two servers use those ports and the others keep their catalog port

#### Scenario: Unknown servers and keys are ignored [auto]
- GIVEN `[ai.servers.claude]`, `[ai.servers.nope]` and `[ai.servers.vllm]` with `port = 8001` and `host = "0.0.0.0"`
- WHEN it is parsed
- THEN only vllm changes (port 8001, host unchanged) and no notice is produced

#### Scenario: A wrong server value falls back alone [auto]
- GIVEN `ai.servers = 1`, `[ai.servers] llamacpp = 39080`, `port = 0`, `port = 70000` and `port = "39080"` in separate files
- WHEN each is parsed
- THEN every server keeps its catalog port and the notice names the matching problem

#### Scenario: A turn goes to the configured port [auto]
- GIVEN llamacpp and ollama configured on two different fake servers
- WHEN a llamacpp turn runs
- THEN only the llamacpp fake receives a connection

#### Scenario: Saves keep the table [auto]
- GIVEN a file with commented `[ai.servers.*]` tables, one of them invalid
- WHEN the menu saves other keys
- THEN the tables are unchanged byte for byte, and a file without them gains none

### Requirement: Extra llama-server arguments
`[ai.servers.llamacpp] args` MUST be read as a list of strings, whether `managed` is on or not, and passed to the managed llama-server after `--host 127.0.0.1 --port <port> -m <model>`, each entry as one argv element with no shell. A list holding an option lazarobox sets (`--host`, `--port`, `-m`, `--model`, also in the `--name=value` form) MUST be ignored whole with the problem `ai.servers.llamacpp.args: "<entry>" is set by lazarobox`; a value that is not a list of strings MUST be ignored with `ai.servers.llamacpp.args: expected a list of strings`. The menu MUST NOT write `args`.

#### Scenario: Extra arguments [auto]
- GIVEN `args = ["-ngl", "99", "--alias", "my model"]`
- WHEN the managed llama-server starts
- THEN its argv ends with `-ngl`, `99`, `--alias`, `my model` after `-m <model>`

#### Scenario: A clashing entry [auto]
- GIVEN `args = ["-ngl", "99", "--port=8080"]`
- WHEN the config loads
- THEN no extra argument is used and the problem names "--port=8080"

### Requirement: The idle time of the managed llama-server
`[ai.servers.llamacpp] idle_minutes` MUST be read as a whole number of minutes from 0 (never) up, whether `managed` is on or not; a missing key is 15, and any other value falls back to 15 with the problem `ai.servers.llamacpp.idle_minutes: expected a whole number of minutes (0 = never)`. The Settings tab of the menu MUST have a `llama-server idle` row, after `Sound`, that steps through `5 min`, `15 min`, `30 min` and `never` (`l` forward, `h` back), shows a value between those as `<n> min` and steps from it to the next one each way, and saves only `idle_minutes`, written as a TOML integer.

#### Scenario: Stepping [auto]
- GIVEN the row on `15 min`
- WHEN `l` is pressed four times
- THEN it reads `30 min`, `never`, `5 min`, `15 min`

#### Scenario: Saved as a number [auto]
- GIVEN the row moved to `30 min`
- WHEN the menu saves
- THEN config.toml holds `idle_minutes = 30` under `[ai.servers.llamacpp]` and its other keys are kept

### Requirement: Tools key
The file MAY hold a table `[ai.tools]` with `enabled` (boolean, default `true`). It is the off switch of all agent tools: the run action of pane-commands, the built-in tools, the lazarobox MCP server and the user MCP servers (see agent-tools). A wrong type falls back to `true` for that key only and reports `ai.tools.enabled: expected a boolean`; a non-table `ai.tools` reports `ai.tools: expected a table` and uses the default. Unknown keys under `ai.tools` MUST be ignored. The key is read at startup and when changed from the config menu; a change takes effect at the next turn, never on a stream or approval already running. No secret is part of the schema.

#### Scenario: Default on [auto]
- GIVEN a file with no `[ai.tools]`
- WHEN it is parsed
- THEN tools are on and no notice is produced

#### Scenario: Off [auto]
- GIVEN `[ai.tools]` with `enabled = false`
- WHEN it is parsed
- THEN tools are off and no notice is produced

#### Scenario: Wrong type [auto]
- GIVEN `[ai.tools]` with `enabled = "no"`
- WHEN the app starts
- THEN tools are on (the default) and the notice is `config: ai.tools.enabled: expected a boolean`

#### Scenario: Non-table [auto]
- GIVEN `[ai]` holding `tools = 3`
- WHEN the app starts
- THEN tools use the default and the notice is `config: ai.tools: expected a table`

#### Scenario: Counted with other problems [auto]
- GIVEN `enabled = 1` under `[ai.tools]` and `[statusline] position = "middle"`
- WHEN the app starts
- THEN the notice starts with `config (2 problems): ` and both keys use their defaults

#### Scenario: Unknown keys ignored [auto]
- GIVEN `[ai.tools]` with `mode = "fast"` next to `enabled = true`
- WHEN it is parsed
- THEN `enabled` applies and no notice is produced

### Requirement: Approval mode key
The table `[ai.tools]` MAY hold `approval`, the string `"ask"`, `"auto"` or `"run"` (default `"ask"`, compared exactly, lowercase). It is the approval mode a chat pane takes when it opens and at each conversation switch inside it (see agent-tools, Chat approval mode); the current conversation of an open pane keeps its mode. Any other value or type (including `"RUN"`) falls back to `"ask"` for that key only and reports `ai.tools.approval: expected "ask", "auto" or "run"`, after the `ai.tools.enabled` problem in the notice order. A non-table `ai.tools` uses the default with the existing `ai.tools: expected a table` problem only. The key is read at startup and when changed from the config menu. The default is never `run`.
(Previously: accepted `"ask"` or `"auto"`; the problem text read `expected "ask" or "auto"`)

#### Scenario: Default ask [auto]
- GIVEN a file with no `approval` under `[ai.tools]`, and a file with no `[ai.tools]`
- WHEN each is parsed
- THEN the approval mode is `ask` and no notice is produced

#### Scenario: Auto [auto]
- GIVEN `[ai.tools]` with `approval = "auto"`
- WHEN it is parsed
- THEN the approval mode is `auto` and no notice is produced

#### Scenario: Run [auto]
- GIVEN `[ai.tools]` with `approval = "run"`
- WHEN it is parsed
- THEN the approval mode is `run` and no notice is produced

#### Scenario: Bad value [auto]
- GIVEN `approval = "AUTO"`, `approval = "Run"`, `approval = "yes"` and `approval = true` in separate files
- WHEN the app starts
- THEN the mode is `ask` each time and the notice is `config: ai.tools.approval: expected "ask", "auto" or "run"`

#### Scenario: Notice order [auto]
- GIVEN `enabled = 1` and `approval = 2` under `[ai.tools]`
- WHEN the app starts
- THEN the notice is `config (2 problems): ai.tools.enabled: expected a boolean`, tools are on and the mode is `ask`

#### Scenario: Older build reads run as a bad value [manual]
- GIVEN a file saved with `approval = "run"` and a build without this change
- WHEN it starts
- THEN the mode is `ask` and the notice names `ai.tools.approval`

### Requirement: MCP server lists
The file MAY hold an array of tables `[[ai.mcp.servers]]`, the global list of user MCP servers (see mcp-servers). Each entry has `name` (lowercase letters, digits and `-`, 1 to 24 characters, unique in its scope: the global list, or one project root; it is also the server part of a qualified tool name), `command` (non-empty string, run directly without a shell), `args` (array of strings, default empty), `env` (array of environment variable NAMES to pass from lazarobox's own environment, default empty, each an identifier of at most 64 bytes; values are never in the file) and `enabled` (boolean, default `true`). Per-project extras MUST be declared in this same user file, keyed by project root, in a shape pinned by design; no other file is ever read for servers. An invalid entry MUST be skipped as a whole and reported, the other entries kept: `ai.mcp.servers[N].name: expected a name of lowercase letters, digits and - (1 to 24 characters)`, `ai.mcp.servers[N].command: expected a non-empty string`, `ai.mcp.servers[N].args: expected an array of strings`, `ai.mcp.servers[N].env: expected an array of variable names`, `ai.mcp.servers[N].enabled: expected a boolean` (only `enabled` falls back alone), `ai.mcp.servers[N].name: duplicate name` (the later entry is skipped), `ai.mcp.servers: expected an array of tables` and `ai.mcp: expected a table`. Unknown keys inside an entry MUST be ignored. There are no `Authorization`, token or secret keys in the schema. The list is read once at startup (no hot reload). The menu has no row for it in this change: it is edited by hand.

#### Scenario: Defaults [auto]
- GIVEN a file with no `[ai.mcp]`
- WHEN it is parsed
- THEN there are no servers and no notice is produced

#### Scenario: Entry parsed [auto]
- GIVEN `[[ai.mcp.servers]]` with `name = "fs"`, `command = "mcp-fs"`, `args = ["--root", "/home/u/dev"]`, `env = ["GITHUB_TOKEN"]`
- WHEN it is parsed
- THEN one enabled server `fs` exists with those values and no notice is produced

#### Scenario: Invalid entry skipped, others kept [auto]
- GIVEN two entries where the first has `command = 5` and the second is valid
- WHEN the app starts
- THEN only the second is kept and the notice is `config: ai.mcp.servers[0].command: expected a non-empty string`

#### Scenario: Duplicate name [auto]
- GIVEN two entries named `fs`
- WHEN the app starts
- THEN the first is kept, the second is skipped and the notice is `config: ai.mcp.servers[1].name: duplicate name`

#### Scenario: Values never in the file [auto]
- GIVEN `env = { GITHUB_TOKEN = "abc" }`
- WHEN the app starts
- THEN the entry is skipped with the `env` problem and `abc` appears in no notice or log

#### Scenario: enabled false [auto]
- GIVEN an entry with `enabled = false`
- WHEN servers are resolved
- THEN the entry is not active and no process is started for it

#### Scenario: Wrong container types [auto]
- GIVEN `ai.mcp = 3`, and in another file `ai.mcp.servers = 3`
- WHEN each is parsed
- THEN the notices are `config: ai.mcp: expected a table` and `config: ai.mcp.servers: expected an array of tables`

#### Scenario: Unknown keys ignored [auto]
- GIVEN an entry with `timeout = 5` next to valid keys
- WHEN it is parsed
- THEN the valid keys apply and no notice is produced

#### Scenario: Project extras only from the user file [design-confirm]
- GIVEN a repository file that declares a server and a project entry in the user file for another root
- WHEN servers are resolved for a chat in the repository
- THEN only entries from the user file for that root are active

### Requirement: Family toggle keys
The table `[ai.tools]` MAY hold five booleans: `search`, `write`, `exec`, `git` and `git_write` (provisional names). Each defaults to `false`. They control the families Search, Write, Exec, Git read verbs and Git write verbs (see agent-tools, Tool families). A family is active only when `[ai.tools] enabled` is also `true`. A wrong type falls back to `false` for that key only and reports `ai.tools.<key>: expected a boolean`, after the `ai.tools.approval` problem in the notice order. A non-table `ai.tools` uses the defaults with the existing `ai.tools: expected a table` problem only. Unknown keys MUST be ignored. A change takes effect at the next turn, never on a stream or approval already running.

#### Scenario: Defaults off [auto]
- GIVEN a file with no family keys, and a file with no `[ai.tools]`
- WHEN each is parsed
- THEN all five toggles are off and no notice is produced

#### Scenario: On [auto]
- GIVEN `[ai.tools]` with `write = true` and `exec = true`
- WHEN it is parsed
- THEN Write and Exec are on and the other three are off

#### Scenario: Wrong type [auto]
- GIVEN `[ai.tools]` with `git = "yes"`
- WHEN the app starts
- THEN `git` is off and the notice is `config: ai.tools.git: expected a boolean`

#### Scenario: Master switch wins [auto]
- GIVEN `enabled = false` and `write = true`
- WHEN the tool list is built
- THEN no tool is offered

#### Scenario: Git toggles are independent [auto]
- GIVEN `git = false` and `git_write = true`
- WHEN the tool list is built
- THEN only `git_add` and `git_commit` are offered among the git tools

### Requirement: Workspace key
The file MAY hold a table `[workspace]` with `dir`, a string: an absolute path, or a path starting with `~` which expands to an absolute `$HOME`. A missing key and an empty string both mean unset. `dir` is the start folder of the app when no `--dir` flag is given (see start-dir). Invalid values fall back to unset for that key only and report a problem under the existing startup-notice rules (`config: <problem>`, or `config (N problems): <first problem>`, the other keys respected): `workspace.dir: expected a string` for a non-string, `workspace.dir: expected an absolute path` for a relative path, and `workspace: expected a table` for a non-table `workspace`. A well-formed path that does not exist or is not a directory at startup is not a parse problem: the start dir falls back to the launch cwd and the notice is `config: workspace.dir: folder not found`. Unknown keys under `workspace` MUST be ignored. A valid `dir` never raises a notice. The key is read once at startup; there is no hot reload.

#### Scenario: Default [auto]
- GIVEN a file with no `[workspace]`
- WHEN it is parsed
- THEN `dir` is unset and no notice is produced

#### Scenario: Absolute and tilde [auto]
- GIVEN `dir = "/w/proj"`, and in another file `dir = "~/code"` with `HOME=/h`
- WHEN each is parsed
- THEN the values are `/w/proj` and `/h/code` and no notice is produced

#### Scenario: Empty string is unset [auto]
- GIVEN `dir = ""`
- WHEN it is parsed
- THEN `dir` is unset and no notice is produced

#### Scenario: Relative path [auto]
- GIVEN `dir = "proj"`
- WHEN the app starts
- THEN `dir` is unset and the notice is `config: workspace.dir: expected an absolute path`

#### Scenario: Wrong types [auto]
- GIVEN `dir = 3`, and in another file `workspace = 3`
- WHEN each is parsed
- THEN the notices are `config: workspace.dir: expected a string` and `config: workspace: expected a table`

#### Scenario: Counted with other problems [auto]
- GIVEN `dir = 3` and `[statusline] position = "middle"`
- WHEN the app starts
- THEN the notice starts with `config (2 problems): ` and both keys use their defaults

#### Scenario: Unknown keys ignored [auto]
- GIVEN `[workspace]` with `colour = "red"` next to a valid `dir`
- WHEN it is parsed
- THEN `dir` applies and no notice is produced

### Requirement: Orchestrate slot [S1]
The configuration MUST carry a sixth AI task slot, `orchestrate`, with the same `engine` and `model` keys and the same validation as the other tasks, read from the table `[ai.tasks.orchestrate]`. An absent table means unassigned. The slot MUST NOT be counted by any logic that is defined over the five original tasks (Set all, its aggregate).

#### Scenario: Slot parsed [auto]
- GIVEN `[ai.tasks.orchestrate]` with `engine = "claude"` and `model = "opus"`
- WHEN the configuration is loaded
- THEN `orchestrate` is `claude` / `opus` and the other slots are as before

#### Scenario: Absent means unassigned [auto]
- GIVEN no `[ai.tasks.orchestrate]` table
- WHEN the configuration is loaded
- THEN `orchestrate` is unassigned and falls back to `general`

### Requirement: Orchestrator keys [S1, S3, S4]
The configuration MUST read these keys under `[ai.orchestrator]`: `enabled` (bool, default `false`; the global gate), `acknowledged` (bool, default `false`), `worker_timeout_secs` (integer, default 600, valid range 30 to 3600; the worker deadline), [S3] `max_parallel` (integer, default 2, valid range 1 to 4), `max_workers` (integer, default 8, valid range 1 to 32) `run_timeout_mins` (integer, default 60, valid range 1 to 480) and [S4] `max_turns` (integer, default 6, valid range 1 to 24; the orchestrator turns one run may take, the first included). The per-pane ORCH mode is not a configuration key. A wrong type or out-of-range value MUST fall back to the default for that key and produce the existing startup notice, without discarding other keys. Unknown keys MUST be ignored.

#### Scenario: Defaults [auto]
- GIVEN no `[ai.orchestrator]` table
- WHEN the configuration is loaded
- THEN `enabled` and `acknowledged` are false, `worker_timeout_secs` is 600, `max_parallel` is 2, `max_workers` is 8, `run_timeout_mins` is 60 and `max_turns` is 6

#### Scenario: Bad value falls back [auto]
- GIVEN `max_parallel = 0` and `enabled = true`
- WHEN the configuration is loaded
- THEN `max_parallel` is 2, `enabled` is true and a startup notice names the key

#### Scenario: Bad max_turns falls back [auto]
- GIVEN `max_turns = 25` and `enabled = true`
- WHEN the configuration is loaded
- THEN `max_turns` is 6, `enabled` is true and a startup notice names the key and the range 1 to 24

#### Scenario: Older build ignores keys [auto]
- GIVEN a file with `[ai.tasks.orchestrate]` and `[ai.orchestrator]` read by the previous schema
- WHEN it is loaded
- THEN the earlier keys load unchanged and the new ones are ignored
