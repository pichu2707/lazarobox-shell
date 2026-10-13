# Delta for Chat History

Tags: [auto] = `cargo test` (temporary directories, a real SQLite file, fake engram); [manual] = real machine. This delta changes the store: chat.db goes to schema version 2 with two link tables (the proposal's earlier "chat.db is not changed" is superseded, ruling ME1). All modes, privacy and secure-delete rules of the store requirements stay as specified in ai-chat and apply to the new tables. Reasoning is still never stored.

## ADDED Requirements

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

#### Scenario: Old build refuses v2 [auto]
- GIVEN a v2 database and a store opener that supports only v1
- WHEN it opens
- THEN history is unavailable with the usual system line and the file bytes are unchanged

#### Scenario: Link rows hold no content and keep file modes [auto]
- GIVEN a saved note with content `SECRET-BODY`
- WHEN the database file and side files are inspected
- THEN `SECRET-BODY` is not found and the modes are 0600

## MODIFIED Requirements

### Requirement: Retention
The store MUST keep at most 200 conversations. When a new conversation would make it 201, the conversation with the oldest last activity MUST be evicted, with its messages, in the same transaction. Retention is checked when a conversation is created. Tags do not protect a conversation, and the conversation being written is never the one evicted. A conversation that is open in any pane is also never evicted (the next oldest is chosen). The user MUST be told: the new conversation's transcript shows the system line `oldest conversation removed (limit 200)`. If the store holds more than 200 at that moment, enough of the oldest are evicted to return to 200. Eviction is automatic: it MUST NEVER prompt and MUST NEVER delete notes in engram. The same transaction counts the note rows of the evicted conversations, and the creation reply carries that count (`evicted_notes`). When the count is not zero, a second system line follows the first: `N notes of removed conversations remain in engram` (`1 note of a removed conversation remains in engram` for one). This holds whether or not memory is currently on, and without any engram request.
(Previously: the user was told only with the line `oldest conversation removed (limit 200)`, with no mention of notes)

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
In the conversation list, `d` MUST ask for confirmation; only an unmodified lowercase key listed in the prompt confirms, any other key cancels. With no notes to offer, the prompt is `Delete conversation? (y/n)` and `y` confirms. When the conversation has note rows, memory is on and a token is present (`ENGRAM_HTTP_TOKEN` set and non-empty; the app knows only that it is present), the prompt is `Delete conversation? y · n · a also delete its N notes` (`its 1 note`): `y` deletes the conversation and keeps the notes, `a` deletes the conversation and then its notes, anything else cancels. If notes exist but memory is off, or the token is missing, the prompt is the plain one and, after a confirmed delete, one system line says why the notes stay (see chat-memory). Confirming MUST remove the conversation and all its messages (securely, see above) and refresh the list; the delete transaction returns the `(project, observation id)` rows of the conversation's notes before they are cascaded. If another pane has that conversation open, that pane keeps working, further messages are not persisted, and its transcript shows the system line `conversation deleted; new messages are not saved`. For `a`, the notes are deleted afterwards (see engram-client) and never affect the conversation delete, which has already happened. The result is one system line: `deleted N notes from engram`, `deleted K of N notes from engram` or `could not delete notes: <short reason>`.
(Previously: the prompt was always `Delete conversation? (y/n)` and notes were not considered)

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
