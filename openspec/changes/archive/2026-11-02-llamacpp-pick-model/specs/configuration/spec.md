# Configuration Specification (delta: llamacpp pick model)

Tags as in `configuration`: [auto] = `cargo test`; [manual] = real machine.

## MODIFIED Requirements

### Requirement: Server ports
The file MAY hold one table `[ai.servers.<id>]` per server engine (`ollama`, `lmstudio`, `llamacpp`, `vllm`) with `port` (integer from 1 to 65535, default the catalog port). Detection and chat both use it, always on `127.0.0.1`. Unknown ids and unknown keys under `ai.servers` MUST be ignored. A wrong type or out-of-range value MUST fall back to the catalog port for that server only and report a problem under the existing startup-notice rules, with the problems `ai.servers.<id>.port: expected an integer from 1 to 65535`, `ai.servers.<id>: expected a table` when the server entry is not a table, and `ai.servers: expected a table` when `ai.servers` is not a table. The menu MUST NOT write `ai.servers` except `managed` and `model` of `[ai.servers.llamacpp]` (see config-menu, Choosing a local model manages llama-server), and every save MUST keep the rest of it byte for byte. The ports are read at startup; only a save that turns the managed llama-server on changes llamacpp's port while running.
