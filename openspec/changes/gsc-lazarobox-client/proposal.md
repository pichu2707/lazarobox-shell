# Proposal: Link Search Console with the lazarobox OAuth Client

Status: proposal, not started. Refs: #733. Builds on `google-connect` (#736).

## Intent

Linking Search Console today needs the user's own Google Cloud project and Desktop OAuth client (the checklist and the `o` form of Connect Google). Non-technical SEO users stop there. This change lets them press **Link with Google**, sign in and accept, with no Google Cloud step, using a Desktop client owned by the `lazarobox` Google Cloud project whose only scope, `webmasters.readonly`, is non-sensitive (no Google verification, no user cap).

**For:** users who want Search Console in the chat and have never opened Google Cloud.

## Scope

### In Scope
- **Fetch on demand:** only when the user links Search Console with the lazarobox client, one HTTPS `GET https://www.javilazaro.es/lazarobox/google-client.json` (the JSON Google downloads, `{"installed": {...}}`).
- **Integrity pin:** an embedded constant, the project number of `lazarobox`; a fetched `client_id` must be `<number>-…apps.googleusercontent.com`. Until the owner fills it, the lazarobox client is unavailable and the screen says so (never guessed).
- **Cache:** `~/.config/lazarobox/lazarobox-google-client.json` (name to confirm in design), mode 0600, written with the secrets writer's handle checks; never the user's `google-oauth-client.json`.
- **Failure path:** fetch error, bad JSON or pin mismatch → a visible notice, and the screen offers "use your own client".
- **Connect Google screen:** with only Search Console ticked, no own-client requirement and no Cloud checklist; the plan sets `[gsc] GSC_OAUTH_CLIENT_SECRETS_FILE` to the cache path. Workspace ticked → exactly as today.
- **Own client stays:** an explicit "use your own client" choice for Search Console (today's `o` form).
- **Docs:** README ("Sin telemetría" network list, Connectors), `docs/conectores.md`, CHANGELOG.

### Out of Scope
- GA4 (`analytics.readonly` is sensitive), Workspace, Gmail, Drive (restricted): none of their scopes join this client.
- The web pages on javilazaro.es (separate repo), app publishing, the blog article.
- Changing users already linked with their own client.
- Any change to `mcp-search-console` 0.4.2.

## Capabilities

### New Capabilities
- `lazarobox-google-client`: when the fetch may run, the URL, curl options, size cap, pin, cache file rules, failure notices, what is sent.

### Modified Capabilities
- `config-menu`: Connect Google requirement (Search-Console-only path, own-client choice, checklist shown only when needed); gsc entry Link wording.
- `mcp-secrets`: Location and ownership gains a second lazarobox-written file (the cache); OAuth client file read back applies to it with the pin.
- `agent-tools`: none expected (the config folder is already denied); design confirms.

## Approach

1. **Pure** (`src/core/ai/mcp_secrets/client.rs` or a sibling): `PROJECT_NUMBER` constant (`Option`, `None` until filled), `pinned(id)`, and the existing `read_client` for parsing.
2. **Fetch** (`src/runtime/`): reuse the `curl` child of `src/runtime/update.rs` / `hf.rs` (no new crate; Cargo.toml has no HTTP client): `--proto =https`, no redirects, 16 KiB cap (`CLIENT_MAX`), no cookies or token.
3. **Cache write**: the secrets writer's folder/handle checks, atomic rename, 0600.
4. **Link trigger**: in `Linker::check` (`src/runtime/mcp/link.rs`), when the plan's file is the cache path, fetch, pin, cache, then the existing `uv` checks and login line. Any other path is untouched, so own-client links keep working.
5. **Planner/screen**: `plan_with` (`src/runtime/secrets_write/google.rs`) stops requiring the own client when only `gsc` is planned on the lazarobox route; `connect.rs` picks the file value; `src/core/menu/connectors/connect.rs` hides the checklist and client line on that route.

## Affected Areas

| Area | Impact |
|------|--------|
| `src/core/ai/mcp_secrets/client.rs` | Pin constant and check |
| `src/runtime/update.rs`, `hf.rs` (pattern), new fetch module | HTTPS GET via `curl` |
| `src/runtime/secrets_write/` | Cache write; plan without own client |
| `src/runtime/mcp/link.rs` | Fetch before the gsc login line |
| `src/core/ai/mcp_catalog/connect.rs`, `entries.rs` | Client route, gsc instructions text |
| `src/core/menu/connectors/connect.rs`, `connectors.rs` | Screen route, own-client choice, notices |
| `src/app.rs`, `src/runtime.rs` | Event for fetch result, if separate from `GscLine` |
| `README.md`, `docs/conectores.md`, `CHANGELOG.md`, `openspec/specs/*` | Docs and specs |

**Size:** about 450-600 production lines. **Slicing (3 PRs):** (1) pin + fetch + cache, with fake-curl tests, not yet reachable; (2) link trigger, planner and screen; (3) docs and specs.

## Risks

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| A spoofed or swapped JSON | Low | HTTPS only, no redirects, project-number pin, size cap |
| Owner rotates/deletes the client; old tokens stop refreshing | Med | Relink fetches the fresh client; notice says to link again |
| App still in Testing: non-test users get `403 access_denied`, tokens expire in 7 days | Certain until published | Merge and release, do not announce until published |
| Network guarantee in README changes | Certain | Update "Sin telemetría" list; state the request carries only the UA (version) and the IP |
| `XDG_CONFIG_HOME` vs the home-relative preset path | Med | Design resolves the cache path from the real config folder |

## Rollback Plan

Revert the chain. Users linked through the lazarobox client keep a working token while the cache file exists; others fall back to "use your own client". The cache file can be deleted by hand.

## Dependencies

- Owner: the `lazarobox` project number (pin), the JSON published at the URL, app publishing before the announcement.
- `mcp-search-console==0.4.2` (`GSC_OAUTH_CLIENT_SECRETS_FILE`), system `curl`.

## Owner Decisions (proposal question round, 2026-11-12)

1. **Refetch policy:** fetch on every Link, so a rotated client is picked up. If the fetch fails but a valid pinned cache exists, use the cache and show a visible notice.
2. **Own client present, Search Console only:** the lazarobox client stays the default even when `google-oauth-client.json` exists; the screen offers an explicit "use my own client" choice.
3. **gsc entry `f` preset:** the single Search Console row defaults to the lazarobox client too (cache path), not only Connect Google.
4. **Before the pin is filled:** the lazarobox route is shown disabled with a notice explaining why (for example `lazarobox client not configured in this build`), and the own-client route is offered. Never hidden, never silent.

## Open Questions

1. The project number of the `lazarobox` Google Cloud project (owner to supply; required before PR 2 is useful). Never guessed.

## Success Criteria

- [ ] With nothing declared and no own client, ticking only Search Console ends linked after `n`, `y`, `i`, `g`, sign-in and accept.
- [ ] A fetched client with a foreign project number, a redirect, non-HTTPS or over 16 KiB is refused with a notice and the own-client offer; nothing is cached.
- [ ] No request to javilazaro.es happens outside the Link action.
- [ ] Users linked with their own client see no change; Workspace ticked behaves as in #736.
- [ ] The cache is 0600, never a symlink, and its secret is never drawn or logged.
