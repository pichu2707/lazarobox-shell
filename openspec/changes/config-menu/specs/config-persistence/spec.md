# Config Persistence Specification

## Purpose

How the config menu writes `config.toml`. Only the menu-owned keys change; the rest of the file is the user's. Tags: [auto] = `cargo test`; [manual] = real Kitty.

## ADDED Requirements

### Requirement: Save on Enter
Enter in MENU MUST request a save of the draft values and, only when the save succeeds, close the menu with the draft as the new current values (mode TERMINAL). Until the answer arrives the menu stays open and swallows input. When the draft equals the values captured at open, Enter MUST close the menu without reading or writing the file.

#### Scenario: Save closes [auto]
- GIVEN a changed draft
- WHEN Enter is pressed and the save succeeds
- THEN the menu closes, mode is TERMINAL, and the draft values are the current values

#### Scenario: Unchanged draft [auto]
- GIVEN the draft equals the original values
- WHEN Enter is pressed
- THEN the menu closes and the file is not read or written

### Requirement: Owned keys only
The save MUST read the file at save time and change only `statusline.position` and `tabbar.position`. Comments, key order, whitespace, unknown keys and tables and every other key MUST be preserved, including any `mouse` entry as it is. The menu MUST NEVER write `mouse = true`, and MUST NOT add or modify the `mouse` key. A missing owned key or table MUST be created; the value MUST be one of `"top"` or `"bottom"`.

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
- THEN the positions equal the saved draft and no notice is produced

### Requirement: Create file and directories
When the file or its parent directories do not exist, the save MUST create them (to the path resolved by the configuration spec). When no config path can be resolved (no usable `XDG_CONFIG_HOME` or absolute `HOME`), the save MUST fail with a footer message and write nothing.

#### Scenario: Missing file and dirs [auto]
- GIVEN no `lazarobox` directory under the config home
- WHEN a save succeeds
- THEN the directory and `config.toml` exist and contain the saved positions

#### Scenario: No config path [auto]
- GIVEN no resolvable config path
- WHEN Enter is pressed on a changed draft
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

### Requirement: Refuse on errors
If the existing file cannot be parsed as TOML (or cannot be read, or an owned key path collides with a non-table value such as `statusline = 3`), the save MUST be refused: nothing is written, the menu stays open with the draft intact, and the footer shows `config.toml has errors; fix it before saving` for the parse failure. Per-key invalid values that still form valid TOML (for example `position = "middle"`) are NOT errors for this rule; the save replaces the owned key with the valid draft value.

#### Scenario: Unparseable file refused [auto]
- GIVEN a file with a TOML syntax error
- WHEN Enter is pressed on a changed draft
- THEN the file is byte-identical afterward, the menu is still open, and the footer shows `config.toml has errors; fix it before saving`

#### Scenario: Invalid value replaced [auto]
- GIVEN `[statusline] position = "middle"` (valid TOML)
- WHEN `top` is saved
- THEN the value becomes `"top"` and the save succeeds

#### Scenario: Non-table collision refused [auto]
- GIVEN a file with `statusline = 3`
- WHEN a position is saved
- THEN the save is refused, the file is unchanged and the footer shows an error

### Requirement: Failure feedback
When the save fails for any reason (I/O error, no path, refused), the menu MUST stay open in MENU mode with the draft and the live preview unchanged, and the footer MUST show a one-line error (for write failures, `save failed: <short error>`). Enter MAY be pressed again to retry, and Esc MUST still revert and close. The runtime answers a save request with exactly one success or failure result.

#### Scenario: Write fails [auto]
- GIVEN a save that fails with an I/O error
- WHEN the failure result arrives
- THEN mode is MENU, the preview is unchanged, and the footer shows `save failed: <error>`

#### Scenario: Retry and cancel after failure [auto]
- GIVEN a failed save with the error in the footer
- WHEN Enter succeeds on a retry, or Esc is pressed instead
- THEN the menu closes with the saved values, or reverts to the original values, respectively

#### Scenario: Persistence in Kitty [manual, Kitty]
- GIVEN a hand-commented `config.toml`
- WHEN a position is changed and saved from the menu, then the file is inspected, then broken on purpose and saved again
- THEN comments survive and only the owned key changed; the broken file is never overwritten and the footer reports the error
