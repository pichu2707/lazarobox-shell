# mcp-catalog Specification

## Purpose

A curated catalog of existing MCP servers (C2: no home-grown connectors, lazarobox does not reimplement any Google or Microsoft API) that tells the user exactly what to declare in `config.toml` (C4, C5), which values go to `secrets.toml` (C6), how to install and log in, and the read/write class of each tool (C11). The catalog never declares a server by itself: a server is active only when the user's `[[ai.mcp.servers]]` entry declares it (see mcp-servers). Entries follow the order of C8: `clarity`, `ga4`, then `gsc`, then the Workspace entries `google` and `apps-script`. Slice tags: [S2] catalog core and the Clarity and GA4 entries, [S3] probe, [S5] GSC, [S6c] Workspace entries, filters and catalog fixes, [S6b] the GSC OAuth way's login (the former tag [S6] is split into S6a-S6d on 2026-11-11). Upstream facts marked V were verified on 2026-11-11 (proposal, "Upstream facts re-verified on 2026-11-11"). Scenario tags: [auto] = `cargo test` (pure data, fake server child); [manual] = real service account. Items marked "assumption" are proposals, not user decisions; binary names marked U are unverified and pinned when the slice is built.

Terms: "entry" = one catalog entry. "Declared" = a `[[ai.mcp.servers]]` entry whose `name` equals the entry id. "Class" = `read` or `write`.

## Requirements

### Requirement: Entries are data [S2]
The catalog MUST be a pure data module in core with no IO. Each entry MUST hold: its id (a valid server `name`, see configuration); its group (`Analytics` or `Workspace`); the server's project and license; the pinned install command; the run command and args, narrowing flags included; the `env` names and, for each, whether its value belongs in `secrets.toml`; the login command, if any; the guide steps and links; the token location; and the class of each known tool. [S6c] (2026-11-11) An entry MAY also hold: alternative ways (each a title, the names it fills, their presets and its own login command; the snippet's `env` is the union of every way's names, so switching ways never touches `config.toml`); the tools it hides from chats (never registered, see Gmail tool filter); whether its Gmail tools are limited to its allowlist; and its link kind (none, a login line in the pane, or `start_google_auth` through the link session; see mcp-connect, Link with Google). Every guide MUST end with a line pointing at the tutorial `docs/conectores.md` [S6d]. Adding an entry MUST NOT require changing the probe, the menu drawing or the registry code.

#### Scenario: Catalog integrity [auto]
- GIVEN the catalog
- WHEN it is validated
- THEN the ids are `clarity`, `ga4`, `gsc`, `google`, `apps-script` in that order, each a valid unique server name, each with a non-empty install and run command, `clarity`, `ga4` and `gsc` in `Analytics` and `google` and `apps-script` in `Workspace`, and every guide link passes the opener's `https://` check

### Requirement: Declared entries are matched by name [S2]
A user server MUST be treated as a catalog entry when its `name` equals the entry id (assumption: this keeps the `[[ai.mcp.servers]]` format unchanged, C5). A server with any other name is an ordinary user server, untouched by the catalog. The catalog never adds, removes or rewrites a declared server's command, args or `env`.

#### Scenario: Matched [auto]
- GIVEN a declared server `clarity`
- WHEN its catalog entry is looked up
- THEN the `clarity` entry is found

#### Scenario: Not matched [auto]
- GIVEN a declared server `fs`
- WHEN its tools are registered
- THEN no catalog rule applies and the server's command is unchanged

### Requirement: Read/write class is recorded only [S2]
Each known tool of an entry MUST have a class, `read` or `write` (C11). A tool the server lists that the catalog does not know MUST get class `write` (assumption: the safe default). The class MUST NOT change approval: every call to a tool of a declared entry MUST need the user's `y`, in `ask`, `auto` and `run`, whatever its class and whatever the server's `readOnlyHint` says (C11; see mcp-servers, Every call needs approval, and agent-tools, Approval model). A later change decides how classes meet the `run` mode of chat-auto-permissions.

#### Scenario: Read tool still asks [auto]
- GIVEN declared `ga4` and its `run_report` of class `read`, annotated `readOnlyHint` by the server, in a chat with approval `auto`, and in another run `run`
- WHEN the model calls it
- THEN a confirm is shown and the server is not contacted before `y`

#### Scenario: Unknown tool is write [auto]
- GIVEN declared `clarity` listing a tool `new_tool` the catalog does not know
- WHEN the class is looked up
- THEN it is `write`

### Requirement: Pre-installed binary [S2]
The run command of every entry MUST be an installed binary (installed by `uv tool install`, `pipx install` or `npm i -g`), never a first-run download such as `uvx`, `npx` or `pipx run`, which could exceed the 20-second handshake.

#### Scenario: No first-run download [auto]
- GIVEN the catalog
- WHEN each entry's run command is checked
- THEN none is `uvx`, `npx`, `pipx` or `pnpm dlx`

### Requirement: Probe [S3]
The probe MUST start one declared server outside any chat turn, with exactly the spawn rules of a chat start (direct exec, scrubbed environment, `secrets.toml` values; see mcp-servers), wait for `initialize` and `tools/list` within the 20-second handshake, report, and stop the server (process group killed and reaped within 3 seconds, no orphan). It is started only from the Connectors area after `y` (see mcp-connect), never by a chat turn, and writes no file. Its report MUST be, as sanitized lines (texts provisional, pinned by design):
- `mcp: started <name> (<N> tools)` on success, or `mcp: <name> unavailable: <short reason>` on failure (a handshake over 20 seconds reads `timed out after 20 s`);
- `<name>: lists <N> tools; only the first 64 are kept` when the server lists more than 64 tools;
- `<name>: <a> and <b> collide after the 24-character cut; <b> is dropped` for each pair of tools whose qualified names are equal after lowercasing, the character mapping and the 24-character cut;
- `<name>: <N> tools not in the catalog (class write)` when the server lists tools the entry does not know.

The 64-tool and collision checks MUST be pure functions over a tool-name list.

#### Scenario: Success [auto]
- GIVEN declared `clarity` and a fake server listing three tools
- WHEN the probe runs
- THEN the report is `mcp: started clarity (3 tools)` and no server pid exists 3 seconds after it

#### Scenario: Too many tools [auto]
- GIVEN a fake server listing 70 tools
- WHEN the probe runs
- THEN the report holds `lists 70 tools; only the first 64 are kept`

#### Scenario: Collision after the cut [auto]
- GIVEN tool names `get_gmail_thread_content_a` and `get_gmail_thread_content_b`
- WHEN the collision check runs
- THEN it reports the pair and names the second as dropped

#### Scenario: Handshake timeout [auto]
- GIVEN a fake server that never answers `initialize`
- WHEN the probe runs
- THEN the report is `mcp: <name> unavailable: timed out after 20 s` and the server is stopped

#### Scenario: Probe uses the secrets file [auto]
- GIVEN declared `clarity` naming `CLARITY_API_TOKEN` and the value only in `secrets.toml`
- WHEN the probe runs with a recorder
- THEN the child gets the value, and the report holds no value

### Requirement: Clarity entry [S2]
The `clarity` entry MUST install with `npm i -g @microsoft/clarity-mcp-server`, run `clarity-mcp-server`, name `CLARITY_API_TOKEN` (value in `secrets.toml`), have no login command, and give all of the server's three tools class `read`. Its guide MUST explain creating a project-admin API token in the Clarity project settings and MUST warn that the API allows 10 requests per project per day, covers the last 1 to 3 days only, and returns at most 1,000 rows.

#### Scenario: Snippet and secrets [auto]
- GIVEN the `clarity` entry
- WHEN its snippet and secrets template are built (see mcp-connect)
- THEN the snippet declares `name = "clarity"`, `command = "clarity-mcp-server"`, `env = ["CLARITY_API_TOKEN"]`, and the template is `[clarity]` with `CLARITY_API_TOKEN = ""`

#### Scenario: Limits in the guide [auto]
- GIVEN the `clarity` guide
- WHEN it is rendered
- THEN it states the 10 requests per project per day

### Requirement: GA4 entry [S2]
The `ga4` entry MUST install with `pipx install analytics-mcp`, run `analytics-mcp` (binary name V 2026-11-11: analytics-mcp 1.1.0 installs `analytics-mcp` and `google-analytics-mcp`), run without `--allow-write`, name `GOOGLE_CLOUD_PROJECT` (value in `secrets.toml`), and log in with `gcloud auth application-default login --scopes analytics.readonly,cloud-platform --client-id-file=<client file>` (the form of the analytics-mcp README; the exact scope spelling gcloud accepts is pinned when the slice is built) in the login pane. The server reads the resulting ADC from `~/.config/gcloud` through the real `HOME`. Its guide MUST walk step by step through creating a Desktop OAuth client in the user's own Google Cloud project (C7), enabling the Google Analytics APIs, and saving the client file, and MUST warn that a client left in "Testing" gets refresh tokens that expire after 7 days (C7). Assumption: the guide suggests saving the client file as `~/.config/lazarobox/google-oauth-client.json`, which the read tools already refuse, and the login command uses that path. Tools: ~~`run_report`, `run_funnel_report`, `run_realtime_report` and the account and property info tools are `read`; `create_property_annotation` is `write`.~~ [S6c] (2026-11-11) The entry pins exactly the 10 tools the server lists without `--allow-write`, all `read`: `get_account_summaries`, `list_google_ads_links`, `get_property_details`, `list_property_annotations`, `list_key_events`, `get_custom_dimensions_and_metrics`, `run_report`, `run_realtime_report`, `run_funnel_report`, `run_conversions_report`. The write tools that exist only with `--allow-write` (`create_property_annotation`, `create_custom_dimension`, `create_key_event`) are NOT pinned: the entry never sets the flag, and an unpinned tool is `write` anyway.

#### Scenario: Read-only run [auto]
- GIVEN the `ga4` entry
- WHEN its snippet is built
- THEN its args hold no `--allow-write` and `env` is `["GOOGLE_CLOUD_PROJECT"]`

#### Scenario: Testing warning [auto]
- GIVEN the `ga4` guide
- WHEN it is rendered
- THEN it says that a client in "Testing" gets refresh tokens that expire after 7 days

#### Scenario: Only the read tools are pinned [auto]
- GIVEN the `ga4` entry
- WHEN its tools are listed
- THEN they are the 10 read tools above, each `read`, and `create_property_annotation` is not among them

#### Scenario: Live report [manual]
- GIVEN a connected `ga4` and a property the user can read
- WHEN the model calls `run_report` and the user presses `y`
- THEN a capped, untrusted-wrapped result is returned

### Requirement: GSC entry with two ways [S5][S6c][S6b]
Renamed from "GSC entry with a service account" on 2026-11-11 (C10 amended: "Search Console gets an OAuth path with the user's OWN Google client ... keeping the service-account path as the alternative").

The `gsc` entry MUST install with `uv tool install mcp-search-console==0.4.2` (pinned: the OAuth login calls an internal function of that version; console scripts `mcp-search-console` and `mcp-gsc`, V), run `mcp-search-console`, have `env` = `["GSC_OAUTH_CLIENT_SECRETS_FILE", "GSC_CREDENTIALS_PATH", "GSC_SKIP_OAUTH"]` (all values in `secrets.toml`), and MUST NOT name `GSC_ALLOW_DESTRUCTIVE` or `GSC_CONFIG_DIR` (C10). Without `GSC_ALLOW_DESTRUCTIVE` the server requests `webmasters.readonly` (V). It has two ways:
- **OAuth** (the default way shown, S6b): `GSC_OAUTH_CLIENT_SECRETS_FILE` preset to `~/.config/lazarobox/google-oauth-client.json` (the Desktop OAuth client file of the GA4 guide, C7; the server expands `~`, V) and `GSC_SKIP_OAUTH` preset to `"false"`. Its login line, typed by Link with Google (mcp-connect), runs the server's own OAuth function in the pane, outside MCP:
  `env -u GSC_ALLOW_DESTRUCTIVE -u GSC_SKIP_OAUTH -u GSC_CONFIG_DIR GSC_OAUTH_CLIENT_SECRETS_FILE=$HOME/.config/lazarobox/google-oauth-client.json "$(uv tool dir)/mcp-search-console/bin/python" -I -c 'import gsc_server as g; g.get_gsc_service_oauth(); print("Search Console linked")'`
  (V 2026-11-11: the module imports without starting the server and the function raises `FileNotFoundError` without a client file; the browser flow itself was not run. U: the full consent run, task 6b.6.) The token is `~/.config/mcp-gsc/token.json` on Linux and `~/Library/Application Support/mcp-gsc/token.json` on macOS (V), the state `linked` checks its existence only.
- **Service account** (unchanged from S5): `GSC_CREDENTIALS_PATH` (typed path) and `GSC_SKIP_OAUTH` preset to `"true"`; no login.

The guide MUST explain both ways: for OAuth, reusing (or creating) the Desktop OAuth client of the GA4 guide, enabling the Search Console API, linking, and that the token file is written by mcp-gsc with the default umask (`chmod 600` it; lazarobox's read tools refuse its folder); for the service account, the S5 steps (create the account and its JSON key, save it as `~/.config/lazarobox/gsc-service-account.json`, add its e-mail to the property). It MUST warn that in the OAuth way, a call made before linking starts the browser flow inside the MCP call, where it cannot work: link first. Tools: the 21 tools pinned by S5 with their classes; `reauthenticate` is hidden from chats (it deletes the token and starts a browser flow inside the call; see Gmail tool filter, hidden tools).

#### Scenario: Both ways in one snippet [auto]
- GIVEN the `gsc` entry
- WHEN its snippet is built
- THEN `env` is `["GSC_OAUTH_CLIENT_SECRETS_FILE", "GSC_CREDENTIALS_PATH", "GSC_SKIP_OAUTH"]` and `GSC_ALLOW_DESTRUCTIVE` and `GSC_CONFIG_DIR` appear nowhere in the entry

#### Scenario: Templates per way [auto]
- GIVEN the `gsc` entry
- WHEN the template of each way is built
- THEN the OAuth way gives `GSC_OAUTH_CLIENT_SECRETS_FILE = "~/.config/lazarobox/google-oauth-client.json"` and `GSC_SKIP_OAUTH = "false"`, and the service-account way gives an empty `GSC_CREDENTIALS_PATH` and `GSC_SKIP_OAUTH = "true"`

#### Scenario: The OAuth login stays outside MCP [auto]
- GIVEN the OAuth way's login line
- WHEN it is checked
- THEN it holds no control character, no first-run download (`uvx`, `npx`, `pipx run`, `dlx`), unsets `GSC_ALLOW_DESTRUCTIVE`, `GSC_SKIP_OAUTH` and `GSC_CONFIG_DIR`, and holds no value from `secrets.toml`

#### Scenario: Old service-account entries keep working [auto]
- GIVEN a user entry pasted from S5 with `env = ["GSC_CREDENTIALS_PATH", "GSC_SKIP_OAUTH"]` and the S5 table
- WHEN the area builds its state
- THEN it shows the service-account way as complete and `declared`, with no `values missing`

#### Scenario: Live query [manual]
- GIVEN a connected `gsc` (either way) whose account can read a property
- WHEN the model calls a search analytics query and the user presses `y`
- THEN a capped, untrusted-wrapped result is returned

### Requirement: Google Workspace entry, drafts only [S6c]
Rewritten on 2026-11-11 with the upstream facts of workspace-mcp 2.4.1 (V); the earlier "login command is the workspace-mcp pre-authorization run in the login pane" is dropped: no such command exists (V).

The `google` entry MUST install with `uv tool install workspace-mcp==2.4.1` and run `workspace-mcp` with args `--single-user --permissions gmail:readonly+drafts docs:full sheets:full --disabled-tools send_gmail_message` (the `--disabled-tools` layer is kept only if the probe shows the server accepts it with these permissions; U, task 6c.5). It MUST name `GOOGLE_OAUTH_CLIENT_ID`, `GOOGLE_OAUTH_CLIENT_SECRET` (the user's own Desktop OAuth client, C7), `USER_GOOGLE_EMAIL` (the account) and `WORKSPACE_MCP_DISABLE_LOCAL_FILES` (preset `"true"`, see proposal open question 12), all with values in `secrets.toml`, and MUST NOT name `WORKSPACE_MCP_CREDENTIALS_DIR` (so the tokens stay in `~/.google_workspace_mcp`, which the read tools refuse). It has no login command; it is linked by Link with Google (mcp-connect), which calls `start_google_auth`. Its guide MUST walk through creating the client and enabling the Gmail, Docs, Sheets and Drive APIs; carry the 7-day "Testing" warning (C7); say that replies are created as Gmail drafts and the user sends them from Gmail (C9, C12); say that the server holds a `gmail.compose` token that could send at the API level (the accepted residual risk of C9); say that workspace-mcp keeps its tokens as plaintext JSON in `~/.google_workspace_mcp/credentials`; say that the Docs and Sheets permissions include `drive.readonly`, so the server can read the whole Drive; and say that `get_gmail_attachment_content` saves files to local disk and that `draft_gmail_message` could attach local files unless `WORKSPACE_MCP_DISABLE_LOCAL_FILES` is `true`.

The entry MUST pin the 42 tools the server lists with these flags (V): `start_google_auth` (`write`, hidden from chats); the 8 Gmail tools of the allowlist (Gmail tool filter) with `search_gmail_messages`, `get_gmail_message_content`, `get_gmail_messages_content_batch`, `get_gmail_thread_content`, `get_gmail_threads_content_batch`, `list_gmail_labels` as `read` and `get_gmail_attachment_content` (writes local disk) and `draft_gmail_message` as `write`; the Docs tools `search_docs`, `get_doc_content`, `list_docs_in_folder`, `inspect_doc_structure`, `debug_table_structure`, `get_doc_as_markdown`, `list_document_comments` as `read` and `create_doc`, `modify_doc_text`, `find_and_replace_doc`, `insert_doc_elements`, `insert_doc_image`, `update_doc_headers_footers`, `batch_update_doc`, `create_table_with_data`, `export_doc_to_pdf`, `update_paragraph_style`, `manage_doc_tab` as `write`; the Sheets tools `list_spreadsheets`, `get_spreadsheet_info`, `read_sheet_values`, `list_sheet_tables`, `list_spreadsheet_comments` as `read` and `modify_sheet_values`, `format_sheet_range`, `manage_conditional_formatting`, `create_spreadsheet`, `create_sheet`, `append_table_rows`, `manage_sheet_tab`, `resize_sheet_dimensions`, `move_sheet_rows`, `manage_named_range` as `write`. The probe confirms the count stays under 64 (41 registered after `start_google_auth` is hidden).

#### Scenario: Permissions in the snippet [auto]
- GIVEN the `google` entry
- WHEN its snippet is built
- THEN its args hold `--single-user`, `--permissions gmail:readonly+drafts docs:full sheets:full` and `--disabled-tools send_gmail_message`, no `--tools` or `--read-only`, and `env` is `["GOOGLE_OAUTH_CLIENT_ID", "GOOGLE_OAUTH_CLIENT_SECRET", "USER_GOOGLE_EMAIL", "WORKSPACE_MCP_DISABLE_LOCAL_FILES"]`

#### Scenario: Local files off by default [auto]
- GIVEN the `google` entry
- WHEN its secrets template is built
- THEN it holds `WORKSPACE_MCP_DISABLE_LOCAL_FILES = "true"` and empty values for the other three names

#### Scenario: Classes [auto]
- GIVEN the `google` entry
- WHEN the classes of `draft_gmail_message`, `get_gmail_attachment_content`, `start_google_auth` and `search_gmail_messages` are looked up
- THEN the first three are `write` and the last is `read`

#### Scenario: No login, a link [auto]
- GIVEN the `google` entry screen
- WHEN it is rendered
- THEN no login action is listed and the link action is

#### Scenario: Draft after y [manual]
- GIVEN a linked `google`
- WHEN the model calls `draft_gmail_message` and the user presses `y`
- THEN a draft exists in the user's Gmail and nothing is sent

### Requirement: Gmail tool filter [S6c]
Rewritten on 2026-11-11: the allowlist is now the 8 verified tool names, and an entry can hide tools from chats.

For a declared `google` server, lazarobox MUST register a Gmail tool (a tool whose name contains `gmail`, ASCII case-insensitive) only when its name is exactly one of the allowlist: `search_gmail_messages`, `get_gmail_message_content`, `get_gmail_messages_content_batch`, `get_gmail_attachment_content`, `draft_gmail_message`, `get_gmail_thread_content`, `get_gmail_threads_content_batch`, `list_gmail_labels` (C9: "lazarobox lists only read and draft tools"). Every other Gmail tool the server lists, `send_gmail_message` included, MUST NOT be registered, whatever the server's flags. Independently, a tool an entry hides (`start_google_auth` for `google`, `reauthenticate` for `gsc`) MUST NOT be registered in any chat; the link session of mcp-connect still calls `start_google_auth` by name. Both filters MUST apply before the 64-tool cap (in `list_tools_with`, design D5), so a dropped tool never takes a kept tool's place, and the probe counts only admitted tools. Non-Gmail tools that are not hidden follow the ordinary rules; a server with another name is untouched. Sending with an explicit user permission is a later, undecided change (C12).

#### Scenario: Send is not registered [auto]
- GIVEN a declared `google` and a fake server listing `search_gmail_messages`, `draft_gmail_message` and `send_gmail_message`
- WHEN the registry is built
- THEN `google__search_gmail_messages` and `google__draft_gmail_message` exist and no `send_gmail_message` tool exists

#### Scenario: Unknown Gmail tool dropped [auto]
- GIVEN the same server also listing `delete_gmail_label_x` and `Modify_GMAIL_Labels`
- WHEN the registry is built
- THEN neither is registered

#### Scenario: Hidden tools not registered [auto]
- GIVEN a declared `google` listing `start_google_auth`, and a declared `gsc` listing `reauthenticate`
- WHEN the registries are built
- THEN neither tool is registered

#### Scenario: Filter before the cap [auto]
- GIVEN a fake `google` listing 70 tools: `send_gmail_message`, `start_google_auth` and 6 unknown Gmail tools first, then 62 others
- WHEN the tools are listed
- THEN 62 tools are kept, none of the dropped ones, and the probe reports no "only the first 64" line

#### Scenario: Other servers untouched [auto]
- GIVEN a declared server `mail` listing `send_gmail_message` and `start_google_auth`
- WHEN the registry is built
- THEN `mail__send_gmail_message` and `mail__start_google_auth` are registered and still need `y` on every call

### Requirement: Apps Script entry [S6c]
Updated on 2026-11-11 with clasp 3.4.1 (V).

The `apps-script` entry MUST install with `npm i -g @google/clasp@3.4.1` (3.4.0 or later is required: it fixed the project-dir path validation, CWE-22), run `clasp mcp`, name no `env`, and log in with `clasp login` in the login pane (clasp's own OAuth client; the token is `~/.clasprc.json`, refused by the read tools). Its guide MUST say to switch on the Apps Script API for the account (https://script.google.com/home/usersettings), that `push_files` uploads code from a directory on disk and that pushed code runs with the script's scopes, and that `pull_files` and `clone_project` write (and may overwrite) local files. Tools: `list_projects` is `read`; `push_files`, `create_project`, `pull_files` and `clone_project` are `write` (the last two changed from `read` on 2026-11-11: they write local disk). No `run_script_function` or deployment tool is cataloged (clasp's MCP has none, V).

#### Scenario: No secrets [auto]
- GIVEN the `apps-script` entry
- WHEN its snippet and secrets template are built
- THEN the snippet has no `env`, there is no secrets template, and no fill-values action is offered

#### Scenario: Pinned version [auto]
- GIVEN the `apps-script` entry
- WHEN its install command is checked
- THEN it names `@google/clasp@3.4.1`

#### Scenario: Local writes are write [auto]
- GIVEN the `apps-script` entry
- WHEN the classes are looked up
- THEN `list_projects` is `read` and `push_files`, `create_project`, `pull_files`, `clone_project` are `write`

#### Scenario: Push after y [manual]
- GIVEN a connected `apps-script` and an Apps Script project on disk under `$HOME`
- WHEN the model calls `push_files` and the user presses `y`
- THEN the project is uploaded
