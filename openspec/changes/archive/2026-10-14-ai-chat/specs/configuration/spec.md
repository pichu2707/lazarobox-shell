# Delta for Configuration

Tags: [auto] = `cargo test`; [manual] = real Kitty. The file location, the fallback rules and the notices of the existing spec are unchanged; this delta adds the `[ai] profile` key.

## MODIFIED Requirements

### Requirement: Schema and defaults
The file is TOML with `[statusline] position` and `[tabbar] position`, each `"top"` or `"bottom"`, a top-level `mouse` boolean, `[ai] engine` and `[ai] model`, each a string (the assignment of the `general` task), `[ai] profile`, a string that is `"analyst"` or `"programmer"`, and for each of the tasks `code`, `chat`, `review` and `summarize` a table `[ai.tasks.<task>]` with `engine` and `model`, each a string. Defaults: statusline `"bottom"`, tabbar `"top"`, `mouse = false`, profile `"programmer"`, and every `engine` and `model` unset (no engine, no model). An empty string equals unset (for `profile`, the default), and an unassigned task falls back to `general` (see ai-task-models). Any key may be omitted. Unknown keys and tables MUST be ignored, including `[ai.tasks.general]` (ignored and preserved: `[ai]` is the general task) and any unknown task name under `ai.tasks`. All four position combinations are valid. Any string is valid for every `engine` and `model`, including an id that is not in the engine catalog; it MUST be kept as written and MUST NOT raise a notice. No secret is part of the schema. The profile is not an engine key: only `analyst` and `programmer` are valid (see Profile key validation).
(Previously: no `profile` key; the `[ai]` table held only `engine` and `model`)

#### Scenario: Defaults [auto]
- GIVEN an empty file
- WHEN it is parsed
- THEN statusline is bottom, tabbar is top, mouse is false, the profile is `programmer`, and ai engine and model are unset

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

## ADDED Requirements

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
