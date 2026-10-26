# Chat Engines Specification (delta: managed llama-server options)

Tags as in `chat-engines`: [auto] = `cargo test`; [manual] = real machine.

## ADDED Requirements

### Requirement: The model file is checked before each start
Before it starts the managed llama-server, once the program is found, lazarobox MUST check that the configured model file exists. When it does not, it MUST NOT start anything and the turn MUST fail with `llama-server not started: model file not found: <path>`. The check MUST run on every start and never when the config loads.

#### Scenario: Missing model [auto]
- GIVEN `managed = true` and a `model` path with no file
- WHEN a llamacpp turn starts
- THEN nothing is started and the turn reads "llama-server not started: model file not found: <path>"

#### Scenario: Model back in place [auto]
- GIVEN the same config once the file exists (a drive mounted later)
- WHEN the next llamacpp turn starts
- THEN llama-server starts
