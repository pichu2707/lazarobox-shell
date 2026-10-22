# Configuration Specification (delta: configurable server ports)

Tags as in `configuration`: [auto] = `cargo test`; [manual] = real machine.

## ADDED Requirements

### Requirement: Server ports
The file MAY hold one table `[ai.servers.<id>]` per server engine (`ollama`, `lmstudio`, `llamacpp`, `vllm`) with `port` (integer from 1 to 65535, default the catalog port). Detection and chat both use it, always on `127.0.0.1`. Unknown ids and unknown keys under `ai.servers` MUST be ignored. A wrong type or out-of-range value MUST fall back to the catalog port for that server only and report a problem under the existing startup-notice rules, with the problems `ai.servers.<id>.port: expected an integer from 1 to 65535`, `ai.servers.<id>: expected a table` when the server entry is not a table, and `ai.servers: expected a table` when `ai.servers` is not a table. The menu MUST NOT write `ai.servers` and every save MUST keep it byte for byte. The ports are read at startup.

#### Scenario: Server ports default to the catalog [auto]
- GIVEN a file with no `[ai.servers]`, or an empty one
- WHEN it is parsed
- THEN the ports are 11434, 1234, 8080 and 8000 and no notice is produced

#### Scenario: Server ports are read [auto]
- GIVEN `[ai.servers.llamacpp] port = 39080` and `[ai.servers.ollama] port = 11500`
- WHEN it is parsed
- THEN those two servers use those ports and the others keep their catalog port

#### Scenario: Unknown servers and keys are ignored [auto]
- GIVEN `[ai.servers.claude]`, `[ai.servers.nope]` and `[ai.servers.vllm]` with `port = 8001` and `host = "0.0.0.0"`
- WHEN it is parsed
- THEN only vllm changes (port 8001, host unchanged) and no notice is produced

#### Scenario: A wrong server value falls back alone [auto]
- GIVEN `ai.servers = 1`, `[ai.servers] llamacpp = 39080`, `port = 0`, `port = 70000` and `port = "39080"` in separate files
- WHEN each is parsed
- THEN every server keeps its catalog port and the notice names the matching problem

#### Scenario: A turn goes to the configured port [auto]
- GIVEN llamacpp and ollama configured on two different fake servers
- WHEN a llamacpp turn runs
- THEN only the llamacpp fake receives a connection

#### Scenario: Saves keep the table [auto]
- GIVEN a file with commented `[ai.servers.*]` tables, one of them invalid
- WHEN the menu saves other keys
- THEN the tables are unchanged byte for byte, and a file without them gains none
