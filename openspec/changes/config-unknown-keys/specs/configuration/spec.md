# Configuration Specification (delta: config unknown keys)

## ADDED Requirements

### Requirement: Unknown keys in owned tables
The tables lazarobox owns are `[statusline]`, `[tabbar]`, `[updates]`, `[workspace]` and `[ai]` with every table under it, each entry of `[[ai.mcp.servers]]` included. A key inside an owned table that lazarobox does not read MUST be ignored (the file still loads and every other key applies) and MUST be reported as one problem under the existing startup-notice rules: `unknown key: <path>`, where `<path>` is the full dotted path of the key, for example `unknown key: ai.tols`, `unknown key: ai.tasks.general`, `unknown key: ai.servers.llama` or `unknown key: ai.mcp.servers[0].timeout`. An unknown table inside an owned table is one unknown key, reported once, and its content is not walked. A key that is not a bare TOML key (letters, digits, `_` and `-`) MUST be shown quoted, with control characters escaped, so the notice stays one line. These problems MUST come after every other problem found while reading the file, so a wrong value stays the first problem of the notice; a problem found later at startup (such as `workspace.dir: folder not found`) may follow them. The root of the file is not an owned table: its other keys and tables (for example `theme = "x"` or `[custom]`) are the user's own and MUST NOT be reported. Every key that lazarobox reads MUST NOT be reported, the root `mouse` and `ai.servers.llamacpp.model` included. A value of the wrong shape (a non-table where a table is read) keeps its existing problem and is not walked. The requirements modified below say so where they name their unknown keys. This also amends the sentences that say unknown keys under an owned table are ignored without naming a notice (`managed` and `model` under another server id in Managed llama-server, and `[ai.orchestrator]` in Orchestrator keys) and the sentence "Problems under `ai.tasks.general` and under unknown task names are not reported": in every case the value is still ignored and the save still preserves it byte for byte (see config-persistence), and the key is now reported.

#### Scenario: A typo in an owned table [auto]
- GIVEN `[ai]` with `tols = 1`
- WHEN the file is loaded
- THEN the config loads with its defaults and the notice is `config: unknown key: ai.tols`

#### Scenario: Known keys next to an unknown one [auto]
- GIVEN `[ai]` with `tols = 1`, `engine = 3` and `model = "m"`, and `[statusline]` with `position = "top"`
- WHEN the file is loaded
- THEN the model is `m`, the statusline is at the top, and the notice is `config (2 problems): ai.engine: expected a string`, the unknown key being the second problem

#### Scenario: Every owned table is checked [auto]
- GIVEN, one at a time, `[statusline] postion`, `[tabbar] side`, `[updates] chek`, `[workspace] colour`, `[ai.tasks.cod]`, `[ai.tasks.general]`, `[ai.tasks.code] modle`, `[ai.usage] limit`, `[ai.usage.billing] openai`, `[ai.tools] mode`, `[ai.chat] compact`, `[ai.mcp] proxy`, an `[[ai.mcp.servers]]` entry with `timeout`, `[ai.servers.llama]`, `[ai.servers.ollama] managed`, `[ai.servers.llamacpp] modle`, `[ai.memory] endpoint` and `[ai.orchestrator] notice_accepted`
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

## MODIFIED Requirements

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

### Requirement: MCP server lists
The file MAY hold an array of tables `[[ai.mcp.servers]]`, the global list of user MCP servers (see mcp-servers). Each entry has `name` (lowercase letters, digits and `-`, 1 to 24 characters, unique in its scope: the global list, or one project root; it is also the server part of a qualified tool name), `command` (non-empty string, run directly without a shell), `args` (array of strings, default empty), `env` (array of environment variable NAMES to pass from lazarobox's own environment, default empty, each an identifier of at most 64 bytes; values are never in the file) and `enabled` (boolean, default `true`). Per-project extras MUST be declared in this same user file, keyed by project root, in a shape pinned by design; no other file is ever read for servers. An invalid entry MUST be skipped as a whole and reported, the other entries kept: `ai.mcp.servers[N].name: expected a name of lowercase letters, digits and - (1 to 24 characters)`, `ai.mcp.servers[N].command: expected a non-empty string`, `ai.mcp.servers[N].args: expected an array of strings`, `ai.mcp.servers[N].env: expected an array of variable names`, `ai.mcp.servers[N].enabled: expected a boolean` (only `enabled` falls back alone), `ai.mcp.servers[N].name: duplicate name` (the later entry is skipped), `ai.mcp.servers: expected an array of tables` and `ai.mcp: expected a table`. Unknown keys inside an entry MUST be ignored, the entry kept, and each one is reported as `unknown key: ai.mcp.servers[N].<key>` (see Unknown keys in owned tables). There are no `Authorization`, token or secret keys in the schema. The list is read once at startup (no hot reload). The menu has no row for it in this change: it is edited by hand.

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
