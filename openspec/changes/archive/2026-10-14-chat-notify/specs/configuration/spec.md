# Delta for Configuration

Tags: [auto] = `cargo test`.

## ADDED Requirements

### Requirement: The notify flag
`[ai] notify` is a boolean. A missing key is `false`. A value that is not a boolean falls back to `false` and produces the problem `ai.notify: expected a boolean`, which follows the `ai.context` problem in the notice order. It is independent of every other `[ai]` key.

#### Scenario: Default [auto]
- GIVEN no `[ai]` table, or one without `notify`
- WHEN the config is read
- THEN notify is `false` and there is no problem

#### Scenario: Explicit [auto]
- GIVEN `[ai]\nnotify = true`
- WHEN the config is read
- THEN notify is `true` and there is no problem

#### Scenario: Wrong type [auto]
- GIVEN `[ai]\nnotify = "yes"` (or an integer, or a table)
- WHEN the config is read
- THEN notify is `false` and the problem is `ai.notify: expected a boolean`
