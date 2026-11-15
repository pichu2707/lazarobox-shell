# mcp-secrets Specification

## Purpose

The values a user MCP server needs (tokens, OAuth client id and secret, project ids, credential file paths) live in a lazarobox-owned file that the user edits, mode 0600, and each value reaches only its own server (C6). The servers themselves stay declared in `config.toml` `[[ai.mcp.servers]]` (C4, C5); how a value reaches a server is in mcp-servers (Lifecycle, Secrets and privacy), and the read tools refuse the file (agent-tools, Path policy). Slice tag: [S1]. Scenario tags: [auto] = `cargo test` (temp config folder, fake server child); [manual] = real machine. Items marked "assumption" are proposals, not user decisions.

Terms: "the file" = `secrets.toml` in the lazarobox config folder. "Table" = a top-level TOML table of the file. "Server table" = the table whose name equals a server's `name`.

## Requirements

### Requirement: Location and ownership [S1]
The file MUST be `secrets.toml` in the lazarobox config folder, the folder of `config.toml` (see configuration: `$XDG_CONFIG_HOME/lazarobox/`, else `$HOME/.config/lazarobox/`), so its default place is `~/.config/lazarobox/secrets.toml` (C6). It is edited by the user. lazarobox MUST NEVER create, write, rename, delete or change the mode of the file, from the menu or anywhere else, except by the menu save of one value (openspec change `mcp-connectors`, Menu save) and by Connect Google's save of the values its plan lists, in one write with one `secrets.toml.bak` (config-menu, Connect Google), both after the user's confirmation. A missing file is not an error, produces no notice, and leaves every server with the values from lazarobox's own environment only. No value from the file is ever written to `config.toml`.

#### Scenario: Default place [auto]
- GIVEN `XDG_CONFIG_HOME` unset and `HOME=/h`
- WHEN the file is looked up
- THEN its path is `/h/.config/lazarobox/secrets.toml`

#### Scenario: XDG place [auto]
- GIVEN `XDG_CONFIG_HOME=/x`
- WHEN the file is looked up
- THEN its path is `/x/lazarobox/secrets.toml`

#### Scenario: Missing file [auto]
- GIVEN no `secrets.toml` and an entry naming `GITHUB_TOKEN` set in lazarobox's environment
- WHEN the server is started
- THEN it gets the host value and no notice mentions `secrets.toml`

#### Scenario: Never written [auto]
- GIVEN a config folder with and without a `secrets.toml`
- WHEN servers start and stop, the menu opens, saves and closes, and the app quits
- THEN no `secrets.toml` is created, an existing one is byte-identical with the same mode, and `config.toml` holds none of its values

### Requirement: Mode 0600 [S1]
The file MUST be used only when it is a regular file with mode 0600 or stricter, that is with no group or other permission bit set (C6). Otherwise (assumption) it MUST NOT be read at all, every server starts as if it were missing, and the problem `secrets.toml: mode must be 0600 (chmod 600 <path>)` or `secrets.toml: not a regular file` is reported (see Problems without values). The path shown is sanitized.

#### Scenario: 0600 is used [auto]
- GIVEN a file with mode 0600 holding `[clarity]` `CLARITY_API_TOKEN = "tok"`
- WHEN `clarity` is started
- THEN it gets `tok` and no problem is reported

#### Scenario: Readable by others is not used [auto]
- GIVEN the same file with mode 0644
- WHEN `clarity` is started
- THEN it does not get `tok`, and the problem is `secrets.toml: mode must be 0600 (chmod 600 <path>)`

#### Scenario: Directory in its place [auto]
- GIVEN a directory named `secrets.toml`
- WHEN a server is started
- THEN the problem is `secrets.toml: not a regular file` and the server starts with host values only

### Requirement: Format [S1]
The file MUST be TOML. Each table is named after a server `name` and holds that server's values: each key is an environment variable name (an identifier of at most 64 bytes, the same rule as `env` names in configuration) and each value a string. A value MUST reach only the server whose `name` equals its table, and only for a name that server's entry lists in `env` (see mcp-servers). A project entry and a global entry with the same `name` use the same table (assumption). Names that mcp-servers never passes (`LD_*`, `DYLD_*`, `HOME`, the session token) are ignored. A key whose value is not a string, or whose name is not a valid identifier, MUST be skipped alone and reported; a top-level key that is not a table MUST be skipped and reported; the other keys and tables are kept. Unknown tables (no server of that name) are ignored without a notice. Example:

```toml
[clarity]
CLARITY_API_TOKEN = "..."

[ga4]
GOOGLE_CLOUD_PROJECT = "my-project"
```

#### Scenario: Two tables [auto]
- GIVEN the example file and entries `clarity` and `ga4`, each naming its variable
- WHEN both are started
- THEN each gets its own value and not the other's

#### Scenario: Wrong value type [auto]
- GIVEN `[ga4]` holding `GOOGLE_CLOUD_PROJECT = 5` and `X = "ok"`
- WHEN `ga4` (naming both) is started
- THEN it gets `X` only, and the problem is `secrets.toml: ga4.GOOGLE_CLOUD_PROJECT: expected a string`

#### Scenario: Bad name [auto]
- GIVEN `[ga4]` holding `"BAD NAME" = "v"`
- WHEN the file is read
- THEN the key is skipped and the problem is `secrets.toml: ga4."BAD NAME": expected a variable name`

#### Scenario: Top-level value [auto]
- GIVEN a top-level `TOKEN = "v"` next to a valid `[clarity]`
- WHEN the file is read
- THEN `[clarity]` is used and the problem is `secrets.toml: TOKEN: expected a table`

#### Scenario: Unknown table [auto]
- GIVEN `[old-server]` with no server of that name
- WHEN the file is read
- THEN no problem is reported and no server gets its values

### Requirement: Read on each start [S1]
The file MUST be read when a user server is started (assumption: not only at app start), so an edit applies at the next start without restarting lazarobox. The read MUST be bounded (assumption: at most 64 KiB; a larger file is not used and reported as `secrets.toml: larger than 64 KiB`), off the UI thread, and MUST NOT block on a special file.

#### Scenario: Edit applies on the next start [auto]
- GIVEN `clarity` started with value `a`, then stopped, and the file edited to `b`
- WHEN `clarity` is started again
- THEN it gets `b`

#### Scenario: Oversized file [auto]
- GIVEN a 65 KiB file
- WHEN a server is started
- THEN the file is not used and the problem is `secrets.toml: larger than 64 KiB`

### Requirement: Problems without values [S1]
A problem with the file MUST be shown as a sanitized system line `mcp: <problem>` in the chat that starts a server, once per start (the Connectors area of the mcp-connectors change adds a second place when it ships). A problem MUST name at most the table and the key, never a value. A TOML syntax error MUST be reported as `secrets.toml: not valid TOML (line <N>)` without the parser's own message or any text of the line (assumption: the parser message can quote a value). Values MUST NOT be logged, drawn in the transcript or sent to a model.

#### Scenario: Syntax error hides the line [auto]
- GIVEN a file whose line 3 reads `CLARITY_API_TOKEN = "sk-123`
- WHEN a server is started
- THEN the line is `mcp: secrets.toml: not valid TOML (line 3)` and `sk-123` appears in no notice, transcript line or log

#### Scenario: A problem does not stop the start [auto]
- GIVEN a file with one bad key and one good key for `clarity`
- WHEN `clarity` is started
- THEN it starts with the good value and the chat shows the problem once

### Requirement: OAuth client file read back [google-connect]
`google-oauth-client.json` in the config folder MUST be readable back for Connect Google: the file the client form writes, and the JSON Google downloads for a Desktop client (`installed.client_id`, `installed.client_secret`, other keys ignored). It MUST be opened in the config folder judged as the secrets writer judges it (not a symlink, owned by the user, not writable by group or others) and judged on the open handle: a regular file, owned by the user, with one link, with no group or other permission bit, at most 16 KiB. The id MUST end in `.apps.googleusercontent.com` and both values MUST be printable ASCII, as the form checks them. A missing file (or a missing config folder, which is not created) is `None`, not an error. The secret MUST NEVER be drawn, logged or put in a notice; only the id is shown (`client: <id>`). Copying the id and the secret into `[google]` of `secrets.toml` happens only through Connect Google's plan and `y`.

#### Scenario: The form's file and Google's download read back [google-connect, auto]
- GIVEN `google-oauth-client.json` written by the form, then replaced by the JSON Google downloads with mode 0400
- WHEN it is read back
- THEN both give the same id and secret

#### Scenario: A loose or foreign file is refused [google-connect, auto]
- GIVEN the file with mode 0644, or a symlink to a good file, or a hard link to one, or owned by another user
- WHEN it is read back
- THEN it is refused with `google-oauth-client.json: mode must be 0600 (chmod 600 <path>)`, or the symlink, hard link or owner notice, and no notice holds the secret

#### Scenario: Not a Desktop client [google-connect, auto]
- GIVEN a file with only a `web` client
- WHEN it is read back
- THEN the notice is `google-oauth-client.json is not a Desktop OAuth client; save it again (o)`

### Requirement: Several values in one save [google-connect]
Connect Google's save MUST set every value of its plan in one write of `secrets.toml`, by the rules of the menu save of one value (each key keeps its place and comment, a missing key goes at the end of its table, a missing table at the end of the file), with one `secrets.toml.bak` generation, and the first refusal MUST write nothing. A value that already holds something MUST be kept, and the plan MUST say so (`Kept in secrets.toml`).

#### Scenario: All or nothing [google-connect, auto]
- GIVEN a plan with seven values, one of them for a key that holds a number in the file
- WHEN it is saved
- THEN the file is byte-identical and the notice names that key
