# Proposal: Connect Google in One Action

Status: proposal, not started. Refs: issue to open.

## Intent

Connecting a Google service today takes, per service: paste an `[[ai.mcp.servers]]` entry into `config.toml` by hand, fill its values with `f`, install with `i`, link with `g`. Search Console and Google Workspace (Gmail, Docs, Sheets) both run on the same Desktop OAuth client, yet the user types its id and secret again for Workspace, because nothing reads back `~/.config/lazarobox/google-oauth-client.json`. A user who linked Search Console still cannot reach Gmail or Sheets without repeating most of the setup.

This change adds one **Connect Google** action in the Connectors tab. It reuses the client the user already has, asks only for what lazarobox cannot know (the Google e-mail), writes the entries and values itself, installs the pinned servers and runs both links.

**For:** anyone who wants Search Console, Gmail, Docs and Sheets in the chat, after creating their Google Cloud client once.

## Scope

### In Scope
- **A `Connect Google` row** in the Connectors list (group `Google`), with its own screen.
- **Services:** Search Console (`gsc`, OAuth route) and Google Workspace (`google`: Gmail read + drafts, Docs, Sheets). Each is a checkbox, both on by default; a service already declared is shown as `already declared` and left alone.
- **One-time Google Cloud checklist** on the screen, with links that open with `1`-`9`: create the project, enable the APIs (Search Console, Gmail, Docs, Sheets, Drive), consent screen with the user as test user (and the advice to publish it, so tokens do not expire after 7 days), Desktop OAuth client.
- **Client:** the existing `google-oauth-client.json`. When it is missing, the screen offers the `o` form (today offered only on GA4 and GSC). A new reader parses its `client_id` and `client_secret` with the same checks the form applies (`valid_id`, plain printable text).
- **E-mail:** one field, `USER_GOOGLE_EMAIL`, asked once.
- **Plan screen before any write:** the exact entries that will be appended to `config.toml`, the `secrets.toml` names that will be set (values never shown; the secret masked), and the install lines. Nothing is written until `y`.
- **Writes, after `y`:**
  - `secrets.toml` through the existing hardened writer: `[gsc]` the OAuth presets (`GSC_OAUTH_CLIENT_SECRETS_FILE`, `GSC_CONFIG_DIR`, `GSC_SKIP_OAUTH = "false"`); `[google]` `GOOGLE_OAUTH_CLIENT_ID`, `GOOGLE_OAUTH_CLIENT_SECRET` (copied from the client file), `USER_GOOGLE_EMAIL`, `WORKSPACE_MCP_DISABLE_LOCAL_FILES = "true"`. A name that already has a value keeps it; the plan screen says `kept`.
  - `config.toml`: the catalog `snippet()` of each selected service, **appended** at the end of the file (see Approach). Existing bytes are never changed.
- **Install:** the pinned `uv tool install` lines of the selected services, in one new tab, as `i` does today.
- **Link:** once the pinned versions are installed, `g` on the screen runs the Search Console link and then the Workspace link, the same two flows `g` runs today. Two Google consents remain: each server keeps its own token.
- **Restart notice:** chats load MCP servers at startup, so the screen ends with the existing `restart lazarobox to use it in chats`. Links work before the restart (they already use the disk copy).

### Out of Scope
- **A lazarobox-owned OAuth client** ("click and accept" with no Google Cloud step). Gmail and Drive scopes are restricted: Google requires app verification and a yearly security assessment (CASA), and an unverified app is capped at 100 users with a warning screen. Not this change.
- **Creating the Google Cloud project, enabling APIs or creating the client automatically** (for example with `gcloud`). The checklist links to each page instead.
- **GA4 and Apps Script.** GA4 logs in through `gcloud` and installs with `pipx`; Apps Script through `clasp`. They can join the same screen later as extra checkboxes.
- **Hot reload of MCP servers in running chats.**
- **Editing or removing an existing entry.** The action only appends entries for services that are not declared.

## Capabilities

### Modified Capabilities
- `config-persistence`: "MCP server lists are never written" becomes "the menu never changes or removes an `ai.mcp.servers` entry; the only write is Connect Google appending catalog entries for undeclared services".
- `configuration`: `ai.mcp.servers` is edited by hand **or** appended by Connect Google.
- `mcp-secrets`: a new caller of the existing writer (several names in one save), and the client file gains a reader.
- `mcp-catalog`: the `Connect Google` group and the checklist links.
- `config-menu`: the new row, screen, keys and notices.

## Approach

1. **Pure planner** (`src/core/ai/mcp_catalog/connect.rs`): from the disk copy of `config.toml` (servers only), the secrets presence, the client file reading and the chosen services, it returns a `Plan`: entries to append, secrets to set or keep, install lines, and the refusals. Fully unit-tested, no I/O.
2. **Client reader** (`src/core/ai/mcp_secrets/client.rs`): parse `{"installed": {client_id, client_secret}}` (also the `web`-less JSON Google downloads), bounded size, same validation as the form. The runtime opens the file with the same handle checks as the secrets writer (owner, not a symlink, not group/world readable).
3. **Append writer for `config.toml`** (`src/runtime/config_append.rs`), built on the secrets writer's safety model instead of `save_config`'s: open the folder and file by handle, refuse a symlink, a file not owned by the user, a hard link or one over a size bound; keep the file's mode; write `config.toml.bak` first; new bytes = old bytes, a newline if missing, then the snippets; **re-parse the result** and refuse unless it parses, lists every old server unchanged and adds exactly the new ones (this catches an inline `ai.mcp.servers = [...]` array, which a `[[...]]` header cannot extend); refuse with `Changed` if the file changed since the plan was shown; atomic rename. On refusal, the screen falls back to today's flow: it shows the snippet to paste (`c` copies it).
4. **Menu screen** in `src/core/menu/connectors.rs` (with a new submodule if the file grows): checklist → client → e-mail → plan → `y` → writes → install tab → wait for `uv tool list` to show the pinned versions → `g` link both → restart notice. Every step reuses existing effects (`SaveSecret`/`SaveClient`, `Connector(line)`, `LinkGsc`, `LinkGoogle`, `LoadPresence`), plus one new `Effect::AppendServers`.
5. **Docs:** README (the two guarantee sentences at ~1281 and ~1318, the "Conectar un servicio" section), `docs/conectores.md` ("Ruta rápida" becomes the Connect Google path; the per-service steps stay as the manual route), CHANGELOG.

About four chained PRs: (1) client reader + planner; (2) append writer; (3) screen and wiring; (4) docs and specs.

## Affected Areas

| Area | Impact |
|------|--------|
| `src/core/ai/mcp_catalog/` (new `connect.rs`) | Planner, Google group, checklist links |
| `src/core/ai/mcp_secrets/client.rs` | Client file reader |
| `src/runtime/` (new `config_append.rs`) | Hardened append writer with `.bak` and re-parse |
| `src/runtime/secrets_write.rs` | Several names in one save |
| `src/core/menu/connectors.rs`, `connectors/fields.rs` | Row, screen, `o` offered from the new screen |
| `src/app.rs`, `src/runtime.rs` | `Effect::AppendServers` and its event |
| `src/core/config_edit.rs` tests, `connectors_fields_tests.rs` | The "never writes `ai.mcp`" tests change to "never changes an existing entry; only the append writer writes, only from Connect Google" |
| `README.md`, `docs/conectores.md`, `CHANGELOG.md`, `openspec/specs/*` | Docs and specs |

## Risks

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| The append corrupts or reorders `config.toml` | Low | Bytes-prefix append, `.bak`, re-parse check that every old server is unchanged, `Changed` refusal, atomic rename; tests over CRLF, BOM, no trailing newline, comments, inline arrays |
| A model or tool triggers the write | Low | Only the menu screen builds `AppendServers`, after `y`; a source-scan test pins it to that file, as today's tests do for `SaveSecret` |
| The client secret is copied into `secrets.toml` | Accepted | It lives in two `0600` files the read tools already deny; the plan says so. Alternative rejected: injecting it at server start from the client file adds a second secret source to the runtime |
| Gmail tools beyond the allowlist | Low | The entry comes from the catalog with `name = "google"`, which is what the allowlist and the hidden tools key on |
| Users still stuck in the Google Cloud step | Medium | The checklist with direct links and the 403 `access_denied` / test-user hint in `docs/conectores.md` troubleshooting |
| Documented guarantee changes | Certain | Specs, README and guide updated in the same chain; CHANGELOG states the new write under Changed |

## Rollback Plan

Revert the chain. Entries already appended stay in the user's `config.toml` as ordinary hand-written entries, and `config.toml.bak` holds the file before the append.

## Dependencies

- `mcp-search-console==0.4.2` and `workspace-mcp==2.4.1`, as pinned today.
- The BROWSER fix for the Search Console link on WSL (#732).

## Open Questions

1. Whether one Google Cloud link can enable all five APIs at once (`console.cloud.google.com/flows/enableapi?apiid=...` with a comma list); verify before writing the checklist, else one link per API.
2. The key for the row's screen actions: reuse `g` for "link both", or a new key, given `g` already means "link this service".

## Success Criteria

- [ ] With only `google-oauth-client.json` present and nothing declared, Connect Google ends with `gsc` and `google` declared, values set, both pinned servers installed and both linked, after the user typed only the e-mail and `y`/`g`.
- [ ] An existing `config.toml` keeps every byte before the appended entries; `config.toml.bak` equals the old file.
- [ ] A service already declared, in any form, is never touched.
- [ ] An inline `ai.mcp.servers = [...]` array, a symlinked or foreign-owned `config.toml`, or a file changed after the plan is refused with a notice and the paste fallback.
- [ ] `p` reports `mcp: started gsc (N tools)` and `mcp: started google (41 tools)` after a restart.
