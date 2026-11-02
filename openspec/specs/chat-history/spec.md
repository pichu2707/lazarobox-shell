# Chat History Specification

## Purpose

Every conversation is saved, organized by project, and can be resumed. This spec covers the SQLite store (location, file modes, secure delete, retention), deleting, resuming, the project tag and extra tags, the conversation list, and what happens when the store cannot be used. "Memory" here means organizing and resuming; nothing is summarized or shared between conversations. Nothing is encrypted at rest, and the README says so. Tags: [auto] = `cargo test` (temporary directories, a real SQLite file); [manual] = real machine.

Terms: "conversation" = an ordered list of messages plus metadata (profile, project tag, extra tags, timestamps). "Last activity" = the time of the latest message or tag edit. "Project tag" = the single project label of a conversation; "extra tags" = additional labels.

## Requirements

### Requirement: Store location and privacy
Conversations MUST be stored in one SQLite database file under `$XDG_DATA_HOME/lazarobox/`. When `XDG_DATA_HOME` is unset, empty or relative, the base MUST be `$HOME/.local/share`, only when `HOME` is absolute; with neither there is no store and the unavailable rule applies. The directory MUST be created with mode 0700 when missing. The store MUST hold only conversation data (messages, timestamps, profile, tags, the engine and model used, and the last chat folder path) and usage data (counts, timings, engine, model, timestamps; see Usage table); it MUST NOT hold credentials. The store MUST NOT be encrypted, and the README MUST say so, give the file path and how to delete it, say that folder paths are saved in it, and state what usage data the same file holds. Message text MUST NOT be written to any log.
(Previously: the stored data did not include the chat folder and the README did not mention paths)

#### Scenario: Path resolution [auto]
- GIVEN `XDG_DATA_HOME=/x` and `HOME=/h`
- WHEN the store path is resolved
- THEN it is under `/x/lazarobox/`; without a usable XDG value it is under `/h/.local/share/lazarobox/`; without either there is no store

#### Scenario: Directory mode [auto]
- GIVEN no `lazarobox` data directory
- WHEN the store is opened
- THEN the directory exists with mode 0700

#### Scenario: README states the facts [manual]
- GIVEN the README
- WHEN it is read
- THEN it states that conversations are saved in plain text in an unencrypted SQLite file, gives its path and how to delete it, and says folder paths are saved too

### Requirement: File modes and secure delete
Every database file MUST have mode 0600: the main file and any SQLite side file that exists (`-journal`, `-wal`, `-shm`), from the moment each is created. The journal mode is DELETE (the rollback journal is removed after each transaction); the store MUST NOT use WAL. The mode MUST be checked again on open and on each connection start; a wider mode MUST be corrected to 0600, and a symlink or a file of a foreign owner makes the store unavailable. The store MUST run with `secure_delete` on, so deleted and evicted content is overwritten in the file.

#### Scenario: Fresh files are 0600 [auto]
- GIVEN no store and a permissive umask (0022)
- WHEN the store is created and a message is written
- THEN the main file is mode 0600, and any SQLite side file that exists (`-journal`, `-wal`, `-shm`) is mode 0600

#### Scenario: Journal mode is DELETE [auto]
- GIVEN an open store
- WHEN the `journal_mode` setting is read
- THEN it is `delete`

#### Scenario: Symlink or foreign owner is refused [auto]
- GIVEN a store path that is a symlink, and another file owned by a different user (fake owner)
- WHEN the store is opened
- THEN the store is unavailable in both cases and the target is untouched

#### Scenario: Wide mode corrected [auto]
- GIVEN an existing store file with mode 0644
- WHEN the store is opened
- THEN the file is mode 0600 afterwards

#### Scenario: secure_delete is on [auto]
- GIVEN an open store
- WHEN the `secure_delete` setting is read
- THEN it is on

#### Scenario: Deleted text is gone from the file [auto]
- GIVEN a conversation containing the marker `ZZ-MARKER-ZZ`
- WHEN the conversation is deleted
- THEN the bytes of the database file and of any side file that exists do not contain the marker

### Requirement: Saving conversations
Conversations MUST always be saved; there is no off switch. A conversation MUST be created when its first user message is sent. A user message MUST be saved when sent; an assistant message MUST be saved when it finishes, is cancelled, is cut, fails after partial text, or its pane is closed, with a status (`complete`, `cancelled`, `interrupted`, `truncated` or `error`); a message is partial when its status is not `complete`. System lines MUST NOT be saved. Store operations MUST run off the UI thread: a slow or blocked database MUST NOT delay key handling or drawing. Messages MUST be stored exactly as sanitized (see chat-engines).

#### Scenario: Created on first send [auto]
- GIVEN an empty store and a new conversation
- WHEN the first message is sent
- THEN one conversation exists with that user message, the profile and the project tag

#### Scenario: Assistant saved on finish [auto]
- GIVEN a streaming answer
- WHEN the final event arrives
- THEN the full assistant message is stored with status `complete`

#### Scenario: Partial saved on cancel [auto]
- GIVEN a streaming answer with `Hel` received
- WHEN cancel is requested
- THEN `Hel` is stored with status `cancelled`

#### Scenario: Cut and over-long answers keep their status [auto]
- GIVEN an answer cut without a final event, and another that reached the 1 MiB cap
- WHEN each is stored
- THEN their statuses are `interrupted` and `truncated` and both count as partial

#### Scenario: Error lines not stored [auto]
- GIVEN a turn that ended with an error system line
- WHEN the store is read
- THEN it holds the user message and no error text

#### Scenario: Slow store does not block the UI [auto]
- GIVEN a store whose writes take 2 seconds (fake)
- WHEN keys are handled and frames are drawn meanwhile
- THEN they are handled without waiting for the write

#### Scenario: Stored text is sanitized [auto]
- GIVEN an answer that arrived as `a\u{1b}[31mb`
- WHEN it is stored and read back
- THEN it reads `ab`

### Requirement: Reasoning is not stored
Engine reasoning (thinking deltas, see chat-engines and chat-pane) MUST NOT be written to the database, in any table, column or side file, and MUST NOT be sent to the store thread: the store commands carry only user messages and the answer text of assistant messages. A turn that produced reasoning and no answer text MUST store no assistant message. A resumed conversation MUST show no reasoning blocks and no placeholder for them. Storing optimized memory (tags and categories) is out of scope here and belongs to the planned follow-up change `engram-memory`.

#### Scenario: Reasoning never reaches the store [auto]
- GIVEN a turn with thinking deltas `secret plan` followed by the answer `Hi` and a final event
- WHEN the store commands emitted for the turn are inspected
- THEN they hold the user message and the assistant message `Hi` with status `complete`, and no command contains `secret plan`

#### Scenario: Reasoning is absent from the file [auto]
- GIVEN the same turn saved to a real temporary database
- WHEN the bytes of the database file and of any side file are searched for `secret plan`
- THEN it is not found

#### Scenario: Reasoning only, then cancel, error or cut [auto]
- GIVEN a turn that delivered only reasoning and then ended by cancel, by an error and by a cut stream in separate runs
- WHEN the store is read
- THEN each time it holds the user message and no assistant message

#### Scenario: Partial answer keeps no reasoning [auto]
- GIVEN a turn with reasoning and the partial answer `Hel`, then cancel
- WHEN the store is read
- THEN the assistant message is `Hel` with status `cancelled` and no reasoning

#### Scenario: Resumed conversation shows no reasoning [auto]
- GIVEN a stored conversation whose original turns had reasoning
- WHEN it is resumed
- THEN the transcript has the stored messages and no reasoning line, and the Ctrl+R toggle does nothing

#### Scenario: Reasoning is not engine history after resume [auto]
- GIVEN a resumed conversation
- WHEN the next message is sent
- THEN the request history holds only stored message texts

### Requirement: Retention
The store MUST keep at most 200 conversations. When a new conversation would make it 201, the conversation with the oldest last activity MUST be evicted, with its messages, in the same transaction. Retention is checked when a conversation is created. Tags do not protect a conversation, and the conversation being written is never the one evicted. A conversation that is open in any pane is also never evicted (the next oldest is chosen). The user MUST be told: the new conversation's transcript shows the system line `oldest conversation removed (limit 200)`. If the store holds more than 200 at that moment, enough of the oldest are evicted to return to 200. Eviction is automatic: it MUST NEVER prompt and MUST NEVER delete notes in engram. The same transaction counts the note rows of the evicted conversations, and the creation reply carries that count (`evicted_notes`). When the count is not zero, a second system line follows the first: `N notes of removed conversations remain in engram` (`1 note of a removed conversation remains in engram` for one). This holds whether or not memory is currently on, and without any engram request.

#### Scenario: The 201st evicts the oldest [auto]
- GIVEN 200 conversations with distinct last activity
- WHEN a new conversation is created
- THEN the store has 200, the one with the oldest last activity and its messages are gone, and the new one exists

#### Scenario: Activity keeps a conversation alive [auto]
- GIVEN 200 conversations where the oldest by creation received a new message yesterday
- WHEN a new conversation is created
- THEN the conversation with the oldest last activity is evicted, not the recently active one

#### Scenario: User is told [auto]
- GIVEN 200 stored conversations
- WHEN a new conversation is created
- THEN its transcript shows `oldest conversation removed (limit 200)`, and with fewer than 200 it shows nothing

#### Scenario: Tagged conversations count [auto]
- GIVEN 200 conversations of which the oldest has extra tags
- WHEN a new conversation is created
- THEN that oldest tagged conversation is evicted

#### Scenario: Open conversation is skipped [auto]
- GIVEN 200 conversations of which the one with the oldest last activity is open in a pane
- WHEN a new conversation is created
- THEN the open one remains and the next oldest is evicted

#### Scenario: Over the limit is trimmed [auto]
- GIVEN a store holding 205 conversations
- WHEN a new conversation is created
- THEN the store holds 200

#### Scenario: Notes remain, no prompt [auto]
- GIVEN 200 conversations of which the oldest has 3 note rows, memory on and the token set
- WHEN a new conversation is created
- THEN the creation reply carries `evicted_notes` 3, the transcript shows the first line and then `3 notes of removed conversations remain in engram`, no prompt is shown and no engram request is made

#### Scenario: Singular and none [auto]
- GIVEN the evicted conversation has 1 note row, and in another run none
- WHEN a new conversation is created
- THEN the lines read `1 note of a removed conversation remains in engram` and only the unchanged first line

#### Scenario: Memory off still reports [auto]
- GIVEN memory off and an evicted conversation with 2 note rows
- WHEN a new conversation is created
- THEN the second line reports 2 notes and no engram request is made

### Requirement: Delete
In the conversation list, `d` MUST ask for confirmation; only an unmodified lowercase key listed in the prompt confirms, any other key cancels. With no notes to offer, the prompt is `Delete conversation? (y/n)` and `y` confirms. When the conversation has note rows, memory is on and a token is present (`ENGRAM_HTTP_TOKEN` set and non-empty; the app knows only that it is present), the prompt is `Delete conversation? y · n · a also delete its N notes` (`its 1 note`): `y` deletes the conversation and keeps the notes, `a` deletes the conversation and then its notes, anything else cancels. If notes exist but memory is off, or the token is missing, the prompt is the plain one and, after a confirmed delete, one system line says why the notes stay (see chat-memory). Confirming MUST remove the conversation and all its messages (securely, see above) and refresh the list; the delete transaction returns the `(project, observation id)` rows of the conversation's notes before they are cascaded. If another pane has that conversation open, that pane keeps working, further messages are not persisted, and its transcript shows the system line `conversation deleted; new messages are not saved`. For `a`, the notes are deleted afterwards (see engram-client) and never affect the conversation delete, which has already happened. The result is one system line: `deleted N notes from engram`, `deleted K of N notes from engram` or `could not delete notes: <short reason>`; a refusal that is part of a partial result is appended after a semicolon (`deleted 2 of 3 notes from engram; engram refused the delete (check ENGRAM_HTTP_TOKEN)`). For `a`, every note row the delete transaction returns is deleted, which can be more than the N the prompt showed when a note was saved after the list was read; the result line counts the real N. A delete that cannot run at all (memory unavailable, queue full) has its own failure reply and is shown as `could not delete notes: <reason>`; it is never mixed up with the failure of a project or context request. A conversation whose notes the list did not know (saved after the list was read) gets the plain prompt and the line `its N notes stay in engram (they were saved after the list was read)`.

#### Scenario: Delete confirmed [auto]
- GIVEN a list with three conversations and the cursor on the second
- WHEN `d` then `y` are pressed
- THEN the second conversation and its messages are removed and two rows remain

#### Scenario: Delete declined [auto]
- GIVEN the prompt `Delete conversation? (y/n)`
- WHEN `n`, Esc, `x` or `Y` is pressed
- THEN nothing is removed

#### Scenario: Deleted while open elsewhere [auto]
- GIVEN a conversation open in a chat pane and deleted from another pane's list
- WHEN a message is sent in the first pane
- THEN the answer is shown, nothing is stored and the system line appears

#### Scenario: Delete failure [auto]
- GIVEN a store whose delete fails
- WHEN delete is confirmed
- THEN the list keeps the row and shows the system line `could not delete conversation`, and no note request is made

#### Scenario: Three-way prompt with a token [auto]
- GIVEN memory on, a token present and a conversation with 3 note rows
- WHEN `d` is pressed
- THEN the prompt reads `Delete conversation? y · n · a also delete its 3 notes`

#### Scenario: Delete both [auto]
- GIVEN that prompt and a fake engram
- WHEN `a` is pressed
- THEN the conversation is removed, three `DELETE` requests with `expected_project` and the bearer token are sent, and the line `deleted 3 notes from engram` is shown

#### Scenario: Delete conversation only [auto]
- GIVEN that prompt
- WHEN `y` is pressed
- THEN the conversation is removed, no engram request is made and its note rows are gone

#### Scenario: Other keys cancel the three-way prompt [auto]
- GIVEN that prompt
- WHEN `n`, Esc, `x`, `Y` and `A` are pressed in separate runs
- THEN nothing is removed and no request is made

#### Scenario: No token [auto]
- GIVEN memory on, no token and 3 note rows
- WHEN `d` then `y` are pressed
- THEN the prompt was the plain one, no request is made and the line `its 3 notes stay in engram (set ENGRAM_HTTP_TOKEN to delete them)` is shown

#### Scenario: Memory off [auto]
- GIVEN memory off and 2 note rows
- WHEN `d` then `y` are pressed
- THEN the line reads `its 2 notes stay in engram (memory is off)` and no request is made

#### Scenario: Partial result [auto]
- GIVEN three notes of which the fake engram refuses one with 403
- WHEN `a` is pressed
- THEN the line reads `deleted 2 of 3 notes from engram` and the conversation stays deleted

#### Scenario: Engram unreachable [auto]
- GIVEN `a` pressed and nothing on the memory port
- WHEN the deletes run
- THEN the line starts with `could not delete notes:` and the list is usable

#### Scenario: Conversation without notes [auto]
- GIVEN a conversation with no note rows
- WHEN `d` then `y` are pressed
- THEN the prompt is the plain one and no note line appears

### Requirement: Resume
Enter on a list row MUST open that conversation in the pane with its full history: every stored message in order, partial ones marked, with its stored profile and tags. New messages MUST be appended to the same conversation. The engine used MUST be the current assignment of the `chat` task, not the stored one. The system prompt MUST use the conversation's stored profile (see prompt-profiles). A conversation already open in another pane MUST NOT be opened a second time: the list shows the system line `already open in another pane`. The chat folder MUST become the stored folder when it is an acceptable folder that is not broad. When the conversation has no stored folder (older rows), or the stored folder no longer exists, is not a directory, is a secret root, or is broad, the folder MUST be treated as unknown-for-this-chat: the history is shown, a system line says `stored folder not available: <folder>` (or `no stored folder`), and the folder picker (see chat-folder) opens over the conversation; nothing is chosen silently, and Esc leaves the chat in the folder the pane had before the resume. The pane's folder is never changed by a resume that fails.
(Previously: resume restored history, profile and tags only; the chat kept the origin of the pane)

#### Scenario: Full history restored [auto]
- GIVEN a stored conversation of 30 messages, the last one partial
- WHEN it is resumed
- THEN all 30 are shown in order, the last marked as partial

#### Scenario: Appends to the same conversation [auto]
- GIVEN a resumed conversation
- WHEN a message is sent and answered
- THEN the same conversation holds 32 messages and no new conversation exists

#### Scenario: Uses the current engine [auto]
- GIVEN a conversation saved with `claude` and `chat` now assigned to `ollama`
- WHEN a message is sent after resuming
- THEN the ollama engine is used

#### Scenario: Already open [auto]
- GIVEN a conversation open in pane A
- WHEN pane B's list resumes it
- THEN it is not opened in B and the system line is shown

#### Scenario: Resume opens the stored folder [auto]
- GIVEN a stored conversation in `/w/proj` (exists) resumed from a pane in `$HOME`
- WHEN it is resumed
- THEN the chat folder is `/w/proj`, no picker is shown and tools and memory use it

#### Scenario: Stored folder vanished [auto]
- GIVEN a stored conversation whose folder was deleted
- WHEN it is resumed
- THEN the history is shown, the line `stored folder not available: <folder>` appears and the picker opens; accepting a folder sets it and stores it, and Esc keeps the folder the pane had

#### Scenario: Older row without a folder [auto]
- GIVEN a conversation created before this change
- WHEN it is resumed
- THEN the line `no stored folder` appears and the picker opens

#### Scenario: Stored broad folder [auto]
- GIVEN a conversation whose stored folder is `$HOME`
- WHEN it is resumed
- THEN the picker opens

### Requirement: Project tag
When a chat is opened, its project tag MUST be proposed from the origin cwd: walk up from the cwd to the filesystem root (at most 64 levels) looking for a `.git` entry (a directory, or a file as in worktrees and submodules; a symlink is NOT a marker: it is not followed, it is looked at with `symlink_metadata`, and the walk goes on); the tag is the final path component of the directory that holds it. When that `.git` is a file whose `gitdir:` line (read within 1 KiB) contains `/.git/worktrees/`, the tag is the final path component of the main repository instead; a `.git` file without such a line gives the directory that holds it. When none is found, the tag is the final path component of the cwd itself (`/` for the root). When the origin cwd is unknown or is not an existing directory, there is no project tag. `git` MUST NOT be executed. A tag MUST be trimmed, MUST have control and format characters removed, and MUST be at most 64 characters (the rest is cut). A tag that is empty after cleaning is no tag.

#### Scenario: Git repository [auto]
- GIVEN a cwd `/work/proj/src/deep` inside a directory `/work/proj` that contains `.git`
- WHEN the project tag is proposed
- THEN it is `proj`

#### Scenario: Worktree file [auto]
- GIVEN `/work/wt` contains a `.git` file and the cwd is `/work/wt/a`
- WHEN the project tag is proposed
- THEN it is `wt`

#### Scenario: Worktree of a main repository [auto]
- GIVEN `/work/wt` contains a `.git` file whose `gitdir:` line points into `/work/proj/.git/worktrees/wt`
- WHEN the project tag is proposed from `/work/wt/a`
- THEN it is `proj`, so the worktrees of one project share a tag

#### Scenario: Not a repository [auto]
- GIVEN a cwd `/tmp/notes` with no `.git` above it
- WHEN the project tag is proposed
- THEN it is `notes`

#### Scenario: Unavailable cwd [auto]
- GIVEN an unknown cwd, or one that no longer exists
- WHEN the project tag is proposed
- THEN there is no project tag

#### Scenario: Tag cleaning [auto]
- GIVEN a directory named with 100 characters and a bidi override inside
- WHEN the project tag is proposed
- THEN it has at most 64 characters and no bidi or control characters

#### Scenario: No git process [auto]
- GIVEN `git` replaced by a script that records its invocation
- WHEN the project tag is proposed
- THEN the script was never run

### Requirement: Edit tags
The user MUST be able to edit the project tag and to add extra tags, from the conversation list (`t` on a row) and from an open conversation (`Ctrl+Space T`; see modal-input). The editor MUST show the current project tag and extra tags, accept edits, save on Enter and discard on Esc. A conversation MAY have at most 8 extra tags; each is cleaned like a project tag; empty tags are dropped and duplicates (case-insensitive) are kept once. Tags are typed as a comma-separated line (the first entry is the project tag): a comma separates tags and is never part of one. Saved changes MUST be stored at once, update the last activity, and be reflected in the list filter.

#### Scenario: Edit the project tag [auto]
- GIVEN a conversation tagged `proj`
- WHEN the project tag is edited to `client-a` and saved
- THEN the stored project tag is `client-a` and the list filter for `client-a` shows it

#### Scenario: Add extra tags [auto]
- GIVEN a conversation with no extra tags
- WHEN `sql` and `etl` are added and saved
- THEN both are stored and shown in the editor on reopening

#### Scenario: Limits and duplicates [auto]
- GIVEN an editor with `sql`, `SQL`, an empty entry and 9 distinct extra tags
- WHEN saved
- THEN `sql` appears once, the empty entry is dropped, and only 8 extra tags are kept

#### Scenario: Discard [auto]
- GIVEN an edited tag
- WHEN Esc is pressed
- THEN the stored tags are unchanged

#### Scenario: Tags of a conversation not yet created [auto]
- GIVEN a new conversation view with no message sent
- WHEN the tags are edited and saved
- THEN nothing is stored yet, the pane keeps them, and the first message creates the conversation with those tags; a project tag the user removed is not proposed again from the origin

#### Scenario: Editor shares the pane [auto]
- GIVEN the note editor or the summary draft is open
- WHEN `Ctrl+Space T` is pressed
- THEN the tag editor does not open; and while the tag editor is open `Ctrl+Space n` and `Ctrl+Space s` open nothing, and the `engram serve` question does not take its keys

#### Scenario: Comma separates tags [auto]
- GIVEN the editor line `proj,a,b`
- WHEN it is saved
- THEN the project tag is `proj`, the extra tags are `a` and `b`, and no tag contains a comma

### Requirement: Conversation list
Opening a chat pane MUST show the conversation list before the conversation view. The list MUST show the conversations of the current project (the project tag proposed for the pane), newest last activity first. Each row MUST show the date of the last activity (`YYYY-MM-DD`), the profile, and the first user message on one line, with newlines turned into spaces and cut with `…` to fit. Keys: `j`/`k` and Up/Down move the cursor (clamped), Enter resumes, `n` or Esc starts a new conversation, `d` deletes, `t` edits tags, and `a` toggles between the current project and all projects. When there is no project tag the list MUST show all conversations and say so in its title. When the current project has no conversations the list MUST show `No conversations for <project>. Press n to start one.`; when the store is empty it MUST show `No conversations yet. Press n to start one.`. A new conversation takes the proposed project tag and is created in the store when its first message is sent.

#### Scenario: Filtered by project [auto, TestBackend + insta]
- GIVEN conversations tagged `proj` (2), `other` (1), and the chat opened in `proj`
- WHEN the list is rendered
- THEN it shows the 2 `proj` conversations, newest first, each with date, profile and first message

#### Scenario: Toggle all projects [auto]
- GIVEN the same data
- WHEN `a` is pressed, then pressed again
- THEN 3 rows show, then 2

#### Scenario: Empty project [auto]
- GIVEN conversations only for `other` and the chat opened in `proj`
- WHEN the list is rendered
- THEN it reads `No conversations for proj. Press n to start one.`

#### Scenario: No project [auto]
- GIVEN the chat opened with an unknown cwd and three stored conversations
- WHEN the list is rendered
- THEN all three show and the title says all projects

#### Scenario: Empty store [auto]
- GIVEN an empty store
- WHEN the list is rendered
- THEN it reads `No conversations yet. Press n to start one.`

#### Scenario: Long first message [auto]
- GIVEN a first message of 300 characters with newlines
- WHEN the row is rendered in 40 columns
- THEN it is one line ending in `…` and nothing is drawn outside the pane

#### Scenario: New conversation [auto]
- GIVEN the list
- WHEN `n` (or Esc) is pressed
- THEN the conversation view opens empty with the proposed project tag and no row is stored yet

#### Scenario: Cursor clamps [auto]
- GIVEN the cursor on the last row
- WHEN `j` is pressed
- THEN the cursor stays on the last row

### Requirement: Unavailable store
When the store cannot be opened, is corrupt, or its path cannot be resolved, the chat MUST still work in memory: the user can chat, streams work, and nothing is persisted. The transcript MUST show the system line `history unavailable: <short sanitized error>` once, and the list MUST show no conversations. lazarobox MUST NOT delete, truncate or overwrite the unusable file. A failed write mid-session MUST show the system line `could not save conversation` once per failure run and MUST NOT interrupt the stream. The app MUST NOT crash or hang.

#### Scenario: Corrupt file [auto]
- GIVEN a store file that contains random bytes
- WHEN a chat pane is opened and a message is sent
- THEN the chat works, the system line `history unavailable: ` followed by a short error is shown, and the file's bytes are unchanged

#### Scenario: Cannot open [auto]
- GIVEN a data directory that cannot be created (a file occupies its path)
- WHEN a chat pane is opened
- THEN the list is empty, a new conversation can be used, and the system line is shown

#### Scenario: No path [auto]
- GIVEN no usable `XDG_DATA_HOME` or `HOME`
- WHEN a chat pane is opened
- THEN the chat works in memory and the system line is shown

#### Scenario: Write fails mid-session [auto]
- GIVEN a store that fails every write after the first message
- WHEN two more messages are exchanged
- THEN the answers stream normally and `could not save conversation` is shown once

#### Scenario: Error text is bounded [auto]
- GIVEN an open error message with escape sequences and 500 characters
- WHEN it is shown
- THEN it is sanitized and at most 120 characters

### Requirement: Memory link tables
The store MUST migrate from schema v1 to v2 in one additive transaction on first open, keeping every existing row. Schema v2 adds `memory_sessions` (one row per conversation: the engram session id of the form `lazarobox-<32 lowercase hex>`, the engram project; removed with the conversation) and `memory_notes` (one row per note, keyed by project and topic key, holding the observation id, the conversation that wrote it last, and the title; removed with that conversation). A save upserts the `memory_notes` row, so a note rewritten by a newer conversation belongs to that conversation. The tables hold no note content. Writes run on the store thread. Rows are covered by `secure_delete` and by the file-mode rules. A conversation delete and an eviction remove their rows in the same transaction. A build that only knows v1 MUST treat a v2 database as unavailable history and leave the file untouched (rollback note in the README).

#### Scenario: Migration keeps data [auto]
- GIVEN a v1 database with conversations and messages
- WHEN it is opened
- THEN the schema is v2, every conversation and message is intact, and the two tables exist and are empty

#### Scenario: Link upsert re-points [auto]
- GIVEN conversation A with a note row for `(p, chat/decision/use-axum)` and conversation B saving the same project and topic key
- WHEN B's link is written
- THEN one row exists and it belongs to B

#### Scenario: Rows go with the conversation [auto]
- GIVEN a conversation with a session row and two note rows
- WHEN the conversation is deleted, and in another run evicted
- THEN all its rows are gone in both cases

#### Scenario: Session id reused on resume [auto]
- GIVEN a conversation with a stored session id
- WHEN it is resumed and reads its memory session, even if the caller proposes another id and another project
- THEN the stored id and the stored project are returned (engram rejects a session under a different project, 400 `session_project_mismatch`, so the caller reuses the stored pair)

#### Scenario: A panic in a link keeps history available [auto]
- GIVEN an open store and a link command that panics inside its transaction
- WHEN the store thread answers
- THEN the reply is `LinkFailed`, the transaction is rolled back, and, when the connection is back in autocommit (no transaction left open), the next conversation create and message append succeed; a link panic that leaves a transaction open, like a panic in a create or an append, marks the store unusable for the run

#### Scenario: Old build refuses v2 [auto]
- GIVEN a v2 database and a store opener that supports only v1
- WHEN it opens
- THEN history is unavailable with the usual system line and the file bytes are unchanged

#### Scenario: Link rows hold no content and keep file modes [auto]
- GIVEN a saved note with content `SECRET-BODY`
- WHEN the database file and side files are inspected
- THEN `SECRET-BODY` is not found and the modes are 0600

### Requirement: Chat folder is stored
Each conversation MUST store the last chat folder (an absolute path) with it: set when the conversation is created from the folder the pane has at that moment, and updated whenever the chat folder changes (a confirmed `/cd`) once the conversation exists. A change made before the first message is not stored until the conversation is created, with the new folder. The write runs off the UI thread like every store write and its failure follows the existing unavailable and write-failure rules. The folder is removed with the conversation (delete and eviction) and is covered by `secure_delete` and the file-mode rules. The addition to the schema is additive: a build that does not know the folder ignores it. Conversations created before this change have no folder.

#### Scenario: Stored on create [auto]
- GIVEN a new conversation in `/w/proj`
- WHEN the first message is sent
- THEN the conversation row holds `/w/proj`

#### Scenario: Updated on change [auto]
- GIVEN a stored conversation in `/w/a`
- WHEN the folder is changed to `/w/b`
- THEN the stored folder is `/w/b`

#### Scenario: Change before the first message [auto]
- GIVEN a new conversation view where `/cd /w/b` was confirmed and no message sent
- WHEN the first message is sent
- THEN nothing was stored earlier and the conversation holds `/w/b`

#### Scenario: Gone with the conversation [auto]
- GIVEN a conversation with a folder
- WHEN it is deleted, and in another run evicted
- THEN the folder is gone from the file bytes in both cases

#### Scenario: Old rows migrate [auto]
- GIVEN a database from before this change with conversations
- WHEN it is opened
- THEN every conversation and message is intact and the conversations report no folder

### Requirement: Project tag follows the folder only at proposal
The project tag of an existing conversation MUST NOT change when the chat folder changes; it changes only by the tag editor. A new conversation proposes its project tag from the chat folder as accepted from the picker (see Project tag). The list filter keeps using the project tag.

#### Scenario: Tag survives `/cd` [auto]
- GIVEN a conversation tagged `proj` and a change to a folder named `other`
- WHEN the stored tags are read
- THEN the project tag is still `proj`

#### Scenario: New chat takes the picked folder [auto]
- GIVEN the picker accepted `/w/client-a`
- WHEN a new conversation is started
- THEN its proposed project tag is `client-a`

### Requirement: Usage table
The schema MUST move from version 3 to version 4 by an additive migration that creates a `turn_usage` table and changes nothing else; a version 3 store opens, migrates in one transaction and keeps every conversation, message and tag. A build that only knows version 3 MUST be able to ignore the table. A row MUST hold only: an id, `chat_id` (nullable), `engine`, `model`, `ts` (UTC whole seconds), the optional counts and `cost_usd` of the usage record, a `partial` flag, and the optional short `role` identifier (see `ai-usage`). It MUST NOT hold message text, prompts, answers, reasoning, paths or errors. Unknown (`None`) counts MUST be stored as SQL NULL and never as 0. `chat_id` references the conversation and is set to NULL when the conversation is deleted or evicted (no cascade). One row is written per turn that has a usage record, from the store thread, off the UI thread; a failed usage write (insert error, migration error, prune error) MUST NOT fail or block the turn and MUST NOT affect the conversation or the answer, but it MUST NEVER be silent: the pane shows the system line `Usage not saved: <reason>` (reason short, sanitized, at most 120 characters, no message text, no path), at most once per app run (later failures in the same run show nothing more). A migration error at open that leaves the usage table unusable shows the same line in the first chat pane that tries to store usage; the rest of the store keeps working. `/cost` keeps working with the rows that exist. A conversation that is not saved (deleted while open, store unavailable) writes its usage with `chat_id` NULL when the store is available, and nothing when it is not. The table is covered by the same file modes, `secure_delete` and journal rules as the rest of the file.

#### Scenario: Migration keeps data [auto]
- GIVEN a version 3 database with conversations, messages and tags
- WHEN the store is opened
- THEN the version is 4, `turn_usage` exists and empty, and all earlier rows are unchanged

#### Scenario: Older build ignores the table [auto]
- GIVEN a version 4 database and the version 3 reader (the old statements)
- WHEN the old queries run
- THEN they succeed unchanged

#### Scenario: Unknown stays NULL [auto]
- GIVEN a record with only `output_tokens` 5
- WHEN it is stored and read back
- THEN the other count columns are NULL, not 0

#### Scenario: Columns are the allowed set [auto]
- GIVEN the created table
- WHEN its columns are listed
- THEN they are exactly the allowed set and none can hold message text

#### Scenario: Fresh store is version 4 [auto]
- GIVEN no store
- WHEN it is created
- THEN the version is 4 and `turn_usage` exists

#### Scenario: Failed usage write shows a notice [auto]
- GIVEN a store whose usage insert fails with a reason (fake)
- WHEN a turn ends
- THEN the assistant message is stored as usual, the answer and the composer are unaffected, and the pane shows `Usage not saved: <reason>`

#### Scenario: Notice shown once per run [auto]
- GIVEN the same failing store and three turns in one app run
- WHEN each turn ends
- THEN `Usage not saved:` appears after the first turn only, and every turn still completes

#### Scenario: Worker row fields [auto]
- GIVEN a record of role `worker` with a worker id
- WHEN it is stored
- THEN the row holds that role and id (identifiers only: no text, no path) and the same NULL-for-unknown rule

#### Scenario: Migration failure is visible [auto]
- GIVEN a version 3 store whose v4 migration fails (fake)
- WHEN a turn with usage ends
- THEN conversations still save and `Usage not saved: <reason>` is shown once

#### Scenario: Notice carries no text [auto]
- GIVEN a failure whose raw error contains `ZZ-MARKER-ZZ` and a path
- WHEN the notice is built
- THEN it contains neither and is at most 120 characters of reason

#### Scenario: Slow usage write does not block the UI [auto]
- GIVEN a store whose writes take 2 seconds (fake)
- WHEN keys are handled and frames are drawn after a turn ends
- THEN they are handled without waiting for the usage write

#### Scenario: Marker not in usage rows [auto]
- GIVEN a turn whose answer contains `ZZ-MARKER-ZZ`
- WHEN the turn and its usage are stored and the file bytes are searched
- THEN the marker appears only in the message that held it (until that conversation is deleted), never in any usage row

### Requirement: Usage survives chat deletion
Deleting a conversation, evicting it by the 200-conversation limit or removing the store entries of a conversation MUST NOT delete or alter any `turn_usage` row except by setting its `chat_id` to NULL, in the same transaction as the removal. The usage totals of `/cost` MUST be the same before and after a conversation is deleted. After the link is dropped, nothing in the file may allow reading the deleted conversation's text, and the secure-delete scenario for messages still holds. Usage rows of a deleted conversation no longer count as "this chat" for any pane.

#### Scenario: Delete keeps usage [auto]
- GIVEN a conversation with 3 usage rows
- WHEN the conversation is deleted
- THEN the 3 rows remain with `chat_id` NULL and their counts unchanged

#### Scenario: Eviction keeps usage [auto]
- GIVEN 200 conversations and usage rows on the oldest
- WHEN a new conversation evicts it
- THEN its rows remain with `chat_id` NULL

#### Scenario: Totals unchanged [auto]
- GIVEN `/cost` totals before deleting a chat
- WHEN the chat is deleted and `/cost` runs again
- THEN `today` and `last 7 days` are equal, and `this chat` reads `no data` in a new chat

#### Scenario: Text gone, usage stays [auto]
- GIVEN a conversation containing `ZZ-MARKER-ZZ` and a usage row
- WHEN it is deleted
- THEN the file and side files do not contain the marker and the usage row is still readable

### Requirement: Usage retention
Usage rows older than 90 days MUST be removed: counted from `ts` against the current time, when the store is opened and when a usage row is written. Pruning MUST run in the store thread in one transaction, MUST NOT depend on the 200-conversation limit, MUST NOT touch conversations or messages, and MUST NOT prompt or show a message when it succeeds; a failed prune MUST show `Usage not saved: <reason>` under the once-per-run rule of Usage table (the old rows stay, nothing else is affected). A row exactly 90 days old is kept; a row 90 days and one second old is removed. Removed rows are overwritten in the file (`secure_delete`). The clock is injectable in tests. If the clock is before the newest row's `ts`, nothing is pruned.

#### Scenario: Old rows pruned on open [auto]
- GIVEN rows at 89, 90 and 91 days old
- WHEN the store is opened
- THEN the 91-day row is gone and the other two remain

#### Scenario: Pruned on write [auto]
- GIVEN a long-running store holding a row that became older than 90 days
- WHEN a new usage row is written
- THEN the old row is removed

#### Scenario: Conversations untouched [auto]
- GIVEN a 200-day-old conversation and a 200-day-old usage row
- WHEN the store is opened
- THEN the conversation remains and the usage row is gone

#### Scenario: Clock before newest row [auto]
- GIVEN a clock set before the newest row's `ts`
- WHEN pruning runs
- THEN no row is removed

#### Scenario: Successful prune is quiet [auto]
- GIVEN a prune that removes rows
- WHEN it runs
- THEN no system line, notice or prompt is produced

#### Scenario: Failed prune is visible [auto]
- GIVEN a prune that fails (fake)
- WHEN it runs
- THEN `Usage not saved: <reason>` is shown once per run and the store keeps working
