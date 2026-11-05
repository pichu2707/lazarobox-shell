# Chat Folder Specification (delta: WSL drive access)

## ADDED Requirements

### Requirement: WSL drive folders
Under WSL, a Windows drive folder that the WSL drive read policy (agent-tools) allows MUST be an acceptable chat folder, chosen in the picker, with `/cd` or by resume. A drive root (`/mnt/<d>`), `<drive>/Users` and the own profile root are broad, like `$HOME`: accepted only when typed in the picker or given to `/cd`, never a candidate row, and the picker is shown again when the chat is reopened or resumed. A folder the policy refuses is refused with `not allowed`.

#### Scenario: A project on C: [auto]
- GIVEN WSL and `/mnt/c/Users/ana/Documents/app`
- WHEN it is given to `/cd` and confirmed
- THEN it is the chat folder

#### Scenario: Broad and refused drive folders [auto]
- GIVEN WSL
- WHEN `/mnt/c/Users/ana` is typed in the picker, and `/mnt/c/Windows` is given to `/cd`
- THEN the first is accepted as an explicit choice and the second is refused with `not allowed`

### Requirement: WSL drive grant
After the picker accepts, a `/cd` is confirmed or a conversation resumes in a Windows drive folder that is not broad, the chat pane MUST ask `Allow writes, commands and git in <folder>? (y/n)`. Only an unmodified lowercase `y` grants; any other key leaves the chat read-only in that folder, where writes, commands and git return the folder-level drive text of code-write. The grant covers that folder and its subfolders only. It MUST be held in memory for that chat: cleared on every folder change, never stored with the conversation (a resume asks again), and never inherited by another chat. The prompt MUST NOT be shown for a broad folder, a folder under `$HOME` or a refused folder, and MUST NOT be shown while the chat is busy (a `/cd` to the current folder while busy is refused like any `/cd`). The model MUST have no tool or text that opens or answers it.

#### Scenario: Granting [auto]
- GIVEN the prompt for `/mnt/c/Users/ana/Documents/app`
- WHEN `y` is pressed
- THEN writes, commands and git work in that folder

#### Scenario: Declining and asking again [auto]
- GIVEN the prompt for the same folder
- WHEN `n` is pressed, and later the user sends `/cd /mnt/c/Users/ana/Documents/app`
- THEN the chat stays read-only, and the second command shows the prompt again instead of `already in`

#### Scenario: Other keys [auto]
- GIVEN the prompt
- WHEN `Y`, Esc or `Alt+y` is pressed
- THEN no grant is given

#### Scenario: Not kept [auto]
- GIVEN a granted chat
- WHEN the folder changes and comes back, the conversation is resumed, or a new chat inherits the folder
- THEN each time the grant is gone and the prompt is shown again

## MODIFIED Requirements

### Requirement: One folder policy
Opening a chat, the picker, `/cd` and resuming a conversation MUST judge a folder by the same rules and the same code path as the tools, so that the picker never accepts a folder the tools would refuse as missing, not a directory or a secret root. A folder that is not an acceptable folder MUST be refused with a short reason: `folder not found`, `not a folder`, `not allowed` (secret root) or `/ is not a valid chat folder`. `$HOME` (and an ancestor of it other than `/`) is accepted only as an explicit choice (see below). A typed or `/cd` choice of a system or outside-`$HOME` folder follows the tools' refusal rules and is refused with `not allowed`, except, under WSL, a Windows drive folder that the WSL drive read policy allows (see WSL drive folders); `/` is always refused. (Amended 2026-10-28: the draft accepted outside-`$HOME` folders with a notice.) The folder stored, shown and used MUST be the resolved absolute path.

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

### Requirement: The `/cd` command
A composer text whose first token is exactly `/cd` (the text is `/cd`, or `/cd` followed by whitespace) MUST be treated as the user-only command to change the chat folder, MUST NOT be sent to the model and MUST NOT be stored as a message. Any other text beginning with `/` MUST be sent as a normal message. The argument is the rest of the line, trimmed, with no quoting. `~` and `~/...` expand to `$HOME`; a relative path is taken relative to the current chat folder (not the launch folder); `..` is allowed. With no argument the command shows the system line `usage: /cd <folder>`. A path equal to the current folder shows `already in <folder>` and changes nothing, except a Windows drive folder without a grant, which shows the grant prompt again (see WSL drive grant). `/cd` works whatever the engine state (it does not send). The model MUST have no tool or text it can use to trigger it.

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
