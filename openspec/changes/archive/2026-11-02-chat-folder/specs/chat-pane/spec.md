# Delta for Chat Pane

Term change: the "origin cwd" of the Purpose is now the "chat folder" (see chat-folder): it starts as the effective cwd of the pane the chat was opened from and can change during the chat. Note: the change `coding-tools` also adds requirements to chat-pane; this delta touches different requirements.

## MODIFIED Requirements

### Requirement: Chat pane kind and opening
`Ctrl+Space c` MUST open a chat: it splits the focused pane to the right exactly like `w v` (same minimum-size refusal, same focus rule), and the new pane is a chat pane. No shell is spawned and no PTY exists for it. The chat MAY be opened from a shell pane or from a chat pane, and several chat panes MAY exist at once in any tab. Opening a chat records the chat folder (the effective cwd of the pane it was opened from, or the current folder of the chat it was opened from), requests an engine detection run (single flight, see ai-engine-detection) and, when the folder is `$HOME`, `/` or unknown, shows the folder picker (see chat-folder) before the conversation list; otherwise it shows the conversation list directly (see chat-history).
(Previously: recorded the origin cwd and always showed the conversation list first)

#### Scenario: Open from a shell pane [auto]
- GIVEN one shell pane in TERMINAL
- WHEN `Ctrl+Space` then `c` are pressed
- THEN two panes exist side by side, the new pane is a focused chat pane, no SpawnPane effect is emitted and mode is TERMINAL

#### Scenario: Refused when too small [auto]
- GIVEN a focused pane already at the minimum size in the split direction
- WHEN `Ctrl+Space` then `c` are pressed
- THEN the layout is unchanged and no chat pane exists

#### Scenario: Several chats [auto]
- GIVEN a chat pane focused
- WHEN `Ctrl+Space` then `c` are pressed
- THEN a second, independent chat pane exists and each has its own composer and transcript

#### Scenario: Opening requests detection [auto]
- GIVEN no detection run in flight
- WHEN a chat pane is opened
- THEN exactly one detection run is requested; with a run in flight none is requested

#### Scenario: Chat in a real session [manual, Kitty]
- GIVEN the app in Kitty
- WHEN `Ctrl+Space c` is pressed
- THEN a chat pane appears beside the shell and typing goes to the chat, not to the shell

#### Scenario: Broad folder shows the picker [auto]
- GIVEN a shell pane whose cwd is `$HOME`
- WHEN `Ctrl+Space` then `c` are pressed
- THEN the chat pane shows the folder picker and detection is still requested once

#### Scenario: Project folder skips the picker [auto]
- GIVEN a shell pane whose cwd is an acceptable project folder
- WHEN a chat is opened
- THEN the conversation list is shown and no picker exists

### Requirement: Composer
The composer MUST accept typed characters, Backspace, Delete, Left, Right, Home and End, and insert a newline on the newline binding (see modal-input). A paste MUST be inserted as text and MUST NOT send the message. Control and format characters other than newline MUST be dropped on input and on paste, and CRLF MUST become a newline. The composer MUST hold at most 8000 characters; input beyond the cap is not accepted. Sending a message that is empty or only whitespace MUST do nothing. While a stream is running a send MUST be refused: the composer text is kept and a system line asks to wait or cancel. A refused or guidance-blocked send MUST keep the composer text. A composer text whose first token is exactly `/cd` MUST be handled as the folder command and not sent to the model (see chat-folder); `/cd` is the only composer command, and any other text starting with `/` is a normal message. A command that is refused keeps the composer text.
(Previously: no composer command existed; every non-blank text was a message)

#### Scenario: Type and send [auto]
- GIVEN an idle chat with a ready engine and the composer holding `hello`
- WHEN send is requested
- THEN a user message `hello` is appended, the composer is empty and a ChatSend effect names the pane

#### Scenario: Newline does not send [auto]
- GIVEN the composer holding `a`
- WHEN the newline binding is used and `b` is typed
- THEN the composer reads `a`, newline, `b` and no ChatSend is emitted

#### Scenario: Paste is not a send [auto]
- GIVEN an idle chat
- WHEN a paste of `x\r\ny\u{1b}[31m` arrives
- THEN the composer holds `x`, newline, `y[31m` (the ESC is dropped) and no ChatSend is emitted

#### Scenario: Blank send is ignored [auto]
- GIVEN the composer holding `   `
- WHEN send is requested
- THEN no message is added and no effect is emitted

#### Scenario: Send while streaming is refused [auto]
- GIVEN a stream running and the composer holding `next`
- WHEN send is requested
- THEN no second ChatSend is emitted, the composer still holds `next` and a system line asks to wait or cancel

#### Scenario: Composer cap [auto]
- GIVEN the composer holding 8000 characters
- WHEN another character is typed
- THEN the composer is unchanged

#### Scenario: Slash command is not sent [auto]
- GIVEN an idle chat with a ready engine and the composer holding `/cd /w/proj`
- WHEN send is requested
- THEN no ChatSend effect is emitted and no user message is appended

#### Scenario: Other slash text is sent [auto]
- GIVEN the composer holding `/usr/bin is where?`
- WHEN send is requested
- THEN a user message is appended and ChatSend is emitted

## ADDED Requirements

### Requirement: Folder views in the pane
The folder picker and the `/cd` confirmation MUST be drawn inside the chat pane area in place of the conversation view while open, like the memory views. While one is open the composer text, the transcript, the scroll position and any running stream MUST be kept and keep updating underneath; closing it returns to the conversation view (or, for the picker, to the conversation list). A pane has at most one such view at a time and never together with a memory view. The picker shows the candidate rows with the highlighted one marked, the typed-path field, the reason of the last refusal and a one-line key hint. All text drawn is already sanitized text in styled spans (folder names are sanitized like any displayed path), and rendering MUST NOT panic at any pane size down to 1x1. Keys: Up/Down move the highlight, printable characters edit the typed path, Tab completes, Enter accepts, Esc dismisses; no key reaches the composer while the picker is open.

#### Scenario: Picker drawn [auto, TestBackend + insta]
- GIVEN the picker with three candidates, the first highlighted, in a 50x16 pane
- WHEN rendered
- THEN the rows, the highlight, the typed-path field and the key hint are visible

#### Scenario: Composer survives [auto]
- GIVEN the composer holding `draft` and a `/cd` confirmation opened and cancelled
- WHEN the conversation view is shown again
- THEN the composer still holds `draft`

#### Scenario: Keys do not reach the composer [auto]
- GIVEN the picker open
- WHEN `x` is typed and a paste arrives
- THEN they edit the typed path and the composer is unchanged

#### Scenario: Tiny pane [auto]
- GIVEN a pane of any size from 1x1 to 10x2
- WHEN the picker and the confirmation are rendered
- THEN nothing panics

#### Scenario: Hostile folder name [auto]
- GIVEN a candidate directory name containing an escape sequence and a bidi override
- WHEN the picker is rendered
- THEN the drawn text holds no control or bidi characters
