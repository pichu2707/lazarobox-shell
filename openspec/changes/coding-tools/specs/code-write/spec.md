# Code Write Specification

## Purpose

Let a model create and change files in one project root without touching the disk until the user approves. The model stages changes, closes the batch, and the user reviews it all-or-nothing. Linux only in v1. Tags: [auto] = `cargo test` (temp dirs, fake clocks); [manual] = real terminal; [design-confirm] = value or name that design must pin. Names marked (provisional) are pinned by design.

Terms: "root" = the chat origin directory. "Batch" = the staged changes of one chat. "Proposed" = the model closed the batch with the propose tool. "Orphan" = a batch left staged after its turn was cancelled. "Refusal text" = `path not available`.

## Requirements

### Requirement: Family and toggle
`create_dir`, `write_file`, `edit_file` and the propose tool (`propose_batch`, provisional) MUST belong to the `Write` family. They MUST be offered only when tools are on and the Write toggle is on. The toggle MUST be off by default. With it off, no write tool is registered or executed.

#### Scenario: Off by default [auto]
- GIVEN a config with no write toggle
- WHEN the tool list is built
- THEN no write tool is present and a call to `write_file` returns an error result

### Requirement: Root policy
Writes MUST be allowed only under one root: the chat origin directory. They MUST be refused when the origin is missing, is `$HOME`, is a parent of `$HOME`, or is a system directory. A target path MUST be refused when, after resolving `..` and symlinks, it is outside the root. Every refusal, including the refusals of the deny rules below, MUST return the same text `path not available`, so the result does not reveal whether a path exists.

#### Scenario: Origin refused [auto]
- GIVEN origins `$HOME`, `/home`, `/usr` and a missing directory
- WHEN `write_file` is called with each
- THEN each returns `path not available` and nothing is staged

#### Scenario: Escape refused [auto]
- GIVEN a root and the paths `../x`, `/etc/x` and `link/x` where `link` points outside the root
- WHEN `write_file` is called with each
- THEN each returns `path not available`

#### Scenario: One refusal text [auto]
- GIVEN an existing denied path and a missing path outside the root
- WHEN `edit_file` is called on each
- THEN both results carry the identical text

### Requirement: Deny rules
A write MUST be refused (with the refusal text) when the target is inside `.git/` or `node_modules/`, or matches the existing secret lists of agent-tools (for example `.env`, `.npmrc`). Reads of `.git/config` MUST be refused. A write MUST be refused when the content holds invalid UTF-8 or a bidi or zero-width control character, or holds CRLF line endings. `edit_file` MUST be refused on a file that is not UTF-8 or holds CRLF. Refused content MUST NOT be cleaned or rewritten.

#### Scenario: Protected directories [auto]
- GIVEN the targets `.git/hooks/pre-commit` and `node_modules/x/index.js`
- WHEN `write_file` is called with each
- THEN each is refused and nothing is staged

#### Scenario: Secret names [auto]
- GIVEN the target `.env`
- WHEN `write_file` is called
- THEN it is refused

#### Scenario: Git config unreadable [auto]
- GIVEN `.git/config` in the root
- WHEN `read_file` is called on it
- THEN it is refused with the refusal text

#### Scenario: Hostile characters [auto]
- GIVEN content with a right-to-left override, content with a zero-width space, and content with `\r\n`
- WHEN `write_file` is called with each
- THEN each is refused and the content is not altered or staged

#### Scenario: Non-UTF-8 file [auto]
- GIVEN an existing Latin-1 file
- WHEN `edit_file` is called on it
- THEN it is refused

### Requirement: Caps
`write_file` content MUST be at most 32 KiB. Each `edit_file` field (`old`, `new`) MUST be at most 16 KiB. A file to be edited or overwritten MUST be at most 1 MiB. A call over a cap MUST return an error result and stage nothing.

#### Scenario: At and over the cap [auto]
- GIVEN content of 32 KiB and of 32 KiB + 1 byte
- WHEN `write_file` is called with each
- THEN the first is staged and the second is refused

#### Scenario: Edit field and file caps [auto]
- GIVEN an `old` of 16 KiB + 1 byte, and a 2 MiB file
- WHEN `edit_file` is called with each
- THEN both are refused

### Requirement: create_dir and write_file staging
`create_dir` and `write_file` MUST add to the batch and MUST NOT touch the disk. `create_dir` joins the batch like every other write. `write_file` on a missing file stages a create. `write_file` on an existing file stages an overwrite, which MUST be allowed only when the file is at most the read cap in size and its text is unchanged by redaction (so what the review shows is what is on disk); otherwise it MUST be refused. A repeated call for the same path MUST replace the earlier staged entry for that path.

#### Scenario: Create stays off disk [auto]
- GIVEN an empty root
- WHEN `write_file` stages `src/main.rs` and `create_dir` stages `src/util`
- THEN neither exists on disk and the batch lists both

#### Scenario: Overwrite allowed [auto]
- GIVEN an unredacted 2 KiB `a.txt`
- WHEN `write_file` is called on it
- THEN an overwrite is staged

#### Scenario: Overwrite refused when redaction changes the text [auto]
- GIVEN `a.txt` holding a recognizable API key, and another file over the read cap
- WHEN `write_file` is called on each
- THEN both are refused

#### Scenario: Restage replaces [auto]
- GIVEN a staged `write_file` for `a.txt`
- WHEN `write_file` is called again for `a.txt`
- THEN the batch holds one entry for `a.txt` with the latest content

### Requirement: edit_file staging
`edit_file` MUST take a path, an exact `old` string and a `new` string. It MUST apply to the staged version when one exists, otherwise to the disk version. It MUST be refused with an error result when `old` is empty, matches zero times, or matches more than once. It MUST NOT touch the disk. Because its answers say whether a text occurs in a file, on a Cloud engine it MUST ask the user like `read_file`, once per file per turn.

#### Scenario: Unique match [auto]
- GIVEN `a.txt` holding `foo bar`
- WHEN `edit_file` is called with `old` `bar` and `new` `baz`
- THEN the batch holds `foo baz` and the disk still holds `foo bar`

#### Scenario: No match or many [auto]
- GIVEN `a.txt` holding `x x`
- WHEN `edit_file` is called with `old` `y`, and again with `old` `x`
- THEN both return an error result and stage nothing

#### Scenario: Chained edits on one file [auto]
- GIVEN a staged edit of `a.txt`
- WHEN a second `edit_file` targets text introduced by the first
- THEN it matches the staged version

#### Scenario: Cloud engine asks once per file [ask]
- GIVEN a Cloud engine and `a.txt`, whose text the model has not been allowed to read
- WHEN `edit_file` targets `a.txt`
- THEN the user is asked as for `read_file` of `a.txt` (same prompt, same approval modes, never typed or run directly), and a denial stages nothing and returns the usual denied text
- AND later edits of `a.txt` in the same turn do not ask again, nor do they after an allowed `read_file` of it; a new turn asks again; a Local engine never asks

### Requirement: Propose batch and size cap
The model closes a batch by calling the propose tool. It MAY stage across several turns before calling it. The propose tool MUST refuse an empty batch and MUST refuse a batch over about 50 entries or 1 MiB of total staged content (whichever comes first), with a message that asks the model to split it. A refused proposal MUST leave the batch staged and unchanged. A proposed batch MUST NOT accept new staging until the user answers.

#### Scenario: Empty batch [auto]
- GIVEN no staged entries
- WHEN the propose tool is called
- THEN it returns an error result and no review opens

#### Scenario: Over the cap [auto]
- GIVEN 51 staged entries, and in another run entries totalling 1 MiB + 1 byte
- WHEN the propose tool is called
- THEN it refuses, asks for a split, and the batch is unchanged

#### Scenario: Staging across turns [auto]
- GIVEN entries staged in turn 1
- WHEN more are staged in turn 2 and the propose tool is called
- THEN the review holds all of them

### Requirement: Batch review and coverage gate
A proposed batch MUST open a full-pane review view in the chat pane. It shows every change: a create shows the new text, an overwrite or edit shows a diff, a `create_dir` shows its path. The approving key MUST stay disabled until every row of the whole batch has been drawn on screen (the coverage gate; a row in a hidden pane does not count), and it MUST stay disabled for the arm delay after the view is first drawn (longer than the delay of other approvals, value pinned by design). The review is all-or-nothing: the user approves the whole batch or rejects the whole batch. A change to a build file (for example `Makefile`, `build.rs`, `package.json`) MUST carry a visible warning. A rejected, timed-out or cancelled review MUST end as `denied`, MUST write nothing, and MUST discard the batch. A pending approval of another kind MUST make the propose tool return `denied` (`another approval is pending`).

#### Scenario: Gate blocks early approval [auto]
- GIVEN a batch of 40 diff rows of which 12 have been shown
- WHEN `y` is pressed
- THEN nothing is applied and the view says how many rows remain

#### Scenario: Gate opens after full coverage [auto]
- GIVEN the same batch, every row shown and the arm delay elapsed (fake clock)
- WHEN `y` is pressed
- THEN the apply step starts

#### Scenario: Reject discards [auto]
- GIVEN a proposed batch
- WHEN `n` or Esc is pressed
- THEN the disk is unchanged, the batch is gone and the propose call returns `denied`

#### Scenario: Build file warning [auto]
- GIVEN a batch that stages `build.rs`
- WHEN the review renders
- THEN the row for `build.rs` carries the warning [TestBackend + insta]

#### Scenario: CLI timeout while reviewing [auto]
- GIVEN a CLI call timeout shorter than the review
- WHEN it expires
- THEN the call returns `denied`, the review is withdrawn, the batch is kept as an orphan and a later `y` writes nothing

### Requirement: Apply
After approval the app MUST re-read every target and compare it with the snapshot taken at staging; if anything drifted, the whole batch MUST be refused and nothing written. It MUST write each file with a temp file and an atomic rename, with `openat2`-style no-symlink opens, and MUST NOT follow or write through a symlink or a hard-linked file. If a write fails partway, the files already written MUST be restored to their pre-batch state. If the rollback itself fails, the result MUST report the batch as partial and list exactly the paths that changed. A successful apply MUST return one `Change` record per path (path, kind, bytes, added and removed lines) to the caller.

#### Scenario: Drift refuses the batch [auto]
- GIVEN a reviewed batch and a target changed on disk after staging
- WHEN `y` is pressed
- THEN no file is written and the result says the batch drifted

#### Scenario: Rollback on failure [auto]
- GIVEN a 3-file batch where the third write fails
- WHEN apply runs
- THEN the first two files are back to their pre-batch content and the result says the batch failed

#### Scenario: Rollback fails [auto]
- GIVEN the same batch where restoring the first file also fails
- WHEN apply runs
- THEN the result says `partial` and lists exactly the changed paths

#### Scenario: Symlink target [auto]
- GIVEN a target that became a symlink after staging
- WHEN apply runs
- THEN it is refused and nothing is written through the link

#### Scenario: Change records [auto]
- GIVEN an applied batch with one create and one edit
- WHEN apply returns
- THEN two `Change` records carry path, kind, bytes and line counts

### Requirement: Orphan, close and quit
A batch orphaned by a cancelled turn MUST stay staged in the chat, and the pane MUST show an indicator of it. The user MUST be able to review and apply it, or to discard it. Closing the chat pane MUST discard a staged batch and show a notice. Quitting the app MUST discard staged batches the same way, with a notice. Staged state MUST NOT survive a restart.

#### Scenario: Cancel leaves an orphan [auto]
- GIVEN a staged batch and a turn that is cancelled
- WHEN the pane is rendered
- THEN the batch is still staged and the indicator is visible [TestBackend + insta]

#### Scenario: Review an orphan [auto]
- GIVEN an orphan batch
- WHEN the user opens it, covers it and presses `y`
- THEN it is applied as a normal batch

#### Scenario: Discard an orphan [auto]
- GIVEN an orphan batch
- WHEN the user discards it
- THEN the batch is gone and the disk is unchanged

#### Scenario: Close and quit discard [auto]
- GIVEN a staged batch
- WHEN the pane is closed, and in another run the app quits
- THEN the batch is discarded and a notice is shown each time

### Requirement: Worker batches
A batch staged by an orchestrator worker MUST be reviewed and applied by the user through the approval queue of the orchestrator (S3). A worker MUST NOT apply a batch itself. The interaction details are pinned by design.

#### Scenario: Worker cannot apply [auto]
- GIVEN a worker with a staged batch
- WHEN the worker finishes
- THEN nothing is written until the user approves through the queue

### Requirement: Nothing writes without approval
No tool MUST change a file, directory or permission on disk except the apply step of an approved batch. Staging, searching and proposing MUST NOT touch the disk.

#### Scenario: Recording filesystem [auto]
- GIVEN every write tool called against a recording filesystem and no approval given
- WHEN the calls complete
- THEN no file or directory was created, changed or removed
