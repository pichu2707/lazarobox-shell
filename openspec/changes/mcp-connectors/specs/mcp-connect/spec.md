# mcp-connect Specification

## Purpose

The Connectors area of the config menu makes connecting a catalog server easy (C1, C3) without the menu writing any configuration: the user declares the server in `config.toml` `[[ai.mcp.servers]]` (C4, C5) and puts its values in `secrets.toml` (C6). The area shows the guide, the entry to paste and the values to fill, runs the install and the login as visible commands in a pane, opens guide links in the browser, and probes the server (see mcp-catalog). Slice tags: [S3] install pane, login pane and opener, [S4] menu screens. Scenario tags: [auto] = `cargo test` (TestBackend, temp config folder, fake programs); [manual] = real machine. Items marked "assumption" are proposals, not user decisions; key bindings and texts are provisional and pinned by design.

Terms: "the area" = the Connectors area. "Entry screen" = the screen of one catalog entry.

## Requirements

### Requirement: The area writes no configuration [S4]
No action in the area MUST add, change, reorder or remove any byte of `config.toml` (the rule "MCP server lists are never written" of config-persistence stays, C4) or create, write or change the mode of `secrets.toml` (C6). The area MUST NOT draw any value from `secrets.toml`; it MAY read which names have a non-empty value, to show the state.

#### Scenario: Byte-identical after every action [auto]
- GIVEN a `config.toml` with hand-written `[[ai.mcp.servers]]` entries and comments, and a `secrets.toml` with mode 0600
- WHEN the area is opened, every entry screen is visited, the snippet is copied, install, login and probe are run with fakes, and the menu is saved and closed
- THEN both files are byte-identical and `secrets.toml` keeps mode 0600

#### Scenario: Values never drawn [auto]
- GIVEN `secrets.toml` holding `CLARITY_API_TOKEN = "sk-123"`
- WHEN the `clarity` entry screen is rendered
- THEN no cell holds `sk-123` and the token shows as `set`

### Requirement: Entry list [S4]
The area MUST list the catalog entries grouped as `Analytics` (`clarity`, `ga4`, `gsc`) then `Workspace` (`google`, `apps-script`), in catalog order (C8). Each row shows the service name and its state, read from `config.toml`, `secrets.toml` and this session's probes: `not declared` (no server of that name), `disabled` (declared with `enabled = false`), `values missing: <NAMES>` (a name the catalog puts in `secrets.toml` has no non-empty value), `declared` (otherwise), and, once probed in this session, the last probe result. Enter opens the entry screen; Esc goes back to the `Connectors` row of the `Connectors` menu tab (see config-menu, Connectors row).

#### Scenario: Groups and order [auto, TestBackend + insta]
- GIVEN no declared server
- WHEN the area is rendered
- THEN it shows `Analytics` with Clarity, GA4, Search Console and `Workspace` with Google (Gmail, Docs, Sheets) and Apps Script, each `not declared`

#### Scenario: Missing values [auto]
- GIVEN a declared `ga4` and no `[ga4]` table in `secrets.toml`
- WHEN the list is built
- THEN the `ga4` row reads `values missing: GOOGLE_CLOUD_PROJECT`

### Requirement: Entry screen [S4]
The entry screen MUST show, in order: the guide steps and links of the catalog entry (with its warnings: the Clarity limits, the 7-day "Testing" note of C7, the drafts note of C9 and C12); the exact `[[ai.mcp.servers]]` entry to paste into `config.toml`; the `secrets.toml` table to fill, with each name and an empty value (no table when the entry has no secrets); the paths of `config.toml` and `secrets.toml`; and the actions available for that entry: install, log in (only when the entry has a login command), probe (only when the server is declared), open link, and copy (assumption: copying the snippet and the template to the clipboard makes pasting easy). The snippet and the template are built from the catalog and are valid TOML that parses back to the catalog's command, args and `env`. The screen is drawn inside the popup body, whose height is computed (`view_body()`, at least 13 rows today) and never changes, and scrolls when it is longer (assumption; see config-menu, Connectors row). When the area opens it MUST re-read `config.toml` from disk (`[[ai.mcp.servers]]` only) and use that copy for the entry states and the probe; when it differs from the list loaded at startup the area MUST say `restart lazarobox to use it in chats` (owner decision 2026-11-11; hot reload into chats is a later candidate slice).

#### Scenario: Snippet parses back [auto]
- GIVEN each catalog entry
- WHEN its snippet is parsed by `Config::parse`
- THEN one server with the entry's `name`, command, args and `env` exists and no notice is produced

#### Scenario: Entry pasted after startup [auto]
- GIVEN `config.toml` gains a `clarity` entry after lazarobox started
- WHEN the area is opened
- THEN `clarity` shows `declared`, the probe is offered, and the area says `restart lazarobox to use it in chats`

#### Scenario: Probe only when declared [auto]
- GIVEN `clarity` not declared
- WHEN its entry screen is rendered
- THEN no probe action is offered and the screen says to paste the entry into `config.toml` first

### Requirement: Install and login run in a visible pane [S3]
The install command and the login command of an entry MUST run only after the user answers `y` to a prompt that shows the full command, and MUST run in a new real lazarobox pane with a TTY, typed into its shell as visible text, so the user sees every line, any URL the command prints, and can answer its prompts. `n`, Esc or any other key MUST run nothing. Nothing MUST be installed or logged in hidden or inside a tool call: a login URL printed by a server inside an MCP call never reaches the user. The login MAY be run again at any time (the "Testing" refresh tokens of C7 expire after 7 days). The typed command MUST be sent with Enter: after the `y` it runs on its own, with no second confirmation (owner decision 2026-11-11). lazarobox MUST NOT set `BROWSER` in the pane; Linux and macOS come first, the tools open the browser through the platform's default opener, and if no browser opens the link in the login output stays visible in the pane. WSL handling is deferred to a later item (owner decision 2026-11-11).

#### Scenario: Nothing without y [auto][S4]
- GIVEN the `ga4` entry screen and a fake shell recorder
- WHEN the login action is chosen and the user presses `n`
- THEN no pane is created and nothing is typed

#### Scenario: Visible command [auto][S3 App side, S4 menu keys]
- GIVEN the same screen
- WHEN the login action is chosen and the user presses `y`
- THEN a new pane exists and the text typed into it is the entry's full login command followed by Enter, and no second prompt appears

#### Scenario: Login output keeps the link [manual]
- GIVEN Linux or macOS with a default browser
- WHEN the `ga4` login runs in the pane
- THEN the consent page opens through the platform's opener, and the link printed by the login stays visible in the pane if it does not
- NOTE: WSL is deferred (owner decision 2026-11-11)

### Requirement: Browser opener [S3]
Guide links MUST open through the existing opener (src/core/opener.rs, run by src/runtime/opener.rs, reached from the menu by `MenuCommand::Open`, src/app.rs:2536-2543): `open` on macOS; `wslview`, then `rundll32.exe url.dll,FileProtocolHandler`, then `xdg-open` on WSL; `xdg-open` elsewhere; and only a one-line `https://` link is handed to it. When no opener is found, or the opener fails (it exits with an error within 5 seconds of its start), the link MUST be copied and shown as text, and the footer says so, as for the Hugging Face row.

#### Scenario: Opener chosen [auto]
- GIVEN WSL with `wslview` on PATH
- WHEN a guide link is opened with a fake runner
- THEN `wslview <link>` is run

#### Scenario: No opener [auto]
- GIVEN a host with no opener on PATH
- WHEN a guide link is opened
- THEN no program is run, the link is copied, and the screen shows it as text

### Requirement: Probe from the area [S3][S4]
The probe action MUST ask `probe <name>? (y/n)` and run the mcp-catalog probe only on `y`. While it runs, the footer shows `probing <name>…` and Esc stops it (the server is stopped and reaped). Its report lines are shown on the entry screen and the row state is updated; a `secrets.toml` problem (see mcp-secrets) is shown there too.

#### Scenario: Probe after y [auto]
- GIVEN a declared `clarity` and a fake server
- WHEN the probe action is chosen and the user presses `y`
- THEN the screen shows `mcp: started clarity (3 tools)` and the row state shows the result

#### Scenario: Esc stops the probe [auto]
- GIVEN a probe waiting on a fake server that never answers
- WHEN Esc is pressed
- THEN the server process is gone within 3 seconds and the screen shows the probe as stopped
