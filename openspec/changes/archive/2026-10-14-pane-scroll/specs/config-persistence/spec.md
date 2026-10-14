# Delta for Config Persistence

Tags: [auto] = `cargo test`; [manual] = real Kitty. The other requirements (save on `s`, create file, atomic write, refuse on errors, failure feedback, AI keys collisions, profile key collisions, nested table edits) are unchanged.

Archive-order dependency: the base text of "Owned keys only" is the ai-chat delta (`openspec/changes/ai-chat/specs/config-persistence/spec.md`, which adds `ai.profile`). This delta adds `mouse` to the owned keys and replaces the "never write mouse" rule; apply it after ai-chat (and after any later change that edits the same requirement).

## MODIFIED Requirements

### Requirement: Owned keys only
The save MUST read the file at save time and change only `statusline.position`, `tabbar.position`, `mouse`, `ai.engine`, `ai.model`, `ai.profile` and `ai.tasks.<task>.engine` and `ai.tasks.<task>.model` for `<task>` in `code`, `chat`, `review`, `summarize`. Comments, key order, whitespace, unknown keys and tables and every other key MUST be preserved, including any `[ai.tasks.general]` table. A missing owned key or table MUST be created (including `ai` and `ai.tasks` implicitly, with no empty parent header added); a position value MUST be one of `"top"` or `"bottom"`.

`mouse` MUST be written only when the mouse draft differs from the value captured at open (the config value, default true when the key is absent or invalid), as the TOML boolean `true` or `false` (not a string), and it MUST be written explicitly even when it equals the default. It MUST remain a top-level key: a missing `mouse` key MUST be inserted before the first table header of the file (or appended when the file has no table), never inside a table. An existing `mouse` line (including a non-boolean value such as `mouse = "yes"`) MUST be updated in place keeping its trailing comment. When the mouse draft equals the open value the `mouse` line MUST be left exactly as it is, and a file without `mouse` MUST gain none.

The `ai.*` keys MUST be written only when the AI draft differs from the values captured at open, as TOML strings with correct escaping, and whenever a task's draft differs, both its `engine` and `model` MUST be written together; tasks whose draft equals the open values MUST NOT be written. `ai.profile` MUST be written only when the profile draft differs from the value captured at open, as one of `"analyst"` or `"programmer"` (default `programmer`), and it MUST be written explicitly even when it equals the default; it is never removed. "No engine" and "no model" MUST be written as the empty string (`engine = ""`, `model = ""`), never by removing the key. No secret, token or key MUST ever be written. When the AI draft equals the open values, a file without `[ai]` MUST gain none.
(Previously: `mouse` was not an owned key; the menu MUST NEVER write `mouse = true` and MUST NOT add or modify the `mouse` key)

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
