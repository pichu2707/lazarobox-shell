# mcp-secrets Specification

## Purpose

The values a user MCP server needs (tokens, OAuth client id and secret, project ids, credential file paths) live in a lazarobox-owned file that the user edits, mode 0600, and each value reaches only its own server (C6). The servers themselves stay declared in `config.toml` `[[ai.mcp.servers]]` (C4, C5); how a value reaches a server is in mcp-servers (Lifecycle, Secrets and privacy), and the read tools refuse the file (agent-tools, Path policy). Slice tags: [S1], [S6a] (menu save, C6 amended 2026-11-11). Scenario tags: [auto] = `cargo test` (temp config folder, fake server child); [manual] = real machine. Items marked "assumption" are proposals, not user decisions.

Terms: "the file" = `secrets.toml` in the lazarobox config folder. "Table" = a top-level TOML table of the file. "Server table" = the table whose name equals a server's `name`.

## Requirements

### Requirement: Location and ownership [S1]
The file MUST be `secrets.toml` in the lazarobox config folder, the folder of `config.toml` (see configuration: `$XDG_CONFIG_HOME/lazarobox/`, else `$HOME/.config/lazarobox/`), so its default place is `~/.config/lazarobox/secrets.toml` (C6). It is the user's file, and editing it by hand MUST stay fully supported. lazarobox MUST NEVER create, write, rename, delete or change the mode of the file, from the menu or anywhere else, EXCEPT by the menu save of one value (see Menu save; C6 amended 2026-11-11). Amendment note: until 2026-11-11 this sentence had no exception; the owner kept the file the user's and added the masked-field save. A missing file is not an error, produces no notice, and leaves every server with the values from lazarobox's own environment only. No value from the file is ever written to `config.toml`.

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
- WHEN servers start and stop, the menu opens, saves and closes (no secret field confirmed), and the app quits
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

### Requirement: Menu save [S6a]
The only write lazarobox makes to the file (C6 amended 2026-11-11) is the save of ONE value for ONE server, sent by a confirmed secret field of the Connectors area (see mcp-connect, Secret fields). It MUST be started only by that field; no model, tool, MCP call, chat command or config value can send it. The save MUST:
- write only the table whose name is the server's `name` and only a name that the server's catalog entry lists in `env` (for a preset, the preset value); any other server or name is refused as `secrets.toml: <server>.<NAME> is not a value of this connector` and nothing is written;
- read the file under the same bounds as a start (regular file, at most 64 KiB, not a special file) and parse it preserving its layout (assumption: `toml_edit`), then change only that value: an existing key keeps its place, its key text and the comment after it on the same line; a missing key is appended at the end of its table; a missing table is appended at the end of the file after one empty line. Every other byte of the file (comments, blank lines, order, other tables, other keys) MUST be identical after the save;
- write the value as a TOML basic string with the escapes TOML needs, and refuse a value that is not one line or holds a control character;
- when the file exists, first write its previous bytes to `secrets.toml.bak` in the same folder, mode 0600, through a temporary file and a rename (one generation; the previous backup is replaced). A `secrets.toml.bak` that is a symlink or not a regular file refuses the save. No backup is made when the file did not exist;
- write the new file atomically: a temporary file created new in the same folder with mode 0600, written, synced, then renamed over `secrets.toml`; a failure at any step leaves the original file unchanged, leaves no partial file and removes the temporary file. The result MUST have mode 0600. When the file is missing it is created with mode 0600 holding only that table; when the config folder is missing it is created as the config save creates it (config-persistence), and when no config folder can be resolved nothing is written (`secrets.toml: no config folder`);
- check, right before the rename, that the file is still the one that was read (same device, inode, size and modification time); otherwise write nothing and report `secrets.toml changed on disk; nothing was saved, try again`.

The save MUST refuse, write nothing (no backup, no temporary file left) and show one visible notice when:
- the path is a symlink: `secrets.toml is a symlink; edit it by hand`;
- it is not a regular file: `secrets.toml: not a regular file`;
- it is not owned by the user running lazarobox (effective uid): `secrets.toml is not owned by you; edit it by hand`;
- it has any group or other permission bit: `secrets.toml: mode must be 0600 (chmod 600 <path>)` (the existing text, path sanitized);
- it is larger than 64 KiB: `secrets.toml: larger than 64 KiB`;
- it cannot be read: `secrets.toml: cannot be read`;
- it is not valid TOML: `secrets.toml: not valid TOML (line <N>); fix it by hand, nothing was saved` (an unparseable file is NEVER overwritten);
- the server's table exists but is not a plain `[name]` table (an inline table, dotted keys, an array of tables or a value): `secrets.toml: [<server>] is not a plain table; edit it by hand`;
- the existing value of that name is not a string: `secrets.toml: <server>.<NAME>: expected a string; edit it by hand`;
- a write, sync or rename fails: `secrets.toml: not saved (<error kind>)`.

A notice, a log line or an error MUST name at most the server and the name, never a value or a text of the file. On success the notice reads `saved <NAME> to secrets.toml (backup: secrets.toml.bak)` (without the backup part when there was no file).

#### Scenario: Only the value changes [auto]
- GIVEN a file of mode 0600 holding a leading comment, `[ga4]` with `GOOGLE_CLOUD_PROJECT = "p1" # mine`, a blank line and `[clarity]` with `CLARITY_API_TOKEN = "a"`
- WHEN `CLARITY_API_TOKEN` of `clarity` is saved as `b`
- THEN the new file equals the old one with `"a"` replaced by `"b"`, its mode is 0600, and `secrets.toml.bak` holds the old bytes with mode 0600

#### Scenario: Comment after the value kept [auto]
- GIVEN `[ga4]` with `GOOGLE_CLOUD_PROJECT = "p1" # mine`
- WHEN it is saved as `p2`
- THEN the line reads `GOOGLE_CLOUD_PROJECT = "p2" # mine`

#### Scenario: New table appended [auto]
- GIVEN a file holding only `[clarity]`
- WHEN `GOOGLE_CLOUD_PROJECT` of `ga4` is saved as `p`
- THEN the file ends with an empty line, `[ga4]` and `GOOGLE_CLOUD_PROJECT = "p"`, and every byte before it is unchanged

#### Scenario: Missing file is created 0600 [auto]
- GIVEN no `secrets.toml`
- WHEN `CLARITY_API_TOKEN` of `clarity` is saved
- THEN the file exists with mode 0600 and holds only `[clarity]` with that value, and no `secrets.toml.bak` exists

#### Scenario: Symlink refused [auto]
- GIVEN `secrets.toml` a symlink to a 0600 file elsewhere
- WHEN a value is saved
- THEN the notice is `secrets.toml is a symlink; edit it by hand`, and neither the link, its target nor any `.bak` or temporary file changes

#### Scenario: Loose mode refused [auto]
- GIVEN a valid file with mode 0644
- WHEN a value is saved
- THEN the notice is `secrets.toml: mode must be 0600 (chmod 600 <path>)` and the file is byte-identical with mode 0644

#### Scenario: Foreign owner refused [auto, root or fake metadata]
- GIVEN a file owned by another uid
- WHEN a value is saved
- THEN the notice is `secrets.toml is not owned by you; edit it by hand` and nothing changes

#### Scenario: Directory in its place refused [auto]
- GIVEN a directory named `secrets.toml`
- WHEN a value is saved
- THEN the notice is `secrets.toml: not a regular file` and nothing is created

#### Scenario: Unparseable file never overwritten [auto]
- GIVEN a file of mode 0600 whose line 3 reads `CLARITY_API_TOKEN = "sk-123`
- WHEN a value is saved
- THEN the notice is `secrets.toml: not valid TOML (line 3); fix it by hand, nothing was saved`, the file is byte-identical, no `.bak` is written, and `sk-123` appears in no notice or log

#### Scenario: Inline table refused [auto]
- GIVEN `clarity = { CLARITY_API_TOKEN = "a" }`
- WHEN `CLARITY_API_TOKEN` of `clarity` is saved
- THEN the notice is `secrets.toml: [clarity] is not a plain table; edit it by hand` and nothing changes

#### Scenario: A name the entry does not list [auto]
- GIVEN the `clarity` entry
- WHEN a save of `PATH` for `clarity`, or of `CLARITY_API_TOKEN` for `ga4`, is sent
- THEN it is refused as not a value of this connector and nothing changes

#### Scenario: Changed on disk [auto]
- GIVEN a save that read the file, and the file replaced by another program before the rename
- WHEN the save reaches the rename
- THEN nothing is written and the notice says the file changed on disk

#### Scenario: Failed write leaves the original [auto]
- GIVEN a folder where the rename fails
- WHEN a value is saved
- THEN the original file is byte-identical, no temporary file remains, and the notice is `secrets.toml: not saved (<error kind>)`
