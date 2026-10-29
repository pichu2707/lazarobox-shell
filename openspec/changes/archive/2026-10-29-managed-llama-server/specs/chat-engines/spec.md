# Chat Engines Specification (delta: managed llama-server)

Tags as in `chat-engines`: [auto] = `cargo test`; [manual] = real machine.

## ADDED Requirements

### Requirement: Managed llama-server lifecycle
When llamacpp is managed (see configuration), the first llamacpp chat turn MUST start `llama-server`, found in the login `PATH` lazarobox gives its tools (absolute entries only) and run by that absolute path with that `PATH`, with exactly the arguments `--host 127.0.0.1 --port <port> -m <model>` (no shell), stdin closed, in its own process group, with stdout and stderr in `llama-server.log` under the state folder (`$XDG_STATE_HOME/lazarobox`, else `~/.local/state/lazarobox`), emptied on each start, mode 0600 and never opened through a symlink. The turn MUST wait until `GET /v1/models` answers 200, for at most 90 s, and then stream as any llamacpp turn. Nothing else starts it: not config load, not detection, not a turn of another engine. At most one server runs; later turns reuse it and one that ended is started again. Before a start, if anything answers on the port, the turn MUST fail with `llama-server not started: another program answers on 127.0.0.1:<port>` and nothing is started. A start that fails MUST end the turn with one reason: `could not start llama-server: is it on the PATH?`, or `llama-server stopped` or `llama-server did not answer within 90 s` followed by `: ` and the last line of its log when it has one; a server that did not answer is killed. Quitting lazarobox MUST stop the server's whole process group; on Linux the server MUST also get SIGKILL from the kernel when lazarobox dies without quitting. A cancelled start kills the server it began.

The chat gate MUST treat a managed llamacpp whose binary is installed (`available` or `not running`) as ready, without looking up the assigned model name, which `llama-server` ignores. With no model assigned, the model path is sent.

#### Scenario: A managed llama-server starts on the first turn [auto]
- GIVEN a managed llamacpp and a fake `llama-server` that answers once started
- WHEN two llamacpp turns run, and a turn of another engine runs
- THEN the fake is started once with the exact arguments, both llamacpp turns stream, and the other engine's turn starts nothing

#### Scenario: Quitting stops the managed llama-server [auto]
- GIVEN a running managed server with a child of its own
- WHEN lazarobox quits
- THEN the server and its child are gone

#### Scenario: A port that already answers is not ours [auto]
- GIVEN something that answers 200, 404 or 503 on the managed port
- WHEN a llamacpp turn runs
- THEN the turn fails with the port message and nothing is started

#### Scenario: A failed start says why [auto]
- GIVEN a missing program, a server that exits with `error: failed to open /m/q.gguf` on stderr, or one that never answers
- WHEN a llamacpp turn runs
- THEN the turn fails with the matching reason (the log's last line included) and no server is left

#### Scenario: The managed llama-server never outlives lazarobox (Linux) [auto]
- GIVEN a server started by a thread that then ends
- WHEN the thread is gone
- THEN the server is dead

#### Scenario: Closing the terminal stops the chat servers [manual]
- GIVEN lazarobox with a managed server running, in a terminal that is then closed
- WHEN the terminal goes away
- THEN lazarobox quits through its normal cleanup and no server is left
