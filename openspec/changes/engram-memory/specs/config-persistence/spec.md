# Delta for Config Persistence

Tags: [auto] = `cargo test`. The other requirements (save on `s`, create file, atomic write, refuse on errors, failure feedback, AI keys collisions, profile key collisions, nested table edits) are unchanged. The requirement below EXTENDS the owned-key list of "Owned keys only": at archive, that list gains `ai.memory.enabled`, `ai.memory.acknowledged` and `ai.memory.port`, and every preservation rule of that requirement (comments, key order, unknown keys, write only when changed, no secret) applies to them unchanged.

## ADDED Requirements

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
