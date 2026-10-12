# Delta for Config Persistence

Purpose line is unchanged.

## RENAMED Requirements

- FROM: `### Requirement: Save on Enter`
- TO: `### Requirement: Save on s`

(Reason: the save key moved from Enter to `s`; Enter now cycles a Settings value or opens/selects in the AI picker, see config-menu.)
(Migration: tests and docs that press Enter to save press `s` instead; the footer shows `s save`.)

## MODIFIED Requirements

### Requirement: Save on s
`s` in MENU MUST request a save of the draft values from any view of the menu (Settings rows, task rows, picker level 1 or 2), except while the custom model text field is focused, where `s` is a typed letter (see config-menu). Only when the save succeeds, the menu MUST close with the draft as the new current values (mode TERMINAL). Until the answer arrives the menu stays open and swallows input. When the draft equals the values captured at open, `s` MUST close the menu without reading or writing the file. `s` MUST work before the first detection result arrives. Enter MUST NOT save in any view.
(Previously: Enter saved; `s` did not exist.)

#### Scenario: Save closes [auto]
- GIVEN a changed draft
- WHEN `s` is pressed and the save succeeds
- THEN the menu closes, mode is TERMINAL, and the draft values are the current values

#### Scenario: Unchanged draft [auto]
- GIVEN the draft equals the original values
- WHEN `s` is pressed
- THEN the menu closes and the file is not read or written

#### Scenario: Save from the picker [auto]
- GIVEN the picker is open at level 2 and a task assignment was changed in the draft
- WHEN `s` is pressed and the save succeeds
- THEN the menu closes with the changed assignment saved

#### Scenario: Enter never saves [auto]
- GIVEN a changed draft and the selection on a Settings row
- WHEN Enter is pressed
- THEN no save is requested and the file is not read or written

#### Scenario: Save before first detection [auto]
- GIVEN the menu open and no detection result has arrived yet
- WHEN `s` is pressed
- THEN the draft is saved (or the menu closes if unchanged) and the menu does not close silently without that outcome

### Requirement: Owned keys only
The save MUST read the file at save time and change only `statusline.position`, `tabbar.position`, `ai.engine`, `ai.model` and `ai.tasks.<task>.engine` and `ai.tasks.<task>.model` for `<task>` in `code`, `chat`, `review`, `summarize`. Comments, key order, whitespace, unknown keys and tables and every other key MUST be preserved, including any `mouse` entry as it is and any `[ai.tasks.general]` table. The menu MUST NEVER write `mouse = true`, and MUST NOT add or modify the `mouse` key. A missing owned key or table MUST be created (including `ai` and `ai.tasks` implicitly, with no empty parent header added); a position value MUST be one of `"top"` or `"bottom"`. The `ai.*` keys MUST be written only when the AI draft differs from the values captured at open, as TOML strings with correct escaping, and whenever a task's draft differs, both its `engine` and `model` MUST be written together; tasks whose draft equals the open values MUST NOT be written. "No engine" and "no model" MUST be written as the empty string (`engine = ""`, `model = ""`), never by removing the key. No secret, token or key MUST ever be written. When the AI draft equals the open values, a file without `[ai]` MUST gain none.
(Previously: the owned AI keys were only `ai.engine` and `ai.model`; there were no task tables.)

#### Scenario: Comments and order preserved [auto]
- GIVEN a file with comments, an unknown table, `mouse = false` and `[statusline] position = "bottom"`
- WHEN the position is saved as `top`
- THEN only that value changes and every other byte of the file is identical

#### Scenario: Missing keys are created [auto]
- GIVEN a file with no `[tabbar]` table
- WHEN `tabbar.position` is saved
- THEN a `[tabbar]` table with the value is added and existing content is untouched

#### Scenario: Mouse untouched [auto]
- GIVEN a file with `mouse = true`, and one without a `mouse` key
- WHEN a position is saved
- THEN the first keeps its `mouse = true` line unchanged and the second gains no `mouse` key

#### Scenario: Saved file parses back [auto]
- GIVEN any save result
- WHEN it is parsed by `Config::parse`
- THEN the positions and the ai engine and model equal the saved draft and no notice is produced

#### Scenario: AI table created [auto]
- GIVEN a file `# mine\n[statusline]\nposition = "bottom"\n` and the draft engine `ollama`, model `llama3:8b`
- WHEN the save succeeds
- THEN the original bytes are unchanged and the file ends with `[ai]\nengine = "ollama"\nmodel = "llama3:8b"\n`

#### Scenario: Existing AI keys updated in place [auto]
- GIVEN `[ai]` with `engine = "ollama"`, `model = "a"`, a comment line between them, and the draft engine `claude`, model `default`
- WHEN the save succeeds
- THEN only the two values change, and the comment and order are kept

#### Scenario: No engine saved as empty string [auto]
- GIVEN `[ai]` with `engine = "ollama"`, `model = "llama3:8b"` and the draft engine changed to `none` (model empty)
- WHEN the save succeeds
- THEN the file contains `engine = ""` and `model = ""`, both keys are still present, and parsing gives ai engine and model unset

#### Scenario: Special characters escaped [auto]
- GIVEN the draft model `a"b\c`
- WHEN the save succeeds and the file is parsed
- THEN the parsed model equals `a"b\c`

#### Scenario: AI untouched when unchanged [auto]
- GIVEN a file with no `[ai]` table and a changed statusline position only
- WHEN the save succeeds
- THEN the file has no `[ai]` table

#### Scenario: Undetected saved engine survives [auto]
- GIVEN the file has `engine = "codex"`, detection found codex `not installed`, and only a position changed
- WHEN the save succeeds
- THEN `engine = "codex"` is still in the file

#### Scenario: Task table created [auto]
- GIVEN a file `# mine\n[statusline]\nposition = "bottom"\n` and the draft code task `claude` / `opus`, everything else unchanged
- WHEN the save succeeds
- THEN the original bytes are unchanged and the file ends with `[ai.tasks.code]\nengine = "claude"\nmodel = "opus"\n`, with no `[ai]` or `[ai.tasks]` header added

#### Scenario: Only changed tasks written [auto]
- GIVEN a file with `[ai.tasks.chat]` `engine = "codex"`, `model = "default"` and only the code draft changed
- WHEN the save succeeds
- THEN the chat table is byte-identical and `[ai.tasks.code]` is added

#### Scenario: Unassigned task written as empty strings [auto]
- GIVEN `[ai.tasks.code]` with `engine = "claude"`, `model = "opus"` and the draft code changed to unassigned
- WHEN the save succeeds
- THEN the table is kept with `engine = ""` and `model = ""`, and parsing gives code unset

#### Scenario: Set all writes five assignments [auto]
- GIVEN an empty file and Set all tasks chosen as `claude` / `sonnet`
- WHEN the save succeeds
- THEN the file contains `[ai]` with `engine = "claude"` and `model = "sonnet"`, then `[ai.tasks.code]`, `[ai.tasks.chat]`, `[ai.tasks.review]`, `[ai.tasks.summarize]` in that order, each with the same two keys

#### Scenario: General task table preserved [auto]
- GIVEN a file with `[ai.tasks.general]` `engine = "codex"` and a changed code draft
- WHEN the save succeeds
- THEN `[ai.tasks.general]` is byte-identical

#### Scenario: Download-required model kept [auto]
- GIVEN code is saved as `ollama` / `old:7b` and ollama no longer lists `old:7b`
- WHEN a position is saved
- THEN `model = "old:7b"` is still in the file

### Requirement: Create file and directories
When the file or its parent directories do not exist, the save MUST create them (to the path resolved by the configuration spec). When no config path can be resolved (no usable `XDG_CONFIG_HOME` or absolute `HOME`), the save MUST fail with a footer message and write nothing.
(Previously: the no-path scenario pressed Enter.)

#### Scenario: Missing file and dirs [auto]
- GIVEN no `lazarobox` directory under the config home
- WHEN a save succeeds
- THEN the directory and `config.toml` exist and contain the saved positions

#### Scenario: No config path [auto]
- GIVEN no resolvable config path
- WHEN `s` is pressed on a changed draft
- THEN the menu stays open and the footer shows an error, with nothing written

### Requirement: Refuse on errors
If the existing file cannot be parsed as TOML (or cannot be read, or an owned key path collides with a non-table value such as `statusline = 3`), the save MUST be refused: nothing is written, the menu stays open with the draft intact, and the footer shows `config.toml has errors; fix it before saving` for the parse failure. Per-key invalid values that still form valid TOML (for example `position = "middle"`) are NOT errors for this rule; the save replaces the owned key with the valid draft value.
(Previously: the scenarios pressed Enter.)

#### Scenario: Unparseable file refused [auto]
- GIVEN a file with a TOML syntax error
- WHEN `s` is pressed on a changed draft
- THEN the file is byte-identical afterward, the menu is still open, and the footer shows `config.toml has errors; fix it before saving`

#### Scenario: Invalid value replaced [auto]
- GIVEN `[statusline] position = "middle"` (valid TOML)
- WHEN `top` is saved
- THEN the value becomes `"top"` and the save succeeds

#### Scenario: Non-table collision refused [auto]
- GIVEN a file with `statusline = 3`
- WHEN a position is saved
- THEN the save is refused, the file is unchanged and the footer shows `config.toml: statusline is not a table; fix it before saving`

#### Scenario: Owned key holding a table refused [auto]
- GIVEN a file with `[statusline.position]`, `[[statusline.position]]` or `position = { ... }` under `statusline`
- WHEN a position is saved
- THEN the save is refused and the file is unchanged, so the table's content is never overwritten
- AND the footer shows `config.toml: statusline.position is a table; fix it before saving`

#### Scenario: Line endings kept [auto]
- GIVEN a file whose first line break outside a string is `\r\n` (or `\n`)
- WHEN a position is saved
- THEN every line break outside strings in the result, including inserted ones, is `\r\n` (or `\n`), and line breaks inside multi-line strings are unchanged

### Requirement: Failure feedback
When the save fails for any reason (I/O error, no path, refused), the menu MUST stay open in MENU mode, in the same view (including an open picker level), with the draft and the live preview unchanged, and the footer MUST show a one-line error (for write failures, `save failed: <short error>`). `s` MAY be pressed again to retry, and Esc MUST still go back or revert and close as in config-menu. The runtime answers a save request with exactly one success or failure result.
(Previously: Enter retried; the open picker view did not exist.)

#### Scenario: Write fails [auto]
- GIVEN a save that fails with an I/O error
- WHEN the failure result arrives
- THEN mode is MENU, the preview is unchanged, and the footer shows `save failed: <error>`

#### Scenario: Retry and cancel after failure [auto]
- GIVEN a failed save with the error in the footer
- WHEN `s` succeeds on a retry, or Esc is pressed instead
- THEN the menu closes with the saved values, or reverts to the original values, respectively

#### Scenario: Failure keeps the picker open [auto]
- GIVEN the picker is open at level 2 and a save fails
- WHEN the failure result arrives
- THEN the picker is still open at level 2 with the same entry selected and the footer shows the error

#### Scenario: Persistence in Kitty [manual, Kitty]
- GIVEN a hand-commented `config.toml`
- WHEN a position is changed and saved from the menu with `s`, then the file is inspected, then broken on purpose and saved again
- THEN comments survive and only the owned key changed; the broken file is never overwritten and the footer reports the error

### Requirement: AI keys collisions
When any AI key is being written, the save MUST be refused (file unchanged, menu open, draft intact) if `ai` is not a table. When a task assignment is being written it MUST also be refused if `ai.tasks` is not a table, if `ai.tasks.<task>` is not a table, or if `ai.tasks.<task>.engine` or `.model` holds a table or array of tables. When the general assignment is being written it MUST be refused if `ai.engine` or `ai.model` holds a table or array of tables. The footer MUST show, respectively, `config.toml: ai is not a table; fix it before saving`, `config.toml: ai.tasks is not a table; fix it before saving`, `config.toml: ai.tasks.<task> is not a table; fix it before saving`, `config.toml: ai.engine is a table; fix it before saving`, `config.toml: ai.model is a table; fix it before saving`, `config.toml: ai.tasks.<task>.engine is a table; fix it before saving` or `config.toml: ai.tasks.<task>.model is a table; fix it before saving`. A non-table scalar under any engine or model key (for example `engine = 3`) is replaced by the draft string. These checks MUST NOT apply to a part whose draft equals the values captured at open: `ai` checks apply only when some AI draft differs, and a task's checks only when that task differs. Refused saves write nothing, including no new table.
(Previously: only `ai`, `ai.engine` and `ai.model` collisions existed.)

#### Scenario: ai is a scalar [auto]
- GIVEN a file with `ai = 3` and a changed engine draft
- WHEN `s` is pressed
- THEN the file is byte-identical and the footer shows `config.toml: ai is not a table; fix it before saving`

#### Scenario: engine holds a table [auto]
- GIVEN a file with `[ai.engine]` and a changed engine draft
- WHEN `s` is pressed
- THEN the file is unchanged and the footer shows `config.toml: ai.engine is a table; fix it before saving`

#### Scenario: Scalar of wrong type replaced [auto]
- GIVEN `[ai]` with `engine = 3` and a changed engine draft `ollama`
- WHEN the save succeeds
- THEN the file contains `engine = "ollama"`

#### Scenario: Collision ignored when AI unchanged [auto]
- GIVEN a file with `ai = 3`, an unchanged AI draft and a changed statusline position
- WHEN `s` is pressed
- THEN the position is saved and `ai = 3` is untouched

#### Scenario: ai.tasks is a scalar [auto]
- GIVEN a file with `[ai]` `tasks = 3` and a changed code draft
- WHEN `s` is pressed
- THEN the file is byte-identical and the footer shows `config.toml: ai.tasks is not a table; fix it before saving`

#### Scenario: Task is a scalar [auto]
- GIVEN a file with `[ai.tasks]` `code = 3` and a changed code draft
- WHEN `s` is pressed
- THEN the file is byte-identical and the footer shows `config.toml: ai.tasks.code is not a table; fix it before saving`

#### Scenario: Task engine holds a table [auto]
- GIVEN a file with `[ai.tasks.code.engine]` and a changed code draft
- WHEN `s` is pressed
- THEN the file is unchanged and the footer shows `config.toml: ai.tasks.code.engine is a table; fix it before saving`

#### Scenario: Other task unchanged is not checked [auto]
- GIVEN a file with `[ai.tasks]` `chat = 3` and only the code draft changed
- WHEN `s` is pressed
- THEN the code task is saved and `chat = 3` is untouched, because chat's draft did not change

## ADDED Requirements

### Requirement: Nested table edits
The edit layer MUST support dotted table paths (for example `ai.tasks.code`). A missing intermediate table MUST be created implicitly, without writing an empty parent header, and an existing intermediate table (explicit header or dotted keys) MUST be reused without moving or reformatting it. A missing key in an existing nested table is added at the end of that table. A collision follows the existing rule: the save is refused with the footer message of the colliding path and the file is not touched.

#### Scenario: Implicit parents [auto]
- GIVEN a file with only `[statusline]`
- WHEN `ai.tasks.review.engine` and `ai.tasks.review.model` are written
- THEN the file gains only `[ai.tasks.review]` with both keys and the earlier bytes are identical

#### Scenario: Existing parent reused [auto]
- GIVEN a file with `[ai]` (engine, model) followed by `[statusline]`, and a task `chat` is written
- WHEN the save succeeds
- THEN `[ai]` and `[statusline]` are byte-identical and `[ai.tasks.chat]` is appended at the end

#### Scenario: Key added to existing task table [auto]
- GIVEN `[ai.tasks.code]` with only `engine = "claude"` and a comment after it, and the draft code `claude` / `opus`
- WHEN the save succeeds
- THEN `engine` is kept in place and `model = "opus"` is added to that table

#### Scenario: Round trip [auto]
- GIVEN any save that wrote task tables
- WHEN the file is parsed by `Config::parse`
- THEN every task assignment equals the saved draft and no notice is produced
