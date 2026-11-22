# Delta for mcp-secrets

Change `gsc-lazarobox-client`. Base: `openspec/specs/mcp-secrets/spec.md`.

## MODIFIED Requirements

### Requirement: Location and ownership [S1]
(Base text stays, with one sentence added at its end.) lazarobox also writes one file besides `secrets.toml` and the files named in the base: the lazarobox client cache, `lazarobox-google-client.json` in the same config folder (name confirmed in design), only by Link on the lazarobox route (lazarobox-google-client, Cache file). It is written by the secrets writer's folder and handle checks, mode 0600, atomically, and never replaces `secrets.toml`, `config.toml` or `google-oauth-client.json`. `secrets.toml` holds only the cache's path (`[gsc] GSC_OAUTH_CLIENT_SECRETS_FILE`), never its content.

#### Scenario: Link writes only the cache [gsc-lazarobox-client, auto]
- GIVEN a config folder with `secrets.toml` and `google-oauth-client.json`
- WHEN Link on the lazarobox route succeeds
- THEN `lazarobox-google-client.json` exists with mode 0600, and `secrets.toml`, `config.toml` and `google-oauth-client.json` are byte-identical with the same mode

#### Scenario: Read-only everywhere else [gsc-lazarobox-client, auto]
- GIVEN no Link on the lazarobox route
- WHEN servers start and stop and the menu opens, saves and closes
- THEN the cache is not created, changed or deleted

### Requirement: OAuth client file read back [google-connect]
(Base text stays.) The same reader, with the same open-handle judgment (not a symlink, owned by the user, one link, no group or other permission bit, at most 16 KiB, printable ASCII, id ending in `.apps.googleusercontent.com`, `installed` type), MUST read the lazarobox client cache, with one addition: the cache MUST also pass the integrity pin (lazarobox-google-client, Integrity pin), so an id of another project is refused. A missing cache is `None`, not an error. Its secret is never drawn, logged or put in a notice, and its refusals are named `lazarobox-google-client.json: <reason>` (the same reasons as the own file's). `google-oauth-client.json` is never read as the cache and the cache is never read as `google-oauth-client.json`.

#### Scenario: Cache read back with the pin [gsc-lazarobox-client, auto]
- GIVEN a cache holding a Desktop client of the pinned project, mode 0600
- WHEN it is read back
- THEN the id and secret are returned and no notice is shown

#### Scenario: Cache of a foreign project [gsc-lazarobox-client, auto]
- GIVEN a cache whose id is of another project number
- WHEN it is read back
- THEN it is refused with `lazarobox-google-client.json: not from the lazarobox project` and the secret is in no notice

#### Scenario: Loose cache [gsc-lazarobox-client, auto]
- GIVEN the cache with mode 0644, or a symlink, or a hard link
- WHEN it is read back
- THEN it is refused with `lazarobox-google-client.json: mode must be 0600 (chmod 600 <path>)` or the symlink or hard-link notice, and no notice holds the secret

#### Scenario: The two files are separate [gsc-lazarobox-client, auto]
- GIVEN both files present and valid with different ids
- WHEN each is read back
- THEN each returns its own id and neither read changes the other
