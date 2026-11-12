# Delta for config-menu

Change `gsc-lazarobox-client`. Base: `openspec/specs/config-menu/spec.md`, Requirement `Connect Google [google-connect]`. Fetch, pin, cache and notices are in `lazarobox-google-client`.

## MODIFIED Requirements

### Requirement: Connect Google [google-connect]
(Everything in the base requirement stays as written, except the points below, which replace the matching sentences.)

**Client route.** For Search Console the screen MUST show a client route: `lazarobox client` (the default) or `my own client`. The default is the lazarobox client even when `google-oauth-client.json` reads back. The user MUST be able to choose `my own client` explicitly on the screen (the existing `o` form stays and also selects it); the choice applies to the current screen only and is not stored. The screen MUST draw the route in use (for example `client: lazarobox (Search Console)` or `client: <id>`), never the secret.

**Search Console only.** With only Search Console ticked and not declared, on the lazarobox route: the plan MUST NOT require the own client (the `no client yet: press o ...` pre-check MUST NOT fire), the `client:` line for the own client and the one-time Google Cloud checklist MUST NOT be drawn, and the plan MUST set `GSC_OAUTH_CLIENT_SECRETS_FILE` in `[gsc]` to the cache path (the real config folder's, absolute) in the same write as the other values. The plan MUST NOT contain the own client's id or secret for `gsc` on this route. The cache itself is not written by the plan or by `y`; it is written by Link (lazarobox-google-client, Cache file).

**Own-client route for Search Console.** On `my own client` the screen behaves as in the base requirement: the own client is required (`no client yet: press o ...` when missing), the `client:` line and the Cloud checklist are drawn, and the plan sets `GSC_OAUTH_CLIENT_SECRETS_FILE` to the user's file as today.

**Workspace ticked.** With Google Workspace ticked and not declared, the screen, the pre-checks, the `client:` line, the e-mail requirement, the Cloud checklist and the plan for `google` MUST be exactly as in the base requirement, whatever the Search Console route; Workspace never uses the lazarobox client. With both ticked, the lazarobox route does not apply: both `gsc` and `google` use the own client, the screen requires it, and the route line says the lazarobox client is only for Search Console alone (owner decision, 2026-11-12).

**Pin not filled.** When the build has no pin, the lazarobox route MUST be drawn disabled with the notice `lazarobox client not configured in this build; use your own client`, MUST NOT be selectable, and the own-client route MUST be offered and selected in its place. It MUST NOT be hidden and MUST NOT fail silently.

**Link.** `g` and the Search Console entry's Link, on the lazarobox route, MUST run the fetch of lazarobox-google-client before the `uv` checks; on any failure the screen shows the notice and, without a valid cache, offers the own-client route. Install lines, `Next steps` and the Workspace link are unchanged.

#### Scenario: Search Console only, nothing else declared [gsc-lazarobox-client, auto]
- GIVEN nothing declared, no `google-oauth-client.json`, the pin configured
- WHEN the Connect Google screen opens, `w` unticks Workspace, and `n` is pressed
- THEN no `client missing` notice appears, no Cloud checklist is drawn, and the plan appends `gsc` and sets `GSC_OAUTH_CLIENT_SECRETS_FILE` to the cache path and nothing about the own client

#### Scenario: Neither own client nor checklist [gsc-lazarobox-client, auto, TestBackend + insta]
- GIVEN Search Console only on the lazarobox route
- WHEN the screen and its plan are rendered at 62 columns
- THEN neither shows the Google Cloud checklist or an own-client line, and the popup keeps the size of every other Connectors view

#### Scenario: The lazarobox client wins over an own client [gsc-lazarobox-client, auto]
- GIVEN a readable `google-oauth-client.json` and Search Console only
- WHEN the plan is built without any route choice
- THEN the plan uses the cache path, not the own file

#### Scenario: Explicit own client [gsc-lazarobox-client, auto]
- GIVEN the same state
- WHEN the user chooses `my own client`
- THEN the `client: <id>` line and the checklist are drawn and the plan sets `GSC_OAUTH_CLIENT_SECRETS_FILE` to the own file as in #736

#### Scenario: Workspace ticked is unchanged [gsc-lazarobox-client, auto]
- GIVEN Workspace ticked (alone or with Search Console) and no own client
- WHEN `n` is pressed
- THEN the pre-check is `no client yet: press o to save your client id and secret`, the checklist is drawn, and the Workspace part of the plan is identical to #736

#### Scenario: Pin not filled shows a disabled route [gsc-lazarobox-client, auto]
- GIVEN a build without the pin
- WHEN the screen opens
- THEN the lazarobox route is drawn disabled with `lazarobox client not configured in this build; use your own client`, is not selectable, and `my own client` is selected

#### Scenario: A declared gsc is left alone [gsc-lazarobox-client, auto]
- GIVEN `gsc` declared and its value set to the user's own file
- WHEN the plan is built
- THEN `gsc` is listed under the services left alone and its value is kept

#### Scenario: Link failure offers the own client [gsc-lazarobox-client, auto]
- GIVEN the lazarobox route, no cache, a failing fetch
- WHEN `g` is pressed
- THEN the notice of the failure is shown, no `uv` check starts, and the own-client route is offered

### Requirement: gsc entry Link and `f` preset [gsc-lazarobox-client]
The single Search Console row (the catalog entry's `f` preset, the one-row form) MUST default to the lazarobox client as Connect Google does: `GSC_OAUTH_CLIENT_SECRETS_FILE` is preset to the cache path, even when `google-oauth-client.json` exists, and the entry's instruction text MUST say that Link fetches the lazarobox client, and how to use your own client instead. With the pin not filled, the preset MUST NOT use the cache path, the row MUST say `lazarobox client not configured in this build` and offer the own-client value. A value already held for `GSC_OAUTH_CLIENT_SECRETS_FILE` MUST be kept, never overwritten by the preset. Link on that entry follows the same fetch, fallback and notices as above.

#### Scenario: Preset uses the cache path [gsc-lazarobox-client, auto]
- GIVEN no value held and the pin configured
- WHEN the `gsc` entry's `f` preset is applied
- THEN `GSC_OAUTH_CLIENT_SECRETS_FILE` is the cache path under the real config folder, also when `XDG_CONFIG_HOME` is set

#### Scenario: An existing value is kept [gsc-lazarobox-client, auto]
- GIVEN `[gsc]` already holds a path
- WHEN the preset is applied
- THEN the held value is shown and kept
