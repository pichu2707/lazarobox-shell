# AI Engine Detection Specification (delta: configurable server ports)

Tags as in `ai-engine-detection`: [auto] = `cargo test`; [manual] = real machine.

## MODIFIED Requirements

### Requirement: Engine catalog
The catalog MUST be fixed and ordered as in `ai-engine-detection` (8 ids, 4 servers then 4 CLIs). Servers are probed over HTTP on `127.0.0.1` only, on their catalog port (`ollama` 11434, `lmstudio` 1234, `llamacpp` 8080, `vllm` 8000) unless `[ai.servers.<id>] port` sets another one (see configuration, Server ports). A server's binary name is used only for the PATH lookup and is never executed. `/health` MUST NOT be used to decide availability.

#### Scenario: A configured port is probed [auto]
- GIVEN `[ai.servers.llamacpp] port` set to the port of a fake server that lists one model owned by `llamacpp`
- WHEN detection runs
- THEN llamacpp is `available` with that model, and the fake recorded one `GET /v1/models`

### Requirement: Read-only and localhost-only
As in `ai-engine-detection`, except that the 4 server endpoints are on the configured port of each server: detection MUST send only `GET` requests with no body and no credentials to the 4 server endpoints on `127.0.0.1`, each on its configured port, and MUST NOT connect to any other host or port.

### Requirement: Documented limits
The user documentation MUST state that custom endpoints and hosts are not supported (the host is always `127.0.0.1`), how to set a server's port, and that shell aliases are not detected (the binary must be on PATH).
