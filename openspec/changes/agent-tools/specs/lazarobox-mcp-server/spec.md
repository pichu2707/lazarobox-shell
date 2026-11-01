# lazarobox MCP Server Specification

## Purpose

How a CLI engine (claude, codex, gemini, and opencode when its adapter exists) reaches the tool registry: a per-chat stdio MCP shim, `lazarobox __mcp-serve`, that relays to the running app over a private unix socket guarded by a session token. The app, not the shim, executes tools, so the registry, the path policy and the approval UX (see agent-tools, pane-commands) are the single place of control. The CLIs' own shell, edit and file tools stay off (see chat-engines). Tags: [auto] = `cargo test` (fake CLI that speaks MCP, temp runtime dir); [manual] = real CLI on a real machine; [design-confirm] = injection mechanism or limit that design MUST verify against the pinned CLI version before the scenario becomes [auto]; [conditional] = applies only if the named adapter is merged.

Terms: "shim" = the `__mcp-serve` process the CLI starts. "Session" = one chat's token, socket and tool state. "Runtime dir" = the private per-user directory (mode 0700) that holds the sockets.

## Requirements

### Requirement: Shim process
`lazarobox __mcp-serve` MUST be an internal subcommand that speaks the MCP stdio transport (newline-delimited JSON-RPC, the protocol revision pinned by design). It MUST implement `initialize`, `tools/list` and `tools/call` and answer every other method with a JSON-RPC error. It MUST NOT execute a tool itself, read or write any user file, start a process or open a network connection other than the session socket. It MUST NOT be subject to the nesting guard (see terminal-session). stdout MUST carry only protocol messages; diagnostics go to stderr without content. Malformed JSON, an oversized message (cap pinned by design) and an unknown tool MUST produce an error response and MUST NOT panic or end the process.

#### Scenario: Handshake and list [auto]
- GIVEN a shim connected to a fake app session with tools on
- WHEN a client sends `initialize` and `tools/list`
- THEN the response lists exactly the registry tools with their schemas

#### Scenario: Unknown method [auto]
- GIVEN a connected shim
- WHEN `resources/read` is sent
- THEN a JSON-RPC error is returned and the shim keeps running

#### Scenario: Malformed and oversized input [auto]
- GIVEN a connected shim
- WHEN invalid JSON, then a message over the cap, then a valid call are sent
- THEN the first two get errors and the third is served

#### Scenario: Not blocked by the nesting guard [auto]
- GIVEN `LAZAROBOX` is set in the shim's environment
- WHEN the shim is started with `__mcp-serve`
- THEN it runs

#### Scenario: Shim does no work [auto]
- GIVEN a shim under a syscall-recording harness
- WHEN `tools/call` for `read_file` is served
- THEN the shim opened no user file and started no process; the app executed the tool

### Requirement: Socket and session token
Each chat session MUST have its own unix socket in the runtime dir (mode 0700) with mode 0600, and a random session token of at least 128 bits. The app MUST accept a connection only after the shim presents the token, MUST compare it in constant time, and MUST close a connection with a wrong or missing token without serving it. The token MUST NOT appear in argv, in logs or in the transcript. A session MUST be bound to one chat: its token only authorizes that chat's tools and approvals, and after the chat closes, its socket is removed and every later call fails. Sockets left by a crash MUST be removed at the next start and at exit. Another user's process MUST NOT be able to connect (permissions).

#### Scenario: Permissions [auto]
- GIVEN a started session
- WHEN the runtime dir and the socket are inspected
- THEN the dir is 0700 and the socket is 0600

#### Scenario: Wrong token [auto]
- GIVEN a session
- WHEN a client connects and sends a wrong token, and another sends none
- THEN both are closed and no tool runs

#### Scenario: Token not in argv [auto]
- GIVEN a CLI started with the shim configured
- WHEN the recorded argv and the process list of the CLI and shim are read
- THEN the token is in neither

#### Scenario: Token bound to its chat [auto]
- GIVEN sessions for chats A and B
- WHEN A's token is used on B's socket
- THEN the connection is rejected

#### Scenario: Cleanup [auto]
- GIVEN a closed chat, and in another run a stale socket from a crash
- WHEN the chat closes, or the app starts
- THEN the socket file is removed

### Requirement: Tools served
The shim MUST serve only the tools of the registry that apply to the session (the three read tools, `run_in_pane`, and the user MCP tools when configured, see mcp-servers). With tools off no shim is started. The engine-side permission MUST allow every lazarobox tool, because lazarobox is the policy point: `find_dir` and `list_dir` run freely, and `read_file` and the write tools wait for the user's `y` inside lazarobox (every CLI is a cloud engine, see agent-tools), except `run_in_pane` in a chat in `auto` mode, which is typed without a prompt (see agent-tools, Chat approval mode). Write tools MUST be described as having side effects. A `tools/call` MUST be executed by the app under the registry rules (path policy, caps, validation) and the result MUST be returned as MCP content labeled as untrusted data (see agent-tools). Tool events MUST be emitted to the chat for every call.

#### Scenario: Only registry tools [auto]
- GIVEN a session
- WHEN `tools/call` names `bash` or `write_file`
- THEN an error result is returned and nothing runs

#### Scenario: read_file asks through MCP [auto]
- GIVEN a `read_file` call through the shim of any CLI
- WHEN it is served
- THEN an approval naming the path is pending and the file is not opened until `y`; `find_dir` and `list_dir` through the shim need none

#### Scenario: Policy applies through MCP [auto]
- GIVEN a call to `read_file` on `~/.ssh/id_ed25519` through the shim
- WHEN it is served
- THEN the same refusal as in the registry is returned

#### Scenario: Events emitted [auto]
- GIVEN a `find_dir` call through the shim
- WHEN it completes
- THEN the chat receives a `ToolCall` and a `ToolResult`

#### Scenario: Tools off [auto]
- GIVEN tools off
- WHEN a turn starts
- THEN no shim, socket or token is created and the CLI is started exactly as before

### Requirement: Approval round trip
A `run_in_pane` call through the shim MUST block until the user answers or the wait ends, and MUST return `sent` or `denied` only. In a chat in `auto` mode it returns as soon as the command is typed or refused, unless it falls back to the prompt (see agent-tools, Chat approval mode); the mode is never an approval made by the app on its own, because it only types. The prompt appears in the chat pane (see chat-pane, pane-commands). If the CLI's own MCP call timeout is shorter than the user's decision, the call returns `denied` (never `sent`) when it expires, the prompt is withdrawn, and a later `y` writes nothing. A shim that dies, a closed socket and a cancelled turn MUST end the pending approval as `denied`. The app MUST NOT approve on its own at any point. The CLI's default MCP timeout MUST be measured by design for each engine, and a longer timeout MAY be configured in the injection only where the CLI supports it.

#### Scenario: Blocks until answered [auto]
- GIVEN a `run_in_pane` call through a fake shim
- WHEN `y` is pressed after 30 s (injected clock)
- THEN the call returns `sent` after the write

#### Scenario: CLI timeout expires first [auto]
- GIVEN a fake CLI whose call timeout is 10 s and a user who answers at 20 s
- WHEN the timeout fires
- THEN the result is `denied`, the prompt is withdrawn, and `y` at 20 s writes nothing

#### Scenario: Shim dies [auto]
- GIVEN a pending approval
- WHEN the shim process is killed
- THEN the approval ends as `denied` and the prompt is withdrawn

#### Scenario: Cancel ends the approval [auto]
- GIVEN a pending approval
- WHEN the chat turn is cancelled
- THEN the approval is denied and the CLI is killed as for any cancel

#### Scenario: Measured defaults [design-confirm]
- GIVEN each pinned CLI version
- WHEN design measures its MCP tool-call timeout
- THEN the value is recorded and the injection sets a longer one only where supported

### Requirement: Jail and process rights
The shim MUST run inside the CLI's jail without widening it: the jail spec gains only the minimal grants: read-and-execute on the exact shim binary, the rule for the session socket file, and the shim's ELF loader when it differs from the CLI's own loader (no directory-wide grant, no `$HOME`, no `/tmp`, no new execute right for any other program). If the shim cannot execute or connect inside the jail on a CLI, that CLI MUST ship without tools (text only, plus pane-commands) rather than widen the jail.

#### Scenario: The shim's trust check names its cause and its fix [auto]
- GIVEN tools on and a shim binary (or its loader) that the trust check refuses because it or its folder is writable by its group or others, as after `cargo install` under umask 002 (issue #496)
- WHEN a turn starts
- THEN the turn runs as text and the notice is `tools are off: <paths> are writable by its group; run: chmod g-w <paths>` (or `by others` / `o-w`, or `by its group or others` / `go-w`), naming every offending path in one command, with `~` for `$HOME` and each path quoted; if the user does not own all of them, the chmod covers only the ones they own and the notice says the rest belong to someone else and need their owner (sudo), never only the generic `tools are off for this turn: they could not be set up`
- AND a path that cannot be inspected gives `tools are off: cannot check <path>: permission denied` (or `not found`), never `writable`
- AND the trust rule is unchanged: any group or other write bit refuses

#### Scenario: Jail spec delta [auto]
- GIVEN the jail spec of a CLI with tools on, and without
- WHEN both are built
- THEN the first differs by the shim binary (rx), the socket file rule and, only when it differs from the CLI's loader, the shim's ELF loader (rx)

#### Scenario: Shim runs jailed [auto, tests/jail.rs]
- GIVEN the helper with the shim binary and the socket granted
- WHEN a jailed program executes the shim and connects to the socket
- THEN both succeed, and `execve` of `/usr/bin/curl` still fails

#### Scenario: Landlock connect rights [design-confirm]
- GIVEN the kernel ABI used by the jail
- WHEN design checks that a pathname-socket connect is allowed by the granted rights
- THEN the finding is recorded; if it fails, tools are gated off for jailed engines

### Requirement: Per-engine injection
Each CLI adapter MUST register the shim as the only MCP server of the turn, by the mechanism pinned by design, and MUST keep the CLI's own shell, edit, web and file tools off so the only tools are the lazarobox ones. The adapter MUST allow the lazarobox tools to be called without the CLI asking its own permission (the approval is lazarobox's). An engine whose injection cannot be verified against its pinned version MUST ship without tools and behave exactly as before, and the pane MUST NOT show an error for it. Configuration files written for injection MUST live in the per-turn private tmp dir (mode 0700, already granted to the jail), with file mode 0600, MUST hold no secret other than the session token if the mechanism needs it, and MUST be wiped when the turn ends.

#### Scenario: claude [design-confirm]
- GIVEN the pinned claude version
- WHEN a turn runs with tools on
- THEN argv carries the pinned MCP config option, only the lazarobox tools are allowed, the built-in tools stay disabled, and a `find_dir` call through it works

#### Scenario: codex [design-confirm]
- GIVEN the pinned codex version and the `-c mcp_servers.*` route
- WHEN a turn runs with tools on
- THEN the shim is the only server, codex's shell and patch tools stay disabled, and a `find_dir` call works; if not verifiable, codex runs text-only

#### Scenario: gemini [design-confirm]
- GIVEN the pinned gemini version, its `mcpServers` settings and a policy allow for the lazarobox tools
- WHEN a turn runs with tools on
- THEN the shim is the only server, gemini's built-in tools stay disabled, and a `find_dir` call works; if not verifiable, gemini runs text-only

#### Scenario: opencode [conditional: opencode adapter merged]
- GIVEN the opencode adapter has landed and its config and permission routes are verified per version
- WHEN a turn runs with tools on
- THEN the shim is the only MCP server and opencode's own tools are denied; without the adapter this requirement has no effect and opencode stays `not supported in chat yet`

#### Scenario: Unverified engine unchanged [auto]
- GIVEN an adapter whose injection is flagged unverified
- WHEN a turn runs with tools on
- THEN argv, stdin and environment are byte-identical to the turn with tools off

#### Scenario: Config file hygiene [auto]
- GIVEN an adapter that writes an injection config file
- WHEN a turn runs and ends
- THEN the file was mode 0600 in the per-turn tmp dir (0700) and no longer exists

#### Scenario: CLI tools stay off [manual]
- GIVEN each real CLI with tools on
- WHEN the message asks it to create a file and to run `ls`
- THEN no file is created and no command runs except through a lazarobox confirm

#### Scenario: Whole flow per engine [manual, Kitty]
- GIVEN each supported CLI logged in
- WHEN the user asks to find `curso-rust` and go there
- THEN a `find_dir` row, a confirm and, after `y`, the typed `cd` appear, and the configured engine and model are unchanged
