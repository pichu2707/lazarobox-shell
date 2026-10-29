# Delta for Chat History

## ADDED Requirements

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

## MODIFIED Requirements

### Requirement: Store location and privacy
Conversations MUST be stored in one SQLite database file under `$XDG_DATA_HOME/lazarobox/`. When `XDG_DATA_HOME` is unset, empty or relative, the base MUST be `$HOME/.local/share`, only when `HOME` is absolute; with neither there is no store and the unavailable rule applies. The directory MUST be created with mode 0700 when missing. The store MUST hold only conversation data (messages, timestamps, profile, tags, the engine and model used, and the last chat folder path); it MUST NOT hold credentials. The store MUST NOT be encrypted, and the README MUST say so, give the file path and how to delete it, and say that folder paths are saved in it. Message text MUST NOT be written to any log.
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
