# Delta for Configuration

Tags: [auto] = `cargo test`. The file location, the fallback rules, the notices and every existing key (`ai.profile` and `ai.context` included) are unchanged; `[ai.memory]` was an unknown table before this change and is now known.

## ADDED Requirements

### Requirement: Memory keys
The file MAY hold a table `[ai.memory]` with `enabled` (boolean, default `false`), `port` (integer from 1 to 65535, default the value of the `ENGRAM_PORT` environment variable when that is a valid port, else `7437`) and `acknowledged` (boolean, default `false`). Any key may be omitted. Unknown keys under `ai.memory` MUST be ignored. A wrong type or out-of-range value MUST fall back to the default for that key only and report a problem under the existing startup-notice rules (`config: <problem>`, or `config (N problems): <first problem>`), with the problems `ai.memory.enabled: expected a boolean`, `ai.memory.acknowledged: expected a boolean`, `ai.memory.port: expected an integer from 1 to 65535`, and, when `ai.memory` is not a table, `ai.memory: expected a table`. Memory is active only when `enabled` and `acknowledged` are both true (see chat-memory). The token is never part of the schema: `ENGRAM_HTTP_TOKEN` is read from the environment only and no secret is ever read from or written to the file.

#### Scenario: Defaults [auto]
- GIVEN a file with no `[ai.memory]`
- WHEN it is parsed
- THEN enabled is false, acknowledged is false, the port is 7437 and no notice is produced

#### Scenario: Values parsed [auto]
- GIVEN `[ai.memory]` with `enabled = true`, `acknowledged = true` and `port = 8123`
- WHEN it is parsed
- THEN memory is active on port 8123 and no notice is produced

#### Scenario: Enabled without acknowledgement is inactive [auto]
- GIVEN `[ai.memory]` with `enabled = true` only
- WHEN it is parsed
- THEN memory is not active

#### Scenario: Port default from the environment [auto]
- GIVEN no `port` key and `ENGRAM_PORT=8123`, and in another run `ENGRAM_PORT=abc`
- WHEN each is parsed
- THEN the port is 8123, and 7437 for the invalid value, and a `port` key always wins

#### Scenario: Wrong types and range [auto]
- GIVEN `enabled = "yes"`, `port = 0`, `port = 70000`, `port = "7437"` and `acknowledged = 1` in separate files
- WHEN each is parsed
- THEN each key falls back to its default and the notice names the matching problem

#### Scenario: Counted with other problems [auto]
- GIVEN `port = 0` and `[statusline] position = "middle"`
- WHEN the app starts
- THEN the notice starts with `config (2 problems): ` and both keys use their defaults

#### Scenario: Non-table ai.memory [auto]
- GIVEN a file with `[ai]` holding `memory = 3`
- WHEN the app starts
- THEN memory uses the defaults and the notice is `config: ai.memory: expected a table`

#### Scenario: Unknown keys ignored [auto]
- GIVEN `[ai.memory]` with `endpoint = "http://x"` next to valid keys
- WHEN it is parsed
- THEN the valid keys apply and no notice is produced
