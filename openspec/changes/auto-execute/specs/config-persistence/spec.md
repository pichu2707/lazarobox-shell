# Delta for Config Persistence

Applies on top of the unarchived `agent-tools` delta for config-persistence. Tags as in the main spec.

## MODIFIED Requirements

### Requirement: Approval mode key is owned
The save MUST also own `ai.tools.approval`, with the same rules as `ai.tools.enabled`: written only when its draft differs from the value captured at open, as the TOML string `"ask"`, `"auto"` or `"run"`, explicitly even when it equals the default, in place when the key exists (comments and neighbours untouched), created under an implicit `[ai.tools]` table otherwise, and a value of the wrong type replaced. The save MUST be refused (file unchanged, menu open, draft intact) with `config.toml: ai.tools is not a table; fix it before saving` when `ai.tools` holds a non-table value, and `config.toml: ai.tools.approval is a table; fix it before saving` when `approval` holds a table or an array of tables; these checks apply only when the approval draft differs from the value at open.
(Previously: the written strings were `"ask"` or `"auto"`)

#### Scenario: Created next to enabled [auto]
- GIVEN `[ai.tools]\nenabled = true\n` and the approval draft changed to `run`
- WHEN the save succeeds
- THEN `enabled = true` is byte-identical and the table gains `approval = "run"`

#### Scenario: Updated in place [auto]
- GIVEN `approval = "run"  # mine` and the draft changed to `ask`
- WHEN the save succeeds
- THEN only the value changes: `approval = "ask"  # mine`

#### Scenario: Untouched when unchanged [auto]
- GIVEN the approval draft equals the open value and only a position changed
- WHEN the save succeeds
- THEN no `approval` key is added and an existing one is byte-identical

#### Scenario: Collision [auto]
- GIVEN `[ai.tools.approval]` and a changed approval draft
- WHEN `s` is pressed
- THEN the file is byte-identical and the footer is `config.toml: ai.tools.approval is a table; fix it before saving`

#### Scenario: Saved file parses back [auto]
- GIVEN any save result with an approval draft among `ask`, `auto` and `run`
- WHEN it is parsed by `Config::parse`
- THEN the approval mode equals the draft and no notice is produced
