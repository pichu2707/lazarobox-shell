# Chat Folder Specification

## Purpose

Let the user open lazarobox from anywhere (rofi, a desktop launcher, ssh, macOS without cwd reporting) and have each chat work in the right project folder, without ever choosing a folder silently. Covers the folder picker shown when a chat opens, the user-only `/cd` command, which folders are acceptable, what changes when the folder changes, and inheritance between chats. Where the folder is stored is in chat-history; the start folder of the whole app is in start-dir; tool behaviour in the folder is in agent-tools, command-runner, code-search and git-tools.

Dependency: the tool capabilities named above live in the unarchived change `coding-tools`; this change MUST be archived after `coding-tools`. Tags: [auto] = `cargo test` (pure state machine, temporary directories, fake engine, `TestBackend`); [manual] = real machine.

Terms: "chat folder" = the directory a chat uses for its tools, context block, memory project and new panes (called "origin cwd" before this change). "Broad" = the folder is `$HOME`, `/`, any ancestor of `$HOME` (for example `/home`), a folder the tools already refuse as a root (a system folder such as `/usr` or `/etc`, a secret root, or any folder outside `$HOME`; the same criterion as the tool root policy) or unknown (user decision 2026-10-28). "Unknown" = no cwd was reported, or it is not absolute, does not exist or is not a directory. "Acceptable folder" = an existing directory that is not `/` and is not a secret root under the tool root policy (see code-write, Root policy), after resolving `..` and symlinks. "Busy" = the model is answering, writes are staged or proposed, an approval is pending, a command runs, or a note is still being saved (design D4). "User-only" = the model has no way to call it.

## Requirements

### Requirement: One folder policy
Opening a chat, the picker, `/cd` and resuming a conversation MUST judge a folder by the same rules and the same code path as the tools, so that the picker never accepts a folder the tools would refuse as missing, not a directory or a secret root. A folder that is not an acceptable folder MUST be refused with a short reason: `folder not found`, `not a folder`, `not allowed` (secret root) or `/ is not a valid chat folder`. `$HOME` (and an ancestor of it other than `/`) is accepted only as an explicit choice (see below). A typed or `/cd` choice of a system or outside-`$HOME` folder follows the tools' refusal rules and is refused with `not allowed`; `/` is always refused. (Amended 2026-10-28: the draft accepted outside-`$HOME` folders with a notice.) The folder stored, shown and used MUST be the resolved absolute path.

#### Scenario: Missing, file, secret root, root [auto]
- GIVEN a missing path, a regular file, a secret-root directory, a system folder (`/usr`) and `/`
- WHEN each is typed in the picker and given to `/cd`
- THEN each is refused with its reason and the chat folder is unchanged

#### Scenario: Same verdict as the tools [auto]
- GIVEN any directory
- WHEN the picker policy and the tool root policy judge it for "exists, directory, not secret root"
- THEN the two verdicts are equal

#### Scenario: Symlink resolved [auto]
- GIVEN `/w/link` pointing to the directory `/w/proj`
- WHEN `/w/link` is chosen
- THEN the chat folder is `/w/proj`

### Requirement: Picker at open
When a chat pane is opened and its folder is broad, the pane MUST show a folder picker before the conversation list, and no conversation view until the picker is resolved. When the folder is an acceptable folder that is not broad, no picker is shown and the chat opens as before. The conversation list that follows MUST be filtered by the project tag of the folder the user chose (see chat-history). Opening the picker MUST NOT choose any folder.

#### Scenario: Opened from `$HOME` [auto]
- GIVEN the focused pane's folder is `$HOME`
- WHEN `Ctrl+Space c` is pressed
- THEN the new chat pane shows the picker and no conversation list

#### Scenario: Opened from `/` or an unknown folder [auto]
- GIVEN the focused pane's folder is `/`, and in another run unknown
- WHEN a chat is opened
- THEN the picker is shown both times

#### Scenario: Opened from an ancestor or a system folder [auto]
- GIVEN `HOME=/home/u` and the focused pane's folder is `/home`, and in other runs `/usr` and `/srv/x`
- WHEN a chat is opened
- THEN the picker is shown each time and none of those folders is a candidate row

#### Scenario: Opened from a project [auto]
- GIVEN the focused pane's folder is an acceptable folder `/w/proj`
- WHEN a chat is opened
- THEN no picker is shown and the list is that of `proj`

#### Scenario: List follows the choice [auto]
- GIVEN the picker shown and conversations tagged `proj` and `other`
- WHEN `/w/proj` is accepted
- THEN the conversation list shows the `proj` conversations

#### Scenario: Rofi launch [manual]
- GIVEN lazarobox started from rofi (folder `$HOME`) and a previous chat in a project
- WHEN a chat is opened
- THEN the picker appears with that project selected and one Enter opens the chat there

### Requirement: Picker candidates
The picker MUST offer these candidates, each shown once (duplicates by resolved path collapsed), only when they are acceptable folders and not `$HOME`: the last used project folder, the git roots of the current tab's shell panes, recent chat folders from the conversation store (newest last activity first), and the last non-home shell cwd of the tab. The last used project folder MUST be first and pre-selected, so one Enter accepts it; when there is none, the first candidate is highlighted. A highlighted candidate is never accepted without a key press. The list MUST be bounded (at most 12 rows). A candidate that no longer exists when the picker is drawn or accepted MUST be omitted or refused, never accepted. A store that is unavailable contributes no candidates and does not block the picker. The picker also MUST have a typed-path field (see next requirement).

#### Scenario: Order and pre-selection [auto]
- GIVEN a last used project A, two panes with git roots B and C, recent chat folders A and D, and a shell cwd E
- WHEN the picker opens
- THEN the rows are A, B, C, D, E (A once) and A is highlighted

#### Scenario: One Enter [auto]
- GIVEN the picker with A pre-selected
- WHEN Enter is pressed
- THEN the chat folder is A and the conversation list opens

#### Scenario: Unusable candidates are dropped [auto]
- GIVEN candidates that are `$HOME`, `/`, a deleted directory and a secret root
- WHEN the picker opens
- THEN none of them is listed

#### Scenario: No candidates [auto]
- GIVEN no pane roots, no history and no shell cwd
- WHEN the picker opens
- THEN it shows only the typed-path field with a one-line hint, Enter on an empty field does nothing and Esc still works

#### Scenario: Bounded list [auto]
- GIVEN 30 recent chat folders
- WHEN the picker opens
- THEN at most 12 rows are shown

#### Scenario: Store unavailable [auto]
- GIVEN an unavailable conversation store and one pane git root
- WHEN the picker opens
- THEN the pane root is offered and the picker works

### Requirement: Typed path
The picker MUST accept a typed path. `~` and `~/...` MUST expand to `$HOME`; a relative path is taken relative to the folder the chat would have had without the picker, and is refused (`folder not found`) when that folder is unknown. Tab SHOULD complete directory names only. Enter with a non-empty field accepts the field (it wins over the highlighted row); Enter with an empty field accepts the highlighted candidate. A refused path MUST keep the picker open with the typed text and the reason shown.

#### Scenario: Typed absolute path [auto]
- GIVEN the picker and the text `/w/proj`
- WHEN Enter is pressed
- THEN the chat folder is `/w/proj`

#### Scenario: Tilde [auto]
- GIVEN `HOME=/h` and the text `~/code/proj`, which exists
- WHEN Enter is pressed
- THEN the chat folder is `/h/code/proj`

#### Scenario: Refused path keeps the picker [auto]
- GIVEN the text `/nope`
- WHEN Enter is pressed
- THEN the picker stays open with the text and `folder not found`

#### Scenario: Relative path [auto]
- GIVEN the chat would have opened in `/h` and the text `code/proj`
- WHEN Enter is pressed
- THEN the chat folder is `/h/code/proj`; with an unknown base the path is refused

#### Scenario: Completion [auto]
- GIVEN `/w` holding the directories `proj` and `pkg` and the file `plan.txt`, and the text `/w/p`
- WHEN Tab is pressed
- THEN only the directories are completion options

### Requirement: Esc and no silent choice
Esc in the picker MUST keep today's behaviour: the chat opens in the broad folder the pane had, the conversation list follows, and nothing is stored as a choice. The pane MUST add one system line naming that folder and `/cd`, so the state is visible. A folder MUST NEVER become the chat folder without a key press by the user (Enter, `y`), including the last used project. With Esc, tools keep behaving as the root policy dictates for a broad folder (see agent-tools).

#### Scenario: Esc [auto]
- GIVEN the picker opened from `$HOME`
- WHEN Esc is pressed
- THEN the chat folder is `$HOME`, the conversation list opens and a system line mentions `/cd`

#### Scenario: Never silent [auto]
- GIVEN a last used project and no key pressed
- WHEN time passes, or a frame is drawn, or the engine detection result arrives
- THEN the picker is still open and the chat folder is unchanged

### Requirement: Explicit `$HOME`
`$HOME` MUST be accepted as a chat folder only when the user explicitly picks it by typing it (`~`, `$HOME`'s path) in the picker or with `/cd`; it is never a candidate row. Choosing it does not relax any tool policy: tools that refuse a `$HOME` root keep refusing, and reads that ask in a broad folder keep asking, with a cloud engine as with any other. A chat whose folder is `$HOME` shows the picker again when it is reopened or resumed (the folder is broad).

#### Scenario: Home picker every time [auto]
- GIVEN a conversation whose stored folder is an explicit `$HOME`
- WHEN it is resumed, on two separate occasions
- THEN the picker is shown each time and Esc keeps `$HOME`

#### Scenario: Typed home [auto]
- GIVEN the picker and the text `~`
- WHEN Enter is pressed
- THEN the chat folder is `$HOME`

#### Scenario: Home does not widen tools [auto]
- GIVEN a chat whose folder was explicitly chosen as `$HOME` and a cloud engine
- WHEN `run_command` or a read outside the narrow policy is attempted
- THEN it is refused or asks exactly as with an unchosen `$HOME`

#### Scenario: Home is not offered [auto]
- GIVEN a shell pane whose cwd is `$HOME`
- WHEN the picker opens
- THEN `$HOME` is not a row

### Requirement: The `/cd` command
A composer text whose first token is exactly `/cd` (the text is `/cd`, or `/cd` followed by whitespace) MUST be treated as the user-only command to change the chat folder, MUST NOT be sent to the model and MUST NOT be stored as a message. Any other text beginning with `/` MUST be sent as a normal message. The argument is the rest of the line, trimmed, with no quoting. `~` and `~/...` expand to `$HOME`; a relative path is taken relative to the current chat folder (not the launch folder); `..` is allowed. With no argument the command shows the system line `usage: /cd <folder>`. A path equal to the current folder shows `already in <folder>` and changes nothing. `/cd` works whatever the engine state (it does not send). The model MUST have no tool or text it can use to trigger it.

#### Scenario: Not a message [auto]
- GIVEN an idle chat and the composer `/cd /w/proj`
- WHEN send is requested
- THEN no ChatSend effect is emitted, no message is appended and the folder confirmation opens

#### Scenario: Other slash text is a message [auto]
- GIVEN the composer `/etc/passwd explain this` and, separately, `/cdx foo`
- WHEN send is requested
- THEN each is sent as a normal message

#### Scenario: Relative to the chat folder [auto]
- GIVEN the chat folder `/w/proj` with a subdirectory `src`
- WHEN `/cd src` and `/cd ..` are run
- THEN the proposed folders are `/w/proj/src` and `/w`

#### Scenario: Tilde and usage [auto]
- GIVEN `HOME=/h`
- WHEN `/cd ~/code` and bare `/cd` are run
- THEN the proposed folder is `/h/code` and the second shows `usage: /cd <folder>`

#### Scenario: Same folder [auto]
- GIVEN the chat folder `/w/proj`
- WHEN `/cd /w/proj` is run
- THEN the line `already in /w/proj` is shown and no confirmation opens

#### Scenario: Invalid target [auto]
- GIVEN a missing path, a file, a secret root and `/`
- WHEN each is given to `/cd`
- THEN each shows its reason as a system line, the folder is unchanged and the composer text is kept

#### Scenario: Model cannot call it [auto]
- GIVEN the registered tool list and a model answer that contains `/cd /tmp`
- WHEN the tool list and the answer are inspected
- THEN no tool changes the folder and the answer text changes nothing

### Requirement: `/cd` confirmation
A valid `/cd` MUST always ask before changing the folder, drawn inside the chat pane, showing the old and the new folder: `Change folder? <old> -> <new> (y/n)`. Only an unmodified lowercase `y` confirms; any other key cancels and changes nothing. Busy state MUST be checked again at the moment of `y`; if the chat became busy meanwhile the change is refused with the same reason as below. The composer text is cleared on a confirmed change and kept on cancel or refusal.

#### Scenario: Confirm [auto]
- GIVEN the prompt for `/cd /w/b` from `/w/a`
- WHEN `y` is pressed
- THEN the chat folder is `/w/b` and the composer is empty

#### Scenario: Cancel keys [auto]
- GIVEN that prompt
- WHEN `n`, Esc, `x`, `Y` and Enter are pressed in separate runs
- THEN the folder is unchanged in each and the composer text is kept

#### Scenario: Prompt shows both folders [auto, TestBackend + insta]
- GIVEN that prompt rendered in 60x10
- WHEN drawn
- THEN both the old and the new folder are visible

#### Scenario: Busy at confirmation [auto]
- GIVEN the prompt open and a stream started elsewhere before `y`
- WHEN `y` is pressed
- THEN the change is refused with a reason and the folder is unchanged

### Requirement: `/cd` refused while busy
`/cd` MUST be refused, before any prompt, with the system line `/cd refused: <reason>` while any of these holds: writes are staged or proposed (`writes are staged or proposed`), an approval is pending (`an approval is pending`), a command is running (`a command is running`), the model is answering (`the model is answering`), or a note is still being saved (`a note is still being saved`). The composer text is kept and nothing changes.

#### Scenario: Each reason [auto]
- GIVEN, in separate runs, a staged batch, a proposed batch, a pending approval, a running command and a running stream
- WHEN `/cd /w/b` is run
- THEN each time the matching `/cd refused:` line is shown, no prompt opens and the folder is unchanged

#### Scenario: Idle works [auto]
- GIVEN an idle chat with nothing staged
- WHEN `/cd /w/b` is run
- THEN the confirmation opens

### Requirement: After a folder change
After a confirmed change (and after the picker accepts a folder) the chat folder MUST be the new folder for every later use: tool roots, the working directory of commands and git, the context block, the memory project (see chat-memory), the folder stored for the conversation (see chat-history) and the cwd given to panes opened from this chat (see terminal-session). The pane MUST add the system line `folder changed: <old> -> <new>` (not stored, not sent). The next request to the engine MUST carry a notice that the folder changed from `<old>` to `<new>`, that earlier paths may not exist, and that tool paths are relative to the new folder; the notice is not a stored message and is not repeated afterwards. The conversation, its messages, tags and mode continue unchanged. An approval remembered for the old folder MUST NOT apply to the new one. Pending work of the old folder cannot exist (busy is refused).

#### Scenario: Line and notice [auto]
- GIVEN a confirmed change from `/w/a` to `/w/b`
- WHEN the transcript and the next engine request are inspected
- THEN the transcript has `folder changed: /w/a -> /w/b`, the request carries the model notice naming both folders, and the stored messages contain neither

#### Scenario: Notice sent once [auto]
- GIVEN that change and two later sends
- WHEN both requests are inspected
- THEN only the first carries the notice

#### Scenario: Tools follow [auto]
- GIVEN a change to `/w/b`
- WHEN `read_file` with a relative path and `run_command` are called
- THEN both resolve against `/w/b`

#### Scenario: Conversation continues [auto]
- GIVEN a conversation of 4 messages and a change
- WHEN the next message is sent
- THEN the same conversation holds 6 messages and no new conversation exists

#### Scenario: Approvals do not carry over [auto]
- GIVEN an approval remembered for a path in `/w/a`
- WHEN the folder changes to `/w/b`
- THEN the same relative path in `/w/b` asks again

### Requirement: Inheritance between chats
A chat opened from another chat pane MUST start in that chat's current folder. When that folder is broad the picker is shown as for any broad folder. A chat opened from a shell pane starts in that pane's effective cwd. Changing the folder of one chat MUST NOT change any other chat or shell pane.

#### Scenario: Inherit [auto]
- GIVEN a chat whose folder was changed to `/w/b`
- WHEN a second chat is opened from it
- THEN the second chat's folder is `/w/b` and no picker is shown

#### Scenario: Inherit a broad folder [auto]
- GIVEN a chat whose folder is `$HOME`
- WHEN a second chat is opened from it
- THEN the picker is shown

#### Scenario: Independent chats [auto]
- GIVEN two chats in `/w/a` and a shell pane in `/w/a`
- WHEN the first chat changes to `/w/b`
- THEN the second chat and the shell pane stay in `/w/a`

### Requirement: Unknown folder on macOS or ssh
A shell pane that never reported a cwd (no OSC 7 and no poll value) MUST be treated as an unknown folder, so a chat opened from it shows the picker; the app MUST NOT guess a folder.

#### Scenario: No cwd reported [auto]
- GIVEN a shell pane with no known cwd
- WHEN a chat is opened from it
- THEN the picker is shown

#### Scenario: Remote session [manual]
- GIVEN lazarobox on macOS or over ssh/tmux where the shell reports no cwd
- WHEN a chat is opened
- THEN the picker appears and a typed path works
