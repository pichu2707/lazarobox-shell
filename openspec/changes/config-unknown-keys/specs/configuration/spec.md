# Configuration Specification (delta: config unknown keys)

## ADDED Requirements

### Requirement: Unknown keys in owned tables
The tables lazarobox owns are `[statusline]`, `[tabbar]`, `[updates]`, `[workspace]` and `[ai]` with every table under it, each entry of `[[ai.mcp.servers]]` included. A key inside an owned table that lazarobox does not read MUST be ignored (the file still loads and every other key applies) and MUST be reported as one problem under the existing startup-notice rules: `unknown key: <path>`, where `<path>` is the full dotted path of the key, for example `unknown key: ai.tols`, `unknown key: ai.tasks.general`, `unknown key: ai.servers.llama` or `unknown key: ai.mcp.servers[0].timeout`. An unknown table inside an owned table is one unknown key, reported once, and its content is not walked. A key that is not a bare TOML key (letters, digits, `_` and `-`) MUST be shown quoted, with control characters escaped, so the notice stays one line. These problems MUST come after every other problem, so a wrong value stays the first problem of the notice. The root of the file is not an owned table: its other keys and tables (for example `theme = "x"` or `[custom]`) are the user's own and MUST NOT be reported. Every key that lazarobox reads MUST NOT be reported, the root `mouse` and `ai.servers.llamacpp.model` included. A value of the wrong shape (a non-table where a table is read) keeps its existing problem and is not walked. This amends every sentence of this spec that says unknown keys or ids under an owned table are ignored without a notice (`[ai.memory]`, `[ai.servers]`, `managed` and `model` under another server id, `[ai.tools]`, `[[ai.mcp.servers]]` entries, `[workspace]`, `[ai.orchestrator]`, `[ai.usage.billing]`) and the sentence "Problems under `ai.tasks.general` and under unknown task names are not reported": the value is still ignored and the save still preserves it byte for byte (see config-persistence), and the key is now reported.

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
