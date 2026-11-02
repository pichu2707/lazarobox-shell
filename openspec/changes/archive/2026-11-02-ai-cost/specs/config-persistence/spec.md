# Delta for Config Persistence

Tags as in the main spec.

## ADDED Requirements

### Requirement: Usage keys are owned
The save MUST also own `ai.usage.line`, `ai.usage.statusline`, `ai.usage.turn_tokens`, `ai.usage.day_tokens`, `ai.usage.day_usd` and `ai.usage.billing.claude`, `.codex`, `.gemini`. Each owned key MUST be written only when its draft differs from the value captured at open, in place when the key exists (comments and neighbours untouched) and created otherwise, with missing tables created implicitly holding only the changed keys. Booleans are written as TOML booleans and billing modes as the strings `subscription` or `per-token`, explicitly even when they equal the default. A threshold is written as a TOML integer (`turn_tokens`, `day_tokens`) or float (`day_usd`, with at most 2 decimals); clearing a threshold (draft empty) MUST remove its key, leaving everything else byte for byte, and removing a key that does not exist changes nothing. A scalar of the wrong type under an owned key is replaced. Unknown keys under `ai.usage` and `ai.usage.billing` are preserved. The save MUST be refused (file unchanged, menu open, draft intact) with the footer `config.toml: ai.usage is not a table; fix it before saving` when `ai.usage` holds a non-table value, `config.toml: ai.usage.billing is not a table; fix it before saving` when `billing` does, and `config.toml: ai.usage.<key> is a table; fix it before saving` when an owned key holds a table. The checks apply only when a draft of the matching keys differs from the value at open. No secret is ever written.

#### Scenario: Table created [auto]
- GIVEN a file `# mine\n[statusline]\nposition = "bottom"\n` and the drafts `line = false`, `turn_tokens = 4000`
- WHEN the save succeeds
- THEN the original bytes are unchanged and the file ends with `[ai.usage]\nline = false\nturn_tokens = 4000\n`

#### Scenario: Updated in place [auto]
- GIVEN `[ai.usage]\n# c\nline = true  # on\nfoo = 1\n` and the draft `line = false`
- WHEN the save succeeds
- THEN only the value changes: `line = false  # on`, and `foo` and the comment are byte-identical

#### Scenario: Default written explicitly [auto]
- GIVEN no file and the draft `statusline` changed off and back to on before the save, then `claude` changed to `per-token` and back
- WHEN the save succeeds
- THEN nothing is written for either key (drafts equal the open values)

#### Scenario: Billing written [auto]
- GIVEN no file and the draft `claude = per-token`
- WHEN the save succeeds
- THEN the file is `[ai.usage.billing]\nclaude = "per-token"\n` and it parses back as per-token with no problem

#### Scenario: Clearing a threshold removes the key [auto]
- GIVEN `[ai.usage]\nturn_tokens = 4000\nday_tokens = 9000\n` and the `turn_tokens` draft emptied
- WHEN the save succeeds
- THEN the file has `day_tokens = 9000` and no `turn_tokens`

#### Scenario: Float threshold [auto]
- GIVEN the `day_usd` draft `2.5`
- WHEN the save succeeds
- THEN the file holds `day_usd = 2.5` and parses back as 2.5

#### Scenario: Untouched when unchanged [auto]
- GIVEN usage drafts equal to the open values and only a position changed
- WHEN the save succeeds
- THEN a file without `[ai.usage]` gains none and an existing one is byte-identical

#### Scenario: Collisions refuse [auto]
- GIVEN a file with `[ai]\nusage = 3`, another with `[ai.usage]\nbilling = 1`, another with `[ai.usage.line]`, and a changed usage draft each
- WHEN `s` is pressed
- THEN each file is byte-identical and the matching footer is shown

#### Scenario: Collision ignored when unchanged [auto]
- GIVEN a file with `[ai.usage.line]`, unchanged usage drafts and a changed position
- WHEN `s` is pressed
- THEN the position is saved and the table is untouched

#### Scenario: Saved file parses back [auto]
- GIVEN any save result with usage drafts
- WHEN it is parsed by `Config::parse`
- THEN every draft equals the parsed value and no notice is produced
