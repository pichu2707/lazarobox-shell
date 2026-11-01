# Delta for Chat History

Tags as in the main spec.

## MODIFIED Requirements

### Requirement: Store location and privacy
Conversations MUST be stored in one SQLite database file under `$XDG_DATA_HOME/lazarobox/`. When `XDG_DATA_HOME` is unset, empty or relative, the base MUST be `$HOME/.local/share`, only when `HOME` is absolute; with neither there is no store and the unavailable rule applies. The directory MUST be created with mode 0700 when missing. The store MUST hold only conversation data (messages, timestamps, profile, tags, the engine and model used) and usage data (counts, timings, engine, model, timestamps; see Usage table); it MUST NOT hold credentials. The store MUST NOT be encrypted, and the README MUST say so and give the file path and how to delete it. Message text MUST NOT be written to any log.
(Previously: only conversation data)

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
- THEN it states that conversations are saved in plain text in an unencrypted SQLite file, gives its path and how to delete it, and states what usage data the same file holds

## ADDED Requirements

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
