# Delta for Agent Tools

## Base of this delta

`openspec/specs/` does not hold `agent-tools` (checked: it holds 22 capabilities, none of them `agent-tools` or `lazarobox-mcp-server`). Both exist only in the unarchived change `openspec/changes/agent-tools/specs/agent-tools/spec.md`. This delta targets that file, as it will stand when `agent-tools` is archived. The MODIFIED blocks below are full copies of that file's requirements, edited. Order rule: `agent-tools` MUST be archived before `coding-tools`; otherwise this delta has nothing to replace. The `agent-tools` change is not modified by this change. All other requirements of that spec stay as they are, including Path policy, Secrets in file content, Tool output is untrusted data, Tool events, Off switch and No content in logs.

## ADDED Requirements

### Requirement: Tool families
Every registry entry MUST carry exactly one `Family`: `Read`, `Search`, `Write`, `Exec`, `Git`, `GitWrite` or `Pane`. `Git` holds the read verbs (status, diff, log), and `GitWrite` holds add and commit. The set of tools a session sees MUST be a filter over families. `Toolset::Code` MUST include `Read`, `Search`, `Write`, `Exec`, `Git` and `GitWrite`, and MUST NOT include `Pane` or any external tool. The per-family toggles MUST apply as a second filter: a family whose toggle is off is not registered, offered or executed, even when a toolset includes it.

#### Scenario: Code toolset [auto]
- GIVEN every toggle on
- WHEN the registry is filtered for `Toolset::Code`
- THEN it holds Read, Search, Write, Exec and the Git read tools, and no `run_in_pane` and no external tool

#### Scenario: Toggle is a second filter [auto]
- GIVEN `Toolset::Code` and the Exec toggle off
- WHEN the registry is filtered
- THEN `run_command` is absent

### Requirement: Multi-argument arguments
Each registry entry MUST declare its arguments (name, kind string, integer, boolean or list of strings, required or optional, byte cap). Validation MUST be per argument: a missing required argument, a wrong type, an unknown extra key, a duplicate key and an over-cap value MUST each return an error result and execute nothing. This MUST hold on the MCP path and on the Ollama path. The generated schema MUST set `additionalProperties` to false.

#### Scenario: Per-argument errors [auto]
- GIVEN `edit_file` with no `old`, with `old: 5`, with an extra key `mode`, and with a duplicate `path` key
- WHEN each is dispatched
- THEN each returns an error result and nothing is staged

#### Scenario: Optional argument [auto]
- GIVEN `read_file` with and without `start_line`
- WHEN each is validated
- THEN both pass

#### Scenario: Same path on both engines [auto]
- GIVEN a call with a duplicate key
- WHEN it arrives through MCP and through the Ollama loop
- THEN both reject it

### Requirement: Off equals 0.18.0
With every coding-tools toggle off, the tool list, the schemas, the requests and the behaviour MUST be identical to those of release 0.18.0. A characterization test MUST pin this.

#### Scenario: Characterization [auto]
- GIVEN all family toggles off
- WHEN the tool list and an Ollama request are built
- THEN they equal the stored 0.18.0 fixtures byte for byte

## MODIFIED Requirements

### Requirement: Tool registry
There MUST be one registry of tools, the single source of truth for names, argument schemas, descriptions, the family and the read-only or write classification. The same registry MUST serve every adapter (MCP and native). The original built-in tools are `find_dir`, `list_dir`, `read_file` (read tools) and `run_in_pane` (write tool); the `tools/list` entry of each of these four MUST stay byte-identical to release 0.18.0. Further built-in tools (search, write, run and git, see code-search, code-write, command-runner, git-tools) are present only when their family toggle is on. A user MCP tool MUST be added to the registry only under a server-qualified name that cannot equal a built-in name. Registry lookup, argument validation and the path policy MUST be pure functions without IO. A call to an unknown tool, with missing or mistyped arguments, or with unknown extra arguments, MUST return an error result and MUST NOT execute anything.
(Previously: four built-in tools with one string argument each, and no family.)

#### Scenario: One registry, all engines [auto]
- GIVEN the registry
- WHEN the tool lists for the MCP server and for the Ollama request are built
- THEN both contain the same tools with the same names and argument schemas

#### Scenario: Unknown tool [auto]
- GIVEN a call to `delete_file` or `shell`
- WHEN it is dispatched
- THEN an error result is returned and nothing is executed

#### Scenario: Bad arguments [auto]
- GIVEN `read_file` with no `path`, `path: 5`, and an extra key `mode`
- WHEN each is dispatched
- THEN each returns an error result and no file is opened

#### Scenario: Built-in names reserved [auto]
- GIVEN a user MCP server that exposes a tool named `read_file`
- WHEN the registry is built
- THEN the user tool is exposed only under its server-qualified name and the built-in is unchanged

#### Scenario: Original four are byte-identical [auto]
- GIVEN the 0.18.0 `tools/list` fixture for `find_dir`, `list_dir`, `read_file` and `run_in_pane`
- WHEN the registry is built with all toggles off
- THEN those four entries equal the fixture byte for byte

### Requirement: Argument length
Each argument of the original four built-in tools (`path`, `query`, `command`) MUST be at most 4096 bytes of decoded text; a longer one MUST be refused with a too-long error. A tool added by coding-tools MUST declare its own cap per argument in the registry (for example `write_file` content 32 KiB, `edit_file` fields 16 KiB each, a grep pattern 512 bytes), and an argument over its cap MUST be refused with a too-long error. This bounds the decoded text only: the cap on the raw wire message and the rejection of duplicate JSON keys belong to the raw wire layer.
(Previously: one 4096-byte cap for every built-in argument.)

#### Scenario: Oversized argument of an original tool [auto]
- GIVEN a `path` of 4097 bytes, and one of 4096 bytes
- WHEN the call is parsed
- THEN the first is refused with a too-long error and the second is accepted

#### Scenario: Per-tool caps [auto]
- GIVEN `write_file` content of 32 KiB and of 32 KiB + 1 byte
- WHEN each is parsed
- THEN the first is accepted and the second is refused with a too-long error

### Requirement: Approval model
`find_dir` and `list_dir` MUST run without approval, and `read_file` and `grep` MUST run without approval only on a local engine (see Read tools and their approval, code-search). `glob` MUST run without approval. Every other tool call MUST need an explicit user `y`, every time, and the approval of the file write tools is the batch review (see code-write): `create_dir`, `write_file` and `edit_file` stage and ask nothing by themselves, and nothing reaches the disk until the user approves the proposed batch as a whole. `run_command`, `git_add` and `git_commit` MUST each need their own `y` (see command-runner, git-tools). The only exception is a `run_in_pane` call in a chat whose approval mode is `auto` (see Chat approval mode): it is typed without a prompt and never executed. The fixed program allow-list of `run_command` bounds the program and is not an approval. There MUST be no "always allow", no per-session or per-tool trust, no configuration that approves automatically, and nothing MUST ever execute a command for the user without a `y`. Tools from a user MCP server MUST always be write tools. Waiting for an answer MUST NEVER end in an approval: a timeout, a cancel, a closed chat, a closed target pane, a lost connection and an app quit all end as `denied`. A chat MUST have at most one pending approval; another write request while one is pending MUST be denied at once with the reason `another approval is pending`.
(Previously: every write tool call needed its own `y`; there was no staging, no batch review and only `read_file` was a read tool that asked on a cloud engine.)

#### Scenario: No auto approval [auto]
- GIVEN a write tool call from any engine in a chat in `ask` mode, or an external tool call, a `run_command` or a cloud `read_file` in either mode
- WHEN no key is pressed for any length of time (injected clock)
- THEN the call is never executed

#### Scenario: Timeout is denied [auto]
- GIVEN a pending approval and a caller whose wait expires
- WHEN the expiry fires
- THEN the call result is `denied`, the prompt is withdrawn with a system line, and a later `y` writes nothing

#### Scenario: Each call asks again [auto]
- GIVEN a chat in `ask` mode and two `run_in_pane` calls in one turn, the first approved
- WHEN the second is dispatched
- THEN a new approval is required

#### Scenario: Second request while pending [auto]
- GIVEN a pending approval
- WHEN another write tool call arrives
- THEN it returns `denied` (`another approval is pending`) and the first stays pending

#### Scenario: Denied paths [auto]
- GIVEN a pending approval ended in turn by cancel, pane close, target-pane exit and quit
- WHEN each ends
- THEN the result is `denied` each time and nothing is written

#### Scenario: Free read tools around a pending approval [auto]
- GIVEN a pending approval
- WHEN `find_dir`, `list_dir` or `glob` is called (or `read_file` on a local engine)
- THEN it runs and returns without waiting for the approval

#### Scenario: Second read_file while pending [auto]
- GIVEN a pending approval on a cloud engine
- WHEN a `read_file` call arrives
- THEN it returns `denied` (`another approval is pending`)

#### Scenario: Staging asks nothing, apply needs y [auto]
- GIVEN `write_file` and `edit_file` calls in a chat
- WHEN they are dispatched
- THEN no approval opens and the disk is untouched, and only the review of the proposed batch can write

#### Scenario: Allow-list is not an approval [auto]
- GIVEN `run_command` with an allow-listed program in any mode
- WHEN it is dispatched
- THEN a prompt opens and nothing starts before `y`
