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
The file is TOML with `[statusline] position` and `[tabbar] position`, each `"top"` or `"bottom"`, a top-level `mouse` boolean, `[ai] engine` and `[ai] model`, each a string (the assignment of the `general` task), `[ai] profile`, a string that is `"analyst"` or `"programmer"`, and for each of the tasks `code`, `chat`, `review` and `summarize` a table `[ai.tasks.<task>]` with `engine` and `model`, each a string. Defaults: statusline `"bottom"`, tabbar `"top"`, `mouse = true`, profile `"programmer"`, and every `engine` and `model` unset (no engine, no model). The `mouse` key controls host mouse capture and the wheel (see mouse-input) and takes effect at startup and when changed from the config menu. An empty string equals unset (for `profile`, the default), and an unassigned task falls back to `general` (see ai-task-models). Any key may be omitted. Unknown keys and tables MUST be ignored, including `[ai.tasks.general]` (ignored and preserved: `[ai]` is the general task) and any unknown task name under `ai.tasks`; one inside a table lazarobox owns is also reported (see Unknown keys in owned tables). All four position combinations are valid. Any string is valid for every `engine` and `model`, including an id that is not in the engine catalog; it MUST be kept as written and MUST NOT raise a notice. No secret is part of the schema. The profile is not an engine key: only `analyst` and `programmer` are valid (see Profile key validation).

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
- THEN the valid keys apply, no error is raised, and only the unknown keys inside owned tables are reported

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
- THEN general is `ollama`, no task named `poetry` exists, and the notice is `config (2 problems): unknown key: ai.tasks.general`

#### Scenario: Unknown task engine kept [auto]
- GIVEN `[ai.tasks.review]` with `engine = "future-engine"`
- WHEN it is parsed
- THEN review's engine is `future-engine` and no notice is produced

### Requirement: Fallback and startup notice
A missing file MUST silently give the defaults. An unreadable file or unparseable TOML (a syntax error) MUST give all the defaults and a startup notice `config: <short error>` (one line) shown through the existing statusline notice until the next key.

An invalid value or type MUST fall back to the default for THAT KEY ONLY; every other key in the file is respected. The notice names the failing key, for example `config: statusline.position: expected "top" or "bottom"`; when several keys fail it reports the count and the first: `config (N problems): <first problem>`. A non-table section (`statusline = 3`) reports `statusline: expected a table`, and a non-boolean `mouse` reports `mouse: expected true or false` (the key falls back to the default `true`). A non-string `ai.engine` or `ai.model` reports `ai.engine: expected a string` or `ai.model: expected a string`, and a non-table `ai` reports `ai: expected a table`. A non-table `ai.tasks` reports `ai.tasks: expected a table` (all tasks fall back to unset), a non-table `ai.tasks.<task>` reports `ai.tasks.<task>: expected a table` (that task is unset), and a non-string task key reports `ai.tasks.<task>.engine: expected a string` or `ai.tasks.<task>.model: expected a string` (only that key falls back to unset). Problems under `ai.tasks.general` and under unknown task names are not reported, except the unknown keys themselves, which are reported as `unknown key: <path>` (see Unknown keys in owned tables).

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
The file MAY hold a table `[ai.memory]` with `enabled` (boolean, default `false`), `port` (integer from 1 to 65535, default the value of the `ENGRAM_PORT` environment variable when that is a valid port, else `7437`) and `acknowledged` (boolean, default `false`). Any key may be omitted. Unknown keys under `ai.memory` MUST be ignored, and each one is reported as `unknown key: ai.memory.<key>` (see Unknown keys in owned tables). A wrong type or out-of-range value MUST fall back to the default for that key only and report a problem under the existing startup-notice rules (`config: <problem>`, or `config (N problems): <first problem>`), with the problems `ai.memory.enabled: expected a boolean`, `ai.memory.acknowledged: expected a boolean`, `ai.memory.port: expected an integer from 1 to 65535`, and, when `ai.memory` is not a table, `ai.memory: expected a table`. Memory is active only when `enabled` and `acknowledged` are both true (see chat-memory). The token is never part of the schema: `ENGRAM_HTTP_TOKEN` is read from the environment only and no secret is ever read from or written to the file.

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
- THEN the valid keys apply and the notice is `config: unknown key: ai.memory.endpoint`

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

### Requirement: The tool mark key
`[ai] tool_mark` is the string `"auto"`, `"emoji"` or `"text"` (compared exactly, lowercase). A missing key is `"auto"`: the glyph follows the terminal rule (text on `TERM=linux`, `linux-*`, `dumb` and non-UTF-8 locales, the emoji otherwise). `"text"` always draws the text badge and `"emoji"` always draws the emoji, even on the Linux console or a non-UTF-8 locale. It applies to every place a tool mark is drawn (see ai-task-models, Tool marks on installed Ollama models) and is read at startup. Any other value or type falls back to `"auto"` and reports `ai.tool_mark: expected "auto", "emoji" or "text"`, after the `ai.notify_sound` problem in the notice order. It is independent of every other `[ai]` key.

#### Scenario: Default and explicit values [auto]
- GIVEN no `tool_mark`, or `tool_mark = "auto"`, `"emoji"` or `"text"`
- WHEN the config is read
- THEN the mode is `auto` (missing) or the written one, and there is no problem

#### Scenario: Bad value [auto]
- GIVEN `[ai]\ntool_mark = "Text"` (or `""`, a boolean, an integer, or a table)
- WHEN the config is read
- THEN the mode is `auto` and the problem is `ai.tool_mark: expected "auto", "emoji" or "text"`

#### Scenario: Text wins [auto]
- GIVEN `tool_mark = "text"` on `TERM=xterm-256color` with a UTF-8 locale
- WHEN a tool mark is drawn
- THEN it reads `tools`

### Requirement: Managed llama-server
`[ai.servers.llamacpp]` MAY hold `managed` (boolean, default `false`) and `model` (string, the path of the model file `llama-server` loads, used as written: it SHOULD be absolute, since `~` is not expanded and a relative path is read from lazarobox's working directory). With `managed = true` and a non-empty `model`, lazarobox manages the server (see chat-engines), and llamacpp's port defaults to 39281 instead of 8080; `port` still overrides it. Under any other server id, `managed` and `model` MUST be ignored like any unknown key and reported as such (`unknown key: ai.servers.<id>.managed`, see Unknown keys in owned tables). A wrong value MUST leave the server unmanaged and report one problem under the existing startup-notice rules: `ai.servers.llamacpp.managed: expected true or false`, `ai.servers.llamacpp.model: expected a path to a model file` (also when `managed` is off), or `ai.servers.llamacpp.model: required when managed = true` (missing or empty).

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
The file MAY hold one table `[ai.servers.<id>]` per server engine (`ollama`, `lmstudio`, `llamacpp`, `vllm`) with `port` (integer from 1 to 65535, default the catalog port). Detection and chat both use it, always on `127.0.0.1`. Unknown ids and unknown keys under `ai.servers` MUST be ignored, and each one is reported by its full path, `unknown key: ai.servers.<id>` for an unknown id and `unknown key: ai.servers.<id>.<key>` for an unknown key (see Unknown keys in owned tables). A wrong type or out-of-range value MUST fall back to the catalog port for that server only and report a problem under the existing startup-notice rules, with the problems `ai.servers.<id>.port: expected an integer from 1 to 65535`, `ai.servers.<id>: expected a table` when the server entry is not a table, and `ai.servers: expected a table` when `ai.servers` is not a table. The menu MUST NOT write `ai.servers` except `managed`, `model` and `idle_minutes` of `[ai.servers.llamacpp]` (see config-menu, Choosing a local model manages llama-server), and every save MUST keep the rest of it byte for byte. The ports are read at startup; only a save that turns the managed llama-server on changes llamacpp's port while running.

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
- THEN only vllm changes (port 8001, host unchanged), the problems are `unknown key: ai.servers.claude`, `unknown key: ai.servers.nope` and `unknown key: ai.servers.vllm.host`, and the notice is `config (3 problems): unknown key: ai.servers.claude`

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

### Requirement: Prompt-cache options of the local servers
`[ai.servers.ollama] keep_alive` and `[ai.servers.llamacpp] cache_prompt` MAY be set (#515). `keep_alive` MUST be a Go duration as Ollama reads it (an optional sign, then numbers each with a unit `ns`, `us`, `µs`, `ms`, `s`, `m` or `h`, or a bare `0`) or a TOML integer of seconds; `cache_prompt` MUST be a boolean. Any other value is left out with the problem `ai.servers.ollama.keep_alive: expected a duration such as "30m" or a whole number of seconds` or `ai.servers.llamacpp.cache_prompt: expected true or false`. Unset, `cache_prompt` sends nothing, and an unset `keep_alive` sends `"30m"` to Ollama (Ollama's own default is five minutes). Set, every request to that server (chat, summary, compaction, tool rounds, orchestrator workers) MUST carry `keep_alive` as written (the text; an integer `n` as `"<n>s"`) or `cache_prompt`, and no other server MUST receive either. Both are read at startup; the menu MUST keep them when it saves the table.

#### Scenario: Unset keep-alive sends 30m to Ollama [auto]
- GIVEN no `keep_alive` and no `cache_prompt`
- WHEN a turn is sent to Ollama
- THEN its body holds `model`, `messages`, `stream` and `"keep_alive": "30m"`, and no other server's body changes

#### Scenario: Explicit keep-alive replaces the default [auto]
- GIVEN `keep_alive = "5m"` (or `"1h"`, `"0"`)
- WHEN a turn is sent to Ollama
- THEN its body holds that text as written in place of `"30m"`

#### Scenario: A whole number of seconds is sent as a duration [auto]
- GIVEN `keep_alive = 0` (or `-1`)
- WHEN a turn is sent to Ollama
- THEN its body holds `"0s"` (or `"-1s"`) in place of `"30m"`

#### Scenario: Each to its server [auto]
- GIVEN `keep_alive = "30m"` and `cache_prompt = true`
- WHEN turns go to Ollama, llama.cpp, LM Studio and vLLM
- THEN Ollama's body holds `"keep_alive": "30m"`, llama.cpp's `"cache_prompt": true`, and the others neither

### Requirement: Tools key
The file MAY hold a table `[ai.tools]` with `enabled` (boolean, default `true`). It is the off switch of all agent tools: the run action of pane-commands, the built-in tools, the lazarobox MCP server and the user MCP servers (see agent-tools). A wrong type falls back to `true` for that key only and reports `ai.tools.enabled: expected a boolean`; a non-table `ai.tools` reports `ai.tools: expected a table` and uses the default. Unknown keys under `ai.tools` MUST be ignored, and each one is reported as `unknown key: ai.tools.<key>` (see Unknown keys in owned tables). The key is read at startup and when changed from the config menu; a change takes effect at the next turn, never on a stream or approval already running. No secret is part of the schema.

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
- THEN `enabled` applies and the notice is `config: unknown key: ai.tools.mode`

### Requirement: Approval mode key
The table `[ai.tools]` MAY hold `approval`, the string `"ask"`, `"auto"` or `"run"` (default `"ask"`, compared exactly, lowercase). It is the approval mode a chat pane takes when it opens and at each conversation switch inside it (see agent-tools, Chat approval mode); the current conversation of an open pane keeps its mode. Any other value or type (including `"RUN"`) falls back to `"ask"` for that key only and reports `ai.tools.approval: expected "ask", "auto" or "run"`, after the `ai.tools.enabled` problem in the notice order. A non-table `ai.tools` uses the default with the existing `ai.tools: expected a table` problem only. The key is read at startup and when changed from the config menu. The default is never `run`.

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
The file MAY hold an array of tables `[[ai.mcp.servers]]`, the global list of user MCP servers (see mcp-servers). Each entry has `name` (lowercase letters, digits and `-`, 1 to 24 characters, unique in its scope: the global list, or one project root; it is also the server part of a qualified tool name), `command` (non-empty string, run directly without a shell), `args` (array of strings, default empty), `env` (array of environment variable NAMES to pass from lazarobox's own environment, default empty, each an identifier of at most 64 bytes; values are never in the file) and `enabled` (boolean, default `true`). Per-project extras MUST be declared in this same user file, keyed by project root, in a shape pinned by design; no other file is ever read for servers. An invalid entry MUST be skipped as a whole and reported, the other entries kept: `ai.mcp.servers[N].name: expected a name of lowercase letters, digits and - (1 to 24 characters)`, `ai.mcp.servers[N].command: expected a non-empty string`, `ai.mcp.servers[N].args: expected an array of strings`, `ai.mcp.servers[N].env: expected an array of variable names`, `ai.mcp.servers[N].enabled: expected a boolean` (only `enabled` falls back alone), `ai.mcp.servers[N].name: duplicate name` (the later entry is skipped), `ai.mcp.servers: expected an array of tables` and `ai.mcp: expected a table`. Unknown keys inside an entry MUST be ignored, the entry kept, and each one is reported as `unknown key: ai.mcp.servers[N].<key>` (see Unknown keys in owned tables). There are no `Authorization`, token or secret keys in the schema. The list is read once at startup (no hot reload). It is edited by hand; the only other writer is Connect Google (config-menu, Connect Google), which, after the user's `y` on its plan, appends the catalog entries of `gsc` and `google` that are not declared at the end of the file, keeps the old file as `config.toml.bak`, and never changes or removes an entry.

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
- THEN the valid keys apply, the entry is kept and the notice is `config: unknown key: ai.mcp.servers[0].timeout`

#### Scenario: Project extras only from the user file [design-confirm]
- GIVEN a repository file that declares a server and a project entry in the user file for another root
- WHEN servers are resolved for a chat in the repository
- THEN only entries from the user file for that root are active

### Requirement: Family toggle keys
The table `[ai.tools]` MAY hold seven booleans: `search`, `write`, `exec`, `git`, `git_write`, `github` (provisional names) and `issue_draft`. Each defaults to `false`, except `issue_draft`, which defaults to `true` (a draft reaches nothing without the user's `y` and Ctrl+S). `issue_draft` controls the family IssueDraft. They control the families Search, Write, Exec, Git read verbs, Git write verbs and the GitHub read tools (see agent-tools, Tool families). A family is active only when `[ai.tools] enabled` is also `true`. A wrong type falls back to that key's default for that key only and reports `ai.tools.<key>: expected a boolean`, after the `ai.tools.approval` problem in the notice order. A non-table `ai.tools` uses the defaults with the existing `ai.tools: expected a table` problem only. Unknown keys MUST be ignored and reported as `unknown key: ai.tools.<key>` (see Unknown keys in owned tables), so a build without `github` reads a file that has it and says so. A change takes effect at the next turn, never on a stream or approval already running. A chat grant (see github-tools) is never written to this table.
(Previously: six booleans, with no `issue_draft`; before that, five, with no `github`.)

#### Scenario: Defaults off [auto]
- GIVEN a file with no family keys, and a file with no `[ai.tools]`
- WHEN each is parsed
- THEN all six toggles are off and no notice is produced

#### Scenario: On [auto]
- GIVEN `[ai.tools]` with `write = true` and `exec = true`
- WHEN it is parsed
- THEN Write and Exec are on and the other four are off

#### Scenario: issue_draft defaults on and can be turned off [auto]
- GIVEN a file with no `issue_draft`, and in another run `issue_draft = false`
- WHEN the config is loaded
- THEN the IssueDraft family is on in the first and off in the second, and a wrong type falls back to on with the notice `ai.tools.issue_draft: expected a boolean`

#### Scenario: GitHub on [auto]
- GIVEN `[ai.tools]` with `github = true`
- WHEN it is parsed
- THEN GitHub is on and the other five are off

#### Scenario: Wrong type [auto]
- GIVEN `[ai.tools]` with `git = "yes"`, and in another run `github = "yes"`
- WHEN the app starts
- THEN that key is off and the notice is `config: ai.tools.git: expected a boolean`, or `config: ai.tools.github: expected a boolean`

#### Scenario: Master switch wins [auto]
- GIVEN `enabled = false` and `write = true`, and in another run `enabled = false` and `github = true`
- WHEN the tool list is built
- THEN no tool is offered

#### Scenario: Git toggles are independent [auto]
- GIVEN `git = false` and `git_write = true`
- WHEN the tool list is built
- THEN only `git_add` and `git_commit` are offered among the git tools

#### Scenario: Old file with no key [auto]
- GIVEN a file written before this change
- WHEN it is parsed and the menu saves other keys
- THEN `github` is off, no notice is produced and the file gains no `github` key unless the user changes the row

#### Scenario: Grant never writes the file [auto]
- GIVEN a chat granted with `/github`
- WHEN the config is saved by the menu
- THEN `github` is unchanged in the file

### Requirement: Workspace key
The file MAY hold a table `[workspace]` with `dir`, a string: an absolute path, or a path starting with `~` which expands to an absolute `$HOME`. A missing key and an empty string both mean unset. `dir` is the start folder of the app when no `--dir` flag is given (see start-dir). Invalid values fall back to unset for that key only and report a problem under the existing startup-notice rules (`config: <problem>`, or `config (N problems): <first problem>`, the other keys respected): `workspace.dir: expected a string` for a non-string, `workspace.dir: expected an absolute path` for a relative path, and `workspace: expected a table` for a non-table `workspace`. A well-formed path that does not exist or is not a directory at startup is not a parse problem: the start dir falls back to the launch cwd and the notice is `config: workspace.dir: folder not found`. Unknown keys under `workspace` MUST be ignored, and each one is reported as `unknown key: workspace.<key>` (see Unknown keys in owned tables). A valid `dir` never raises a notice. The key is read once at startup; there is no hot reload.

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
- THEN `dir` applies and the notice is `config: unknown key: workspace.colour`

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
The configuration MUST read these keys under `[ai.orchestrator]`: `enabled` (bool, default `false`; the global gate), `acknowledged` (bool, default `false`), `worker_timeout_secs` (integer, default 600, valid range 30 to 3600; the worker deadline), [S3] `max_parallel` (integer, default 2, valid range 1 to 4), `max_workers` (integer, default 8, valid range 1 to 32) `run_timeout_mins` (integer, default 60, valid range 1 to 480) and [S4] `max_turns` (integer, default 6, valid range 1 to 24; the orchestrator turns one run may take, the first included). The per-pane ORCH mode is not a configuration key. A wrong type or out-of-range value MUST fall back to the default for that key and produce the existing startup notice, without discarding other keys. Unknown keys MUST be ignored and reported as `unknown key: ai.orchestrator.<key>` (see Unknown keys in owned tables).

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

### Requirement: Usage keys
`[ai.usage]` MAY hold:
- `line` (boolean, default `true`): the per-answer usage line;
- `statusline` (boolean, default `true`): the status-bar usage segment;
- `turn_tokens` (whole number greater than 0): per-turn token threshold;
- `day_tokens` (whole number greater than 0): per-day token threshold;
- `day_usd` (number greater than 0, a TOML float or integer): per-day dollar threshold.
A missing threshold key means that threshold is off. `[ai.usage.billing]` MAY hold `claude`, `codex` and `gemini`, each the string `subscription` or `per-token`; a missing key is `subscription`. Any other key under `[ai.usage.billing]` (including local engine ids) and any unknown key under `[ai.usage]` MUST be ignored, and each one is reported by its full path, `unknown key: ai.usage.billing.<key>` or `unknown key: ai.usage.<key>` (see Unknown keys in owned tables). A wrong value MUST fall back to that key's default (thresholds: off) and produce one problem under the startup-notice rules, in this order after the `ai.notify` problem: `ai.usage.line: expected true or false`, `ai.usage.statusline: expected true or false`, `ai.usage.turn_tokens: expected a whole number above 0`, `ai.usage.day_tokens: expected a whole number above 0`, `ai.usage.day_usd: expected a number above 0`, `ai.usage.billing.claude: expected "subscription" or "per-token"` (same for `codex`, `gemini`). `ai.usage` holding a non-table value falls back to all defaults with the problem `ai.usage: expected a table`. Values are case-sensitive. The keys are independent of every other `[ai]` key.

#### Scenario: Defaults [auto]
- GIVEN no `[ai.usage]` table
- WHEN the config is read
- THEN line and statusline are on, all thresholds are off, billing is `subscription` for all three and there is no problem

#### Scenario: Explicit values [auto]
- GIVEN `[ai.usage]\nline = false\nturn_tokens = 4000\nday_usd = 5\n[ai.usage.billing]\nclaude = "per-token"`
- WHEN the config is read
- THEN line is off, turn limit 4000, day dollars 5.0, claude `per-token`, codex and gemini `subscription`, and no problem

#### Scenario: Wrong types and ranges [auto]
- GIVEN `turn_tokens = 0`, `day_tokens = -3`, `day_usd = "5"`, `line = "yes"`
- WHEN the config is read
- THEN each falls back (thresholds off, line on) and one problem per key is reported with the exact texts above

#### Scenario: Wrong billing value [auto]
- GIVEN `[ai.usage.billing]\ncodex = "Subscription"`
- WHEN the config is read
- THEN codex is `subscription` and the problem is `ai.usage.billing.codex: expected "subscription" or "per-token"`

#### Scenario: Local engines ignored [auto]
- GIVEN `[ai.usage.billing]\nollama = "per-token"`
- WHEN the config is read
- THEN the only problem is `unknown key: ai.usage.billing.ollama` and ollama still never shows dollars

#### Scenario: Not a table [auto]
- GIVEN `[ai]\nusage = 3`
- WHEN the config is read
- THEN all defaults apply and the problem is `ai.usage: expected a table`

#### Scenario: Fractional whole number [auto]
- GIVEN `turn_tokens = 1.5`
- WHEN the config is read
- THEN the threshold is off and the problem is `ai.usage.turn_tokens: expected a whole number above 0`

### Requirement: Unknown keys in owned tables
The tables lazarobox owns are `[notices]`, `[statusline]`, `[tabbar]`, `[updates]`, `[workspace]` and `[ai]` with every table under it, each entry of `[[ai.mcp.servers]]` included. A key inside an owned table that lazarobox does not read MUST be ignored (the file still loads and every other key applies) and MUST be reported as one problem under the existing startup-notice rules: `unknown key: <path>`, where `<path>` is the full dotted path of the key, for example `unknown key: ai.tols`, `unknown key: ai.tasks.general`, `unknown key: ai.servers.llama` or `unknown key: ai.mcp.servers[0].timeout`. An unknown table inside an owned table is one unknown key, reported once, and its content is not walked. A key that is not a bare TOML key (letters, digits, `_` and `-`) MUST be shown quoted, with control characters escaped, so the notice stays one line. These problems MUST come after every other problem found while reading the file, so a wrong value stays the first problem of the notice; a problem found later at startup (such as `workspace.dir: folder not found`) may follow them. The root of the file is not an owned table: its other keys and tables (for example `theme = "x"` or `[custom]`) are the user's own and MUST NOT be reported. Every key that lazarobox reads MUST NOT be reported, the root `mouse` and `ai.servers.llamacpp.model` included. A value of the wrong shape (a non-table where a table is read) keeps its existing problem and is not walked. The requirements modified below say so where they name their unknown keys. This also amends the sentences that say unknown keys under an owned table are ignored without naming a notice (`managed` and `model` under another server id in Managed llama-server, and `[ai.orchestrator]` in Orchestrator keys) and the sentence "Problems under `ai.tasks.general` and under unknown task names are not reported": in every case the value is still ignored and the save still preserves it byte for byte (see config-persistence), and the key is now reported.

#### Scenario: A typo in an owned table [auto]
- GIVEN `[ai]` with `tols = 1`
- WHEN the file is loaded
- THEN the config loads with its defaults and the notice is `config: unknown key: ai.tols`

#### Scenario: Known keys next to an unknown one [auto]
- GIVEN `[ai]` with `tols = 1`, `engine = 3` and `model = "m"`, and `[statusline]` with `position = "top"`
- WHEN the file is loaded
- THEN the model is `m`, the statusline is at the top, and the notice is `config (2 problems): ai.engine: expected a string`, the unknown key being the second problem

#### Scenario: Every owned table is checked [auto]
- GIVEN, one at a time, `[statusline] postion`, `[tabbar] side`, `[updates] chek`, `[notices] stars`, `[workspace] colour`, `[ai.tasks.cod]`, `[ai.tasks.general]`, `[ai.tasks.code] modle`, `[ai.usage] limit`, `[ai.usage.billing] openai`, `[ai.tools] mode`, `[ai.chat] compact`, `[ai.mcp] proxy`, an `[[ai.mcp.servers]]` entry with `timeout`, `[ai.servers.llama]`, `[ai.servers.ollama] managed`, `[ai.servers.llamacpp] modle`, `[ai.memory] endpoint` and `[ai.orchestrator] notice_accepted`
- WHEN each file is loaded
- THEN each gives exactly one problem, `unknown key: <path>` with the key's full path, and the same config as the file without that key

#### Scenario: Keys outside the owned tables [auto]
- GIVEN `theme = "x"`, `[custom]` with `other = 1`, `[custom.deep]` and `[[plugins]]`
- WHEN the file is loaded
- THEN no problem is reported

#### Scenario: Every known key [auto]
- GIVEN a file that sets every key lazarobox reads, the root `mouse` and `[ai.servers.llamacpp] managed = true` with `model = "/m/q.gguf"` included
- WHEN it is loaded
- THEN no problem is reported and the managed model is `/m/q.gguf`

#### Scenario: A key that needs quotes [auto]
- GIVEN `[ai]` with `"a.b" = 1` and a key holding an escape character
- WHEN the file is loaded
- THEN the problems are `unknown key: ai."a.b"` and the second key quoted with the escape character written as `\u{1b}`

### Requirement: Ollama context cap
`[ai.servers.ollama] num_ctx_cap` MUST be read as a whole number of tokens from 1024 to 1048576; a missing key is 16384. Any other value or type (negative, a cap below 1024 or above 1048576, float, string) falls back to 16384 and reports `ai.servers.ollama.num_ctx_cap: expected a whole number of tokens from 1024 to 1048576`, through the startup notice rules of Fallback and startup notice; the other keys are respected. A fallback MUST NEVER be silent. The menu MUST NOT write the key, and saving other keys keeps it byte for byte. Old configs without it, and old builds reading it, are unaffected. There is no separate keep-alive key in this change: the keep-alive is the released `[ai.servers.ollama] keep_alive` (see Prompt-cache options of the local servers).

#### Scenario: Default [auto]
- GIVEN a file without the key
- WHEN it is parsed
- THEN the cap is 16384 and no notice is produced

#### Scenario: Valid override [auto]
- GIVEN `num_ctx_cap = 8192`
- WHEN it is parsed
- THEN the cap is 8192 and no notice is produced

#### Scenario: Bad cap falls back with a notice [auto]
- GIVEN `num_ctx_cap = "big"`, or `0`, `-1`, `1023` or `1048577`
- WHEN the app starts
- THEN the cap is 16384 and the notice is `config: ai.servers.ollama.num_ctx_cap: expected a whole number of tokens from 1024 to 1048576`

#### Scenario: Menu never writes them [auto]
- GIVEN a file with `num_ctx_cap` set
- WHEN the menu saves other keys
- THEN the key is unchanged byte for byte, and a file without it gains none

### Requirement: Simple slot [W8]
The configuration MUST carry a seventh AI task slot, `simple`, with the same `engine` and `model` keys and the same validation as the other tasks, read from the table `[ai.tasks.simple]`. An absent table means unassigned. The slot MUST NOT be counted by any logic defined over the five original tasks (Set all, its aggregate). A build that does not know the table MUST ignore it and load every earlier key unchanged.

#### Scenario: Slot parsed [W8, auto]
- GIVEN `[ai.tasks.simple]` with `engine = "ollama"` and `model = "llama3:8b"`
- WHEN the configuration is loaded
- THEN `simple` is `ollama` / `llama3:8b` and the other slots are as before

#### Scenario: Absent means unassigned [W8, auto]
- GIVEN no `[ai.tasks.simple]` table
- WHEN the configuration is loaded
- THEN `simple` is unassigned and falls back to `general`

#### Scenario: Older build ignores the table [W8, auto]
- GIVEN a file with `[ai.tasks.simple]` read by the previous schema
- WHEN it is loaded
- THEN the earlier keys load unchanged and the new table is ignored

#### Scenario: Invalid value follows the task rules [W8, auto]
- GIVEN `[ai.tasks.simple]` with a wrong type for `engine`
- WHEN the configuration is loaded
- THEN the slot falls back as the other task slots do and a startup notice names the key
