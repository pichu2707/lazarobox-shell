# Configuration Specification (delta: managed llama-server)

Tags as in `configuration`: [auto] = `cargo test`; [manual] = real machine.

## ADDED Requirements

### Requirement: Managed llama-server
`[ai.servers.llamacpp]` MAY hold `managed` (boolean, default `false`) and `model` (string, the path of the model file `llama-server` loads). With `managed = true` and a non-empty `model`, lazarobox manages the server (see chat-engines), and llamacpp's port defaults to 39281 instead of 8080; `port` still overrides it. Under any other server id, `managed` and `model` MUST be ignored like any unknown key. A wrong value MUST leave the server unmanaged and report one problem under the existing startup-notice rules: `ai.servers.llamacpp.managed: expected true or false`, `ai.servers.llamacpp.model: expected a path to a model file` (also when `managed` is off), or `ai.servers.llamacpp.model: required when managed = true` (missing or empty).

#### Scenario: A managed llama-server is opt-in [auto]
- GIVEN no `managed`, `managed = false`, or a `model` without `managed`
- WHEN it is parsed
- THEN nothing is managed, llamacpp keeps its port and no notice is produced

#### Scenario: A managed llama-server has its own port [auto]
- GIVEN `managed = true` and `model = "/m/q.gguf"`
- WHEN it is parsed
- THEN the server is managed with that model on port 39281, or on `port` when it is set

#### Scenario: A wrong managed value turns it off with its problem [auto]
- GIVEN `managed = "yes"`, `managed = true` without a model or with `model = ""`, or `model = 1`
- WHEN it is parsed
- THEN nothing is managed, llamacpp keeps port 8080 and the notice names the matching problem
