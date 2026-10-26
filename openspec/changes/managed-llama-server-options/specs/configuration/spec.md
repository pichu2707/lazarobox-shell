# Configuration Specification (delta: managed llama-server options)

## ADDED Requirements

### Requirement: Extra llama-server arguments
`[ai.servers.llamacpp] args` MUST be read as a list of strings, whether `managed` is on or not, and passed to the managed llama-server after `--host 127.0.0.1 --port <port> -m <model>`, each entry as one argv element with no shell. A list holding an option lazarobox sets (`--host`, `--port`, `-m`, `--model`, also in the `--name=value` form) MUST be ignored whole with the problem `ai.servers.llamacpp.args: "<entry>" is set by lazarobox`; a value that is not a list of strings MUST be ignored with `ai.servers.llamacpp.args: expected a list of strings`. The menu MUST NOT write `args`.

#### Scenario: Extra arguments [auto]
- GIVEN `args = ["-ngl", "99", "--alias", "my model"]`
- WHEN the managed llama-server starts
- THEN its argv ends with `-ngl`, `99`, `--alias`, `my model` after `-m <model>`

#### Scenario: A clashing entry [auto]
- GIVEN `args = ["-ngl", "99", "--port=8080"]`
- WHEN the config loads
- THEN no extra argument is used and the problem names "--port=8080"

### Requirement: The idle time of the managed llama-server
`[ai.servers.llamacpp] idle_minutes` MUST be read as a whole number of minutes from 0 (never) up, whether `managed` is on or not; a missing key is 15, and any other value falls back to 15 with the problem `ai.servers.llamacpp.idle_minutes: expected a whole number of minutes (0 = never)`. The Settings tab of the menu MUST have a `llama-server idle` row, after `Sound`, that steps through `5 min`, `15 min`, `30 min` and `never` (`l` forward, `h` back), shows a value between those as `<n> min` and steps from it to the next one each way, and saves only `idle_minutes`, written as a TOML integer.

#### Scenario: Stepping [auto]
- GIVEN the row on `15 min`
- WHEN `l` is pressed four times
- THEN it reads `30 min`, `never`, `5 min`, `15 min`

#### Scenario: Saved as a number [auto]
- GIVEN the row moved to `30 min`
- WHEN the menu saves
- THEN config.toml holds `idle_minutes = 30` under `[ai.servers.llamacpp]` and its other keys are kept
