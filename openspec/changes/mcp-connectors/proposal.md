# Proposal: MCP Connectors (a curated catalog, a secrets file and a guided Connectors area for analytics and Google Workspace)

## Intent

The user asked (2026-10-28, verbatim):

> "las conexiones de MCP por ejemplo Goole para leer correos y poder escribir y dejar en borrador, google docs para subir código en apps script, sheets, gsc, analytics, clarity, y de alguna manera podamos hacer esas conexiones fáciles al usuario y así vamos también dando ya algo de cuerpo a la parte de analítica de la shell"

lazarobox can already run the user's own MCP servers (`[[ai.mcp.servers]]`, README.md:697-734). But today a user must:

- find a server,
- install it,
- create OAuth credentials,
- log in from a terminal,
- export env vars in the shell that launches lazarobox,
- hand-write the `config.toml` entry.

For Google services this is several pages of setup per service.

This change keeps the servers declared by the user in `config.toml` (C4, C5) and makes the rest easy:

- a **curated catalog** of existing MCP servers, with the exact entry to declare, the install and login commands, a step-by-step guide and the read/write class of each tool;
- a **secrets file** `~/.config/lazarobox/secrets.toml` (mode 0600) that gives each server its own values, so no `export` is needed (C6);
- a **Connectors area** in the menu that shows the guide, runs the install and the login in a visible pane, opens the browser, shows the entry for the user to paste and probes the server. It never writes `ai.mcp.*` (C4) and never writes the secrets file (C6).

It covers two blocks, analytics first (C8):

- **Analytics:** Microsoft Clarity, Google Analytics 4, Google Search Console.
- **Google Workspace:** Gmail (read, and create drafts, never send; C9), Docs, Sheets, Apps Script (push project code).

The analytics block is the first real content of the shell's analytics area (C3).

## Decisions (user, 2026-10-28 and 2026-10-29, settled)

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
- **C12 Gmail is common to both profiles (2026-10-29).** User: "también dentro de los mcp que podemos usar común para programadores y analistas el mcp o api para el correo empezamos con gmail para poder conectar y trabajar con sistemas de agentes locales para lectura y redacción de correos de respuesta subiendo a borrador a no ser que se de algún permiso, pero puede ser mejora o bien apuntarlo ya". Mail starts with Gmail and is a default entry for both the `programmer` and the `analyst` profile (not analyst-only). Purpose: read mail and draft replies with local agent systems (local engines included). Replies go to Drafts by default. Sending only with an explicit user permission is a LATER improvement, not in this change's first slices: C9 (drafts only, never send) stays for the shipped slices, and the send permission is a future slice whose mechanism (which permission, how it is granted, whether each send still asks `y`) is undecided (see open question 9).
- **C13 Per-server approval level (2026-10-29).** User: "lo que tenemos que dar la opción de los MCP que se conectan poder dar permisos unitarios a cada uno, es decir, si al de gmail le puedo no dar permiso pero si a GSC"; to the levels question the user chose "Preguntar / Solo lecturas / Todo". Each connected server gets its own level: **ask** (default; `y` on every call, as C11), **reads** (tools the catalog classes as read run unasked, write tools ask), **all** (every tool runs unasked). Example: `gsc` on reads, `google` (Gmail) on ask. C13 refines C11: the default stays `y` on every call, and relaxing is now per server instead of a later global rule. Proposal, not decided: an optional `approval = "ask" | "reads" | "all"` key per `[[ai.mcp.servers]]` entry (additive, so the frozen format still reads old files); a tool the catalog does not list counts as write; a server's own `readOnlyHint` stays ignored; the level applies in chat and orchestrator turns alike, never to workers (they get no user server tools); the C9 Gmail drafts-only limit is a tool filter and holds at every level.

The C8 sentence "The slice table below predates C4 and is rewritten when the spec is written" is done: the table in "Slices and Size Forecast" follows C4-C12. All seven original open questions are resolved; two later-slice questions are listed at the end.

## Facts this proposal rests on

V = verified against the linked source on 2026-10-28 (2026-10-29 for the items marked so). U = unverified. Code references are on `origin/main` 191dc1c.

### lazarobox today

- **Transport.** User servers are stdio only. The client spawns the command with piped stdin and stdout (src/runtime/mcp/client/spawn.rs:111-121), and `src/runtime/mcp/` has no HTTP or SSE client. The protocol is `2025-06-18`, with `2025-03-26` and `2024-11-05` accepted (src/core/ai/tools/mcp.rs:43-46).
- **Environment.**
  - The env is cleared. The server then gets `PATH, USER, LANG, LC_ALL, TMPDIR`, then only the variable NAMES listed in `env` (spawn.rs:44, 72-90).
  - `HOME` is forced to the real home. `LD_*` and `DYLD_*` names and the session token are dropped (spawn.rs:65, 84).
  - The cwd is `$HOME` (spawn.rs:116).
  - The config accepts names only, never values (src/core/ai/config.rs:295-308, 970-978). This stays: values come from `secrets.toml` (C6), never from `config.toml`.
  - No `DISPLAY`, `BROWSER`, `WAYLAND_DISPLAY` or `XDG_*` reaches the server, so a server that opens a browser itself fails. Logins therefore run in a pane (see Login helper).
- **Not jailed.** The server is trusted and runs outside the jail (spawn.rs:8-12), so it can write its token dirs under `$HOME`.
- **Engines.** With claude, codex and gemini, user servers are started and offered only when the CLI turn runs jailed. Ollama and llama.cpp always get them (src/runtime/chat.rs:633; README.md:734). So the local agent use of C12 (local engines reading mail and drafting replies) already has the tools.
- **Timeouts.** The handshake (`initialize` plus `tools/list`) has 20 s; one call has 15 min (src/runtime/mcp/client.rs:73-79).
- **Caps.**
  - At most 64 tools per server; the first 64 are kept (client.rs:51, 191).
  - Tool descriptions 1 KiB and schemas 16 KiB (client.rs:53-55).
  - Call arguments 4 KiB (src/core/ai/tools/mod.rs:584).
  - Results 32 KiB, sanitized and nonce-wrapped as untrusted (src/core/ai/tools/label.rs:4, 21-31).
- **Names.** Each part of `<server>__<tool>` is lowercased, characters outside `[a-z0-9_-]` become `_`, and the part is cut at 24 characters (src/core/ai/tools/servers.rs:45-62). A collision keeps the first tool and drops the later one with a notice (servers.rs:134-160).
- **Approval.** Every call to a user server tool needs `y`, also in `auto`, and `readOnlyHint` is ignored (README.md:730; src/runtime/mcp/external.rs:6-8; spec `agent-tools/specs/mcp-servers/spec.md:100-101`; `coding-tools/specs/agent-tools/spec.md:94`: "Tools from a user MCP server MUST always be write tools"). C11 keeps it.
- **Stdout.** A stdout line that is not JSON is skipped silently (client.rs:299-300). A login URL printed to stdout never reaches the user, and the call waits until its timeout.
- **Menu.** The menu never writes `ai.mcp.*`, and every save preserves those bytes exactly (src/core/config.rs:85; `agent-tools/specs/config-persistence/spec.md:73-74`; tests at src/core/config_edit.rs:1356, 1505, 1528). C4 keeps it.
- **Read-deny list.** The read tools refuse `~/.config/gcloud` and `~/.config/lazarobox` (src/core/ai/tools/paths.rs:23-33), and the runtime also passes lazarobox's real config folder (honouring `XDG_CONFIG_HOME`) as an extra root (`agent-tools/specs/agent-tools/spec.md:96`). So `secrets.toml` in the config folder is already refused. They do NOT refuse `~/.google_workspace_mcp`, `~/.clasprc.json` or `~/.config/mcp-gsc` (`git grep` on main finds none of them).
- **Config folder.** `config.toml` is read from `$XDG_CONFIG_HOME/lazarobox/`, else `$HOME/.config/lazarobox/` (openspec/specs/configuration/spec.md:10). (V 2026-10-29)
- **Browser opener.** A pure opener module already exists: `open` on macOS; `wslview`, then `rundll32.exe`, then `xdg-open` on WSL; `xdg-open` elsewhere; only one-line `https://` links are handed to it (src/core/opener.rs:5-14, 26-30). The Hugging Face row uses it and copies the link when no opener works (`hf-download/specs/config-menu/spec.md:69-70`). (V 2026-10-29)
- **AI tab.** Its last row today is `Hugging Face` (`hf-download/specs/config-menu/spec.md:69-70`; src/core/menu.rs:264). (V 2026-10-29)
- **Profiles.** There are exactly two profiles, `analyst` and `programmer`, and "A profile changes ONLY the prompts: no engine, model, key, layout or behaviour changes with it" (openspec/specs/prompt-profiles/spec.md:5). Default entries per profile (C4, C12) therefore need a change to that rule. (V 2026-10-29)

### The services

- **workspace-mcp** ([taylorwilsdon/google_workspace_mcp](https://github.com/taylorwilsdon/google_workspace_mcp)): community, MIT, about 3.4k stars, v2.2.1 released 2026-10-27 (V).
  - Gmail, Docs, Sheets and Apps Script (and more) in one server, with about 120 tools in total (V, README).
  - Runs over stdio or streamable HTTP, with `uvx workspace-mcp` (V).
  - Needs the user's own OAuth client in `GOOGLE_OAUTH_CLIENT_ID` and `GOOGLE_OAUTH_CLIENT_SECRET` (V).
  - Tokens go to `~/.google_workspace_mcp/credentials` (dir 0700, files 0600), or to `WORKSPACE_MCP_CREDENTIALS_DIR` (V, `auth/credential_store.py`).
  - `--permissions gmail:readonly+drafts` grants only `gmail.readonly` and `gmail.compose`. `send_gmail_message` needs `gmail.send`, so the scope filter hides it (V, `auth/permissions.py`, `gmail/gmail_tools.py`). `draft_gmail_message` only calls `drafts().create` (V).
  - `--permissions` cannot be combined with `--tools` or `--read-only` (V, `main.py`). `--disabled-tools` removes named tools (V, README). Whether `--disabled-tools` combines with `--permissions` is U.
  - In stdio mode it tries `webbrowser.open`. When that fails, it returns the authorization URL in the tool result and listens for the callback on `localhost:8000` (V, `auth/google_auth.py`, `auth/oauth_callback_server.py`). How to pre-authorize it outside an MCP call is U.
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
  - Auth is either the user's own Desktop OAuth client (`GSC_OAUTH_CLIENT_SECRETS_FILE`) or a service account (`GSC_CREDENTIALS_PATH` plus `GSC_SKIP_OAUTH=true`) (V). C10 picks the service account first.
  - It always requests the full `webmasters` scope (V, `gsc_server.py:106`).
  - The token goes to `~/.config/mcp-gsc/token.json`, or to `GSC_CONFIG_DIR`, written with the default umask (V).
  - The OAuth flow (`run_local_server(port=0)`) runs inside the first tool call (V). This is why OAuth waits (C10).
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
  - `gmail.readonly` and `gmail.compose` are **restricted**, and `gmail.compose` "manage drafts and send emails". The token can send even when no tool exposes sending (V). This is the residual risk C9 accepts.
  - Restricted scopes need restricted-scope verification, and storing or transmitting that data on servers needs a security assessment (V). C7 avoids it with a per-user client.
- **OAuth tokens** ([OAuth 2.0](https://developers.google.com/identity/protocols/oauth2)).
  - An external-type consent screen in "Testing" status gets refresh tokens that expire in 7 days (V).
  - There is a limit of 100 refresh tokens per Google Account per client ID (V).
  - For installed apps, "the client secret is obviously not treated as a secret" (V).
  - The 100-user cap for unverified apps is U.

## Scope

### In Scope

1. **Secrets file (C6).** `secrets.toml` in the lazarobox config folder (default `~/.config/lazarobox/secrets.toml`), mode 0600, edited by the user. One table per server name; each key is an env name, each value a string. A name the server entry lists in `env` takes its value from that server's table; a value never reaches another server, `config.toml`, a log, the transcript or a model. lazarobox never writes the file.
2. **Read-deny additions (C6, C8).** `~/.google_workspace_mcp`, `~/.clasprc.json` and `~/.config/mcp-gsc` join the read tools' secret list. `secrets.toml` is already covered by `.config/lazarobox` and the config-folder extra root.
3. **Catalog (C2, C11).** A pure data module in core with one entry per service: id (the server name to declare), the server and its pinned install command, the run command and its narrowing flags, the env names and which of them belong in `secrets.toml`, the login command (if any), the guide steps and links, the token location, and a read/write class for each known tool. The class is recorded only; it never changes approval (C11).
4. **Gmail tool filter (C9).** For the `google` entry, lazarobox registers only the Gmail tools the catalog lists as read or draft tools; any other Gmail tool (`send_gmail_message` included) is not registered, even if the server lists it.
5. **Login helper.** Login commands run in a real lazarobox pane with a TTY, started from the Connectors area after `y`: `gcloud auth application-default login ...`, `clasp login`, and (once verified) the pre-auth for workspace-mcp. Guide links open with the existing opener (src/core/opener.rs); with no working opener the link is copied or shown as text. Logins never run inside a tool call, where lazarobox cannot show them (client.rs:299-300).
6. **Pre-install and the 20 s handshake.** Entries run a pre-installed binary (`uv tool install`, `pipx install`, `npm i -g`), never a first-run `uvx`, `npx` or `pipx run` download, which could exceed the 20 s handshake (client.rs:76). The install command runs in a visible pane after `y`. How long a first download takes is U.
7. **Probe.** From the Connectors area, after `y`, lazarobox starts one declared server outside any chat turn with the normal spawn rules, waits for `initialize` and `tools/list` within the 20 s handshake, reports the result, checks that the server lists at most 64 tools (client.rs:51) and that no two tools collide after the 24-character cut (servers.rs:45-62), and stops it.
8. **Connectors area (C1, C2, C4).** A `Connectors` row at the end of the AI tab opens a list grouped as Analytics then Workspace. Each entry screen shows the guide, the `[[ai.mcp.servers]]` entry to paste into `config.toml`, the `secrets.toml` table to fill, the state (declared or not, secrets present or not, last probe), and the actions install, log in and probe. It writes neither `config.toml` nor `secrets.toml`.
9. **Entries, in C8 order.** Clarity and GA4; then GSC with a service account (C10); then Workspace: `google` (Gmail drafts only, Docs, Sheets; C9) and `apps-script` (clasp).
10. **Docs.** The README section "Tus propios servidores MCP" gets a "Conectar un servicio" part per entry (guide, values, what is sent where, how to disconnect). The specs and the privacy note are updated.
11. **Later slices (listed, not specced here).** Profile default entries (C4, C12) and a Gmail send permission (C12). See "Later slices".

### Out of Scope

- Home-grown connectors, or any Google or Microsoft API code in lazarobox (C2).
- Any write to `ai.mcp.*` or to `secrets.toml` by the menu (C4, C6).
- A new config format, a separate `mcp.json`, or new keys in `[[ai.mcp.servers]]` (C5).
- Streamable-HTTP transport and the Google-hosted Workspace servers (Developer Preview, HTTP only). They are a possible later change, once lazarobox has an HTTP and OAuth 2.1 client and the servers leave preview.
- Dashboards, charts, scheduled reports or any analytics UI beyond connecting the servers.
- Changing the `y`-per-call rule for user server tools (C11). See "Interaction with `chat-auto-permissions`".
- A lazarobox-owned, Google-verified OAuth client (C7).
- Sending email in the shipped slices (C9, C12); the later send permission is undecided.
- GSC OAuth login, until a pane pre-login is verified with mcp-gsc (C10).
- Apps Script `run_script_function` or deployments.
- Other services (Calendar, Drive as its own entry, Slides and so on). The catalog is built to add them later.

## Catalog entries (recommended)

The `env` column lists the names the entry declares; every value lives in that server's `secrets.toml` table (C6).

| Order | Id (server name) | Server | Install → run | Auth (`env` names) | Narrowing | Tools (R = read, W = write) | Fit on main |
|---|---|---|---|---|---|---|---|
| 1 | `clarity` | clarity-mcp-server | `npm i -g @microsoft/clarity-mcp-server` → `clarity-mcp-server` (V) | Project-admin API token: `CLARITY_API_TOKEN` | none (3 tools) | R: all three (dashboard query, session recordings, docs search; exact names U) | Fits. The guide warns about 10 requests per project per day |
| 2 | `ga4` | analytics-mcp | `pipx install analytics-mcp` → `analytics-mcp` (binary name U; V for `pipx run analytics-mcp`) | ADC by `gcloud auth application-default login --scopes analytics.readonly,cloud-platform --client-id-file=<own client>` in the login pane (C7); `GOOGLE_CLOUD_PROJECT` | no `--allow-write` | R: `run_report`, `run_funnel_report`, `run_realtime_report`, account and property info. W: `create_property_annotation` (not exposed without `--allow-write`) | Fits: ADC is read from `~/.config/gcloud` via the forced `HOME` |
| 3 | `gsc` | mcp-gsc | `uv tool install mcp-search-console` (binary name U) | Service account (C10): `GSC_CREDENTIALS_PATH`, `GSC_SKIP_OAUTH` (`true`). No login step | `GSC_ALLOW_DESTRUCTIVE` never declared | R: analytics queries, URL inspection, sitemap lists. W: `manage_sitemaps` (submit/delete) | Fits with the service account; OAuth later (C10) |
| 4 | `google` | workspace-mcp | `uv tool install workspace-mcp` → `workspace-mcp --single-user --permissions gmail:readonly+drafts docs:full sheets:full` (V for the flags, U for the binary name) | User's own Desktop OAuth client (C7): `GOOGLE_OAUTH_CLIENT_ID`, `GOOGLE_OAUTH_CLIENT_SECRET`. Scopes `gmail.readonly`, `gmail.compose` (restricted), `documents`, `spreadsheets`, `drive.file`, `drive.readonly` | `--permissions` as shown, plus the lazarobox Gmail tool filter (C9). Whether this stays under 64 tools is U; the probe checks it | R: `search_gmail_messages`, `get_gmail_message_content`, `get_gmail_thread_content`, `get_doc_content`, Sheets reads. W: `draft_gmail_message`, `modify_doc_text`, `batch_update_doc`, Sheets writes, label edits | Works once tokens exist. Pre-auth outside a chat is needed (end-to-end behaviour U) |
| 5 | `apps-script` | clasp | `npm i -g @google/clasp` → `clasp mcp` (V) | clasp's built-in OAuth client; `clasp login` in the login pane (V); the user switches on the Apps Script API (U). No `env` | none (5 tools) | R: `list_projects`, `pull_files` (writes local files), `clone_project`. W: `push_files`, `create_project` | Fits: pushes from disk, needs no secret value |

**One Workspace server or one per service.** One workspace-mcp entry for Gmail, Docs and Sheets means one OAuth client, one login and one token store, and its permission levels give "drafts, never send" at the tool layer. Apps Script goes to clasp instead of workspace-mcp's `manage_script_content`, because code passed in arguments hits the 4 KiB cap (tools/mod.rs:584) and clasp needs no user-created OAuth client.

**Assumptions in the catalog (not user decisions).**

- A declared server is matched to a catalog entry by its `name` (for example `name = "clarity"`). This avoids a new key in the frozen format (C5).
- A tool the catalog does not list gets class W (the safe default). Class never changes approval (C11).
- The guide recommends saving downloaded credential files (the OAuth client JSON for GA4, the GSC service-account key) inside the lazarobox config folder, for example `~/.config/lazarobox/google-oauth-client.json` and `~/.config/lazarobox/gsc-service-account.json`, so the read tools already refuse them.
- A Gmail tool is a tool whose name contains `gmail`; the filter keeps only the catalog's Gmail R tools and `draft_gmail_message`.

## Interaction with `chat-auto-permissions`

The in-flight feature `chat-auto-permissions` (odd/tasks/chat-auto-permissions.md, branch `feat/chat-auto-permissions`) changes approvals in `approval = "run"`:

- **S2:** reads inside the chat root run unasked.
- **S4:** writes ask, and a full grant lasts for the current conversation only.

Per C11 this change ships with `y` on every connector call, as today, and the spec keeps "Tools from a user MCP server MUST always be write tools" (`coding-tools/specs/agent-tools/spec.md:94`). The catalog records each tool's class so a later change can decide how classes meet `run` mode once S4 lands. A server's own `readOnlyHint` stays ignored.

## Capabilities

### New Capabilities

- `mcp-secrets`: the secrets file (location, mode, format, per-server lookup, errors without values, never written by lazarobox).
- `mcp-catalog`: the entries and their fields, the read/write class, the Gmail tool filter, pre-install, the probe and its checks, and the entries Clarity, GA4, GSC, `google` and `apps-script`.
- `mcp-connect`: the Connectors area, the guide, the snippet to paste, the login pane, the browser opener and the rule that the area writes no config.

### Modified Capabilities

All deltas apply on top of the unarchived `agent-tools` change specs, where these capabilities live; `agent-tools` MUST be archived first.

- `mcp-servers`: "Lifecycle" and "Secrets and privacy" take values from `secrets.toml` as well as from lazarobox's own environment. The `LD_`/`DYLD_`, `HOME` and token rules (spawn.rs:65-90) stay.
- `agent-tools`: "Path policy" gains `.google_workspace_mcp`, `.clasprc.json` and `.config/mcp-gsc`.
- `config-menu`: the AI tab gains the `Connectors` row after `Hugging Face`.

Not modified: `config-persistence` ("MCP server lists are never written" stays, C4) and `configuration` (the `[[ai.mcp.servers]]` schema stays, C5).

## Approach

1. **Secrets first (C8).** `secrets.toml` is read when a server starts, parsed into per-server tables, and merged in `child_env` only for the names the entry lists in `env`, after the same filters (`LD_*`, `DYLD_*`, `HOME`, the session token). Values are never logged, drawn or sent to a model. lazarobox never writes the file; the README and the Connectors area show the table to fill.
2. **Read-deny.** Add the three token locations to `SECRET_DIRS` (paths.rs:23-33), with tests.
3. **Catalog.** Pure data plus pure checks in `src/core/ai/`, tested without processes: entry fields, tool classes, the Gmail filter, the 64-tool and 24-character collision checks.
4. **Probe.** It reuses `ExternalSet::start`/`shutdown` (external.rs:97-132, 199-209) on one server, outside any chat turn, and reports the `mcp: started` line or the `unavailable` notice plus the catalog checks.
5. **Login and install.** Run as visible commands in a new pane after `y`. Nothing installs or logs in hidden. Links open through `src/core/opener.rs`.
6. **Connectors area.** It lives in the menu (`src/core/menu.rs`, `src/ui/`). It reads `config.toml`, `secrets.toml` (only which names have a value, never the values) and the catalog, and writes nothing.
7. **Docs.** The README and specs say that with claude, codex and gemini the connected tools appear only on jailed turns, and that local engines always get them (chat.rs:633).

## Affected Areas

| Area | Impact |
|---|---|
| `src/runtime/mcp/client/spawn.rs` (`child_env`) | Modified (merge `secrets.toml` values, same filters) |
| `src/core/ai/` new secrets parser and catalog modules | New |
| `src/core/ai/tools/servers.rs` (registration) | Modified (Gmail tool filter for the `google` entry) |
| `src/core/ai/tools/paths.rs:23-33` | Modified (token locations) |
| `src/runtime/` probe, login pane and install pane | New |
| `src/core/menu.rs`, `src/ui/components/` | Modified / New (Connectors row and screens) |
| `src/core/opener.rs` | Reused, unchanged |
| `src/core/config.rs:85`, `src/core/config_edit.rs` | Unchanged (C4) |
| README.md:697-734, privacy note, specs | Modified |

## Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| Gmail scopes are restricted: a shared lazarobox OAuth client would need restricted-scope verification and possibly a security assessment (V) | Certain if shared | Bring-your-own Desktop OAuth client per user (C7), with a step-by-step guide |
| BYO client in "Testing" status: refresh tokens expire after 7 days for external-type consent screens (V) | High for personal Gmail | The guide warns about it (C7). The Connectors area offers the login again. Publishing the user's own app (unverified-app warning) is the user's choice (implications U) |
| Prompt injection from email, Docs or Search Console content steering the model (V, warned by every vendor) | High | Results are already untrusted-wrapped and capped (label.rs). `y` on every call (C11). Gmail is drafts-only at the tool layer (C9) |
| `gmail.compose` can send at the API level even though no tool sends (V) | Med | Accepted residual risk (C9). `--permissions gmail:readonly+drafts` plus the lazarobox Gmail filter. Documented as a tool-layer guarantee, not a token guarantee |
| A secret value leaks into `config.toml`, a notice, a log or a model | Low | Values only in `secrets.toml` (C6); parse errors name the table and key, never the value; tests check notices and logs |
| `secrets.toml` readable by other users | Low | Mode 0600 (C6). Assumption: a file with group or other bits set is not used and a notice says why |
| Apps Script push is code execution outside the jail: pushed code runs with the script's scopes, and clasp's login includes `script.webapp.deploy` and `cloud-platform` (V) | Med | Every `push_files` asks `y` and shows the project dir. No `run_script_function` or deployment tools are cataloged. The README states the risk |
| Wide default scopes: mcp-gsc always takes full `webmasters`, and workspace-mcp's `complete` tier includes send and sharing (V) | Med | Narrowing flags in each entry. `GSC_ALLOW_DESTRUCTIVE` never declared. The probe reports an unexpected tool list |
| Experimental or preview servers (clasp MCP, analytics-mcp) change under us (V) | Med | Pinned versions in the catalog. The probe from the Connectors area |
| An OAuth URL printed to stdout is swallowed and the call hangs up to 15 min (client.rs:299-300) | High for mcp-gsc OAuth | Login only in the login pane, never inside a tool call. GSC starts with a service account (C10) |
| Token files readable by the model through the read tools | Med | Add them to `SECRET_DIRS` in the first slice (C8) |
| The user pastes a wrong entry (typo in `name` or `env`) | Med | The Connectors area shows the exact entry and its state ("declared", "secrets missing"); the probe reports `unavailable` with a short reason |
| Clarity's 10 requests per day are used up by a chatty model | Med | Guide warning; `y` per call |

## Rollback Plan

- Each slice reverts on its own.
- Entries are ordinary `[[ai.mcp.servers]]` tables written by the user. Older builds read them, but without `secrets.toml` support their values are missing and the server reports `unavailable`. Exporting the variables in the shell restores the old behaviour.
- `secrets.toml` is a separate file and can be deleted. No migration.

## Dependencies

- `agent-tools` archived before this change (the deltas target its specs).
- `chat-auto-permissions` (in flight). Not a blocker for shipping; a blocker for any relaxation of `y` based on catalog classes (C11).
- Main 191dc1c or later (`[[ai.mcp.servers]]`, the external client and `src/core/opener.rs` are merged).

## Success Criteria

- [ ] `cargo test --locked`, `cargo clippy --all-targets --locked -- -D warnings` and `cargo fmt --all -- --check` pass on every slice.
- [ ] A user connects Clarity by pasting the shown entry into `config.toml` and the token into `secrets.toml`, with no `export` in the shell, and the probe shows `mcp: started clarity`.
- [ ] GA4 connects after its guide and the login pane; `run_report` returns capped, wrapped results.
- [ ] GSC connects with a service account and a query returns capped, wrapped results.
- [ ] `draft_gmail_message` creates a draft after `y`, and no send tool is registered.
- [ ] `push_files` uploads an on-disk Apps Script project after `y`.
- [ ] No menu action changes a byte of `config.toml` or `secrets.toml`.
- [ ] No secret appears in `config.toml`, the transcript, a notice, a log or `ps`. The read tools refuse `secrets.toml` and the token locations.
- [ ] Every connector call asks `y`, whatever its catalog class.
- [ ] On WSL the guide links open with `wslview`. With no opener the link is copied or shown.

## Slices and Size Forecast (production lines, each PR under 400; tests and docs may exceed)

| Slice | Content | Decisions | Size |
|---|---|---|---|
| S1 | `secrets.toml` parser, mode check, `child_env` merge; read-deny additions | C6, C8 | 200–280 |
| S2 | Catalog core (fields, classes, pure checks, name match) with the `clarity` and `ga4` entries | C2, C8, C11 | 220–300 |
| S3 | Probe, install pane and login pane (opener reuse) | C2, C7 | 250–350 |
| S4 | Menu `Connectors` row and screens; README for Clarity and GA4 | C1, C4, C6, C7 | 300–380 |
| S5 | `gsc` entry with a service account, its guide and README | C10 | 80–150 |
| S6 | Workspace: `google` entry and Gmail tool filter, `apps-script` entry, guides and README | C9, C12 | 150–250 |
| S7 (later) | Profile default entries: `analyst` gets `clarity`, `ga4`, `gsc`; both profiles get `google` for Gmail | C4, C12 | 150–250 (U) |
| S8 (later, undecided) | Gmail send with an explicit user permission | C12 | U |
| S9 | Per-server approval level (`ask` / `reads` / `all`), read/write class from the catalog, menu shows the level per entry | C11, C13 | 120–200 (U) |

The total for S1-S6 is about 1,200–1,710 lines. S1-S6 are specced in `specs/`; S7 and S8 are specced when their open questions are answered.

## Later slices

### S7 Profile default entries (C4 "más adelante", C12)

C4 says the `analyst` profile ships default entries (GA4, GSC, Clarity), and C12 makes Gmail a default entry for both `programmer` and `analyst`. Because the menu never writes `ai.mcp.*` (C4), the defaults cannot reach a user by being written into `config.toml`.

**Proposed mechanism (assumption, not a user decision):**

- The catalog marks which entries are defaults for which profile: `clarity`, `ga4`, `gsc` for `analyst`; `google` for both.
- At server resolution, the defaults of the active profile are added in memory to the global list, as if declared, with the catalog's command, args and `env` names. Nothing is written.
- A user `[[ai.mcp.servers]]` entry with the same name replaces the default; one with `enabled = false` turns it off.
- A default whose `secrets.toml` table is missing, or whose binary is not installed, is not started and gives no `unavailable` line in chats (only the Connectors area shows it as "not connected"), so an unconnected default is silent.
- This needs a change to the prompt-profiles rule "A profile changes ONLY the prompts" (openspec/specs/prompt-profiles/spec.md:5).

### S8 Gmail send with an explicit permission (C12)

C12 lists sending as a later improvement: replies go to Drafts unless a permission is given. Which permission, how it is granted, and whether each send still asks `y` are undecided. Until then C9 holds: no send tool is registered.

## Open questions for the user

1. ~~**Lifting the menu rule:**~~ Resolved by C4/C5. may the wizard write `[[ai.mcp.servers]]` entries (append, and remove only its own), keeping every other byte of `ai.mcp.*` as is? Or must it only show a snippet for the user to paste?
2. ~~**Where env values live:**~~ Resolved by C6. a 0600 file per server under `~/.config/lazarobox` (proposed), the OS keyring, or keep "names only" and ask the user to export the values in their shell?
3. ~~**OAuth client:**~~ Resolved by C7. bring-your-own Desktop OAuth client per user (proposed; restricted Gmail scopes make a shared client need Google verification), or another route (service accounts where possible, waiting for Google's hosted servers)?
4. ~~**Slicing order:**~~ Resolved by C8. analytics first (Clarity, GA4, then GSC), Workspace first (Gmail drafts, Docs, Sheets, Apps Script), or infrastructure first as in the table?
5. ~~**Gmail:**~~ Resolved by C9. is "drafts only, at the tool layer" enough, given that the `gmail.compose` token can technically send?
6. ~~**Search Console:**~~ Resolved by C10. accept a service account as the first supported login for mcp-gsc, since its browser flow runs inside a tool call?
7. ~~**Approvals:**~~ Resolved by C11. keep `y` on every connector call until a later change decides how catalog read/write classes relate to `chat-auto-permissions` S2/S4?
8. **OPEN (blocks S7 only) — How profile defaults reach a user.** Is the proposed mechanism right: defaults added in memory by the active profile, overridden or disabled by a same-named `[[ai.mcp.servers]]` entry, silent until connected, and a change to "A profile changes ONLY the prompts"?
9. **OPEN (blocks S8 only) — Gmail send permission (C12).** Which permission allows a send, how the user grants it, and whether each send still asks `y`.
