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

### Requirement: An idle managed llama-server stops
The managed llama-server MUST stop once no llamacpp turn has used it for the idle time (`[ai.servers.llamacpp] idle_minutes`, default 15; 0 never stops it), checked at least every 30 s, and the next llamacpp turn MUST start it again. A turn in progress MUST keep it running; the idle time MUST count from the end of the last turn, or from the start of the server. A new idle time saved from the menu MUST apply at once, with no restart.

#### Scenario: Idle [auto]
- GIVEN a running managed server that no turn used for the idle time
- WHEN the idle check runs
- THEN the server is stopped, and the next llamacpp turn starts it again

#### Scenario: A long answer [auto]
- GIVEN a turn still streaming past the idle time
- WHEN the idle check runs
- THEN the server keeps running; it stops only once the idle time has passed after the turn ended
