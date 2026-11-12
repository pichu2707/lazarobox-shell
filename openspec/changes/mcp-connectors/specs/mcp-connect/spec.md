# mcp-connect Specification

## Purpose

The Connectors area of the config menu makes connecting a catalog server easy (C1, C3) without the menu writing any configuration: the user declares the server in `config.toml` `[[ai.mcp.servers]]` (C4, C5) and puts its values in `secrets.toml` (C6). The area shows the guide, the entry to paste and the values to fill, runs the install and the login as visible commands in a pane, opens guide links in the browser, and probes the server (see mcp-catalog). Slice tags: [S3] install pane, login pane and opener, [S4] menu screens, [S6a2] secret fields, [S6b1] Link with Google for `gsc`, [S6b2] Link with Google for `google`. Owner amendment 2026-11-11 (proposal C6, C7, C10 amended): the area MAY write `secrets.toml` from masked fields the user types (Secret fields), and offers Link with Google for `gsc` and `google`; it still never writes `config.toml`. Scenario tags: [auto] = `cargo test` (TestBackend, temp config folder, fake programs); [manual] = real machine. Items marked "assumption" are proposals, not user decisions; key bindings and texts are provisional and pinned by design.

Terms: "the area" = the Connectors area. "Entry screen" = the screen of one catalog entry.

## Requirements

### Requirement: The area writes no configuration [S4][S6a2]
No action in the area MUST add, change, reorder or remove any byte of `config.toml` (the rule "MCP server lists are never written" of config-persistence stays, C4). The area MUST NOT create, write or change the mode of `secrets.toml`, EXCEPT by a confirmed save from a secret field (see Secret fields and mcp-secrets, Menu save; C6 amended 2026-11-11), which changes only the one value it names, the OAuth client form (mcp-secrets, OAuth client file), and the delete-backup action. Every other action (open, visit, copy, install, login, probe, link, scroll, Esc, menu save and close) MUST leave `secrets.toml` byte-identical with its mode. The area MUST NOT draw any character of a confidential value (see Secret fields); it MAY read which names have a non-empty value, to show the state, and it shows non-confidential values (ids, paths, e-mail) in full.

Amendment note (2026-11-11): until this date the requirement read "No action in the area MUST ... create, write or change the mode of `secrets.toml` (C6)". The owner amended C6: "`secrets.toml` stays the user's file and editing it by hand remains fully supported. IN ADDITION ... the connector screen in the menu offers masked input fields ... and saves them."

#### Scenario: Byte-identical after every other action [auto]
- GIVEN a `config.toml` with hand-written `[[ai.mcp.servers]]` entries and comments, and a `secrets.toml` with mode 0600
- WHEN the area is opened, every entry screen is visited, the snippet is copied, install, login, probe and link are run with fakes, a secret field is opened, typed into and left with Esc, and the menu is saved and closed
- THEN both files are byte-identical and `secrets.toml` keeps mode 0600

#### Scenario: Only a confirmed field save writes [auto]
- GIVEN the same files and the `clarity` entry screen
- WHEN the `CLARITY_API_TOKEN` field is filled with `tok-new` and Enter is pressed
- THEN `config.toml` is byte-identical, and `secrets.toml` differs only in the value of `CLARITY_API_TOKEN` in `[clarity]` (see mcp-secrets, Menu save)

#### Scenario: Values never drawn [auto]
- GIVEN `secrets.toml` holding `CLARITY_API_TOKEN = "sk-123"`
- WHEN the `clarity` entry screen and its field list are rendered
- THEN no cell holds `sk-123` or any part of it, and the token shows as `set`

### Requirement: Entry list [S4]
The area MUST list the catalog entries grouped as `Analytics` (`clarity`, `ga4`, `gsc`) then `Workspace` (`google`, `apps-script`), in catalog order (C8). Each row shows the service name and its state, read from `config.toml`, `secrets.toml` and this session's probes: `not declared` (no server of that name), `disabled` (declared with `enabled = false`), `values missing: <NAMES>` (a name the catalog puts in `secrets.toml` has no non-empty value), `declared` (otherwise), and, once probed in this session, the last probe result. [S6c] For an entry with ways (`gsc`), `values missing` is judged on the way the table already fills completely (the area selects it), else on the selected way (OAuth by default); a table that completes the S5 service-account way is never `values missing`. [S6b1][S6b2] For `gsc` in its OAuth way and for `google`, a declared entry with its values shows `not linked` until the server's token file exists, then `linked` (existence and modification time only; the file is never opened, see Link with Google). Enter opens the entry screen; Esc goes back to the `Connectors` row of the `Connectors` menu tab (see config-menu, Connectors row).

#### Scenario: Groups and order [auto, TestBackend + insta]
- GIVEN no declared server
- WHEN the area is rendered
- THEN it shows `Analytics` with Clarity, GA4, Search Console and `Workspace` with Google (Gmail, Docs, Sheets) and Apps Script, each `not declared`

#### Scenario: Missing values [auto]
- GIVEN a declared `ga4` and no `[ga4]` table in `secrets.toml`
- WHEN the list is built
- THEN the `ga4` row reads `values missing: GOOGLE_CLOUD_PROJECT`

### Requirement: Entry screen [S4]
The entry screen MUST show, in order: the guide steps and links of the catalog entry (with its warnings: the Clarity limits, the 7-day "Testing" note of C7, the drafts note of C9 and C12); the exact `[[ai.mcp.servers]]` entry to paste into `config.toml`; the `secrets.toml` table to fill, with each name and an empty value (no table when the entry has no secrets); the paths of `config.toml` and `secrets.toml`; and the actions available for that entry: install, log in (only when the entry has a login command), probe (only when the server is declared), open link, [S6a2] fill values (only when the entry names a value for `secrets.toml`; see Secret fields), [S6b1] choose the way (only for an entry with more than one way: `gsc`), [S6b1][S6b2] link with Google (only for `gsc` in its OAuth way and for `google`; see Link with Google), and copy (assumption: copying the snippet and the template to the clipboard makes pasting easy). The snippet and the template are built from the catalog and are valid TOML that parses back to the catalog's command, args and `env`. The screen is drawn inside the popup body, whose height is computed (`view_body()`, at least 13 rows today) and never changes, and scrolls when it is longer (assumption; see config-menu, Connectors row). When the area opens it MUST re-read `config.toml` from disk (`[[ai.mcp.servers]]` only) and use that copy for the entry states and the probe; when it differs from the list loaded at startup the area MUST say `restart lazarobox to use it in chats` (owner decision 2026-11-11; hot reload into chats is a later candidate slice).

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

### Requirement: Secret fields [S6a2]
Revised on 2026-11-11 after review (proposal open question 15, resolved by the orchestrator on 2026-11-11 under the owner's instruction to proceed; pending owner confirmation: no character of a confidential value is ever shown).

Terms: a catalog `Var` has `secret: true` when its value lives in `secrets.toml` (the existing field), and, added here, every `secret: true` value is **confidential by default**; an entry opts a value out explicitly (`shown: true`) only when it is meant to be seen: `GOOGLE_CLOUD_PROJECT`, `GOOGLE_OAUTH_CLIENT_ID` (it appears in every sign-in link), `USER_GOOGLE_EMAIL`, `GSC_OAUTH_CLIENT_SECRETS_FILE`, `GSC_CONFIG_DIR`, `GSC_CREDENTIALS_PATH`, `GSC_SKIP_OAUTH` and `WORKSPACE_MCP_DISABLE_LOCAL_FILES`. Today the confidential values are `CLARITY_API_TOKEN` and `GOOGLE_OAUTH_CLIENT_SECRET`, and a value added later is confidential unless it is opted out by name.

On an entry screen whose entry names at least one value for `secrets.toml`, the fill-values action (key `f`, provisional) MUST open a field list inside the popup body: one row per such name of the entry (for `gsc`, those of the selected way). A confidential row shows only `not set` or `set`. A non-confidential row shows `not set` or its value in full (sanitized, cut to the popup width). A preset row shows its preset and is written by its Enter without typing. When the declared server's `command` differs from the catalog entry's command, the list's first row reads `declared command differs from the catalog: <command>` (sanitized), and every save asks `save <NAME> for a server that runs <command>? (y/n)` first (a plain `y` continues, anything else cancels with `not saved`).

Enter on a typed name opens a field with the hint `type or paste <NAME>; Enter saves · Esc cancels`. A confidential field draws a fixed-width mask, `••••••••` once it holds at least one character, whatever the length (the length is never shown); a non-confidential field shows its text. The field accepts typed and pasted text up to 4096 bytes on one line; a control character, a line break or an over-long value is refused with a notice and nothing is saved. Enter on an empty field saves nothing and shows `empty value not saved; edit secrets.toml by hand to remove a value`. Enter on a non-empty field: when the table already holds a different non-empty value for that name, the area MUST ask `replace the saved <NAME>? (y/n)` and only a plain `y` continues (any other key keeps the old value and says `not replaced`); then it sends the save (mcp-secrets, Menu save) and shows its result as a one-line notice. Esc closes the field and drops the typed text. A saved value reaches its server at the next start of that server; the area says `restart the chat or probe again to use it`.

The field list also offers `d` (delete backup), shown when `secrets.toml.bak` exists: `delete secrets.toml.bak? It holds your previous values (y/n)`, then mcp-secrets "delete backup". [S6a3] On the `ga4` and `gsc` entries it offers `o` (OAuth client): a form with the client id (non-confidential) and the client secret (confidential) that writes `google-oauth-client.json` (mcp-secrets, OAuth client file).

What the value may reach is stated as testable claims:
- It comes only from key and paste events of the menu while that field is open: while a field is open, `AppEvent::Paste` MUST go to the field and MUST NOT reach the chat input or any other paste handler (`App::on_paste`, src/app.rs `update`), and no model, tool call, MCP call, chat command, config key or other program can open the field, fill it or send its save.
- It is held in one value type (assumption: `SecretText`) that implements neither `Display` nor `Serialize`, whose `Debug` prints `<redacted>`, and whose buffer is overwritten on drop (best effort: the allocator or a moved copy may keep bytes; no `zeroize` crate is added). Every menu command, effect and event that carries it uses that type.
- No notice, footer, log line, error text, panic message, transcript, prompt history, chat input, clipboard action (`c` and `t` copy only the snippet and the template with empty values) or render snapshot holds any character of a confidential value. No `unwrap`/`expect` message or panic formats it.

#### Scenario: Fixed-width mask [auto, TestBackend + insta]
- GIVEN the `CLARITY_API_TOKEN` field open
- WHEN `sk-abcdef` is pasted, and in another run `sk-abcdefghijklmnopqrstuvwxyz`
- THEN both render exactly `••••••••` and no rendered cell holds any of the pasted characters

#### Scenario: Non-confidential values shown [auto]
- GIVEN `[ga4]` holding `GOOGLE_CLOUD_PROJECT = "my-proj"` and `[google]` holding a client id, a client secret and `USER_GOOGLE_EMAIL`
- WHEN the field lists are rendered
- THEN `my-proj`, the client id and the e-mail are shown in full, and the client secret shows only `set`

#### Scenario: Save a new value [auto]
- GIVEN no `[clarity]` table in `secrets.toml`
- WHEN `tok` is typed into the `CLARITY_API_TOKEN` field and Enter is pressed
- THEN no confirmation is asked, the save is sent for server `clarity` and name `CLARITY_API_TOKEN`, and the notice reads `saved CLARITY_API_TOKEN`

#### Scenario: Backup in the notice [auto]
- GIVEN an existing `secrets.toml`
- WHEN a value is saved
- THEN the notice reads `saved <NAME>; the previous values stay in secrets.toml.bak (0600) until you delete it` and the field list offers `d`

#### Scenario: Overwrite asks first [auto]
- GIVEN `[clarity]` holding `CLARITY_API_TOKEN = "old"`
- WHEN `new` is typed and Enter is pressed, then `n`
- THEN the prompt `replace the saved CLARITY_API_TOKEN? (y/n)` is shown, nothing is sent, and the notice reads `not replaced`
- AND in another run `y` sends the save

#### Scenario: Declared command differs [auto]
- GIVEN `clarity` declared with `command = "/tmp/other-server"`
- WHEN the field list is rendered and a value is entered
- THEN the first row names `/tmp/other-server`, and the save is sent only after `y` to `save CLARITY_API_TOKEN for a server that runs /tmp/other-server? (y/n)`

#### Scenario: Paste goes to the field only [auto]
- GIVEN a chat pane with an empty input and the menu open on a confidential field
- WHEN a bracketed paste of `sk-123` arrives
- THEN the field holds it (masked), the chat input is still empty, and no effect other than a redraw is emitted

#### Scenario: Esc drops the text [auto]
- GIVEN the `CLARITY_API_TOKEN` field holding typed text
- WHEN Esc is pressed
- THEN nothing is sent, `secrets.toml` is byte-identical, and reopening the field shows it empty

#### Scenario: Refused characters [auto]
- GIVEN the `CLARITY_API_TOKEN` field
- WHEN a paste holding a line break is received
- THEN nothing is saved and a notice says the value must be one line

#### Scenario: No other path to the save [auto]
- GIVEN a chat whose model calls every tool lazarobox offers, a user MCP server whose tool results hold the text `CLARITY_API_TOKEN`, and a `config.toml` with every known key
- WHEN the turns finish and the app starts and quits
- THEN no save of `secrets.toml` was sent and the file is byte-identical

#### Scenario: The value appears nowhere else [auto]
- GIVEN `tok-7f3a9c1e2b4d6f80` saved through the `CLARITY_API_TOKEN` field (a confidential value)
- WHEN the notices, the footer, the `Debug` output of every menu command, effect and event carrying it, the log file, the clipboard and the render snapshots are inspected
- THEN none holds `tok-7f3a9c1e2b4d6f80` or any 4-character part of it

#### Scenario: Debug and panics never format it [auto]
- GIVEN a `SecretText` holding `tok-7f3a`
- WHEN it and every type that holds it are formatted with `{:?}`, and a test panics with an `expect` on a `Result` that carries it
- THEN every output reads `<redacted>` where the value would be and none holds `tok-7f3a`

### Requirement: Link with Google [S6b1][S6b2]
Revised on 2026-11-11 after review.

The link action (key `g`, provisional) MUST be offered only on the `gsc` entry with its OAuth way selected and on the `google` entry (C7, C10 amended 2026-11-11). It MUST run only after a plain `y` to a prompt that shows exactly what will run, and only from the user's keyboard in the area: no model, tool call, MCP call or config can start it. The OAuth callback MUST be received by the server's own flow, never by lazarobox. lazarobox MUST NOT open, read, copy, write or delete any Google token file: it only calls `stat` on the server's token path (existence and modification time), and a negative test checks that no open happens on those paths. lazarobox stores no Google token itself (design D21). Only one link or probe runs at a time.

**`gsc` (OAuth way) [S6b1].** Preconditions, checked before the prompt, each with a visible notice and nothing started when it fails:
- `[gsc]` holds `GSC_OAUTH_CLIENT_SECRETS_FILE` and `GSC_CONFIG_DIR` as absolute paths, and `GSC_SKIP_OAUTH` is not `true`, `1` or `yes` (otherwise `fill the OAuth values first (f)`);
- the client file named by `GSC_OAUTH_CLIENT_SECRETS_FILE` exists (otherwise `add your Desktop OAuth client first (o)`);
- the values the line will use are the values the declared server will get: they come from the same `secrets.toml` read, and when the declared server's `env` lacks `GSC_CONFIG_DIR` or `GSC_OAUTH_CLIENT_SECRETS_FILE`, or the host environment would give the server another value, the area refuses with `the declared gsc entry does not pass <NAME>; paste the current entry`;
- `uv` is on `PATH`, and `uv tool list` (run by lazarobox as a direct argv with a 5-second timeout, output parsed, never shown raw) holds the substring `mcp-search-console v0.4.2` (a plain substring match on the output, done behind a test seam: the command runner is a parameter, so tests give a fake output); otherwise the notice names what was found and the fix: `mcp-search-console <version found, or: not installed>; run: uv tool install --force mcp-search-console==0.4.2` (the version is the text after `mcp-search-console v` on its line, sanitized, at most 20 characters), or `uv not found: install uv first`;
- `uv tool dir` (same way) prints one absolute folder; lazarobox uses that folder, never `$(uv tool dir)` in the line, so the path can never expand to empty.

The line is built by lazarobox from those resolved values, each single-quoted (a `'` inside becomes `'\''`; a value with a control character is refused):
`env -u XDG_CONFIG_HOME -u XDG_DATA_HOME -u GSC_ALLOW_DESTRUCTIVE -u GSC_SKIP_OAUTH GSC_CONFIG_DIR='<dir>' GSC_OAUTH_CLIENT_SECRETS_FILE='<file>' '<tool dir>/mcp-search-console/bin/python' -I -c 'import os; os.umask(0o077); import gsc_server as g; g.get_gsc_service_oauth(); print("Search Console linked")'`
Only non-confidential values (paths) are in it, so it may appear in the pane and the shell history. The `y` prompt shows that exact line, wrapped as the install and login prompts are (src/core/menu/connectors.rs, `run`). On `y` it is typed into a new pane with Enter under every rule of "Install and login run in a visible pane". mcp-gsc's own code opens the browser through the platform, listens on a free loopback port (so no port can be busy), receives the callback and writes `<GSC_CONFIG_DIR>/token.json`; the pane shows the URL, `Search Console linked` on success, or the server's error. The row turns `linked` when the area next reads the state and that token file exists. Cancel is Ctrl+C in the pane.

**`google` [S6b2].** Preconditions: declared and enabled (otherwise `paste the entry into config.toml first`); `[google]` holds non-empty `GOOGLE_OAUTH_CLIENT_ID`, `GOOGLE_OAUTH_CLIENT_SECRET` and `USER_GOOGLE_EMAIL` (otherwise `fill <the missing names> first (f)`). The prompt reads `start workspace-mcp and ask Google to link <USER_GOOGLE_EMAIL>? (y/n)`. On `y` lazarobox starts a link session: it spawns the declared server through the same function a chat start and the probe use (`external::connect` with `load_for_start`), so the server gets its values exactly as in a chat; completes the handshake within 20 seconds; records the baseline of the account's credentials file in `~/.google_workspace_mcp/credentials` (exists or not, modification time; the file name pattern is U, pinned in S6b2); then calls the tool `start_google_auth` by name with the account (argument names U, pinned in S6b2) with a 30-second timeout for that call. From the result it takes the first line that, trimmed, is a single link, and accepts it only when all hold: printable ASCII only (0x21-0x7E), at most 2048 bytes, scheme and host `https://accounts.google.com`, path under `/o/oauth2/`, a `client_id` query value equal to the user's `GOOGLE_OAUTH_CLIENT_ID`, and a `redirect_uri` query value that is `http://localhost:<port>` or `http://127.0.0.1:<port>` with a port from 8000 to 8004 (any path). It opens that link through the existing opener (copied and shown when no opener works) and shows `waiting for Google… · Esc cancels` with the link as text. The server stays alive so its callback server can receive the redirect. Success is the credentials file existing with a modification time different from the baseline. The session MUST end, with the server stopped and reaped within 3 seconds, on: success (`linked google`); Esc (`link cancelled`); 5 minutes without success (`link timed out after 5 min`); the 30-second call timeout (`link failed: start_google_auth did not answer in 30 s`); a result with no acceptable link (`link failed: no sign-in link in the answer`); an error result, such as every callback port busy (`link failed: <short reason>`, sanitized, at most 120 characters, never a value from `secrets.toml`); the server exiting (`link failed: the server stopped`). The call is not part of any chat: it enters no transcript or history, and the result text is never shown in full.

#### Scenario: gsc link prompt shows the exact line [auto]
- GIVEN `gsc` with the OAuth way, resolved values `/h/.config/mcp-gsc` and `/h/.config/lazarobox/google-oauth-client.json`, a fake `uv` whose `tool dir` prints `/h/.local/share/uv/tools` and whose `tool list` lists `mcp-search-console v0.4.2`
- WHEN `g` is pressed
- THEN the prompt shows exactly `env -u XDG_CONFIG_HOME -u XDG_DATA_HOME -u GSC_ALLOW_DESTRUCTIVE -u GSC_SKIP_OAUTH GSC_CONFIG_DIR='/h/.config/mcp-gsc' GSC_OAUTH_CLIENT_SECRETS_FILE='/h/.config/lazarobox/google-oauth-client.json' '/h/.local/share/uv/tools/mcp-search-console/bin/python' -I -c 'import os; os.umask(0o077); import gsc_server as g; g.get_gsc_service_oauth(); print("Search Console linked")'`

#### Scenario: gsc link types the line [auto]
- GIVEN the same state
- WHEN `g` and then `y` are pressed
- THEN exactly one new pane receives that line followed by Enter, and no MCP server is spawned by the menu

#### Scenario: gsc quoting [auto]
- GIVEN a client file path holding a `'` and a space
- WHEN the line is built
- THEN the path is single-quoted with the `'` escaped, and a path with a control character is refused with a notice

#### Scenario: gsc without uv or the pinned version [auto]
- GIVEN no `uv` on `PATH`, in another run a fake `uv tool list` output holding `mcp-search-console v0.4.3`, and in a third an output without `mcp-search-console`
- WHEN `g` is pressed
- THEN no prompt is shown and nothing runs, and the notices are `uv not found: install uv first`, `mcp-search-console 0.4.3; run: uv tool install --force mcp-search-console==0.4.2` and `mcp-search-console not installed; run: uv tool install --force mcp-search-console==0.4.2`

#### Scenario: gsc values the server would not get [auto]
- GIVEN a declared `gsc` whose `env` lacks `GSC_CONFIG_DIR` (pasted from S5)
- WHEN `g` is pressed
- THEN no prompt is shown and the notice asks to paste the current entry

#### Scenario: gsc without the client file [auto]
- GIVEN `gsc` with the OAuth way and no client file at the named path
- WHEN `g` is pressed
- THEN no prompt is shown, nothing runs, and the notice points at `o`

#### Scenario: gsc in the service-account way [auto]
- GIVEN `gsc` with the service-account way
- WHEN its entry screen is rendered and `g` is pressed
- THEN no link action is listed and the key shows a notice that linking needs the OAuth way (`w` chooses it)

#### Scenario: google link succeeds [auto]
- GIVEN a declared `google` with its values and a fake server whose `start_google_auth` answers a text holding `https://accounts.google.com/o/oauth2/auth?client_id=<the user's id>&redirect_uri=http://localhost:8000/oauth2callback` and which rewrites the account's credentials file after 1 second (the file existed before with an older time)
- WHEN `g` and `y` are pressed
- THEN the link is opened through a fake opener, the area shows `waiting for Google… · Esc cancels`, then `linked google`, the row reads `linked`, and the server process is gone within 3 seconds

#### Scenario: An old credentials file is not success [auto]
- GIVEN the account's credentials file already exists and the fake server never touches it
- WHEN the link runs for 10 seconds
- THEN the area still shows `waiting for Google…`

#### Scenario: google link cancelled [auto]
- GIVEN the same fake server that never writes the file
- WHEN `g`, `y` and then Esc are pressed
- THEN the area shows `link cancelled` and the server process is gone within 3 seconds

#### Scenario: google link times out [auto, paused clock]
- GIVEN the same fake server that never writes the file
- WHEN `g` and `y` are pressed and 5 minutes pass
- THEN the area shows `link timed out after 5 min` and the server is stopped

#### Scenario: start_google_auth does not answer [auto, paused clock]
- GIVEN a fake server that never answers the call
- WHEN `g` and `y` are pressed and 30 seconds pass
- THEN the area shows `link failed: start_google_auth did not answer in 30 s` and the server is stopped

#### Scenario: Callback ports busy [auto]
- GIVEN a fake server whose `start_google_auth` returns an error result `ports 8000-8004 are in use`
- WHEN `g` and `y` are pressed
- THEN the area shows `link failed: ports 8000-8004 are in use`, no link is opened, and the server is stopped

#### Scenario: No client id yet [auto]
- GIVEN a declared `google` and `[google]` without `GOOGLE_OAUTH_CLIENT_ID`
- WHEN `g` is pressed
- THEN no prompt is shown, no server is spawned, and the notice names `GOOGLE_OAUTH_CLIENT_ID` and points at `f`

#### Scenario: Links that are refused [auto]
- GIVEN fake answers `https://evil.example/o/oauth2/auth?client_id=<id>&redirect_uri=http://localhost:8000/`, `https://accounts.google.com/signin?client_id=<id>&redirect_uri=http://localhost:8000/`, a link with another `client_id`, a link whose `redirect_uri` is `http://localhost:9000/`, and a link holding a non-ASCII character
- WHEN each is received
- THEN the opener is not run and the area shows `link failed: no sign-in link in the answer`

#### Scenario: Token files never opened [auto]
- GIVEN token files of mode 000 at the `gsc` and `google` token paths, and the test seam recording every open
- WHEN the area reads the states and a `google` link succeeds
- THEN the states are computed, no open of any token path is recorded, and the files are unchanged

#### Scenario: Nothing without y [auto]
- GIVEN `google` ready to link
- WHEN `g` is pressed and then `n`, Esc, Ctrl+y or any other key
- THEN no server is spawned and no pane is created

#### Scenario: Live Google link [manual]
- GIVEN a real Desktop OAuth client in the user's project and a declared `google`
- WHEN Link with Google runs and the user consents in the browser
- THEN the area shows `linked google`, `~/.google_workspace_mcp/credentials` holds a token written by workspace-mcp, and a later `search_gmail_messages` call after `y` works without a new consent

#### Scenario: Live Search Console link [manual]
- GIVEN the client file and `gsc` in the OAuth way
- WHEN Link with Google runs in the pane and the user consents
- THEN the pane shows `Search Console linked`, the token exists in `GSC_CONFIG_DIR`, and a later search analytics call after `y` works without opening a browser
