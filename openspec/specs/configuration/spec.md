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
