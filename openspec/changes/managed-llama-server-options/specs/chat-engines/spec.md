# Chat Engines Specification (delta: managed llama-server options)

Tags as in `chat-engines`: [auto] = `cargo test`; [manual] = real machine.

## MODIFIED Requirements

### Requirement: Managed llama-server lifecycle
When llamacpp is managed (see configuration), the first llamacpp chat turn, or a menu save that turns it on (see A managed llama-server turned on from the menu), MUST start `llama-server`, found in the login `PATH` lazarobox gives its tools (absolute entries only) and run by that absolute path with that `PATH`, with exactly the arguments `--host 127.0.0.1 --port <port> -m <model>` (no shell; `<port>` is the port it runs on, see below; the extra `args` of the configuration follow), stdin closed, in its own process group, with stdout and stderr in `llama-server.log` under the state folder (`$XDG_STATE_HOME/lazarobox`, else `~/.local/state/lazarobox`), emptied on each start, mode 0600 and never opened through a symlink. The turn MUST wait until `GET /v1/models` answers 200, for at most 90 s, and then stream as any llamacpp turn. Nothing else starts it: not config load, not detection, not a turn of another engine. One server runs per model, each on its own port: before a start, lazarobox MUST look at the configured port and the seven after it, in order. A ready llama-server there (`GET /v1/models` answers 200) whose `/props` `model_path` is the same model file (links followed), behind a listening socket of the same user, MUST be used as it is, from another lazarobox or started by hand: nothing starts, the turn talks to its port, and lazarobox MUST NOT stop it (not on quit, idle or a model change). Anything else that answers on a port (another model, a server not ready, another program, a socket of another user or one whose owner cannot be read) MUST be left alone, and lazarobox's own server MUST start on the first port where nothing answers; with none, the turn MUST fail with `llama-server not started: no free port on 127.0.0.1 from <port> to <port+7>` and nothing is started. Later turns reuse the server they used; when it is gone (one of lazarobox's own that ended, or a used one that went away) the next turn looks again. A start whose server exits because another lazarobox started the same model on that port first MUST use that server. Detection probes the port the managed server runs on. A start that fails MUST end the turn with one reason: `could not start llama-server: is it on the PATH?`, or `llama-server stopped` or `llama-server did not answer within 90 s` followed by `: ` and the last line of its log when it has one; a server that did not answer is killed. Quitting lazarobox MUST stop the server's whole process group; on Linux the server MUST also get SIGKILL from the kernel when lazarobox dies without quitting. A cancelled start kills the server it began.

The chat gate MUST treat a managed llamacpp whose binary is installed (`available` or `not running`) as ready, without looking up the assigned model name, which `llama-server` ignores. With no model assigned, the model path is sent.

#### Scenario: A managed llama-server starts on the first turn [auto]
- GIVEN a managed llamacpp and a fake `llama-server` that answers once started
- WHEN two llamacpp turns run, and a turn of another engine runs
- THEN the fake is started once with the exact arguments, both llamacpp turns stream, and the other engine's turn starts nothing

#### Scenario: Quitting stops the managed llama-server [auto]
- GIVEN a running managed server with a child of its own
- WHEN lazarobox quits
- THEN the server and its child are gone

#### Scenario: The same model already running is used [auto]
- GIVEN a ready llama-server of the same user serving the configured model on the managed port
- WHEN a llamacpp turn runs
- THEN nothing is started, the turn talks to that port, and the idle stop never stops it

#### Scenario: Another model runs on the port [auto]
- GIVEN a ready llama-server serving another model on the managed port
- WHEN a llamacpp turn runs
- THEN lazarobox's server starts with `--port <port+1>` and the turn talks to that port

#### Scenario: Every port is taken [auto]
- GIVEN something that is not the configured model answering on the managed port and the seven after it
- WHEN a llamacpp turn runs
- THEN the turn fails with "llama-server not started: no free port on 127.0.0.1 from <port> to <port+7>" and nothing is started

#### Scenario: A socket of another user [auto]
- GIVEN the owners of the sockets listening on a port
- WHEN the port is looked at
- THEN its server counts as the same user's only when every listener there is the user's own (Linux) or one of the user's processes listens there (macOS)

#### Scenario: Two lazarobox start the same model at once [auto]
- GIVEN a start whose server exits while the same model starts answering on its port
- WHEN the turn runs
- THEN it uses that server and nothing of its own is left

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

#### Scenario: Two lazarobox, two models [manual]
- GIVEN two lazarobox, one chatting with a managed model
- WHEN the second chats with the same model, then with another one
- THEN the second uses the first one's server, then starts its own on the next port, and both keep answering

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
