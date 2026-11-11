# Tasks: MCP connectors

Order follows C8 (secrets and read-deny, then Clarity and GA4, then GSC, then Workspace). Each slice is one PR under about 400 production lines (tests and docs may exceed; the size is an advisory heuristic). Every RED/GREEN task writes the failing test first through the public interface, then the code. Every slice keeps `y` on every connector call (C11) and writes no byte of `ai.mcp.*` (C4).

## S1 Secrets file and read-deny (C6, C8) — 230–310 lines

- [x] 1.1 RED/GREEN: `secrets.toml` location (XDG and default), missing file is silent (mcp-secrets, Location and ownership).
- [x] 1.2 RED/GREEN: mode 0600 check, not a regular file, 64 KiB bound (Mode 0600, Read on each start).
- [x] 1.3 RED/GREEN: format: per-server tables, string values, bad names and types skipped alone, top-level values reported, unknown tables ignored; syntax error reported by line only, no value in any problem (Format, Problems without values).
- [x] 1.4 RED/GREEN: `child_env` merge: only names in `env`, file wins over the host, host fallback, forbidden names dropped, a value reaches only its own server, file read at each start (mcp-servers, Lifecycle and Secrets and privacy). The lookup becomes per server: today one closure serves every server (src/runtime/chat/external.rs:146 through `ExternalSet::start`, src/runtime/mcp/external.rs:97-133, to `McpClient::spawn`, spawn.rs:106-110).
- [x] 1.5 RED/GREEN: `.google_workspace_mcp`, `.clasprc.json`, `.config/mcp-gsc` refused and omitted from listings; `secrets.toml` refused (agent-tools, Path policy). `SECRET_DIRS` (paths.rs:24-34) is shared with the write check and the git tools (paths.rs:346; git.rs:207, 240): their existing tests must still pass.
- [x] 1.6 Characterization: an entry naming a host variable with no `secrets.toml` behaves as before; the menu save tests at config_edit.rs:1395, 1543, 1567, 1661 pass unchanged.
- [x] 1.7 Docs: README "Tus propios servidores MCP" (README.md:1244-1281) documents `secrets.toml` (place, 0600, format, never written by lazarobox); the read-deny paragraph (README.md:753) lists the three token locations; privacy note.

## S2 Catalog core with Clarity and GA4 (C2, C8, C11) — 220–300 lines

- [x] 2.1 RED/GREEN: catalog integrity (ids, order, groups, commands, `https://` links) (mcp-catalog, Entries are data). As built: `Entry` data in `src/core/ai/mcp_catalog.rs`, `ENTRIES` in `mcp_catalog/entries.rs`. S2 holds `clarity`, `ga4` only, so the test checks that order and `Analytics`; `gsc`, `google`, `apps-script` extend it in S5/S6. Link check uses `opener::is_web_url`; id validity is checked by parsing the snippet with `Config::parse` (no problem).
- [x] 2.2 RED/GREEN: match by name; non-catalog servers untouched (Declared entries are matched by name). As built: `entry(name)` exact equality; `admits(&McpServer, tool)` returns true for everything (no flag yet).
- [x] 2.3 RED/GREEN: class lookup, unknown tool is `write`; a `read` tool with `readOnlyHint` still asks `y` in `auto` (Read/write class is recorded only). As built: `Entry::class`/`class(server, tool)`, unknown or non-entry is `Write`. The `y` test calls `approval::ask_for` on `Call::External` for a `read` tool on both localities (`ask_for` has no mode argument, so `ask`/`auto`/`run` cannot differ). Nothing in approval was touched.
- [x] 2.4 RED/GREEN: no first-run download in any run command (Pre-installed binary). As built: test over `command`, `install` and the login line (`uvx`, `npx`, `pnpm`, `pipx run`, `dlx`).
- [x] 2.5 RED/GREEN: `clarity` and `ga4` entries: snippet, secrets template, no `--allow-write`, guide warnings (Clarity entry, GA4 entry). As built: snippet omits empty `args`/`env`; template is `None` without secret vars. Clarity tools: `query-analytics-dashboard`, `list-session-recordings`, `query-documentation-resources`. GA4 login uses the full `https://www.googleapis.com/auth/...` scope URLs of the analytics-mcp README (U: not run against gcloud here), client file `~/.config/lazarobox/google-oauth-client.json`. Clarity limits (10 requests per project per day, last 1 to 3 days, 1,000 rows, no pagination) confirmed against learn.microsoft.com/en-us/clarity/setup-and-installation/clarity-data-export-api. GA4 lists all 13 upstream tools (README checked). Binary names `clarity-mcp-server`, `analytics-mcp` stay U until task 6.4.
- [x] 2.6 RED/GREEN: pure checks: more than 64 tools, collisions after the 24-character cut, tools not in the catalog. As built: `checks::{report, too_many, collisions, unknown}`; collisions use `servers::qualified_name` and a name that cannot be qualified gets its own line; lines sanitize and cap names; `MAX_TOOLS` moved to `tools/servers.rs` (client imports it).

## S3 Probe, install pane, login pane (C2, C7) — 230–330 lines

- [ ] 3.1 RED/GREEN: probe with a fake server: success line, timeout after 20 s, server reaped within 3 s, `secrets.toml` values used, no value in the report (mcp-catalog, Probe).
- [ ] 3.2 RED/GREEN (App side only): a `MenuCommand::Connector` creates exactly one new shell pane and writes exactly the catalog line followed by Enter (D19), once the pane's first output arrives (fallback 2 s); the menu closes; no `WritePty` and no pane without the command. The menu-level `y`/`n` tests live in 4.4 (mcp-connect, Install and login run in a visible pane).
- [ ] 3.3 RED/GREEN: guide links through `src/core/opener.rs` and the existing `MenuCommand::Open` (src/app.rs:2536-2543); no opener copies and shows the link (Browser opener).
- [x] 3.4 Resolved by D18 (owner, 2026-11-11): lazarobox never sets `BROWSER`; Linux and macOS use the platform opener of gcloud and clasp, and the link in the login output stays visible in the pane. No code; 3.2 also asserts the pane env is untouched. WSL handling is deferred (see Later items).

## S4 Connectors row and area; README for Clarity and GA4 (C1, C4, C6, C7) — 300–380 lines

S4 may split the entry screen (the entry part of `connectors.rs`, tasks 4.3-4.4b) from the list (4.1-4.2) into its own PR when its production lines pass 400.

- [ ] 4.1 RED/GREEN: `Connectors` row, the only row of the `Connectors` menu tab (menu.rs:772-778), `<N> declared` or `none`, opens the area, no config edit; the tab is now visible and reached by Tab and `4`, and the tests that need it hidden use a test table (config-menu, Connectors row, Sections and items are data, Menu tabs, Jump keys).
- [ ] 4.2 RED/GREEN: entry list groups, order and states (`not declared`, `disabled`, `values missing: …`, `declared`, last probe) (mcp-connect, Entry list).
- [ ] 4.3 RED/GREEN: entry screen: guide, snippet that parses back, secrets template, paths, actions; probe offered only when declared; values never drawn; a long screen scrolls inside the popup body, whose height is computed by `view_body()` (menu.rs:863; at least 13 rows today) and never changes: do not hard-code a row count (Entry screen, The area writes no configuration).
- [ ] 4.4 RED/GREEN: `y`/`n` prompts in the area: install and login create no pane and type nothing on `n`, Esc or any other key; on `y` the prompt's full command is what 3.2 writes, Enter included, with no second confirmation (D19); the prompt says unsaved menu changes are discarded when the draft is dirty (mcp-connect, Install and login run in a visible pane). Then the probe: `y` prompt, footer, Esc stops it (Probe from the area).
- [ ] 4.4b RED/GREEN: config re-read (D20): opening the area reads `config.toml` from disk (`[[ai.mcp.servers]]` only); states and the probe use that copy; when it differs from the list loaded at startup the area shows `restart lazarobox to use it in chats`; no byte of either file is written (The area writes no configuration).
- [ ] 4.5 Docs: README "Conectar un servicio" for Clarity and GA4 (guide, values, what is sent where, how to disconnect; claude/codex/gemini only on jailed turns, local engines always); CHANGELOG.
- [ ] 4.6 [manual] Live: Clarity with a real token, GA4 with a real property.

## S5 Search Console with a service account (C10) — 80–150 lines

- [ ] 5.1 RED/GREEN: `gsc` entry: `GSC_CREDENTIALS_PATH`, `GSC_SKIP_OAUTH = "true"`, no `GSC_ALLOW_DESTRUCTIVE`, guide steps (mcp-catalog, GSC entry with a service account).
- [ ] 5.2 Docs: README for GSC.
- [ ] 5.3 [manual] Live: a query on a property the service account was added to.

## S6 Workspace: Gmail drafts, Docs, Sheets, Apps Script (C9, C12) — 170–270 lines

- [ ] 6.1 RED/GREEN: `google` entry: `--permissions gmail:readonly+drafts`, client id and secret in `secrets.toml`, drafts and residual-risk notes in the guide (Google Workspace entry, drafts only).
- [ ] 6.2 RED/GREEN: Gmail tool filter: `send_gmail_message` and unknown Gmail tools not registered, filter before the 64-tool cap, which `list_tools` applies today (client.rs:191), other servers untouched (Gmail tool filter).
- [ ] 6.3 RED/GREEN: `apps-script` entry: `clasp mcp`, `clasp login`, no `env`, tool classes (Apps Script entry).
- [ ] 6.4 Verify (U) and pin the workspace-mcp pre-authorization in the login pane and the installed binary names; probe confirms the tool count stays under 64.
- [ ] 6.5 Docs: README for Google and Apps Script, including that replies go to Drafts and are sent from Gmail.
- [ ] 6.6 [manual] Live: a draft after `y`; `push_files` after `y`.

## S7 (later) Profile default entries (C4, C12)

Not specced. Blocked on open question 8 of the proposal (mechanism and the prompt-profiles rule "A profile changes ONLY the prompts").

## S8 (later, undecided) Gmail send with an explicit permission (C12)

Not specced. Blocked on open question 9 of the proposal.

## Later items (not in S2-S6)

- L1 WSL login handling (deferred by D18, 2026-11-11): decide whether to set `BROWSER` (for example to `wslview`, only when unset) so gcloud opens the Windows browser; needs a manual check on WSL. Until then the link printed in the login pane is the fallback.
- L2 Hot reload of `[[ai.mcp.servers]]` into running chats when the area closes (candidate follow-up of D20): larger change in the chat runtime.
