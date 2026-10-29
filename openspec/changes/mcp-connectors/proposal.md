# Proposal: MCP Connectors (a curated catalog and a menu wizard to connect Google Workspace and analytics services)

## Intent

The user asked (2026-10-28, verbatim):

> "las conexiones de MCP por ejemplo Goole para leer correos y poder escribir y dejar en borrador, google docs para subir código en apps script, sheets, gsc, analytics, clarity, y de alguna manera podamos hacer esas conexiones fáciles al usuario y así vamos también dando ya algo de cuerpo a la parte de analítica de la shell"

lazarobox can already run the user's own MCP servers (`[[ai.mcp.servers]]`, README.md:697-734). But today a user must:

- find a server,
- install it,
- create OAuth credentials,
- log in from a terminal,
- export env vars in the shell that launches lazarobox,
- hand-edit `config.toml`.

The menu never helps, because it never writes `ai.mcp.*` (src/core/config.rs:85). For Google services this is several pages of setup per service.

This change adds a **curated catalog of existing MCP servers** plus a **menu wizard**. The wizard installs a server, guides the login, stores the values the server needs and writes the `[[ai.mcp.servers]]` entry. It covers two blocks at once:

- **Google Workspace:** Gmail (read, and create drafts, never send), Docs, Sheets, Apps Script (push project code).
- **Analytics:** Google Search Console, Google Analytics 4, Microsoft Clarity.

The analytics block is the first real content of the shell's analytics area.

## Decisions (user, 2026-10-28, settled)

- **C1 One proposal.** Both blocks (Workspace and analytics) and the shared "easy connect" wizard are in this one change.
- **C2 Approach.** A curated catalog of existing MCP servers, plus a menu wizard. No home-grown connectors: lazarobox does not reimplement any Google or Microsoft API.
- **C3 Purpose.** Make these connections easy for the user, and give body to the shell's analytics area.
- **C4 Declared in a file, not written by the menu (Q1, 2026-10-29).** User: "Aquí creo que una buena práctica sería como lo hace por ejemplo opencode o bien como sería cualquiera, donde tienen un json donde meten el mcp o tool que quieran implementar, más adelante en el perfile de analista metemos algunos mcp ya por defecto para que puedan conectar con auth como los de google analytics, gsc, clarity...". The rule "the menu never writes `ai.mcp.*`" stays: no wizard-owned append/remove (scope item 5 and slice S2 are dropped). Later, the `analyst` profile ships default entries (GA4, GSC, Clarity) that the user connects through their auth. opencode declares servers under `mcp` in `opencode.json` (`type`, `command`, `environment`, `enabled`; secrets as `{env:VAR}`; https://opencode.ai/docs/mcp-servers).
- **C5 The file is `config.toml` (2026-10-29).** User chose "Seguir en config.toml": servers stay in the existing `[[ai.mcp.servers]]` (no separate `mcp.json`, no migration), the format frozen for 1.0.
- **C6 Secrets in a lazarobox-owned 0600 file (Q2, 2026-10-29).** User chose "Fichero 0600 propio": values (tokens, client id/secret, project ids) live in `~/.config/lazarobox/secrets.toml`, mode 0600, edited by the user, refused by the read tools (paths.rs:32); each value reaches only its own server and never `config.toml`.
- **C7 Bring-your-own OAuth client (Q3, 2026-10-29).** User chose "Cada usuario trae el suyo": each user creates a Desktop OAuth client in their own Google Cloud with a step-by-step guide; its secret goes to `secrets.toml`. No Google verification needed; the guide warns that a client left in "Testing" gets refresh tokens that expire after 7 days.
- **C8 Analytics first (Q4, 2026-10-29).** User chose "Analítica primero": the secrets file and read-deny additions, then Clarity and GA4, then GSC; Workspace (Gmail drafts, Docs, Sheets, Apps Script) after. The slice table below predates C4 and is rewritten when the spec is written.
- **C9 Gmail: drafts, never send (Q5, 2026-10-29).** User chose "Borradores, sin enviar": workspace-mcp runs with `gmail:readonly+drafts`, lazarobox lists only read and draft tools, every call asks `y`, the user sends from Gmail. Accepted residual risk: the third-party server holds a `gmail.compose` token that can send.
- **C10 GSC: service account first (Q6, 2026-10-29).** User chose "Cuenta de servicio primero": mcp-gsc starts with `GSC_CREDENTIALS_PATH` plus `GSC_SKIP_OAUTH=true` (path in `secrets.toml`), a guide explains creating the account and adding it to the property; OAuth with the user's own client (C7) follows once a pane pre-login is verified to work with mcp-gsc.
- **C11 `y` on every connector call for now (Q7, 2026-10-29).** User chose "y en todo, relajar luego": this change asks `y` on every call, as today, and the catalog records each tool's read/write class; a later change decides how classes meet `run` mode once chat-auto-permissions S4 lands. A server's own `readOnlyHint` stays ignored.

Everything else below is a proposal written before C4-C11; where they differ, C4-C11 win. All open questions are resolved.

## Facts this proposal rests on

V = verified against the linked source on 2026-10-28. U = unverified. Code references are on `origin/main` 191dc1c.

### lazarobox today

- **Transport.** User servers are stdio only. The client spawns the command with piped stdin and stdout (src/runtime/mcp/client/spawn.rs:111-121), and `src/runtime/mcp/` has no HTTP or SSE client. The protocol is `2025-06-18`, with `2025-03-26` and `2024-11-05` accepted (src/core/ai/tools/mcp.rs:43-46).
- **Environment.**
  - The env is cleared. The server then gets `PATH, USER, LANG, LC_ALL, TMPDIR`, then only the variable NAMES listed in `env` (spawn.rs:44, 72-90).
  - `HOME` is forced to the real home. `LD_*` and `DYLD_*` names and the session token are dropped (spawn.rs:65, 84).
  - The cwd is `$HOME` (spawn.rs:116).
  - The config accepts names only, never values (src/core/ai/config.rs:295-308, 970-978).
  - No `DISPLAY`, `BROWSER`, `WAYLAND_DISPLAY` or `XDG_*` reaches the server, so a server that opens a browser itself fails.
- **Not jailed.** The server is trusted and runs outside the jail (spawn.rs:8-12), so it can write its token dirs under `$HOME`.
- **Engines.** With claude, codex and gemini, user servers are started and offered only when the CLI turn runs jailed. Ollama and llama.cpp always get them (src/runtime/chat.rs:633; README.md:734).
- **Timeouts.** The handshake (`initialize` plus `tools/list`) has 20 s; one call has 15 min (src/runtime/mcp/client.rs:73-79).
- **Caps.**
  - At most 64 tools per server; the first 64 are kept (client.rs:51, 191).
  - Tool descriptions 1 KiB and schemas 16 KiB (client.rs:53-55).
  - Call arguments 4 KiB (src/core/ai/tools/mod.rs:584).
  - Results 32 KiB, sanitized and nonce-wrapped as untrusted (src/core/ai/tools/label.rs:4, 21-31).
- **Names.** Each part of `<server>__<tool>` is lowercased, characters outside `[a-z0-9_-]` become `_`, and the part is cut at 24 characters (src/core/ai/tools/servers.rs:45-62). A collision keeps the first tool and drops the later one with a notice (servers.rs:134-160).
- **Approval.** Every call to a user server tool needs `y`, also in `auto`, and `readOnlyHint` is ignored (README.md:730; src/runtime/mcp/external.rs:6-8; spec `agent-tools/specs/mcp-servers/spec.md:100-101`; `coding-tools/specs/agent-tools/spec.md:94`: "Tools from a user MCP server MUST always be write tools").
- **Stdout.** A stdout line that is not JSON is skipped silently (client.rs:299-300). A login URL printed to stdout never reaches the user, and the call waits until its timeout.
- **Menu.** The menu never writes `ai.mcp.*`, and every save preserves those bytes exactly (src/core/config.rs:85; `agent-tools/specs/config-persistence/spec.md:73-74`; tests at src/core/config_edit.rs:1356, 1505, 1528).
- **Read-deny list.** The read tools refuse `~/.config/gcloud` and `~/.config/lazarobox` (src/core/ai/tools/paths.rs:23-33). They do NOT refuse `~/.google_workspace_mcp`, `~/.clasprc.json` or `~/.config/mcp-gsc` (`git grep` on main finds none of them).

### The services

- **workspace-mcp** ([taylorwilsdon/google_workspace_mcp](https://github.com/taylorwilsdon/google_workspace_mcp)): community, MIT, about 3.4k stars, v2.2.1 released 2026-10-27 (V).
  - Gmail, Docs, Sheets and Apps Script (and more) in one server, with about 120 tools in total (V, README).
  - Runs over stdio or streamable HTTP, with `uvx workspace-mcp` (V).
  - Needs the user's own OAuth client in `GOOGLE_OAUTH_CLIENT_ID` and `GOOGLE_OAUTH_CLIENT_SECRET` (V).
  - Tokens go to `~/.google_workspace_mcp/credentials` (dir 0700, files 0600), or to `WORKSPACE_MCP_CREDENTIALS_DIR` (V, `auth/credential_store.py`).
  - `--permissions gmail:readonly+drafts` grants only `gmail.readonly` and `gmail.compose`. `send_gmail_message` needs `gmail.send`, so the scope filter hides it (V, `auth/permissions.py`, `gmail/gmail_tools.py`). `draft_gmail_message` only calls `drafts().create` (V).
  - `--permissions` cannot be combined with `--tools` or `--read-only` (V, `main.py`). `--disabled-tools` removes named tools (V, README).
  - In stdio mode it tries `webbrowser.open`. When that fails, it returns the authorization URL in the tool result and listens for the callback on `localhost:8000` (V, `auth/google_auth.py`, `auth/oauth_callback_server.py`).
- **Google-hosted Workspace MCP servers** ([developers.google.com](https://developers.google.com/workspace/guides/configure-mcp-servers), updated 2026-10-27): official, **Developer Preview**, remote streamable HTTP only (V).
  - There are servers for Gmail, Drive, Docs, Sheets and more, but none for Apps Script (V).
  - They need a web OAuth client with "Use this client for an AI-powered agent" and membership in the Developer Preview Program (V).
  - The Gmail server has `create_draft` and no send tool (V).
- **clasp** ([google/clasp](https://github.com/google/clasp)): Google, Apache-2.0, about 5.9k stars, v3.4.1 released 2026-08-28 (V).
  - `clasp mcp` is stdio and marked EXPERIMENTAL (V, README).
  - clasp has its **built-in OAuth client** (V, `src/auth/auth.ts`). The user runs `clasp login` once, and the tokens go to `~/.clasprc.json` (V, README).
  - Tools: `push_files`, `pull_files`, `create_project`, `clone_project`, `list_projects`. The project dir comes from the tool call and must be inside `$HOME` or the cwd (V, `src/mcp/server.ts`).
  - It pushes code **from a directory on disk**, so the code never has to fit in the 4 KiB argument cap.
  - The default login scopes include `script.projects`, `script.deployments`, `script.webapp.deploy`, `drive.file` and `cloud-platform` (V, `src/auth/auth.ts`).
- **mcp-gsc** ([AminForou/mcp-gsc](https://github.com/AminForou/mcp-gsc), PyPI `mcp-search-console` 0.4.1): community, MIT, about 2k stars, last commit 2026-09-15 (V).
  - There is no official Google Search Console server (U: only third-party sources say so).
  - Runs over stdio with `uvx mcp-search-console` (V).
  - Auth is either the user's own Desktop OAuth client (`GSC_OAUTH_CLIENT_SECRETS_FILE`) or a service account (`GSC_CREDENTIALS_PATH` plus `GSC_SKIP_OAUTH=true`) (V).
  - It always requests the full `webmasters` scope (V, `gsc_server.py:106`).
  - The token goes to `~/.config/mcp-gsc/token.json`, or to `GSC_CONFIG_DIR`, written with the default umask (V).
  - The OAuth flow (`run_local_server(port=0)`) runs inside the first tool call (V).
  - Destructive site and sitemap tools are gated by `GSC_ALLOW_DESTRUCTIVE` (V).
- **analytics-mcp** ([googleanalytics/google-analytics-mcp](https://github.com/googleanalytics/google-analytics-mcp)): official, Apache-2.0, about 3.6k stars, 0.8.0 released 2026-10-22, "Experimental" (V).
  - Runs over stdio with `pipx run analytics-mcp` (V).
  - Auth is ADC from `gcloud auth application-default login --scopes analytics.readonly,cloud-platform --client-id-file=...`, plus `GOOGLE_CLOUD_PROJECT` (V).
  - Read-only by default. `--allow-write` with `analytics.edit` adds only `create_property_annotation` (V).
- **Clarity** ([microsoft/clarity-mcp-server](https://github.com/microsoft/clarity-mcp-server), npm 2.0.1): Microsoft, MIT (V).
  - Runs over stdio with `npx @microsoft/clarity-mcp-server` (V).
  - The project-admin API token comes from the `CLARITY_API_TOKEN` env var (V, `src/utils.ts`). It has 3 read-only tools.
  - API limits: 10 requests per project per day, the last 1 to 3 days only, 1,000 rows, no pagination ([Microsoft Learn](https://learn.microsoft.com/en-us/clarity/setup-and-installation/clarity-data-export-api), V).
- **Gmail scopes** ([Gmail scopes](https://developers.google.com/workspace/gmail/api/auth/scopes)).
  - `gmail.readonly` and `gmail.compose` are **restricted**, and `gmail.compose` "manage drafts and send emails". The token can send even when no tool exposes sending (V).
  - Restricted scopes need restricted-scope verification, and storing or transmitting that data on servers needs a security assessment (V).
- **OAuth tokens** ([OAuth 2.0](https://developers.google.com/identity/protocols/oauth2)).
  - An external-type consent screen in "Testing" status gets refresh tokens that expire in 7 days (V).
  - There is a limit of 100 refresh tokens per Google Account per client ID (V).
  - For installed apps, "the client secret is obviously not treated as a secret" (V).
  - The 100-user cap for unverified apps is U.

## Scope

### In Scope

1. **Catalog.** A pure data module in core with one entry per service. Each entry holds:
   - the server and its pinned install command;
   - the run command and its narrowing flags;
   - the env names, and which of them are secrets;
   - the login kind and the setup checklist;
   - the token location;
   - a read/write class for each known tool.
2. **Wizard.** A "Connect a service" area in the menu, grouped as Workspace and Analytics. It:
   - shows the checklist;
   - runs the pre-install;
   - runs the login in a real pane;
   - asks for the values;
   - writes the entry;
   - probes the server once (start, `tools/list`, stop) and reports the result.
3. **Env values store.** The values the catalog needs (client id and secret, `GOOGLE_CLOUD_PROJECT`, `CLARITY_API_TOKEN`, file paths) are stored per server in a file lazarobox owns, mode 0600, under `~/.config/lazarobox`, which the read tools already refuse (paths.rs:32). They are passed only to that server.
4. **Login helper.** Login commands run in a real lazarobox pane with a TTY: `clasp login`, `gcloud auth application-default login ...`, and the pre-auth for workspace-mcp and mcp-gsc. The browser is opened with `xdg-open`, `open` on macOS or **`wslview` on WSL**. When none is available, the URL is shown for the user to copy. This replaces the OAuth flows inside a tool call, which lazarobox cannot show (client.rs:299-300).
5. **Lifting "the menu never writes `ai.mcp.*`".** Only the wizard may append (and, later, remove) its own `[[ai.mcp.servers]]` entries. Every other byte of `ai.mcp.*` stays byte-identical, including comments, hand-written entries and invalid entries. The save of other keys stays unblocked.
6. **Pre-install and the 20 s handshake.** Entries run a pre-installed binary (`uv tool install`, `pipx install`, `npm i -g`), never a first-run `uvx`, `npx` or `pipx run` download, which could exceed the 20 s handshake (client.rs:76). How long a first download takes is U.
7. **Tool-list checks.** The wizard probe checks that the server lists at most 64 tools (client.rs:51). It also checks that no two tools collide after the 24-character cut (servers.rs:45-62). If either fails, the entry is narrowed or the wizard says so.
8. **Read-deny list.** `~/.google_workspace_mcp`, `~/.clasprc.json`, `~/.config/mcp-gsc` and the env values store are added to the read tools' secret list (paths.rs:23-33). `.config/gcloud` and `.config/lazarobox` are already there.
9. **Docs.** The README section "Tus propios servidores MCP" gets a "Conectar un servicio" part. The specs and the privacy note are updated.

### Out of Scope

- Home-grown connectors, or any Google or Microsoft API code in lazarobox (C2).
- Streamable-HTTP transport and the Google-hosted Workspace servers (Developer Preview, HTTP only). They are a possible later change, once lazarobox has an HTTP and OAuth 2.1 client and the servers leave preview.
- Dashboards, charts, scheduled reports or any analytics UI beyond connecting the servers.
- Changing the `y`-per-call rule for user server tools. See "Interaction with `chat-auto-permissions`".
- A lazarobox-owned, Google-verified OAuth client.
- Sending email, and Apps Script `run_script_function` or deployments.
- Project-scoped wizard entries. The wizard writes global entries; `project` stays hand-edited.
- Other services (Calendar, Drive as its own entry, Slides and so on). The catalog is built to add them later.

## Catalog entries (recommended)

| Service | Server | Install / run | Auth and scopes | Narrowing | Tools (R = read, W = write) | Fit on main |
|---|---|---|---|---|---|---|
| Gmail, Docs, Sheets | workspace-mcp (one entry `google`) | `uv tool install workspace-mcp` → `workspace-mcp --single-user ...` (V for the flags, U for the binary name after `uv tool install`) | User's own Desktop OAuth client (secret values in the store). Scopes: `gmail.readonly` and `gmail.compose` (restricted); `documents`, `spreadsheets`, `drive.file` (sensitive), `drive.readonly` | `--permissions gmail:readonly+drafts docs:full sheets:full --disabled-tools send_gmail_message`. Whether `--permissions` alone keeps the list under 64 tools is U, and the probe checks it | R: `search_gmail_messages`, `get_gmail_message_content`, `get_gmail_thread_content`, `get_doc_content`, Sheets reads. W: `draft_gmail_message`, `modify_doc_text`, `batch_update_doc`, Sheets writes, label edits | Works once tokens exist. Pre-auth outside a chat is needed, because the tool-result URL and the `:8000` listener inside a chat are awkward (end-to-end behaviour U) |
| Apps Script | clasp (entry `apps-script`) | `npm i -g @google/clasp` → `clasp mcp` (V) | clasp's built-in OAuth client; `clasp login` in a pane (V); the user switches on the Apps Script API (U) | none (5 tools) | R: `list_projects`, `pull_files` (writes local files), `clone_project`. W: `push_files`, `create_project` | Fits: pushes from disk, needs no secret value |
| Search Console | mcp-gsc (entry `gsc`) | `uv tool install mcp-search-console` (binary name U) | User's own Desktop OAuth client file in `GSC_OAUTH_CLIENT_SECRETS_FILE`, or a service account. Scope: full `webmasters` | `GSC_ALLOW_DESTRUCTIVE` unset | R: analytics queries, URL inspection, sitemap lists. W: `manage_sitemaps` (submit/delete) | Partial: its OAuth flow runs inside a tool call, and the URL is lost on stdout. Needs an out-of-band token, or a service account. How to create the token outside an MCP call is U |
| GA4 | analytics-mcp (entry `ga4`) | `pipx install analytics-mcp` → `analytics-mcp` (U for the binary name; V for `pipx run analytics-mcp`) | ADC by `gcloud auth application-default login --scopes analytics.readonly,cloud-platform --client-id-file=<own client>` in a pane; `GOOGLE_CLOUD_PROJECT` in the store | no `--allow-write` | R: `run_report`, `run_funnel_report`, `run_realtime_report`, account and property info | Fits: ADC is read from `~/.config/gcloud` via the forced `HOME` |
| Clarity | clarity-mcp-server (entry `clarity`) | `npm i -g @microsoft/clarity-mcp-server` → `clarity-mcp-server` (V) | Project-admin API token in `CLARITY_API_TOKEN` (store) | none (3 tools) | R: dashboard query, session recordings, docs search | Fits. The wizard warns about the 10 calls per project per day |

**One Workspace server or one per service.** One workspace-mcp entry for Gmail, Docs and Sheets means one OAuth client, one login and one token store. Its permission levels give "drafts, never send" at the tool layer. Apps Script goes to clasp instead of workspace-mcp's `manage_script_content`, because:

- code passed in arguments hits the 4 KiB cap (tools/mod.rs:584);
- clasp needs no user-created OAuth client.

## Interaction with `chat-auto-permissions`

The in-flight feature `chat-auto-permissions` (odd/tasks/chat-auto-permissions.md, branch `feat/chat-auto-permissions`) changes approvals in `approval = "run"`:

- **S2:** reads inside the chat root run unasked.
- **S4:** writes ask, and a full grant lasts for the current conversation only.

Today every user MCP call asks `y`, and the spec says user server tools "MUST always be write tools" (`coding-tools/specs/agent-tools/spec.md:94`).

The catalog's per-tool read/write class could later let **R tools** (Gmail search, GA4 `run_report`, Clarity queries) follow S2-like rules. It could also let **W tools** (`draft_gmail_message`, `push_files`, Sheets writes) follow S4's per-conversation grant.

This change does **not** decide that. It ships with `y` on every call, as today. It records the class in the catalog so a later change can use it.

Dependency and open question: should catalog classes ever relax the `y` rule, and only after `chat-auto-permissions` S2/S4 land? A server's own `readOnlyHint` stays ignored either way.

## Capabilities

### New Capabilities
- `mcp-catalog`:
  - the catalog entries and their fields;
  - the per-tool read/write class;
  - the narrowing flags;
  - the probe checks (at most 64 tools, no collision after the 24-character cut, the handshake within 20 s).
- `mcp-connect-wizard`:
  - the menu area (Workspace / Analytics) and the checklist;
  - pre-install and the login pane (`xdg-open` / `open` / `wslview`, with a copyable URL as fallback);
  - value entry, the entry write and the probe result.

### Modified Capabilities
All deltas apply on top of the unarchived `agent-tools` and `coding-tools` change specs, where these capabilities live.
- `mcp-servers`:
  - An entry may take env values from the lazarobox store, not only names from the environment. A value from the store wins over a same-named host variable (proposal; see Q2).
  - The `LD_`/`DYLD_`, `HOME` and token rules (spawn.rs:65-90) stay.
- `config-persistence`: "MCP server lists are never written" (`agent-tools/specs/config-persistence/spec.md:73-74`) becomes "written only by the wizard, append or remove of its own entries, every other byte preserved". It still says "no secret is ever written to `config.toml`".
- `configuration`: the store's file format and location, and how an entry points at it.
- `config-menu`: the new "Connect a service" area.
- `agent-tools`: the read-deny list gains the token dirs and the store.

## Approach

1. **Store first.** Make env values possible without changing the trust model:
   - a 0600 file per server, inside a 0700 dir under `~/.config/lazarobox`;
   - read only when that server starts;
   - never logged, never sent to the model, never in `config.toml`.
2. **Persistence.** The wizard's write goes through `config_edit` as an append of a whole `[[ai.mcp.servers]]` table. It is marked so the wizard can recognise its own entries (the exact marker is a design question). The existing preservation tests (config_edit.rs:1356, 1505, 1528) must keep passing unchanged.
3. **Catalog.** Pure data plus pure checks in `src/core/ai/`, tested without processes.
4. **Wizard.** It lives in the menu (`src/core/menu.rs`, `src/ui/`). Pre-install and login run as visible commands in a new pane. Nothing installs or logs in hidden.
5. **Probe.** It reuses `ExternalSet::start`/`shutdown` (external.rs:97-132, 199-209) on one server, outside any chat turn, and prints the `mcp: started` line or the `unavailable` notice.
6. **Docs.** The README and specs say that with claude, codex and gemini the connected tools appear only on jailed turns (chat.rs:633).

## Affected Areas

| Area | Impact |
|---|---|
| `src/core/ai/config.rs` (`McpServer`, `read_mcp_entry`) | Modified (reference to stored values) |
| `src/runtime/mcp/client/spawn.rs` (`child_env`) | Modified (merge stored values, same filters) |
| `src/core/config.rs:85`, `src/core/config_edit.rs` | Modified (wizard-owned append/remove of `ai.mcp.servers`) |
| `src/core/ai/` new catalog module | New |
| `src/core/menu.rs`, `src/ui/components/` | Modified / New (Connect area, wizard steps) |
| `src/runtime/` login pane, browser opener, probe | New |
| `src/core/ai/tools/paths.rs:23-33` | Modified (token dirs, store) |
| README.md:697-734, privacy note, specs | Modified |

## Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| Gmail scopes are restricted: a shared lazarobox OAuth client would need restricted-scope verification and possibly a security assessment (V) | Certain if shared | Bring-your-own OAuth client per user (proposal; Q3). The checklist walks through creating it |
| BYO client in "Testing" status: refresh tokens expire after 7 days for external-type consent screens (V) | High for personal Gmail | The wizard explains it and offers "re-login" from the menu. Publishing the user's own app to production (unverified-app warning) is the user's choice (implications U) |
| Prompt injection from email, Docs or Search Console content steering the model (V, warned by every vendor) | High | Results are already untrusted-wrapped and capped (label.rs). `y` on every call stays. Gmail is drafts-only at the tool layer |
| `gmail.compose` can send at the API level even though no tool sends (V) | Med | `--permissions gmail:readonly+drafts` plus `--disabled-tools send_gmail_message`. Documented as a tool-layer guarantee, not a token guarantee |
| Apps Script push is code execution outside the jail: pushed code runs with the script's scopes, and clasp's login includes `script.webapp.deploy` and `cloud-platform` (V) | Med | Every `push_files` asks `y` and shows the project dir. No `run_script_function` or deployment tools are cataloged. The README states the risk |
| Wide default scopes: mcp-gsc always takes full `webmasters`, and workspace-mcp's `complete` tier includes send and sharing (V) | Med | Narrowing flags in each entry. `GSC_ALLOW_DESTRUCTIVE` unset. The probe rejects an unexpected tool list |
| Experimental or preview servers (clasp MCP, analytics-mcp, Google-hosted) change under us (V) | Med | Pinned versions in the catalog. The probe on connect. Hosted servers out of scope |
| An OAuth URL printed to stdout is swallowed and the call hangs up to 15 min (client.rs:299-300) | High for mcp-gsc | Login only in the login pane, never inside a tool call. A service-account path for GSC |
| Token files readable by the model through the read tools (not in the deny list today) | Med | Add them to `SECRET_DIRS`/`SECRET_FILES` in the first slice |
| Lifting the menu rule corrupts hand-written `ai.mcp.*` | Low | Append-only of whole tables. The existing byte-preservation tests stay. New tests with comments and invalid entries |
| Clarity's 10 requests per day are used up by a chatty model | Med | Wizard warning; `y` per call |

## Rollback Plan

- Each slice reverts on its own.
- Entries the wizard wrote are ordinary `[[ai.mcp.servers]]` tables. Older builds read them, but without the store their secret env values are missing and the server reports `unavailable`. Hand-written entries are untouched.
- The store is a separate file and can be deleted. No migration.

## Dependencies

- `chat-auto-permissions` (in flight). Not a blocker for shipping, but a blocker for any relaxation of `y` based on catalog classes (see above).
- Main 191dc1c or later (`[[ai.mcp.servers]]` and the external client are merged).

## Success Criteria

- [ ] `cargo test --locked`, `cargo clippy --all-targets --locked -- -D warnings` and `cargo fmt --all -- --check` pass on every slice.
- [ ] From the menu, a user connects Clarity with only a token and sees `mcp: started clarity` in the probe.
- [ ] Each Google entry connects after its checklist, with no hand edit of `config.toml` and no `export` in the shell.
- [ ] `draft_gmail_message` creates a draft after `y`, and no send tool is listed.
- [ ] `push_files` uploads an on-disk Apps Script project after `y`.
- [ ] GA4 `run_report` and a GSC query return capped, wrapped results.
- [ ] A save of other menu keys leaves every byte of hand-written `ai.mcp.*` intact, comments and invalid entries included.
- [ ] No secret appears in `config.toml`, the transcript, a notice or `ps`. The read tools refuse the token dirs and the store.
- [ ] On WSL the login pane opens the browser with `wslview`. Without a browser it shows the URL.

## Slices and Size Forecast (non-test lines, each PR under 400)

| Slice | Content | Size |
|---|---|---|
| S1 | Read-deny additions; env values store and `child_env` merge | 200–280 |
| S2 | Wizard-owned append/remove of `[[ai.mcp.servers]]` in `config_edit`, byte-preservation tests | 250–350 |
| S3 | Catalog data and pure checks (64 tools, 24-character collisions, read/write class) | 250–330 |
| S4 | Login pane and browser opener (`xdg-open` / `open` / `wslview`), pre-install runner, probe | 300–380 |
| S5 | Menu "Connect a service" area and wizard steps | 300–380 |
| S6 | README, privacy note, specs | ~40 code + docs |

The total is about 1,350–1,750 lines. The slice order is Q4. One option is to ship Clarity and GA4 first (simplest auth, no restricted scope) to give the analytics area body early.

## Open questions for the user

1. ~~**Lifting the menu rule:**~~ Resolved by C4/C5. may the wizard write `[[ai.mcp.servers]]` entries (append, and remove only its own), keeping every other byte of `ai.mcp.*` as is? Or must it only show a snippet for the user to paste?
2. ~~**Where env values live:**~~ Resolved by C6. a 0600 file per server under `~/.config/lazarobox` (proposed), the OS keyring, or keep "names only" and ask the user to export the values in their shell?
3. ~~**OAuth client:**~~ Resolved by C7. bring-your-own Desktop OAuth client per user (proposed; restricted Gmail scopes make a shared client need Google verification), or another route (service accounts where possible, waiting for Google's hosted servers)?
4. ~~**Slicing order:**~~ Resolved by C8. analytics first (Clarity, GA4, then GSC), Workspace first (Gmail drafts, Docs, Sheets, Apps Script), or infrastructure first as in the table?
5. ~~**Gmail:**~~ Resolved by C9. is "drafts only, at the tool layer" enough, given that the `gmail.compose` token can technically send?
6. ~~**Search Console:**~~ Resolved by C10. accept a service account as the first supported login for mcp-gsc, since its browser flow runs inside a tool call?
7. ~~**Approvals:**~~ Resolved by C11. keep `y` on every connector call until a later change decides how catalog read/write classes relate to `chat-auto-permissions` S2/S4?
