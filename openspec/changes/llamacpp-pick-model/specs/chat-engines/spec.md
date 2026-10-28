# Chat Engines Specification (delta: llamacpp pick model)

Tags as in `chat-engines`: [auto] = `cargo test`; [manual] = real machine.

## MODIFIED Requirements

### Requirement: Managed llama-server lifecycle
When llamacpp is managed (see configuration), the first llamacpp chat turn, or a menu save that turns it on (see A managed llama-server turned on from the menu), MUST start `llama-server`, found in the login `PATH` lazarobox gives its tools (absolute entries only) and run by that absolute path with that `PATH`, with exactly the arguments `--host 127.0.0.1 --port <port> -m <model>` (no shell), stdin closed, in its own process group, with stdout and stderr in `llama-server.log` under the state folder (`$XDG_STATE_HOME/lazarobox`, else `~/.local/state/lazarobox`), emptied on each start, mode 0600 and never opened through a symlink. The turn MUST wait until `GET /v1/models` answers 200, for at most 90 s, and then stream as any llamacpp turn. Nothing else starts it: not config load, not detection, not a turn of another engine. At most one server runs; later turns reuse it and one that ended is started again. Before a start, if anything answers on the port, the turn MUST fail with `llama-server not started: another program answers on 127.0.0.1:<port>` and nothing is started. A start that fails MUST end the turn with one reason: `could not start llama-server: is it on the PATH?`, or `llama-server stopped` or `llama-server did not answer within 90 s` followed by `: ` and the last line of its log when it has one; a server that did not answer is killed. Quitting lazarobox MUST stop the server's whole process group; on Linux the server MUST also get SIGKILL from the kernel when lazarobox dies without quitting. A cancelled start kills the server it began.

## ADDED Requirements

### Requirement: A managed llama-server turned on from the menu
A save that turns the managed llama-server on, or changes its model, MUST start it in the background at once, without a restart: the statusline reads `starting llama-server with <file>…`, then `llama-server is ready` (and engines are detected again) or the reason it did not start. From then on turns and detection use its port. The server it replaces MUST stop and never start again; a turn that still holds it fails with `llama-server was switched to another model; send again`. Quitting stops the current one as before.

#### Scenario: The next turn uses it [auto]
- GIVEN lazarobox started with llamacpp unmanaged
- WHEN the menu turns it on and a llamacpp turn is sent
- THEN the turn starts that server and talks to its port, and quitting stops it
