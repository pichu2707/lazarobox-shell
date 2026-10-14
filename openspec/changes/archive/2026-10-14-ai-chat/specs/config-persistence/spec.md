# Delta for Config Persistence

Tags: [auto] = `cargo test`; [manual] = real Kitty. The other requirements (save on `s`, create file, atomic write, refuse on errors, failure feedback, AI keys collisions, nested table edits) are unchanged; the profile follows the `ai` collision rules already defined there.

## MODIFIED Requirements

### Requirement: Owned keys only
The save MUST read the file at save time and change only `statusline.position`, `tabbar.position`, `ai.engine`, `ai.model`, `ai.profile` and `ai.tasks.<task>.engine` and `ai.tasks.<task>.model` for `<task>` in `code`, `chat`, `review`, `summarize`. Comments, key order, whitespace, unknown keys and tables and every other key MUST be preserved, including any `mouse` entry as it is and any `[ai.tasks.general]` table. The menu MUST NEVER write `mouse = true`, and MUST NOT add or modify the `mouse` key. A missing owned key or table MUST be created (including `ai` and `ai.tasks` implicitly, with no empty parent header added); a position value MUST be one of `"top"` or `"bottom"`. The `ai.*` keys MUST be written only when the AI draft differs from the values captured at open, as TOML strings with correct escaping, and whenever a task's draft differs, both its `engine` and `model` MUST be written together; tasks whose draft equals the open values MUST NOT be written. `ai.profile` MUST be written only when the profile draft differs from the value captured at open, as one of `"analyst"` or `"programmer"` (default `programmer`), and it MUST be written explicitly even when it equals the default; it is never removed. "No engine" and "no model" MUST be written as the empty string (`engine = ""`, `model = ""`), never by removing the key. No secret, token or key MUST ever be written. When the AI draft equals the open values, a file without `[ai]` MUST gain none.
(Previously: `ai.profile` did not exist and was not an owned key)

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
- THEN the positions, the profile and the ai engine and model equal the saved draft and no notice is produced

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

#### Scenario: Profile untouched when unchanged [auto]
- GIVEN a file with `profile = "programmer"` and only a position changed
- WHEN the save succeeds
- THEN the profile line is byte-identical and a file without `profile` gains none

#### Scenario: Profile does not write engine keys [auto]
- GIVEN only the profile draft changed
- WHEN the save succeeds
- THEN no `engine`, `model` or `[ai.tasks.*]` key is added or changed

## ADDED Requirements

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
