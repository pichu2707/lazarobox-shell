# Delta for Config Persistence

Tags: [auto] = `cargo test`. Same rules as the `ai.context` flag.

## ADDED Requirements

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
