# mcp-secrets Specification

## Purpose

The values a user MCP server needs (tokens, OAuth client id and secret, project ids, credential file paths) live in a lazarobox-owned file that the user edits, mode 0600, and each value reaches only its own server (C6). The servers themselves stay declared in `config.toml` `[[ai.mcp.servers]]` (C4, C5); how a value reaches a server is in mcp-servers (Lifecycle, Secrets and privacy), and the read tools refuse the file (agent-tools, Path policy). Slice tags: [S1], [S6a1] (menu save, C6 amended 2026-11-11), [S6a3] (OAuth client file). Scenario tags: [auto] = `cargo test` (temp config folder, fake server child); [manual] = real machine. Items marked "assumption" are proposals, not user decisions.

Terms: "the file" = `secrets.toml` in the lazarobox config folder. "Table" = a top-level TOML table of the file. "Server table" = the table whose name equals a server's `name`.

## Requirements

### Requirement: Location and ownership [S1]
The file MUST be `secrets.toml` in the lazarobox config folder, the folder of `config.toml` (see configuration: `$XDG_CONFIG_HOME/lazarobox/`, else `$HOME/.config/lazarobox/`), so its default place is `~/.config/lazarobox/secrets.toml` (C6). It is the user's file, and editing it by hand MUST stay fully supported. lazarobox MUST NEVER create, write, rename, delete or change the mode of the file, from the menu or anywhere else, EXCEPT by the menu save of one value and the deletion of its backup (see Menu save; C6 amended 2026-11-11). Amendment note: until 2026-11-11 this sentence had no exception; the owner kept the file the user's and added the masked-field save. A missing file is not an error, produces no notice, and leaves every server with the values from lazarobox's own environment only. No value from the file is ever written to `config.toml`.

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

### Requirement: Menu save [S6a1]
Revised on 2026-11-11 after review: the writer works on open handles, not paths.

The only writes lazarobox makes to the file (C6 amended 2026-11-11) are the save of ONE value for ONE server, sent by a confirmed secret field of the Connectors area (see mcp-connect, Secret fields), and the "delete backup" action on `secrets.toml.bak`. lazarobox's writer MUST be reachable only from those menu actions: no model, tool, MCP call, chat command or config value can send a save. This bounds lazarobox's own writer only: every user MCP server and every program the user runs has the same uid and can read or change the file directly; the file protects values from other users and from lazarobox's own read tools, not from code the user chose to run (README and tutorial say so). The save MUST:
- accept only a name that the server's catalog entry lists in `env` (for a preset, the preset value, see mcp-catalog); any other server or name is refused as `secrets.toml: <server>.<NAME> is not a value of this connector` and nothing is written;
- open the config folder with `O_DIRECTORY | O_NOFOLLOW` and judge it on `fstat` of that handle: refuse a folder that is a symlink (`the config folder is a symlink; edit secrets.toml by hand`), not owned by the effective uid, or writable by group or others (`the config folder is writable by group or others; run chmod 700 <path>`, with the real folder path, for example `chmod 700 ~/.config/lazarobox`). This refusal stays for a group-writable folder too (a Debian or Ubuntu umask of 002 gives 0775), consistent with the jail's strict stance on group-writable paths; the tutorial explains it. A symlinked config folder is refused for the save only: reading `secrets.toml` and editing it by hand keep working (the guide says so). A missing folder is created with mode 0700 and then opened the same way; when no config folder can be resolved nothing is written (`secrets.toml: no config folder`);
- open `secrets.toml` relative to that folder handle with `O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_NOCTTY` (as the reader does, src/runtime/mcp_secrets.rs `read_private`) and judge it on `fstat` of the open handle: a regular file, owned by the effective uid, mode exactly 0600 (owner read and write and no other bit: 0400, 0700, 0640 and every other mode are refused), at most 64 KiB; read at most 64 KiB + 1 bytes from that handle;
- parse it preserving its layout (assumption: `toml_edit`) and change only that value: an existing key keeps its place, its key text and the comment after it on the same line; a missing key is appended at the end of its table; a missing table is appended at the end of the file after one empty line. Every other byte (comments, blank lines, order, other tables, other keys) MUST be identical, and the file's line ending MUST be kept: a file whose lines end in CRLF gets CRLF on the lines the save adds;
- write the value as a TOML basic string with the escapes TOML needs, and refuse a value that is not one line or holds a control character;
- when the file exists, first write the bytes it read to `secrets.toml.bak` by the same temporary-file-and-rename steps below (mode 0600; one generation; the previous backup is replaced only by a save that succeeds up to this step). A `.bak` that is a symlink or not a regular file refuses the save and is left untouched. No backup is made when the file did not exist;
- create the temporary file in the same folder, through the folder handle, with `O_CREAT | O_EXCL | O_NOFOLLOW | O_WRONLY` and mode 0600, named `.secrets.toml.<pid>.<random>.tmp`. When the name exists (`EEXIST`, including a pre-planted symlink) a new random name is tried, at most 3 times, then the save is refused (`secrets.toml: not saved (temporary file exists)`); an existing name is never followed, opened or removed. Leftover temporary files from a crash are not cleaned up automatically (mode 0600, inside the 0700 folder; the guide says they can be deleted). The file is written, `fsync`ed, renamed over `secrets.toml` with `renameat` on the folder handle, and the folder handle is `fsync`ed. A failure at any step leaves the original file unchanged, removes the temporary file it created, and leaves no partial file. When the file is missing it is created by the same steps with mode 0600 holding only that table;
- right before the rename, `fstat` the path again (through the folder handle, without following) and compare device, inode, size and modification time with the handle that was read; when they differ, write nothing and report `secrets.toml changed on disk; nothing was saved, try again`. This narrows but does NOT close the race with a concurrent editor: an edit made between that re-check and the rename is replaced by the save (its previous content is not in `secrets.toml.bak` either, which holds what the save read). The guide says not to edit the file by hand while saving from the menu.

The save MUST refuse, write nothing, leave any existing `secrets.toml.bak` untouched, remove any temporary file it created, and show one visible notice when:
- the path is a symlink: `secrets.toml is a symlink; edit it by hand`;
- it is not a regular file: `secrets.toml: not a regular file`;
- it is not owned by the effective uid: `secrets.toml is not owned by you; edit it by hand`;
- it has any group or other permission bit: `secrets.toml: mode must be 0600 (chmod 600 <path>)` (the existing text, path sanitized);
- it lacks the owner write bit (for example 0400): `secrets.toml is read-only (mode 0400); chmod 600 <path> to save from the menu` (decision: a read-only file is refused, never made writable);
- it is larger than 64 KiB: `secrets.toml: larger than 64 KiB`;
- it cannot be read: `secrets.toml: cannot be read`;
- it is not valid TOML: `secrets.toml: not valid TOML (line <N>); fix it by hand, nothing was saved` (an unparseable file is NEVER overwritten);
- the server's table exists but is not a plain `[name]` table (an inline table, dotted keys, an array of tables or a value): `secrets.toml: [<server>] is not a plain table; edit it by hand`;
- the existing value of that name is not a string: `secrets.toml: <server>.<NAME>: expected a string; edit it by hand`;
- the config folder fails its checks (above);
- a write, sync or rename fails: `secrets.toml: not saved (<error kind>)`.

A notice, a log line, an error or a panic message MUST name at most the server and the name, never a value or a text of the file. On success the notice reads `saved <NAME>; the previous values stay in secrets.toml.bak (0600) until you delete it` (only `saved <NAME>` when there was no file).

The "delete backup" action (mcp-connect, Secret fields) removes `secrets.toml.bak` with `unlinkat` on the folder handle after an `fstat` without following shows a regular file owned by the effective uid; a symlink or anything else is refused and left. The read tools refuse the backup like the file (it lies in `~/.config/lazarobox`).

The I/O MUST go through one seam (assumption: a `SaveFs` trait with the real implementation and a test one) so tests can make `write`, `fsync` or `renameat` fail and check that the original is unchanged.

#### Scenario: Only the value changes [auto]
- GIVEN a file of mode 0600 holding a leading comment, `[ga4]` with `GOOGLE_CLOUD_PROJECT = "p1" # mine`, a blank line and `[clarity]` with `CLARITY_API_TOKEN = "a"`
- WHEN `CLARITY_API_TOKEN` of `clarity` is saved as `b`
- THEN the new file equals the old one with `"a"` replaced by `"b"`, its mode is 0600, and `secrets.toml.bak` holds the old bytes with mode 0600

#### Scenario: Comment after the value kept [auto]
- GIVEN `[ga4]` with `GOOGLE_CLOUD_PROJECT = "p1" # mine`
- WHEN it is saved as `p2`
- THEN the line reads `GOOGLE_CLOUD_PROJECT = "p2" # mine`

#### Scenario: CRLF kept [auto]
- GIVEN a file whose lines end in CRLF holding `[clarity]`
- WHEN `GOOGLE_CLOUD_PROJECT` of `ga4` is saved
- THEN every line of the result, the added ones included, ends in CRLF, and the bytes before the added table are unchanged

#### Scenario: New table appended [auto]
- GIVEN a file holding only `[clarity]`
- WHEN `GOOGLE_CLOUD_PROJECT` of `ga4` is saved as `p`
- THEN the file ends with an empty line, `[ga4]` and `GOOGLE_CLOUD_PROJECT = "p"`, and every byte before it is unchanged

#### Scenario: Missing file is created 0600 [auto]
- GIVEN no `secrets.toml`
- WHEN `CLARITY_API_TOKEN` of `clarity` is saved
- THEN the file exists with mode 0600 and holds only `[clarity]` with that value, no `secrets.toml.bak` exists, and the notice reads `saved CLARITY_API_TOKEN`

#### Scenario: Symlink refused [auto]
- GIVEN `secrets.toml` a symlink to a 0600 file elsewhere
- WHEN a value is saved
- THEN the notice is `secrets.toml is a symlink; edit it by hand`, and neither the link, its target nor any `.bak` or temporary file changes

#### Scenario: Symlinked config folder refused [auto]
- GIVEN `~/.config/lazarobox` a symlink to a folder elsewhere
- WHEN a value is saved
- THEN the notice says the config folder is a symlink and nothing is written in either place

#### Scenario: Group-writable config folder (umask 002) [auto]
- GIVEN a config folder created under umask 002 (mode 0775) holding a 0600 `secrets.toml`
- WHEN a value is saved
- THEN nothing is written and the notice reads `the config folder is writable by group or others; run chmod 700 <folder>` with the real folder path
- AND with the folder at 0777 the same refusal is shown

#### Scenario: Loose mode refused [auto]
- GIVEN a valid file with mode 0644
- WHEN a value is saved
- THEN the notice is `secrets.toml: mode must be 0600 (chmod 600 <path>)` and the file is byte-identical with mode 0644

#### Scenario: Read-only file refused [auto]
- GIVEN a valid file with mode 0400
- WHEN a value is saved
- THEN the notice says the file is read-only and how to fix it, and the file keeps its bytes and mode 0400

#### Scenario: Foreign owner refused [auto, root or the test seam]
- GIVEN a file owned by another uid
- WHEN a value is saved
- THEN the notice is `secrets.toml is not owned by you; edit it by hand` and nothing changes

#### Scenario: Directory in its place refused [auto]
- GIVEN a directory named `secrets.toml`
- WHEN a value is saved
- THEN the notice is `secrets.toml: not a regular file` and nothing is created

#### Scenario: FIFO in its place [auto]
- GIVEN a FIFO named `secrets.toml` with no writer
- WHEN a value is saved
- THEN the save returns at once with `secrets.toml: not a regular file`

#### Scenario: Pre-planted symlink at the temporary name [auto]
- GIVEN a symlink named like the save's temporary file pointing at a file outside the folder (the test fixes the random part through the seam)
- WHEN a value is saved
- THEN the symlink and its target are untouched, the save uses another name, and the result is the expected file with mode 0600

#### Scenario: Unparseable file never overwritten [auto]
- GIVEN a file of mode 0600 whose line 3 reads `CLARITY_API_TOKEN = "sk-123`
- WHEN a value is saved
- THEN the notice is `secrets.toml: not valid TOML (line 3); fix it by hand, nothing was saved`, the file is byte-identical, `secrets.toml.bak` keeps its previous bytes, and `sk-123` appears in no notice or log

#### Scenario: Inline table refused [auto]
- GIVEN `clarity = { CLARITY_API_TOKEN = "a" }`
- WHEN `CLARITY_API_TOKEN` of `clarity` is saved
- THEN the notice is `secrets.toml: [clarity] is not a plain table; edit it by hand` and nothing changes

#### Scenario: A name the entry does not list [auto]
- GIVEN the `clarity` entry
- WHEN a save of `PATH` for `clarity`, or of `CLARITY_API_TOKEN` for `ga4`, is sent
- THEN it is refused as not a value of this connector and nothing changes

#### Scenario: Changed on disk [auto]
- GIVEN a save that read the file, and the file replaced by another program before the re-check (through the seam)
- WHEN the save reaches the re-check
- THEN nothing is written, `secrets.toml.bak` keeps its previous bytes, and the notice says the file changed on disk

#### Scenario: Failed write leaves the original [auto]
- GIVEN the test seam failing `renameat` (and, in other runs, `write` and `fsync`)
- WHEN a value is saved
- THEN the original file is byte-identical, no temporary file remains, and the notice is `secrets.toml: not saved (<error kind>)`

#### Scenario: A refused save keeps the backup [auto]
- GIVEN `secrets.toml.bak` holding `X` and a file with mode 0644
- WHEN a value is saved
- THEN the save is refused and `secrets.toml.bak` still holds `X`

#### Scenario: Delete the backup [auto]
- GIVEN `secrets.toml.bak` a regular 0600 file, and in another run a symlink named `secrets.toml.bak`
- WHEN "delete backup" is confirmed
- THEN the regular file is removed, and the symlink is refused and left with its target untouched

### Requirement: OAuth client file [S6a3]
Added on 2026-11-11 (proposal open question 11, resolved by the orchestrator on 2026-11-11 under the owner's instruction to proceed; pending owner confirmation). The Connectors area MAY also write the Desktop OAuth client file `google-oauth-client.json` in the config folder (the file the `ga4` login and the `gsc` OAuth way read) from a client id and a client secret the user pastes (mcp-connect, Secret fields). The write MUST follow every rule of Menu save that applies to a whole file: folder checks, open by handle with `O_NOFOLLOW`, regular file owned by the effective uid with mode 0600 when it exists, temporary file `O_EXCL | O_NOFOLLOW` 0600 in the same folder, `fsync`, `renameat`, folder `fsync`, backup `google-oauth-client.json.bak` (0600) of an existing file, the same refusals and texts with the file name, the same seam. The content is the Desktop client JSON shape `{"installed": {"client_id": …, "client_secret": …, "auth_uri": "https://accounts.google.com/o/oauth2/auth", "token_uri": "https://oauth2.googleapis.com/token", "redirect_uris": ["http://localhost"]}}`, written by a JSON serializer (escaping done by it). When the file exists it MUST be replaced only after the user confirms `replace google-oauth-client.json? (y/n)`; a file that does not parse as such a client is still only replaced after that confirmation, and its bytes go to the backup. A client id is accepted only when it ends in `.apps.googleusercontent.com` and holds only printable ASCII; the secret only printable ASCII, one line.

#### Scenario: Client file written [auto]
- GIVEN no `google-oauth-client.json`
- WHEN the id `123-abc.apps.googleusercontent.com` and a secret are confirmed
- THEN the file exists with mode 0600, parses as the Desktop client shape with those values, and no backup exists

#### Scenario: Existing client file asks first [auto]
- GIVEN an existing client file
- WHEN a new id and secret are entered and `n` is pressed
- THEN the file is byte-identical and the notice reads `not replaced`

#### Scenario: Bad client id [auto]
- GIVEN the client form
- WHEN `abc` is entered as the id
- THEN nothing is written and a notice says the id must end in `.apps.googleusercontent.com`
