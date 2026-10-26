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
