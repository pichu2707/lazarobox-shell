# Delta for Agent Tools

Slice tags as in the agent-orchestrator delta of this change (W1..W10).

## MODIFIED Requirements

### Requirement: Toolsets by session role [S1, W2, W6, W8, W11]
The tool registry MUST offer by role: chat session = chat tools as before; orchestrator session = chat read tools plus `list_panes` and `read_pane` with the same asks as in a chat [W11], plus `spawn_agent`, `wait`, `collect`, `cancel` ([S4] and `finish`), plus the tools of the user's MCP servers, each call asking `y` exactly as in a chat (user decision, 2026-10-29), and never `run_in_pane` and never write tools or `run_command` (the coordinator keeps Read plus Orchestra whatever the toggles say); worker session = read tools for every kind, plus the coding-tools write tools for a `code` or `simple` worker, plus `run_command` for a `code` or `simple` worker ([W6]), never `run_in_pane`, never user server tools, never orchestration tools. [W2] The Write and Exec toggles are a per-role ceiling: a write tool is offered to a worker only when Write is on, `run_command` only when Exec is on, and a toggle never adds a tool the role does not have (a `review` worker stays read-only with both on). Orchestration tools MUST NOT be reachable from user MCP servers, from a chat pane whose ORCH mode is off, or while the global orchestrator gate is off. The orchestrator session exists only for a turn of an orchestrating pane.
(Previously: "worker session = read tools only (until write tools exist)"; the menu toggles did not limit workers; no worker could be offered `run_command`)

#### Scenario: The coordinator reads panes but never types in them [W11, auto]
- GIVEN an orchestrator turn, a `code` worker and a `review` worker
- WHEN each tool list is built and the orchestrator calls `run_in_pane`
- THEN the orchestrator holds `list_panes` and `read_pane` and not `run_in_pane`, its `run_in_pane` call is refused unasked, and neither worker holds any of the three

#### Scenario: Role toolsets [auto]
- GIVEN each role
- WHEN the tool list is built
- THEN it matches the role definition exactly

#### Scenario: Orchestrator user server tool asks [auto]
- GIVEN an orchestrator turn and a user MCP server with tool `search`
- WHEN the tool list is built and `search` is called
- THEN `search` is listed beside the orchestration tools, the call asks `y`, and the server is contacted only after `y`; a worker's call to it is refused unasked

#### Scenario: Ceiling limits a writer [W2, auto]
- GIVEN Write off and Exec off
- WHEN the tool lists of a `code` worker, a `simple` worker and a `review` worker are built
- THEN each holds the read tools only

#### Scenario: Ceiling never widens [W2, auto]
- GIVEN Write and Exec on
- WHEN the tool lists of a `review` worker and of the orchestrator are built
- THEN the `review` worker holds the read tools only and the orchestrator holds no write tool and no `run_command`

#### Scenario: A forged call is refused visibly [W2, auto]
- GIVEN a worker without `run_command` because Exec is off
- WHEN it calls `run_command` anyway
- THEN the call returns an error result naming the cause, no ask opens and nothing starts

### Requirement: read_pane tool [W11]
A read-only tool `read_pane` of the `PaneRead` family (with `list_panes`; `Pane` keeps only `run_in_pane`) MUST let a chat or the orchestrator (coordinator) session read the text of one pane. It MUST be part of the default tool list and MUST NOT be offered to a `Toolset::Code` worker or a worker session. The coordinator's call asks exactly as a chat's. Arguments: `pane` (integer, required) and `scrollback` (integer, optional, a number of lines). The target MUST be a pane that `list_panes` lists, EXCLUDING the chat's own pane; any other number returns an error result and reads nothing.

By default the text is the visible screen. With `scrollback` it also holds scrollback lines, at most 200 lines and 8 KiB. The cap MUST also bound a large visible screen. When the text is over a cap the TAIL is kept and the text MUST say it was cut. There is no paging and nothing is stored.

The call MUST ask on EVERY call, on every engine (local included, an Ollama `*-cloud` model too). No approval mode (`ask`, `auto`, `run`), grant or toggle answers it for the user. The text MUST be read and frozen when the prompt opens; the approved frozen text is what is returned, even if the pane changes or closes afterwards. The text MUST pass through `redact(clean())` and then the untrusted wrapper with a fresh nonce (see Tool output is untrusted data). The tool has no side effects and MUST NOT write to a pane.

#### Scenario: Asks in every mode and engine [auto]
- GIVEN a local engine and a cloud engine, each in `ask`, `auto` and `run`
- WHEN `read_pane` is called
- THEN each call raises a pending approval and nothing is returned before `y`

#### Scenario: Approved text is the sent text [auto]
- GIVEN a prompt opened on pane 2 and the pane then prints more lines
- WHEN the user presses `y`
- THEN the result equals the previewed text, wrapped as untrusted

#### Scenario: Pane closes after the freeze [auto]
- GIVEN an open prompt and pane 2 then closed
- WHEN the user presses `y`
- THEN the frozen text is still returned

#### Scenario: Secrets redacted [auto]
- GIVEN a screen showing `API_KEY=sk-abc123secret`
- WHEN the prompt text is built
- THEN neither the preview nor the result contains the secret value

#### Scenario: Scrollback tail kept [auto]
- GIVEN 500 scrollback lines and `scrollback: 500`
- WHEN the text is built
- THEN it holds the last 200 lines at most, within 8 KiB, and says it was cut

#### Scenario: Large visible screen capped [auto]
- GIVEN a visible screen over 8 KiB
- WHEN the text is built
- THEN the tail within 8 KiB is kept and the cut is marked

#### Scenario: Targets limited [auto]
- GIVEN the chat in pane 1 and `list_panes` listing panes 1 and 2
- WHEN `read_pane` is called with pane 1, with 9 and with 2
- THEN pane 1 and 9 return an error and read nothing; pane 2 asks

#### Scenario: Not offered to workers [auto]
- GIVEN a worker and a `Toolset::Code` session
- WHEN each tool list is built
- THEN neither holds `read_pane`, and a call to it is refused unasked

#### Scenario: The coordinator reads a pane [W11, auto]
- GIVEN an orchestrator turn and `nvim` in pane 2
- WHEN the coordinator calls `read_pane` with pane 2
- THEN it asks exactly as in a chat, and after `y` the result is the previewed text, wrapped as untrusted

#### Scenario: Bad arguments [auto]
- GIVEN `read_pane` with no `pane`, `pane: "2"`, `scrollback: "x"` and an extra key
- WHEN each is dispatched
- THEN each returns an error result and no pane is read
(Previously: "MUST NOT be offered to a `Toolset::Code` worker, an orchestrator (coordinator) session or a worker session".)
