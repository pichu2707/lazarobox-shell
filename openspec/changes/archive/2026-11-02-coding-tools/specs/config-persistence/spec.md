# Delta for Config Persistence

## Base of this delta

`openspec/specs/config-persistence/spec.md` exists. The owned key `ai.tools.enabled` is added by the unarchived `agent-tools` change, so `agent-tools` MUST be archived first. Only ADDED requirements are used.

## ADDED Requirements

### Requirement: Family toggle keys are owned
The five family keys under `[ai.tools]` (see configuration, Family toggle keys) MUST be owned by the config menu like `ai.tools.enabled`: saving writes only a key whose value changed, through the existing comment-preserving edit, and leaves every other key, table and comment byte for byte as it was. A key the user never changed MUST NOT be added to the file. A save MUST NOT write any MCP server list or any secret.

#### Scenario: Only the changed key is written [auto]
- GIVEN a file with comments and the Write toggle changed to on
- WHEN the config is saved
- THEN the file holds `write = true` under `[ai.tools]`, no other key was added, and all comments are unchanged

#### Scenario: No change, no write [auto]
- GIVEN the menu opened and closed with all toggles unchanged
- WHEN `s` is pressed
- THEN the file is not touched

#### Scenario: Round trip [auto]
- GIVEN every family toggle saved as on
- WHEN the file is parsed again
- THEN all five toggles read on
