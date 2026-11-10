# mcp-catalog Specification

## Purpose

A curated catalog of existing MCP servers (C2: no home-grown connectors, lazarobox does not reimplement any Google or Microsoft API) that tells the user exactly what to declare in `config.toml` (C4, C5), which values go to `secrets.toml` (C6), how to install and log in, and the read/write class of each tool (C11). The catalog never declares a server by itself: a server is active only when the user's `[[ai.mcp.servers]]` entry declares it (see mcp-servers). Entries follow the order of C8: `clarity`, `ga4`, then `gsc`, then the Workspace entries `google` and `apps-script`. Slice tags: [S2] catalog core and the Clarity and GA4 entries, [S3] probe, [S5] GSC, [S6] Workspace. Scenario tags: [auto] = `cargo test` (pure data, fake server child); [manual] = real service account. Items marked "assumption" are proposals, not user decisions; binary names marked U are unverified and pinned when the slice is built.

Terms: "entry" = one catalog entry. "Declared" = a `[[ai.mcp.servers]]` entry whose `name` equals the entry id. "Class" = `read` or `write`.

## Requirements

### Requirement: Entries are data [S2]
The catalog MUST be a pure data module in core with no IO. Each entry MUST hold: its id (a valid server `name`, see configuration); its group (`Analytics` or `Workspace`); the server's project and license; the pinned install command; the run command and args, narrowing flags included; the `env` names and, for each, whether its value belongs in `secrets.toml`; the login command, if any; the guide steps and links; the token location; and the class of each known tool. Adding an entry MUST NOT require changing the probe, the menu drawing or the registry code.

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
The `ga4` entry MUST install with `pipx install analytics-mcp`, run `analytics-mcp` (binary name U), run without `--allow-write`, name `GOOGLE_CLOUD_PROJECT` (value in `secrets.toml`), and log in with `gcloud auth application-default login --scopes analytics.readonly,cloud-platform --client-id-file=<client file>` (the form of the analytics-mcp README; the exact scope spelling gcloud accepts is pinned when the slice is built) in the login pane. The server reads the resulting ADC from `~/.config/gcloud` through the real `HOME`. Its guide MUST walk step by step through creating a Desktop OAuth client in the user's own Google Cloud project (C7), enabling the Google Analytics APIs, and saving the client file, and MUST warn that a client left in "Testing" gets refresh tokens that expire after 7 days (C7). Assumption: the guide suggests saving the client file as `~/.config/lazarobox/google-oauth-client.json`, which the read tools already refuse, and the login command uses that path. Tools: `run_report`, `run_funnel_report`, `run_realtime_report` and the account and property info tools are `read`; `create_property_annotation` is `write`.

#### Scenario: Read-only run [auto]
- GIVEN the `ga4` entry
- WHEN its snippet is built
- THEN its args hold no `--allow-write` and `env` is `["GOOGLE_CLOUD_PROJECT"]`

#### Scenario: Testing warning [auto]
- GIVEN the `ga4` guide
- WHEN it is rendered
- THEN it says that a client in "Testing" gets refresh tokens that expire after 7 days

#### Scenario: Live report [manual]
- GIVEN a connected `ga4` and a property the user can read
- WHEN the model calls `run_report` and the user presses `y`
- THEN a capped, untrusted-wrapped result is returned

### Requirement: GSC entry with a service account [S5]
The `gsc` entry MUST install with `uv tool install mcp-search-console` (binary name U), name `GSC_CREDENTIALS_PATH` and `GSC_SKIP_OAUTH` (values in `secrets.toml`, `GSC_SKIP_OAUTH = "true"`), have no login command, and MUST NOT name `GSC_ALLOW_DESTRUCTIVE` (C10). Its guide MUST explain creating a service account and its JSON key in the user's Google Cloud project, enabling the Search Console API, and adding the service account's email as a user of the Search Console property (C10). Assumption: the guide suggests saving the key as `~/.config/lazarobox/gsc-service-account.json`, which the read tools already refuse. OAuth with the user's own client (C7) is not in this entry until a pane pre-login is verified to work with mcp-gsc (C10). Tools: the search analytics queries, URL inspection and sitemap lists are `read`; `manage_sitemaps` is `write`.

#### Scenario: Service account values [auto]
- GIVEN the `gsc` entry
- WHEN its snippet and secrets template are built
- THEN `env` is `["GSC_CREDENTIALS_PATH", "GSC_SKIP_OAUTH"]`, the template holds `GSC_SKIP_OAUTH = "true"` and an empty `GSC_CREDENTIALS_PATH`, and `GSC_ALLOW_DESTRUCTIVE` appears nowhere

#### Scenario: Live query [manual]
- GIVEN a connected `gsc` whose service account was added to a property
- WHEN the model calls a search analytics query and the user presses `y`
- THEN a capped, untrusted-wrapped result is returned

### Requirement: Google Workspace entry, drafts only [S6]
The `google` entry MUST install with `uv tool install workspace-mcp`, run `workspace-mcp --single-user --permissions gmail:readonly+drafts docs:full sheets:full` (binary name U), and name `GOOGLE_OAUTH_CLIENT_ID` and `GOOGLE_OAUTH_CLIENT_SECRET` (values in `secrets.toml`, from the user's own Desktop OAuth client, C7). Its guide MUST walk through creating that client and enabling the Gmail, Docs, Sheets and Drive APIs, and MUST carry the 7-day "Testing" warning (C7). It MUST say that replies are created as Gmail drafts, that the user sends them from Gmail (C9, C12), and that the server holds a `gmail.compose` token that could send at the API level (the accepted residual risk of C9). Its login command is the workspace-mcp pre-authorization run in the login pane, pinned when it is verified to work outside an MCP call (U).

#### Scenario: Permissions in the snippet [auto]
- GIVEN the `google` entry
- WHEN its snippet is built
- THEN its args hold `--permissions gmail:readonly+drafts` and no `--tools` or `--read-only`

#### Scenario: Draft after y [manual]
- GIVEN a connected `google`
- WHEN the model calls `draft_gmail_message` and the user presses `y`
- THEN a draft exists in the user's Gmail and nothing is sent

### Requirement: Gmail tool filter [S6]
For a declared `google` server, lazarobox MUST register only the Gmail tools the catalog lists for it: the read tools (`search_gmail_messages`, `get_gmail_message_content`, `get_gmail_thread_content` and the other cataloged reads) and the draft tool `draft_gmail_message` (C9: "lazarobox lists only read and draft tools"). Every other Gmail tool the server lists, `send_gmail_message` included, MUST NOT be registered, whatever the server's permissions. A Gmail tool is a tool whose name contains `gmail` (assumption). The filter applies before the 64-tool cap. Non-Gmail tools follow the ordinary rules. Sending with an explicit user permission is a later, undecided change (C12).

#### Scenario: Send is not registered [auto]
- GIVEN a declared `google` and a fake server listing `search_gmail_messages`, `draft_gmail_message` and `send_gmail_message`
- WHEN the registry is built
- THEN `google__search_gmail_messages` and `google__draft_gmail_message` exist and no `send_gmail_message` tool exists

#### Scenario: Unknown Gmail tool dropped [auto]
- GIVEN the same server also listing `delete_gmail_label_x`
- WHEN the registry is built
- THEN it is not registered

#### Scenario: Other servers untouched [auto]
- GIVEN a declared server `mail` listing `send_gmail_message`
- WHEN the registry is built
- THEN `mail__send_gmail_message` is registered and still needs `y` on every call

### Requirement: Apps Script entry [S6]
The `apps-script` entry MUST install with `npm i -g @google/clasp`, run `clasp mcp`, name no `env`, and log in with `clasp login` in the login pane (clasp's own OAuth client). Its guide MUST say to switch on the Apps Script API for the account, that `push_files` uploads code from a directory on disk and that pushed code runs with the script's scopes. Tools: `list_projects`, `pull_files` and `clone_project` are `read`; `push_files` and `create_project` are `write`. No `run_script_function` or deployment tool is cataloged.

#### Scenario: No secrets [auto]
- GIVEN the `apps-script` entry
- WHEN its snippet and secrets template are built
- THEN the snippet has no `env` and there is no secrets template

#### Scenario: Push after y [manual]
- GIVEN a connected `apps-script` and an Apps Script project on disk under `$HOME`
- WHEN the model calls `push_files` and the user presses `y`
- THEN the project is uploaded
