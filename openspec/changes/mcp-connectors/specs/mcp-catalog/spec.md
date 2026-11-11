# mcp-catalog Specification

## Purpose

A curated catalog of existing MCP servers (C2: no home-grown connectors, lazarobox does not reimplement any Google or Microsoft API) that tells the user exactly what to declare in `config.toml` (C4, C5), which values go to `secrets.toml` (C6), how to install and log in, and the read/write class of each tool (C11). The catalog never declares a server by itself: a server is active only when the user's `[[ai.mcp.servers]]` entry declares it (see mcp-servers). Entries follow the order of C8: `clarity`, `ga4`, then `gsc`, then the Workspace entries `google` and `apps-script`. Slice tags: [S2] catalog core and the Clarity and GA4 entries, [S3] probe, [S5] GSC, [S6c] Workspace entries, filters and catalog fixes, [S6b1] the GSC OAuth way's login and chat-start check (the former tag [S6] is split into S6a-S6d on 2026-11-11). Upstream facts marked V were verified on 2026-11-11 (proposal, "Upstream facts re-verified on 2026-11-11"). Scenario tags: [auto] = `cargo test` (pure data, fake server child); [manual] = real service account. Items marked "assumption" are proposals, not user decisions; binary names are all verified as of 2026-11-11 (no U remains on them).

Terms: "entry" = one catalog entry. "Declared" = a `[[ai.mcp.servers]]` entry whose `name` equals the entry id. "Class" = `read` or `write`.

## Requirements

### Requirement: Entries are data [S2]
The catalog MUST be a pure data module in core with no IO. Each entry MUST hold: its id (a valid server `name`, see configuration); its group (`Analytics` or `Workspace`); the server's project and license; the pinned install command; the run command and args, narrowing flags included; the `env` names and, for each, whether its value belongs in `secrets.toml`; the login command, if any; the guide steps and links; the token location; and the class of each known tool. [S6c] (2026-11-11) An entry MAY also hold: alternative ways (each a title, the names it fills, their presets and its own login command; the snippet's `env` is the union of every way's names, so switching ways never touches `config.toml`); the tools it hides from chats (never registered, see Gmail tool filter); whether its Gmail tools are limited to its allowlist; and its link kind (none, a login line in the pane, or `start_google_auth` through the link session; see mcp-connect, Link with Google). A `secret: true` `Var` is confidential by default and MAY opt out with `shown: true` (mcp-connect, Secret fields) and whether its preset is a path under `$HOME`, which the menu save writes expanded to an absolute path (a server may not expand `~`). Every guide MUST end with a line pointing at the tutorial `docs/conectores.md` [S6d]. Adding an entry MUST NOT require changing the probe, the menu drawing or the registry code.

#### Scenario: Shown values are listed by name [auto]
- GIVEN the catalog
- WHEN every `secret: true` var with `shown: true` is collected
- THEN the set is exactly `GOOGLE_CLOUD_PROJECT`, `GOOGLE_OAUTH_CLIENT_ID`, `USER_GOOGLE_EMAIL`, `GSC_OAUTH_CLIENT_SECRETS_FILE`, `GSC_CONFIG_DIR`, `GSC_CREDENTIALS_PATH`, `GSC_SKIP_OAUTH` and `WORKSPACE_MCP_DISABLE_LOCAL_FILES`, and every other `secret: true` var (today `CLARITY_API_TOKEN` and `GOOGLE_OAUTH_CLIENT_SECRET`) is confidential

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

### Requirement: GSC entry with two ways [S5][S6c][S6b1]
Renamed from "GSC entry with a service account" on 2026-11-11 (C10 amended: "Search Console gets an OAuth path with the user's OWN Google client ... keeping the service-account path as the alternative"); revised after review the same day (fixed token folder).

The `gsc` entry MUST install with `uv tool install mcp-search-console==0.4.2` (pinned: the OAuth login calls an internal function of that version, proposal open question 14, resolved by the orchestrator on 2026-11-11 under the owner's instruction to proceed; pending owner confirmation; console scripts `mcp-search-console` and `mcp-gsc`, V), run `mcp-search-console`, have `env` = `["GSC_OAUTH_CLIENT_SECRETS_FILE", "GSC_CONFIG_DIR", "GSC_CREDENTIALS_PATH", "GSC_SKIP_OAUTH"]` (all values in `secrets.toml`), and MUST NOT name `GSC_ALLOW_DESTRUCTIVE` (C10). Without `GSC_ALLOW_DESTRUCTIVE` the server requests `webmasters.readonly` (V).

`GSC_CONFIG_DIR` is fixed to `$HOME/.config/mcp-gsc`, written by the menu save as an absolute path (mcp-gsc does not expand `~` in it, V): the login pane runs with the user's full environment and the chat server with a scrubbed one, so without it platformdirs would put the token in different folders when `XDG_CONFIG_HOME` is set (the login would write where the chat server never looks, and the chat server would then start a browser flow inside an MCP call). The same folder on Linux and macOS; it is already refused by the read tools (`.config/mcp-gsc`, S1), and the macOS default `Library/Application Support/mcp-gsc` is added as well for tokens written before (agent-tools).

It has two ways:
- **OAuth** (the default way shown, S6b1): `GSC_OAUTH_CLIENT_SECRETS_FILE` preset to the config folder's `google-oauth-client.json` (written absolute; the Desktop client of the GA4 guide, or written by the OAuth client form, mcp-secrets), `GSC_CONFIG_DIR` preset as above, and `GSC_SKIP_OAUTH` preset to `"false"`. Its login line is built by the area from those resolved values (mcp-connect, Link with Google); it runs the server's own OAuth function in the pane, outside MCP (V 2026-11-11: the module imports without starting the server and the function raises `FileNotFoundError` without a client file; the browser flow itself was not run, task 6b1.6).
- **Service account** (unchanged from S5): `GSC_CREDENTIALS_PATH` (typed path) and `GSC_SKIP_OAUTH` preset to `"true"`; no login.

**Chat start with missing OAuth values [S6b1].** When a chat (or the probe) starts a declared `gsc` whose values select OAuth (`GSC_SKIP_OAUTH` not `true`/`1`/`yes` and `GSC_OAUTH_CLIENT_SECRETS_FILE` set) and `GSC_CONFIG_DIR` is unset, empty or not an absolute path, lazarobox MUST NOT start that server and MUST show `mcp: gsc unavailable: fill the OAuth values in Connectors (f)` once per start (without a fixed folder the server would look for its token elsewhere and start a browser flow inside the call).

**Chat start without a token [S6b1].** When a chat (or the probe) starts a declared `gsc` whose values select OAuth (`GSC_SKIP_OAUTH` not `true`/`1`/`yes` and `GSC_OAUTH_CLIENT_SECRETS_FILE` set) and `<GSC_CONFIG_DIR>/token.json` does not exist (`stat` only), lazarobox MUST NOT start that server and MUST show `mcp: gsc unavailable: not linked; open Connectors and use Link with Google` once per start. Reason: with no token, ANY gsc tool starts `run_local_server` inside the MCP call, where no browser opens and the URL goes to stdout, which lazarobox skips; the call would wait for its 15-minute call timeout (client.rs, unchanged) unless the user denies or cancels it. Hiding `reauthenticate` alone does not prevent that. With a token present the server starts normally; an expired token whose refresh fails falls into the same flow, and the guide says to link again when calls hang (Esc cancels the call).

**Name limit.** Like the Gmail filter, the hidden tools and the chat-start check apply only to a server declared with the name `gsc`; a mcp-gsc server declared under another name gets neither (guide and README say to use `name = "gsc"`).

The guide MUST explain both ways: for OAuth, adding the Desktop OAuth client (paste it with `o`, or reuse the GA4 file), that with a symlinked config folder the menu save refuses while reading and hand-editing still work, enabling the Search Console API, linking, that the token file is written by mcp-gsc with the default umask (`chmod 600` it; lazarobox's read tools refuse its folder), and that a call made before linking is refused at start; for the service account, the S5 steps. Tools: the 21 tools pinned by S5 with their classes; `reauthenticate` is hidden from chats (Gmail tool filter, hidden tools).

#### Scenario: Both ways in one snippet [auto]
- GIVEN the `gsc` entry
- WHEN its snippet is built
- THEN `env` is `["GSC_OAUTH_CLIENT_SECRETS_FILE", "GSC_CONFIG_DIR", "GSC_CREDENTIALS_PATH", "GSC_SKIP_OAUTH"]` and `GSC_ALLOW_DESTRUCTIVE` appears nowhere in the entry

#### Scenario: Templates per way [auto]
- GIVEN the `gsc` entry and `HOME=/h`
- WHEN the presets of each way are resolved for the save
- THEN the OAuth way gives `GSC_OAUTH_CLIENT_SECRETS_FILE = "/h/.config/lazarobox/google-oauth-client.json"`, `GSC_CONFIG_DIR = "/h/.config/mcp-gsc"` and `GSC_SKIP_OAUTH = "false"`, and the service-account way gives an empty `GSC_CREDENTIALS_PATH` and `GSC_SKIP_OAUTH = "true"`; no preset holds `~`

#### Scenario: The OAuth login stays outside MCP [auto]
- GIVEN the login line built for `HOME=/h` with `XDG_CONFIG_HOME=/x` in lazarobox's environment
- WHEN it is checked
- THEN it holds no control character and no first-run download (`uvx`, `npx`, `pipx run`, `dlx`), unsets `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `GSC_ALLOW_DESTRUCTIVE` and `GSC_SKIP_OAUTH`, sets `GSC_CONFIG_DIR='/h/.config/mcp-gsc'` (the same value the declared server gets from `secrets.toml`), holds no confidential value, and holds no `$(`

#### Scenario: Same token folder for the pane and the chat [auto]
- GIVEN `XDG_CONFIG_HOME=/x` and the `gsc` OAuth values saved
- WHEN the child env of a chat start of `gsc` and the env of the login line are compared
- THEN both have `GSC_CONFIG_DIR=/h/.config/mcp-gsc`

#### Scenario: OAuth way without a usable GSC_CONFIG_DIR [auto]
- GIVEN a declared `gsc` with `GSC_OAUTH_CLIENT_SECRETS_FILE` set, `GSC_SKIP_OAUTH = "false"`, and `GSC_CONFIG_DIR` missing, and in another run `GSC_CONFIG_DIR = "~/.config/mcp-gsc"`
- WHEN a chat turn with tools starts the user servers
- THEN `gsc` is not spawned and the chat shows `mcp: gsc unavailable: fill the OAuth values in Connectors (f)` once

#### Scenario: Not linked is refused at chat start [auto]
- GIVEN a declared `gsc` with the OAuth values and no `/h/.config/mcp-gsc/token.json`
- WHEN a chat turn with tools starts the user servers
- THEN `gsc` is not spawned and the chat shows `mcp: gsc unavailable: not linked; open Connectors and use Link with Google` once

#### Scenario: Old service-account entries keep working [auto]
- GIVEN a user entry pasted from S5 with `env = ["GSC_CREDENTIALS_PATH", "GSC_SKIP_OAUTH"]` and the S5 table
- WHEN the area builds its state and a chat starts it
- THEN the area shows the service-account way as complete and `declared`, with no `values missing`, and the chat starts it with no not-linked notice

#### Scenario: Another name is not covered [auto]
- GIVEN mcp-search-console declared as `search` with the OAuth values and no token
- WHEN a chat starts it
- THEN no not-linked check applies and `search__reauthenticate` is registered (the documented limit)

#### Scenario: Live query [manual]
- GIVEN a connected `gsc` (either way) whose account can read a property
- WHEN the model calls a search analytics query and the user presses `y`
- THEN a capped, untrusted-wrapped result is returned

### Requirement: Google Workspace entry, drafts only [S6c]
Rewritten on 2026-11-11 with the upstream facts of workspace-mcp 2.4.1 (V); the earlier "login command is the workspace-mcp pre-authorization run in the login pane" is dropped: no such command exists (V).

The `google` entry MUST install with `uv tool install workspace-mcp==2.4.1` and run `workspace-mcp` with args `--single-user --permissions gmail:readonly+drafts docs:full sheets:full`, plus `--disabled-tools send_gmail_message` as a second layer ONLY IF the probe of task 6c.5 (U) shows the server starts and lists the same 42 tools with it; otherwise the flag is left out and the allowlist is the only second layer. It MUST name `GOOGLE_OAUTH_CLIENT_ID`, `GOOGLE_OAUTH_CLIENT_SECRET` (the user's own Desktop OAuth client, C7), `USER_GOOGLE_EMAIL` (the account) and `WORKSPACE_MCP_DISABLE_LOCAL_FILES` (preset `"true"`, proposal open question 12, resolved by the orchestrator on 2026-11-11 under the owner's instruction to proceed; pending owner confirmation, conditional on task 6c.6 (U) showing it breaks none of the other allowlisted tools; if it does, the preset is dropped and the guide says so), all with values in `secrets.toml`, and MUST NOT name `WORKSPACE_MCP_CREDENTIALS_DIR` (so the tokens stay in `~/.google_workspace_mcp`, which the read tools refuse). It has no login command; it is linked by Link with Google (mcp-connect), which calls `start_google_auth`. Its guide MUST walk through creating the client and enabling the Gmail, Docs, Sheets and Drive APIs; carry the 7-day "Testing" warning (C7); say that replies are created as Gmail drafts and the user sends them from Gmail (C9, C12); say that the server holds a `gmail.compose` token that could send at the API level (the accepted residual risk of C9); say that workspace-mcp keeps its tokens as plaintext JSON in `~/.google_workspace_mcp/credentials`; say that the Docs and Sheets permissions include `drive.readonly`, so the server can read the whole Drive; and say that `get_gmail_attachment_content` saves files to local disk and that `draft_gmail_message` could attach local files unless `WORKSPACE_MCP_DISABLE_LOCAL_FILES` is `true`. Accepted risk (proposal open question 12, resolved by the orchestrator on 2026-11-11 under the owner's instruction to proceed; pending owner confirmation): `get_gmail_attachment_content` stays in the allowlist although it writes local disk outside lazarobox's write checks; it is class `write`, every call asks `y` and shows its arguments.

The entry MUST pin the 42 tools the server lists with these flags (V): `start_google_auth` (`write`, hidden from chats); the 8 Gmail tools of the allowlist (Gmail tool filter) with `search_gmail_messages`, `get_gmail_message_content`, `get_gmail_messages_content_batch`, `get_gmail_thread_content`, `get_gmail_threads_content_batch`, `list_gmail_labels` as `read` and `get_gmail_attachment_content` (writes local disk) and `draft_gmail_message` as `write`; the Docs tools `search_docs`, `get_doc_content`, `list_docs_in_folder`, `inspect_doc_structure`, `debug_table_structure`, `get_doc_as_markdown`, `list_document_comments` as `read` and `create_doc`, `modify_doc_text`, `find_and_replace_doc`, `insert_doc_elements`, `insert_doc_image`, `update_doc_headers_footers`, `batch_update_doc`, `create_table_with_data`, `export_doc_to_pdf`, `update_paragraph_style`, `manage_doc_tab` as `write`; the Sheets tools `list_spreadsheets`, `get_spreadsheet_info`, `read_sheet_values`, `list_sheet_tables`, `list_spreadsheet_comments` as `read` and `modify_sheet_values`, `format_sheet_range`, `manage_conditional_formatting`, `create_spreadsheet`, `create_sheet`, `append_table_rows`, `manage_sheet_tab`, `resize_sheet_dimensions`, `move_sheet_rows`, `manage_named_range` as `write`. The probe confirms the count stays under 64 (41 registered after `start_google_auth` is hidden).

#### Scenario: Permissions in the snippet [auto]
- GIVEN the `google` entry
- WHEN its snippet is built
- THEN its args hold `--single-user` and `--permissions gmail:readonly+drafts docs:full sheets:full`, hold `--disabled-tools send_gmail_message` if and only if task 6c.5 recorded that the server accepts it, hold no `--tools` or `--read-only`, and `env` is `["GOOGLE_OAUTH_CLIENT_ID", "GOOGLE_OAUTH_CLIENT_SECRET", "USER_GOOGLE_EMAIL", "WORKSPACE_MCP_DISABLE_LOCAL_FILES"]`

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

For a declared `google` server, lazarobox MUST register a Gmail tool (a tool whose name contains `gmail`, ASCII case-insensitive) only when its name is exactly one of the allowlist: `search_gmail_messages`, `get_gmail_message_content`, `get_gmail_messages_content_batch`, `get_gmail_attachment_content`, `draft_gmail_message`, `get_gmail_thread_content`, `get_gmail_threads_content_batch`, `list_gmail_labels` (C9: "lazarobox lists only read and draft tools"). Every other Gmail tool the server lists, `send_gmail_message` included, MUST NOT be registered, whatever the server's flags. Independently, a tool an entry hides (`start_google_auth` for `google`, `reauthenticate` for `gsc`) MUST NOT be registered in any chat (by server name: a server declared under another name is not covered, for `gsc` as for `google`); the link session of mcp-connect still calls `start_google_auth` by name. Both filters MUST apply before the 64-tool cap (in `list_tools_with`, design D5), so a dropped tool never takes a kept tool's place, and the probe counts only admitted tools. Non-Gmail tools that are not hidden follow the ordinary rules; a server with another name is untouched. Sending with an explicit user permission is a later, undecided change (C12).

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
