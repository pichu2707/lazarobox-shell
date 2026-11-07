# Delta for Agent Tools

## ADDED Requirements

### Requirement: read_pane tool
A read-only tool `read_pane` of the `Pane` family MUST let a chat read the text of one pane. It MUST be part of the default tool list and MUST NOT be offered to a `Toolset::Code` worker, an orchestrator (coordinator) session or a worker session. Arguments: `pane` (integer, required) and `scrollback` (integer, optional, a number of lines). The target MUST be a pane that `list_panes` lists, EXCLUDING the chat's own pane; any other number returns an error result and reads nothing.

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

#### Scenario: Not offered to other roles [auto]
- GIVEN a worker, an orchestrator and a `Toolset::Code` session
- WHEN each tool list is built
- THEN none holds `read_pane`, and a call to it is refused unasked

#### Scenario: Bad arguments [auto]
- GIVEN `read_pane` with no `pane`, `pane: "2"`, `scrollback: "x"` and an extra key
- WHEN each is dispatched
- THEN each returns an error result and no pane is read

## MODIFIED Requirements

### Requirement: Tool registry
There MUST be one registry of tools, the single source of truth for names, argument schemas, descriptions, the family and the read-only or write classification. The same registry MUST serve every adapter (MCP and native). The original built-in tools are `find_dir`, `list_dir`, `read_file` (read tools) and `run_in_pane` (write tool); the `tools/list` entry of each of these four MUST stay byte-identical to release 0.18.0. The default tool list also holds `list_panes` and `read_pane` (see their requirements). Further built-in tools (search, write, run and git, see code-search, code-write, command-runner, git-tools) are present only when their family toggle is on. A user MCP tool MUST be added to the registry only under a server-qualified name that cannot equal a built-in name. Registry lookup, argument validation and the path policy MUST be pure functions without IO. A call to an unknown tool, with missing or mistyped arguments, or with unknown extra arguments, MUST return an error result and MUST NOT execute anything.
(Previously: the default tool list held only `list_panes` besides the original four.)

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

### Requirement: Off equals 0.18.0
With every coding-tools toggle off, the tool list, the schemas, the requests and the behaviour MUST be identical to those of release 0.18.0, except that the default tool list also holds `list_panes` and `read_pane` (see their requirements; the pinned bytes are no longer the 0.18.0 ones, on purpose). A characterization test MUST pin this.
(Previously: only `list_panes` was the exception.)

#### Scenario: Characterization [auto]
- GIVEN all family toggles off
- WHEN the tool list and an Ollama request are built
- THEN they equal the stored fixtures byte for byte (the 0.18.0 ones plus `list_panes` and `read_pane`), and the golden `tools_list.json` holds `read_pane`
