# Config Menu Specification (delta: configurable server ports)

Tags as in `config-menu`: [auto] = `cargo test`; [manual] = real machine.

## ADDED Requirements

### Requirement: Stopped servers in the picker
This amends level 1 and level 2 of the Task picker. Level 1 MUST list, after the available engines and before the assigned engines that are not detected, every server engine (`ollama`, `lmstudio`, `llamacpp`, `vllm`) whose binary is on PATH but that did not answer on its port, in catalog order, with the status text `not running`, selectable. Such an engine is listed once, even when a task has it assigned. Level 2 of that engine MUST show, after its assigned models (disabled), a note instead of `engine not detected`: `<id> not running on 127.0.0.1:<port> · start it, then press r: <command>`, where `<port>` is the port detection used (see configuration, Server ports) and `<command>` starts that server on that port on `127.0.0.1`: `ollama serve` (or `OLLAMA_HOST=127.0.0.1:<port> ollama serve` off its default port), `lms server start --port <port>`, `llama-server --port <port> -m <model.gguf>`, `vllm serve <model> --host 127.0.0.1 --port <port>`. The note follows the copyable-note rules (wrapped, not selectable, Enter or `y` copies the whole text). lazarobox never runs that command. With the cursor on one of its saved models (disabled), the footer reads `not running · not selectable`. A server that is not installed keeps `engine not detected`.

#### Scenario: A stopped server is listed [auto, TestBackend + insta]
- GIVEN ollama and llamacpp installed but not answering, lmstudio not installed, claude available
- WHEN level 1 opens
- THEN after claude come `ollama not running` and `llamacpp not running`, both selectable, and lmstudio is not listed

#### Scenario: Choosing a stopped server says how to start it [auto]
- GIVEN `[ai.servers.llamacpp] port = 39080`, llamacpp installed but not answering, and chat assigned to `llamacpp` / `qwen`
- WHEN level 2 of llamacpp opens
- THEN it shows `qwen` disabled and current (footer `not running · not selectable`), then the note `llamacpp not running on 127.0.0.1:39080 · start it, then press r: llama-server --port 39080 -m <model.gguf>`, and Enter on the note copies it

#### Scenario: A server that is not installed is unchanged [auto]
- GIVEN llamacpp not installed
- WHEN level 2 of llamacpp opens
- THEN its last row is `engine not detected`
