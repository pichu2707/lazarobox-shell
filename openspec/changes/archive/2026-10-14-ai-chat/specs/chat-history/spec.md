# Chat History Specification

## Purpose

Every conversation is saved, organized by project, and can be resumed. This spec covers the SQLite store (location, file modes, secure delete, retention), deleting, resuming, the project tag and extra tags, the conversation list, and what happens when the store cannot be used. "Memory" here means organizing and resuming; nothing is summarized or shared between conversations. Nothing is encrypted at rest, and the README says so. Tags: [auto] = `cargo test` (temporary directories, a real SQLite file); [manual] = real machine.

Terms: "conversation" = an ordered list of messages plus metadata (profile, project tag, extra tags, timestamps). "Last activity" = the time of the latest message or tag edit. "Project tag" = the single project label of a conversation; "extra tags" = additional labels.

## ADDED Requirements

### Requirement: Store location and privacy
Conversations MUST be stored in one SQLite database file under `$XDG_DATA_HOME/lazarobox/`. When `XDG_DATA_HOME` is unset, empty or relative, the base MUST be `$HOME/.local/share`, only when `HOME` is absolute; with neither there is no store and the unavailable rule applies. The directory MUST be created with mode 0700 when missing. The store MUST hold only conversation data (messages, timestamps, profile, tags, the engine and model used); it MUST NOT hold credentials. The store MUST NOT be encrypted, and the README MUST say so and give the file path and how to delete it. Message text MUST NOT be written to any log.

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
- THEN it states that conversations are saved in plain text in an unencrypted SQLite file, gives its path and how to delete it

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
The store MUST keep at most 200 conversations. When a new conversation would make it 201, the conversation with the oldest last activity MUST be evicted, with its messages, in the same transaction. Retention is checked when a conversation is created. Tags do not protect a conversation, and the conversation being written is never the one evicted. A conversation that is open in any pane is also never evicted (the next oldest is chosen). The user MUST be told: the new conversation's transcript shows the system line `oldest conversation removed (limit 200)`. If the store holds more than 200 at that moment, enough of the oldest are evicted to return to 200.

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

### Requirement: Delete
In the conversation list, `d` MUST ask `Delete conversation? (y/n)`; only an unmodified lowercase `y` confirms, any other key cancels. Confirming MUST remove the conversation and all its messages (securely, see above) and refresh the list. If another pane has that conversation open, that pane keeps working, further messages are not persisted, and its transcript shows the system line `conversation deleted; new messages are not saved`.

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
- THEN the list keeps the row and shows the system line `could not delete conversation`

### Requirement: Resume
Enter on a list row MUST open that conversation in the pane with its full history: every stored message in order, partial ones marked, with its stored profile and tags. New messages MUST be appended to the same conversation. The engine used MUST be the current assignment of the `chat` task, not the stored one. The system prompt MUST use the conversation's stored profile (see prompt-profiles). A conversation already open in another pane MUST NOT be opened a second time: the list shows the system line `already open in another pane`.

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
