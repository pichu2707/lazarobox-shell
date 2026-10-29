# mcp-secrets Specification

## Purpose

The values a user MCP server needs (tokens, OAuth client id and secret, project ids, credential file paths) live in a lazarobox-owned file that the user edits, mode 0600, and each value reaches only its own server (C6). The servers themselves stay declared in `config.toml` `[[ai.mcp.servers]]` (C4, C5); how a value reaches a server is in mcp-servers (Lifecycle, Secrets and privacy), and the read tools refuse the file (agent-tools, Path policy). Slice tag: [S1]. Scenario tags: [auto] = `cargo test` (temp config folder, fake server child); [manual] = real machine. Items marked "assumption" are proposals, not user decisions.

Terms: "the file" = `secrets.toml` in the lazarobox config folder. "Table" = a top-level TOML table of the file. "Server table" = the table whose name equals a server's `name`.

## Requirements

### Requirement: Location and ownership [S1]
The file MUST be `secrets.toml` in the lazarobox config folder, the folder of `config.toml` (see configuration: `$XDG_CONFIG_HOME/lazarobox/`, else `$HOME/.config/lazarobox/`), so its default place is `~/.config/lazarobox/secrets.toml` (C6). It is edited by the user. lazarobox MUST NEVER create, write, rename, delete or change the mode of the file, from the menu or anywhere else. A missing file is not an error, produces no notice, and leaves every server with the values from lazarobox's own environment only. No value from the file is ever written to `config.toml`.

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
A problem with the file MUST be shown as a sanitized system line `mcp: <problem>` in the chat that starts a server, once per start, and in the Connectors area (see mcp-connect). A problem MUST name at most the table and the key, never a value. A TOML syntax error MUST be reported as `secrets.toml: not valid TOML (line <N>)` without the parser's own message or any text of the line (assumption: the parser message can quote a value). Values MUST NOT be logged, drawn in the transcript or sent to a model.

#### Scenario: Syntax error hides the line [auto]
- GIVEN a file whose line 3 reads `CLARITY_API_TOKEN = "sk-123`
- WHEN a server is started
- THEN the line is `mcp: secrets.toml: not valid TOML (line 3)` and `sk-123` appears in no notice, transcript line or log

#### Scenario: A problem does not stop the start [auto]
- GIVEN a file with one bad key and one good key for `clarity`
- WHEN `clarity` is started
- THEN it starts with the good value and the chat shows the problem once
