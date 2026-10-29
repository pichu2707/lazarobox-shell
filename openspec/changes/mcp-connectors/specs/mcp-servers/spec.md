# Delta for MCP Servers

## Base of this delta

`openspec/specs/` does not hold `mcp-servers`; it exists only in the unarchived change `openspec/changes/agent-tools/specs/mcp-servers/spec.md`, which MUST be archived before `mcp-connectors`. The MODIFIED blocks below are full copies of its requirements "Lifecycle" and "Secrets and privacy", where only the sentence on where environment values come from is changed and scenarios are added at the end. All other requirements stay as they are, including "Every call needs approval" (every call needs `y`, read-only hints ignored; C11 keeps it) and "Where servers are declared" (servers stay in `config.toml`; C4, C5).

Traceability: C6 ("each value reaches only its own server and never `config.toml`"), proposal scope item 1. Assumptions (not user decisions): a name must be listed in the entry's `env` to be passed (the config keeps naming what reaches the server, and the `[[ai.mcp.servers]]` format is unchanged, C5); a value in `secrets.toml` wins over a same-named variable in lazarobox's environment.

## MODIFIED Requirements

### Requirement: Lifecycle
A user server MUST be started only when a chat turn with tools on needs the tool list, by direct exec (no shell), each argument a separate argv element, in a neutral cwd, with a scrubbed environment plus only the variables the entry names, each taking its value from that server's table in `secrets.toml` when the table holds that name, else from lazarobox's own environment (see configuration and mcp-secrets; `LD_*` and `DYLD_*` names, `HOME` and the session token are never taken from an entry or from `secrets.toml`). It MUST NOT receive the lazarobox session token or socket path. The start MUST be announced in the chat with a system line `mcp: started <name>`, and a failure with `mcp: <name> unavailable: <short reason>`; one failing server MUST NOT stop the others or the turn. A server MUST be stopped (process group killed and reaped) when no open chat uses it, on closing the chat that started it, and on quit, inside the same 3-second bound as the PTY teardown, leaving no orphan. Nothing MUST be started when tools are off. A call in flight MUST be cancelled when its turn is cancelled.

#### Scenario: Started on demand [auto]
- GIVEN a configured server and no chat turn
- WHEN the app starts
- THEN no process is started

#### Scenario: Start announced [auto]
- GIVEN a configured server and a turn with tools on
- WHEN the tool list is first needed
- THEN the server is started once and the transcript shows `mcp: started <name>`

#### Scenario: Failure is isolated [auto]
- GIVEN servers `a` (command missing) and `b` (working)
- WHEN a turn starts
- THEN `mcp: a unavailable: ...` is shown, `b`'s tools are available, and the turn runs

#### Scenario: Direct exec, scrubbed environment [auto]
- GIVEN a server entry with args containing `; touch /tmp/x` and one named variable
- WHEN it is started with a recorder
- THEN no shell is involved, the arg is one argv element, the environment holds only the scrubbed base and the named variable, and no lazarobox token

#### Scenario: Shutdown leaves no orphan [auto, unix PTY]
- GIVEN two running user servers
- WHEN the app quits, and in another run the chat is closed
- THEN no server pid exists within 3 seconds after the quit, and the closed chat's exclusive server is gone

#### Scenario: Off starts nothing [auto]
- GIVEN tools off and servers configured
- WHEN a turn runs
- THEN no user server is started

#### Scenario: Cancel in flight [auto]
- GIVEN a tool call in flight on a user server
- WHEN the turn is cancelled
- THEN the call is cancelled and the chat is idle

#### Scenario: Value from the secrets file [auto]
- GIVEN an entry `clarity` with `env = ["CLARITY_API_TOKEN"]`, a `secrets.toml` holding `[clarity]` with `CLARITY_API_TOKEN = "tok"`, and no such variable in lazarobox's environment
- WHEN it is started with a recorder
- THEN the child's environment holds `CLARITY_API_TOKEN=tok`, the scrubbed base, and nothing else from `secrets.toml`

#### Scenario: Secrets file wins over the host variable [auto]
- GIVEN the same entry and file, and `CLARITY_API_TOKEN=host` in lazarobox's environment
- WHEN it is started with a recorder
- THEN the child gets `tok`

#### Scenario: Host variable when the file lacks the name [auto]
- GIVEN an entry naming `GITHUB_TOKEN`, no `GITHUB_TOKEN` in its `secrets.toml` table, and `GITHUB_TOKEN=gh` in lazarobox's environment
- WHEN it is started with a recorder
- THEN the child gets `gh`, as before this change

#### Scenario: Forbidden names stay out [auto]
- GIVEN an entry naming `LD_PRELOAD` and `HOME`, and a `secrets.toml` table for it holding `LD_PRELOAD`, `HOME` and the session token name
- WHEN it is started with a recorder
- THEN none of them is taken from the entry or the file, and `HOME` is the real home

### Requirement: Secrets and privacy
Environment values for a user server MUST come only from the variables its entry names: from that server's table in `secrets.toml` (see mcp-secrets), else from lazarobox's own environment; the config holds names, never values. A value in `secrets.toml` MUST reach only the server whose name is its table, and only when the entry names it. Values MUST NOT be logged, drawn in the transcript or sent to a model. A user server's stderr MUST NOT be shown raw: at most a sanitized, capped line in the failure notice. Tool arguments and results MUST NOT be written to any log.

#### Scenario: Names, not values [auto]
- GIVEN an entry naming `GITHUB_TOKEN`
- WHEN the server is started, the transcript is drawn and logs are inspected
- THEN the child has the variable, and its value appears nowhere else

#### Scenario: Stderr is sanitized [auto]
- GIVEN a server that prints an escape sequence and a long line on stderr and exits
- WHEN the failure notice is built
- THEN the notice is one sanitized, capped line

#### Scenario: A value reaches only its own server [auto]
- GIVEN entries `clarity` and `ga4`, both naming `CLARITY_API_TOKEN`, and a `secrets.toml` holding it only under `[clarity]`
- WHEN both are started with a recorder and no host variable is set
- THEN `clarity` gets the value and `ga4` does not

#### Scenario: Unnamed key is not passed [auto]
- GIVEN `[clarity]` in `secrets.toml` holding `CLARITY_API_TOKEN` and `EXTRA`, and an entry naming only `CLARITY_API_TOKEN`
- WHEN it is started with a recorder
- THEN the child has no `EXTRA`

#### Scenario: File values appear nowhere else [auto]
- GIVEN a `secrets.toml` value `sk-123` passed to a server
- WHEN the server is started, fails, and the transcript, the notices and the logs are inspected
- THEN `sk-123` appears in none of them, and not in `config.toml`
