# Config Persistence Specification

## Purpose

How the config menu writes `config.toml`. Only the menu-owned keys change; the rest of the file is the user's. Tags: [auto] = `cargo test`; [manual] = real Kitty.

## Requirements

### Requirement: Save on s
`s` in MENU MUST request a save of the draft values from any view of the menu (Settings rows, task rows, picker level 1 or 2), except while the custom model text field is focused, where `s` is a typed letter (see config-menu). Only when the save succeeds, the menu MUST close with the draft as the new current values (mode TERMINAL). Until the answer arrives the menu stays open and swallows input. When the draft equals the values captured at open, `s` MUST close the menu without reading or writing the file. `s` MUST work before the first detection result arrives. Enter MUST NOT save in any view.

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
The save MUST read the file at save time and change only `statusline.position`, `tabbar.position`, `mouse`, `ai.engine`, `ai.model`, `ai.profile` and `ai.tasks.<task>.engine` and `ai.tasks.<task>.model` for `<task>` in `code`, `chat`, `review`, `summarize`. Comments, key order, whitespace, unknown keys and tables and every other key MUST be preserved, including any `[ai.tasks.general]` table. A missing owned key or table MUST be created (including `ai` and `ai.tasks` implicitly, with no empty parent header added); a position value MUST be one of `"top"` or `"bottom"`.

`mouse` MUST be written only when the mouse draft differs from the value captured at open (the config value, default true when the key is absent or invalid), as the TOML boolean `true` or `false` (not a string), and it MUST be written explicitly even when it equals the default. It MUST remain a top-level key: a missing `mouse` key MUST be inserted before the first table header of the file (or appended when the file has no table), never inside a table. An existing `mouse` line (including a non-boolean value such as `mouse = "yes"`) MUST be updated in place keeping its trailing comment. When the mouse draft equals the open value the `mouse` line MUST be left exactly as it is, and a file without `mouse` MUST gain none.

The `ai.*` keys MUST be written only when the AI draft differs from the values captured at open, as TOML strings with correct escaping, and whenever a task's draft differs, both its `engine` and `model` MUST be written together; tasks whose draft equals the open values MUST NOT be written. `ai.profile` MUST be written only when the profile draft differs from the value captured at open, as one of `"analyst"` or `"programmer"` (default `programmer`), and it MUST be written explicitly even when it equals the default; it is never removed. "No engine" and "no model" MUST be written as the empty string (`engine = ""`, `model = ""`), never by removing the key. No secret, token or key MUST ever be written. When the AI draft equals the open values, a file without `[ai]` MUST gain none.

#### Scenario: Comments and order preserved [auto]
- GIVEN a file with comments, an unknown table, `mouse = false` and `[statusline] position = "bottom"`
- WHEN the position is saved as `top` and the mouse draft is unchanged
- THEN only that value changes and every other byte of the file is identical

#### Scenario: Missing keys are created [auto]
- GIVEN a file with no `[tabbar]` table
- WHEN `tabbar.position` is saved
- THEN a `[tabbar]` table with the value is added and existing content is untouched

#### Scenario: Mouse untouched when unchanged [auto]
- GIVEN a file with `mouse = true`, and one without a `mouse` key, and an unchanged mouse draft
- WHEN a position is saved
- THEN the first keeps its `mouse = true` line unchanged and the second gains no `mouse` key

#### Scenario: Mouse updated in place [auto]
- GIVEN a file with `# input` then `mouse = true # wheel` and `[statusline]` after it, and the mouse draft changed to off
- WHEN the save succeeds
- THEN the line reads `mouse = false # wheel`, and every other byte is identical

#### Scenario: Mouse created at top level [auto]
- GIVEN a file `# mine\n[statusline]\nposition = "bottom"\n` (open value: default true) and the mouse draft changed to off
- WHEN the save succeeds
- THEN `mouse = false` appears before the `[statusline]` header, the original bytes are otherwise unchanged, and parsing gives `mouse` false with `statusline.position` still `bottom`

#### Scenario: Mouse created in an empty or table-less file [auto]
- GIVEN an empty file and the mouse draft changed to off
- WHEN the save succeeds
- THEN the file is `mouse = false\n`

#### Scenario: Toggled back is not written [auto]
- GIVEN a file without `mouse` (open value true) and a draft cycled to `off` and back to `on`
- WHEN a position is saved
- THEN the file gains no `mouse` key

#### Scenario: Default value written explicitly [auto]
- GIVEN a file with `mouse = false` and the mouse draft changed to `on` (the default)
- WHEN the save succeeds
- THEN the file contains `mouse = true` and the key is not removed

#### Scenario: Wrong type replaced [auto]
- GIVEN a file with `mouse = "yes"` (open value: the default true after a notice) and the mouse draft changed to off
- WHEN the save succeeds
- THEN the line reads `mouse = false` as a boolean

#### Scenario: Mouse does not write other keys [auto]
- GIVEN only the mouse draft changed
- WHEN the save succeeds
- THEN no position, `ai.*` or profile key is added or changed

#### Scenario: Saved file parses back [auto]
- GIVEN any save result
- WHEN it is parsed by `Config::parse`
- THEN the positions, `mouse`, the profile and the ai engine and model equal the saved draft and no notice is produced

#### Scenario: AI table created [auto]
- GIVEN a file `# mine\n[statusline]\nposition = "bottom"\n` and the draft engine `ollama`, model `llama3:8b`
- WHEN the save succeeds
- THEN the original bytes are unchanged and the file ends with `[ai]\nengine = "ollama"\nmodel = "llama3:8b"\n`

#### Scenario: Existing AI keys updated in place [auto]
- GIVEN `[ai]` with `engine = "ollama"`, `model = "a"`, a comment line between them, and the draft engine `claude`, model `default`
- WHEN the save succeeds
- THEN only the two values change, and the comment and order are kept

#### Scenario: AI untouched when unchanged [auto]
- GIVEN a file with no `[ai]` table and a changed statusline position only
- WHEN the save succeeds
- THEN the file has no `[ai]` table

#### Scenario: Profile untouched when unchanged [auto]
- GIVEN a file with `profile = "programmer"` and only a position changed
- WHEN the save succeeds
- THEN the profile line is byte-identical and a file without `profile` gains none

#### Scenario: No engine saved as empty string [auto]
- GIVEN `[ai]` with `engine = "ollama"`, `model = "llama3:8b"` and the draft engine changed to `none` (model empty)
- WHEN the save succeeds
- THEN the file contains `engine = ""` and `model = ""`, both keys are still present, and parsing gives ai engine and model unset

#### Scenario: Special characters escaped [auto]
- GIVEN the draft model `a"b\c`
- WHEN the save succeeds and the file is parsed
- THEN the parsed model equals `a"b\c`

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
- THEN the file contains `[ai]` with `engine = "claude"` and `model = "sonnet"`, then `[ai.tasks.code]`, `[ai.tasks.chat]`, `[ai.tasks.review]`, `[ai.tasks.summarize]` in that order, each with the same two keys, and no `profile` key

#### Scenario: General task table preserved [auto]
- GIVEN a file with `[ai.tasks.general]` `engine = "codex"` and a changed code draft
- WHEN the save succeeds
- THEN `[ai.tasks.general]` is byte-identical

#### Scenario: Download-required model kept [auto]
- GIVEN code is saved as `ollama` / `old:7b` and ollama no longer lists `old:7b`
- WHEN a position is saved
- THEN `model = "old:7b"` is still in the file

#### Scenario: Profile created [auto]
- GIVEN a file `# mine\n[statusline]\nposition = "bottom"\n` (open profile is the default `programmer`) and the profile draft changed to `analyst`
- WHEN the save succeeds
- THEN the original bytes are unchanged and the file ends with `[ai]\nprofile = "analyst"\n`

#### Scenario: Profile updated in place [auto]
- GIVEN `[ai]` with `engine = "ollama"`, a comment, `profile = "analyst"` and the draft profile `programmer`
- WHEN the save succeeds
- THEN only the profile value changes and the engine line and the comment are byte-identical

#### Scenario: Default profile written explicitly [auto]
- GIVEN a file with `profile = "analyst"` and the profile draft changed to `programmer` (the default)
- WHEN the save succeeds
- THEN the file contains `profile = "programmer"` and the key is not removed

#### Scenario: Profile does not write engine keys [auto]
- GIVEN only the profile draft changed
- WHEN the save succeeds
- THEN no `engine`, `model` or `[ai.tasks.*]` key is added or changed

### Requirement: Create file and directories
When the file or its parent directories do not exist, the save MUST create them (to the path resolved by the configuration spec). When no config path can be resolved (no usable `XDG_CONFIG_HOME` or absolute `HOME`), the save MUST fail with a footer message and write nothing.

#### Scenario: Missing file and dirs [auto]
- GIVEN no `lazarobox` directory under the config home
- WHEN a save succeeds
- THEN the directory and `config.toml` exist and contain the saved positions

#### Scenario: No config path [auto]
- GIVEN no resolvable config path
- WHEN `s` is pressed on a changed draft
- THEN the menu stays open and the footer shows an error, with nothing written

### Requirement: Atomic write
The file MUST be written atomically: the new content is written to a temporary file in the same directory and then renamed over `config.toml`. A failure at any step MUST leave the original file unchanged and MUST NOT leave a partial `config.toml`.

#### Scenario: Atomic replace [auto]
- GIVEN an existing valid file
- WHEN a save succeeds
- THEN the file is replaced in one rename and no temporary file remains

#### Scenario: Failure keeps the original [auto]
- GIVEN a destination whose write or rename fails
- WHEN a save is attempted
- THEN the original file content is unchanged

#### Scenario: Read-only file refused [auto]
- GIVEN an existing `config.toml` (after resolving symlinks) without owner write permission
- WHEN a position is saved
- THEN the save is refused, the file is unchanged, no temp file remains and the footer shows `config.toml is read-only`

#### Scenario: Dangling symlink followed [auto]
- GIVEN `config.toml` is a symlink (or a chain of at most 8) whose final target does not exist
- WHEN a position is saved
- THEN the target and its parent directories are created, and every link is still a link
- AND a chain longer than 8 links is refused with `save failed: too many symlink levels`

### Requirement: Refuse on errors
If the existing file cannot be parsed as TOML (or cannot be read, or an owned key path collides with a non-table value such as `statusline = 3`), the save MUST be refused: nothing is written, the menu stays open with the draft intact, and the footer shows `config.toml has errors; fix it before saving` for the parse failure. Per-key invalid values that still form valid TOML (for example `position = "middle"`) are NOT errors for this rule; the save replaces the owned key with the valid draft value.

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

### Requirement: Profile key collisions
When the profile is being written, the save MUST be refused (file unchanged, menu open, draft intact) if `ai.profile` holds a table or an array of tables, with the footer `config.toml: ai.profile is a table; fix it before saving`. A non-table scalar under `ai.profile` (for example `profile = 3`) is replaced by the draft string. The check applies only when the profile draft differs from the value captured at open. An `ai` that is not a table is refused by the existing `ai` rule.

#### Scenario: Profile holds a table [auto]
- GIVEN a file with `[ai.profile]` and a changed profile draft
- WHEN `s` is pressed
- THEN the file is byte-identical and the footer shows `config.toml: ai.profile is a table; fix it before saving`

#### Scenario: Scalar of wrong type replaced [auto]
- GIVEN `[ai]` with `profile = 3` (open value: the default `programmer`) and a changed profile draft `analyst`
- WHEN the save succeeds
- THEN the file contains `profile = "analyst"`

#### Scenario: Collision ignored when the profile is unchanged [auto]
- GIVEN a file with `[ai.profile]`, an unchanged profile draft and a changed statusline position
- WHEN `s` is pressed
- THEN the position is saved and `[ai.profile]` is untouched

#### Scenario: ai is a scalar [auto]
- GIVEN a file with `ai = 3` and a changed profile draft
- WHEN `s` is pressed
- THEN the file is byte-identical and the footer shows `config.toml: ai is not a table; fix it before saving`

### Requirement: Memory keys are owned
The save MUST also own `ai.memory.enabled`, `ai.memory.acknowledged` and `ai.memory.port`. The menu has no draft for `port` (it is edited by hand), so the save MUST NEVER add or change it: an existing `port` line and every unknown key under `ai.memory` are preserved byte for byte. Each owned memory key MUST be written only when its draft differs from the value captured at open, as a TOML boolean, and explicitly even when it equals the default; it is never removed. Turning memory off writes `enabled = false` and leaves `acknowledged` as it is. A missing `ai.memory` table MUST be created implicitly with only the changed keys. The save MUST be refused (file unchanged, menu open, draft intact) with the footer `config.toml: ai.memory is not a table; fix it before saving` when `ai.memory` holds a non-table value, and with `config.toml: ai.memory.enabled is a table; fix it before saving` (same for `acknowledged`) when an owned key holds a table; a scalar of the wrong type under an owned key is replaced by the draft boolean. The checks apply only when the draft of that key differs from the value at open. No secret is ever written.

#### Scenario: Table created [auto]
- GIVEN a file `# mine\n[statusline]\nposition = "bottom"\n` and the drafts `enabled = true`, `acknowledged = true`
- WHEN the save succeeds
- THEN the original bytes are unchanged and the file ends with `[ai.memory]\nenabled = true\nacknowledged = true\n`

#### Scenario: Updated in place [auto]
- GIVEN `[ai.memory]` with `enabled = false`, a comment, `port = 8123`, `acknowledged = true` and the draft `enabled = true`
- WHEN the save succeeds
- THEN only the `enabled` value changes and the comment and the `port` line are byte-identical

#### Scenario: Off keeps the acknowledgement [auto]
- GIVEN `enabled = true` and `acknowledged = true` and the draft `enabled = false`
- WHEN the save succeeds
- THEN the file has `enabled = false` and still `acknowledged = true`

#### Scenario: Port never written [auto]
- GIVEN a file without `port` and memory drafts changed
- WHEN the save succeeds
- THEN no `port` key is added

#### Scenario: Untouched when unchanged [auto]
- GIVEN memory drafts equal to the open values and only a position changed
- WHEN the save succeeds
- THEN a file without `[ai.memory]` gains none and an existing one is byte-identical

#### Scenario: Saved file parses back [auto]
- GIVEN any save result with memory drafts
- WHEN it is parsed by `Config::parse`
- THEN enabled and acknowledged equal the drafts and no notice is produced

#### Scenario: Memory holds a scalar or an owned key a table [auto]
- GIVEN a file with `[ai]` holding `memory = 3`, and another with `[ai.memory.enabled]`, and a changed memory draft
- WHEN `s` is pressed
- THEN each file is byte-identical and the matching footer is shown

#### Scenario: Wrong-type scalar replaced [auto]
- GIVEN `[ai.memory]` with `enabled = 3` and the draft `enabled = true`
- WHEN the save succeeds
- THEN the file contains `enabled = true`

#### Scenario: Collision ignored when unchanged [auto]
- GIVEN a file with `[ai.memory.enabled]`, unchanged memory drafts and a changed position
- WHEN `s` is pressed
- THEN the position is saved and the table is untouched

### Requirement: Notify key is owned
The save MUST also own `ai.notify`. It is written only when its draft differs from the value at open, as a TOML boolean (`ConfigEdit::is_flag`), explicitly even when it equals the default, in place when the key exists (comments and neighbours untouched), and created under `[ai]` otherwise. A scalar of the wrong type is replaced. When `ai.notify` holds a table (`[ai.notify]`, an inline table or an array of tables) the save MUST be refused with the footer `config.toml: ai.notify is a table; fix it before saving` and the file unchanged; an unchanged draft is not checked.

#### Scenario: Written as boolean [auto]
- GIVEN no file and the draft `notify = true`
- WHEN the save succeeds
- THEN the file is `[ai]\nnotify = true\n` and parses back as notify on

#### Scenario: Updated in place [auto]
- GIVEN `[ai]\n# c\nnotify = false  # off\nmodel = "m"\n` and the draft `true`
- WHEN the save succeeds
- THEN only the value changes: `notify = true  # off`

#### Scenario: Table collision [auto]
- GIVEN `[ai.notify]\nx = 1\n` and a changed draft
- WHEN the save is attempted
- THEN it is refused with `config.toml: ai.notify is a table; fix it before saving` and the file is byte-identical
