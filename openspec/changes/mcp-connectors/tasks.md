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

- [ ] 2.1 RED/GREEN: catalog integrity (ids, order, groups, commands, `https://` links) (mcp-catalog, Entries are data).
- [ ] 2.2 RED/GREEN: match by name; non-catalog servers untouched (Declared entries are matched by name).
- [ ] 2.3 RED/GREEN: class lookup, unknown tool is `write`; a `read` tool with `readOnlyHint` still asks `y` in `auto` (Read/write class is recorded only).
- [ ] 2.4 RED/GREEN: no first-run download in any run command (Pre-installed binary).
- [ ] 2.5 RED/GREEN: `clarity` and `ga4` entries: snippet, secrets template, no `--allow-write`, guide warnings (Clarity entry, GA4 entry).
- [ ] 2.6 RED/GREEN: pure checks: more than 64 tools, collisions after the 24-character cut, tools not in the catalog.

## S3 Probe, install pane, login pane (C2, C7) — 230–330 lines

- [ ] 3.1 RED/GREEN: probe with a fake server: success line, timeout after 20 s, server reaped within 3 s, `secrets.toml` values used, no value in the report (mcp-catalog, Probe).
- [ ] 3.2 RED/GREEN: install and login: nothing without `y`, a new pane with the full command typed visibly (mcp-connect, Install and login run in a visible pane).
- [ ] 3.3 RED/GREEN: guide links through `src/core/opener.rs` and the existing `MenuCommand::Open` (src/app.rs:2536-2543); no opener copies and shows the link (Browser opener).
- [ ] 3.4 [design-confirm] `BROWSER` in the login pane when unset.

## S4 Connectors row and area; README for Clarity and GA4 (C1, C4, C6, C7) — 300–380 lines

- [ ] 4.1 RED/GREEN: `Connectors` row, the only row of the `Connectors` menu tab (menu.rs:772-778), `<N> declared` or `none`, opens the area, no config edit; the tab is now visible and reached by Tab and `4`, and the tests that need it hidden use a test table (config-menu, Connectors row, Sections and items are data, Menu tabs, Jump keys).
- [ ] 4.2 RED/GREEN: entry list groups, order and states (`not declared`, `disabled`, `values missing: …`, `declared`, last probe) (mcp-connect, Entry list).
- [ ] 4.3 RED/GREEN: entry screen: guide, snippet that parses back, secrets template, paths, actions; probe offered only when declared; values never drawn; a long screen scrolls inside the fixed 12-row body (Entry screen, The area writes no configuration).
- [ ] 4.4 RED/GREEN: probe from the area: `y` prompt, footer, Esc stops it (Probe from the area).
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
