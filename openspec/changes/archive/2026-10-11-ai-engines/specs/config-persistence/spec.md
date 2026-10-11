# Delta for Config Persistence

No Purpose change needed ("Only the menu-owned keys change" stays true).

## ADDED Requirements

### Requirement: AI keys collisions
When the AI keys are being written, the save MUST be refused (file unchanged, menu open, draft intact) if `ai` is not a table, or if `ai.engine` or `ai.model` holds a table or array of tables. The footer MUST show `config.toml: ai is not a table; fix it before saving`, `config.toml: ai.engine is a table; fix it before saving` or `config.toml: ai.model is a table; fix it before saving` respectively. A non-table scalar under `ai.engine` or `ai.model` (for example `engine = 3`) is replaced by the draft string. These checks MUST NOT apply when the AI draft equals the values captured at open.

#### Scenario: ai is a scalar [auto]
- GIVEN a file with `ai = 3` and a changed engine draft
- WHEN Enter is pressed
- THEN the file is byte-identical and the footer shows `config.toml: ai is not a table; fix it before saving`

#### Scenario: engine holds a table [auto]
- GIVEN a file with `[ai.engine]` and a changed engine draft
- WHEN Enter is pressed
- THEN the file is unchanged and the footer shows `config.toml: ai.engine is a table; fix it before saving`

#### Scenario: Scalar of wrong type replaced [auto]
- GIVEN `[ai]` with `engine = 3` and a changed engine draft `ollama`
- WHEN the save succeeds
- THEN the file contains `engine = "ollama"`

#### Scenario: Collision ignored when AI unchanged [auto]
- GIVEN a file with `ai = 3`, an unchanged AI draft and a changed statusline position
- WHEN Enter is pressed
- THEN the position is saved and `ai = 3` is untouched

## MODIFIED Requirements

### Requirement: Owned keys only
The save MUST read the file at save time and change only `statusline.position`, `tabbar.position`, `ai.engine` and `ai.model`. Comments, key order, whitespace, unknown keys and tables and every other key MUST be preserved, including any `mouse` entry as it is. The menu MUST NEVER write `mouse = true`, and MUST NOT add or modify the `mouse` key. A missing owned key or table MUST be created; a position value MUST be one of `"top"` or `"bottom"`. The `ai.*` keys MUST be written only when the AI draft differs from the values captured at open, as TOML strings with correct escaping, and whenever the AI draft differs, both `ai.engine` and `ai.model` MUST be written together. "No engine" and "no model" MUST be written as the empty string (`engine = ""`, `model = ""`), never by removing the key. No secret, token or key MUST ever be written. When the AI draft equals the open values, a file without `[ai]` MUST gain none.
(Previously: owned keys were only `statusline.position` and `tabbar.position`.)

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
