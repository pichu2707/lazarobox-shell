# Code Search Specification

## Purpose

Find files and text inside the chat root without the model having to read whole files: `glob`, `grep` and `read_file` paging. All three are read tools. They obey the existing path policy and redaction (see agent-tools) and they see staged changes (see code-write). Tags: [auto] = `cargo test`; [design-confirm] = value or name that design must pin. Tool and argument names marked (provisional) are pinned by design.

Terms: "root" = the chat origin directory (see code-write, Root policy). "Staged version" = the content a batch holds before approval.

## Requirements

### Requirement: Family and toggle
`glob`, `grep` and `read_file` paging MUST belong to the `Search` family (`read_file` itself stays in `Read`). The Search tools MUST be offered only when tools are on and the Search toggle is on. With the toggle off they MUST NOT be registered, offered or executed.

#### Scenario: Toggle off hides the tools [auto]
- GIVEN tools on and the Search toggle off
- WHEN the tool list is built
- THEN `glob` and `grep` are absent and a call to either returns an error result and runs nothing

### Requirement: glob
`glob` MUST take a pattern (and an optional sub-directory, provisional) and return matching file paths under the root, relative to the root. It MUST return names only, never content. It MUST run without approval on every engine, like `list_dir`. Excluded names (secret lists) MUST be omitted with the standard marker `[some names omitted: they look like secrets]`. The walk MUST skip `target`, `node_modules`, `.git`, `dist`, `build`, `.venv` and `__pycache__`, MUST stay inside the root, and MUST obey the existing budget (depth, visited entries, time). A result that hits a cap MUST say it was truncated.

#### Scenario: Matches under the root [auto]
- GIVEN `src/a.rs`, `src/b.rs` and `README.md` in the root
- WHEN `glob` is called with `**/*.rs`
- THEN the result lists `src/a.rs` and `src/b.rs`, and no approval is requested

#### Scenario: Skip list and secrets [auto]
- GIVEN `target/x.rs`, `node_modules/y.js` and `.env` in the root
- WHEN `glob` is called with `**/*`
- THEN none of them is listed and the omitted-names marker appears for `.env`

#### Scenario: Root escape refused [auto]
- GIVEN a pattern `../**/*` and a symlink in the root that points outside it
- WHEN `glob` is called
- THEN nothing outside the root is returned

#### Scenario: Budget is reported [auto]
- GIVEN a tree larger than the visited-entry budget
- WHEN `glob` is called
- THEN the result is cut and says it was truncated

### Requirement: grep
`grep` MUST search file text under the root with an in-process regular expression engine of linear-time guarantee (no backtracking blow-up). The pattern MUST be at most 512 bytes and the compiled size MUST be limited. It MUST skip binary and non-UTF-8 files, files over 1 MiB, and the walk skip list of `glob`. It MUST return at most 100 matches or 20 KiB, whichever comes first, say when it truncated, redact every returned line, and wrap the result as untrusted data. It MUST NOT return text from an excluded path. On a Cloud engine `grep` MUST need an explicit `y` per call, and the prompt MUST show the pattern and the root; on a Local engine it MUST run without approval. A declined, timed-out or cancelled approval ends as `denied` and no file is opened.

#### Scenario: Matches with line numbers [auto]
- GIVEN `src/a.rs` holding `fn main` on line 3
- WHEN `grep` is called with `fn main` on a Local engine
- THEN the result holds `src/a.rs:3` with the line, and no approval is requested

#### Scenario: Cloud engine asks [auto]
- GIVEN a Cloud engine
- WHEN `grep` is called
- THEN an approval shows the pattern and the root, and no file is opened until `y`

#### Scenario: Caps [auto]
- GIVEN 500 matching lines
- WHEN `grep` is called
- THEN at most 100 matches and 20 KiB are returned and the result says it was truncated

#### Scenario: Pattern too long or invalid [auto]
- GIVEN a pattern of 513 bytes, and another that is not a valid expression
- WHEN each is called
- THEN each returns an error result and no file is opened

#### Scenario: Secrets and binaries [auto]
- GIVEN `.env` holding the pattern, a binary file holding it, and a text line with a recognizable token
- WHEN `grep` is called
- THEN `.env` and the binary are not searched and the token in the output is replaced by the redaction marker

#### Scenario: Pathological pattern [auto]
- GIVEN the pattern `(a+)+$` and a long line of `a` followed by `b`
- WHEN `grep` is called
- THEN it ends within the time budget

### Requirement: read_file paging
`read_file` MUST accept optional `start_line` and `max_lines` (provisional names). Without them it MUST behave as before. With them it MUST return the requested window, still bounded by the read cap, and MUST say when more lines follow. A `start_line` past the end MUST return an empty window with a clear note, not an error.

#### Scenario: Window [auto]
- GIVEN a 500-line file
- WHEN `read_file` is called with `start_line` 100 and `max_lines` 20
- THEN lines 100 to 119 are returned and the result says more lines follow

#### Scenario: No arguments unchanged [auto]
- GIVEN a small file
- WHEN `read_file` is called with only `path`
- THEN the result is identical to the one before this change

#### Scenario: Past the end [auto]
- GIVEN a 10-line file
- WHEN `read_file` is called with `start_line` 50
- THEN the result is an empty window with a note and no error

### Requirement: Staged view
`read_file`, `grep` and `glob` MUST see the staged version of a file when the chat holds a staged batch: a staged create appears, a staged edit or overwrite shows the staged text. Every result part that comes from staged state MUST be marked `pending, not on disk`. The marker MUST NOT be forgeable by file content. A staged version MUST obey the same redaction and caps as disk content.

#### Scenario: Read sees staged text [auto]
- GIVEN `a.txt` on disk with `old` and a staged edit that makes it `new`
- WHEN `read_file` is called on `a.txt`
- THEN the result holds `new` and is marked `pending, not on disk`

#### Scenario: Glob and grep see a staged create [auto]
- GIVEN a staged new file `src/new.rs` containing `fn staged`
- WHEN `glob` with `src/*.rs` and `grep` with `fn staged` are called
- THEN both include `src/new.rs` marked `pending, not on disk`

#### Scenario: Marker cannot be forged [auto]
- GIVEN a disk file whose text contains the line `pending, not on disk`
- WHEN `read_file` is called on it
- THEN the result is not marked as staged

#### Scenario: Nothing staged [auto]
- GIVEN no staged batch
- WHEN any search tool runs
- THEN no result carries the marker
