# Delta for config-menu

Change `gsc-lazarobox-client`. Base: `openspec/specs/config-menu/spec.md`, Requirement `Connect Google [google-connect]`. Fetch, pin, cache and notices are in `lazarobox-google-client`.

## MODIFIED Requirements

### Requirement: Connect Google [google-connect]
(Everything in the base requirement stays as written, except the points below, which replace the matching sentences.)

**Client route.** For Search Console the screen MUST show a client route: `lazarobox client` (the default) or `my own client`. The default is the lazarobox client even when `google-oauth-client.json` reads back. The user MUST be able to choose `my own client` explicitly on the screen: `u` toggles the route and the existing `o` form stays and also selects it, but only when the client file is saved (a cancelled form, a declined replace prompt or a refused write changes nothing); the choice applies to the current screen only and is not stored. The screen MUST draw the route in use: `route: lazarobox client (u uses your own client)` with `client: lazarobox (fetched from www.javilazaro.es when you link)`, or `route: my own client (u uses the lazarobox client)` with `client: <id>`; never the secret. `u` without Search Console ticked says `u picks the client of Search Console; f on its entry changes it`.

**Value kept in `secrets.toml`.** When Search Console is ticked and `secrets.toml` already holds `GSC_OAUTH_CLIENT_SECRETS_FILE`, that value wins and the route follows it: the screen draws `route: <route>` and `kept from secrets.toml: lazarobox client|your own client; f on the entry changes it`, and `u` shows that same line instead of toggling.

**Search Console only.** With only Search Console ticked and not declared, on the lazarobox route: the plan MUST NOT require the own client (the `no client yet: press o ...` pre-check MUST NOT fire), the `client:` line for the own client and the one-time Google Cloud checklist MUST NOT be drawn, and the plan MUST set `GSC_OAUTH_CLIENT_SECRETS_FILE` in `[gsc]` to the cache path (the real config folder's, absolute) in the same write as the other values. The plan MUST NOT contain the own client's id or secret for `gsc` on this route. The cache itself is not written by the plan or by `y`; it is written by Link (lazarobox-google-client, Cache file).

**Own-client route for Search Console.** On `my own client` the screen behaves as in the base requirement: the own client is required (`no client yet: press o ...` when missing), the `client:` line and the Cloud checklist are drawn, and the plan sets `GSC_OAUTH_CLIENT_SECRETS_FILE` to the user's file as today.

**Workspace ticked.** With Google Workspace ticked and not declared, the screen, the pre-checks, the `client:` line, the e-mail requirement, the Cloud checklist and the plan for `google` MUST be exactly as in the base requirement, whatever the Search Console route; Workspace never uses the lazarobox client. With both ticked, the lazarobox route does not apply: both `gsc` and `google` use the own client, the screen requires it, and the route line and the `u` key say `the lazarobox client is only for Search Console alone; with Workspace ticked your own client is used` (owner decision, 2026-11-12).

**Pin not filled.** When the build has no pin, the lazarobox route MUST be drawn disabled with the line `lazarobox client not configured in this build; this screen uses your own client` (also the text of `u`), MUST NOT be selectable, and the own-client route MUST be offered and selected in its place (`route: my own client`). It MUST NOT be hidden and MUST NOT fail silently.

**Link.** `g` and the Search Console entry's Link, on the lazarobox route, MUST run the fetch of lazarobox-google-client before the `uv` checks; on any failure the footer shows the failure notice of lazarobox-google-client (instruction first: `own client: Esc, f on gsc · lazarobox: <cause>`) and, without a valid cache, no `uv` check runs. Install lines, `Next steps` and the Workspace link are unchanged.

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
- THEN the lazarobox route is drawn disabled with `lazarobox client not configured in this build; this screen uses your own client`, is not selectable, and `my own client` is selected

#### Scenario: A declared gsc is left alone [gsc-lazarobox-client, auto]
- GIVEN `gsc` declared and its value set to the user's own file
- WHEN the plan is built
- THEN `gsc` is listed under the services left alone and its value is kept

#### Scenario: Link failure offers the own client [gsc-lazarobox-client, auto]
- GIVEN the lazarobox route, no cache, a failing fetch
- WHEN `g` is pressed
- THEN the failure notice is shown with the instruction first, and no `uv` check starts

### Requirement: gsc entry Link and `f` preset [gsc-lazarobox-client]
The Search Console client row of the `f` screen MUST show which client the held value is (`lazarobox client`, `your own client` or `not set`) and, in a `client file:` line under the values, what Enter does: `Enter writes the lazarobox client` when nothing is held and the pin is configured (the value is the cache path, even when `google-oauth-client.json` exists); `Enter writes your own client` when the pin is configured and there is no cache path to offer; with the pin not filled, `lazarobox client not configured in this build; Enter writes your own client`, and the cache path is never offered. When the held value is the cache path, Enter switches to the own client (after the replace prompt) and the line says `Enter switches to your own client; then Esc, g links`, preceded by `o saves your own client first; ` when no own client file exists. The entry's instruction text MUST say that Link fetches the lazarobox client and how to use your own client instead. A held value is never overwritten without that explicit Enter and the replace prompt. Link on that entry follows the same fetch, fallback and notices as above, with the `own client: f` prefix.

#### Scenario: Enter writes the cache path [gsc-lazarobox-client, auto]
- GIVEN no value held and the pin configured
- WHEN Enter is pressed on the `gsc` client row
- THEN `GSC_OAUTH_CLIENT_SECRETS_FILE` is the cache path under the real config folder, also when `XDG_CONFIG_HOME` is set

#### Scenario: An existing value is kept [gsc-lazarobox-client, auto]
- GIVEN `[gsc]` already holds a path
- WHEN the row is shown
- THEN the held value is shown and kept until Enter and the replace prompt
