# Chat Pane Specification

## Purpose

The chat pane is a second kind of pane. It lives in the tile tree next to shell panes, owns its own conversation state (composer, transcript, stream), and talks to the engine assigned to the `chat` task (see chat-engines). It shows plain text and fenced code blocks only and does not see terminal output. Persistence is in chat-history, prompts in prompt-profiles, key routing in modal-input. Tags: [auto] = `cargo test` (pure state machine, fake engine, `TestBackend`); [manual] = real machine.

Terms: "idle" = no stream running; "streaming" = a request was sent and the answer is not finished; "system line" = a transcript line produced by lazarobox (error, notice, marker). System lines are never stored in the database and never sent to the model. "Origin cwd" = the effective cwd of the pane the chat was opened from.

## ADDED Requirements

### Requirement: Chat pane kind and opening
`Ctrl+Space c` MUST open a chat: it splits the focused pane to the right exactly like `w v` (same minimum-size refusal, same focus rule), and the new pane is a chat pane. No shell is spawned and no PTY exists for it. The chat MAY be opened from a shell pane or from a chat pane, and several chat panes MAY exist at once in any tab. Opening a chat records the origin cwd, requests an engine detection run (single flight, see ai-engine-detection) and shows the conversation list (see chat-history).

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

### Requirement: Pane content layout
A chat pane MUST show the transcript above and the composer below. The composer MUST grow with its content up to 5 rows and MUST never take the transcript below 1 row. All text MUST be drawn through styled spans of already sanitized text; model output MUST NOT be written to the terminal as raw bytes. Rendering MUST NOT panic at any pane size down to 1x1.

#### Scenario: Transcript and composer [auto, TestBackend + insta]
- GIVEN a chat with one user and one assistant message and an empty composer
- WHEN rendered in a 40x12 pane
- THEN the transcript is above and the composer row is at the bottom, with user and assistant messages visually distinct

#### Scenario: Composer growth is capped [auto]
- GIVEN a composer holding 9 lines in a pane 20 rows high
- WHEN rendered
- THEN the composer uses 5 rows and the transcript keeps the remaining rows

#### Scenario: Tiny pane [auto]
- GIVEN a chat pane of any size from 1x1 to 10x2
- WHEN rendered, with and without a stream running
- THEN nothing panics

### Requirement: Composer
The composer MUST accept typed characters, Backspace, Delete, Left, Right, Home and End, and insert a newline on the newline binding (see modal-input). A paste MUST be inserted as text and MUST NOT send the message. Control and format characters other than newline MUST be dropped on input and on paste, and CRLF MUST become a newline. The composer MUST hold at most 8000 characters; input beyond the cap is not accepted. Sending a message that is empty or only whitespace MUST do nothing. While a stream is running a send MUST be refused: the composer text is kept and a system line asks to wait or cancel. A refused or guidance-blocked send MUST keep the composer text.

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

### Requirement: Stream lifecycle
A send MUST append the user message, show a waiting state and start one request. Until the first byte arrives the pane MUST show `waiting for the model…`, and when the first byte has not arrived after 3 seconds it MUST show `loading model…` instead (a cold model load is not a hang). Each text delta MUST be appended to the assistant message in arrival order and drawn incrementally. A final event MUST finish the message, return the pane to idle and persist the exchange (see chat-history). An error MUST add an error system line, keep any partial text received before it, and return the pane to idle. A stream that ends without a final event (cut stream) MUST keep the partial text, add the system line `stream interrupted`, and return to idle. A pane MUST run at most one stream at a time; several chat panes MAY stream at once.

#### Scenario: First-byte wait [auto]
- GIVEN a send was issued and no byte arrived
- WHEN 1 second passes, then 3 seconds
- THEN the pane shows `waiting for the model…` and then `loading model…`

#### Scenario: Deltas in order [auto]
- GIVEN a stream waiting for its first byte
- WHEN the deltas `Hel`, `lo` and then the final event arrive
- THEN the assistant message reads `Hello`, the pane is idle and the waiting text is gone

#### Scenario: Error keeps partial text [auto]
- GIVEN a stream that delivered `Hel`
- WHEN an error event `connection reset` arrives
- THEN the assistant text `Hel` is kept, an error system line is shown, the pane is idle and the composer accepts a new message

#### Scenario: Error before any text [auto]
- GIVEN a send waiting for its first byte
- WHEN an error event arrives
- THEN no empty assistant message remains, an error system line is shown and the pane is idle

#### Scenario: Cut stream [auto]
- GIVEN a stream that delivered `Hel` and then the connection closes without a final event
- WHEN the end of the stream is detected
- THEN `Hel` is kept, the system line `stream interrupted` is shown and the pane is idle

#### Scenario: Error lines are not context [auto]
- GIVEN a conversation that ended with an error system line
- WHEN the next message is sent
- THEN the history given to the engine contains no system line

#### Scenario: Two chats stream independently [auto]
- GIVEN two chat panes each streaming
- WHEN a delta arrives for the first
- THEN only the first transcript changes

### Requirement: Cancel
Cancel MUST stop the stream of the focused chat pane, keep the partial answer in the transcript, mark it as cancelled, persist it as a partial message, return the pane to idle and emit a ChatCancel effect so the engine stops (server: socket closed; CLI: child killed, see chat-engines). Events of a cancelled stream that arrive afterwards MUST be dropped. Cancel on an idle pane MUST do nothing. Ctrl+C on an idle chat pane MUST NOT quit the app and MUST NOT write to any PTY.

#### Scenario: Cancel keeps the partial answer [auto]
- GIVEN a stream that delivered `Hel`
- WHEN cancel is requested
- THEN the assistant text `Hel` stays with a cancelled mark, a ChatCancel effect is emitted and the pane is idle

#### Scenario: Late events dropped [auto]
- GIVEN a cancelled stream
- WHEN a late delta, final or error event for it arrives
- THEN the transcript and the state are unchanged

#### Scenario: Cancel while waiting [auto]
- GIVEN a send waiting for its first byte
- WHEN cancel is requested
- THEN a ChatCancel effect is emitted, no empty assistant message remains and the pane is idle

#### Scenario: Cancel while idle [auto]
- GIVEN an idle chat pane
- WHEN cancel is requested
- THEN nothing changes and no effect is emitted

#### Scenario: Ctrl+C idle never quits [auto]
- GIVEN an idle focused chat pane in TERMINAL
- WHEN Ctrl+C is pressed
- THEN no Quit and no WritePty effect is emitted and the state is unchanged

### Requirement: Transcript content
The transcript MUST show messages as plain text, wrapped at the pane width, and fenced code blocks (lines between a line starting with three backticks and the next such line) drawn in a distinct style without the fence lines' backticks being interpreted further. No other markdown MUST be interpreted: `*`, `#`, `|` and the like are shown literally. An unclosed fence MUST render the rest of the message as code and MUST NOT panic, including while the message is still streaming. System lines MUST be visually distinct from messages.

#### Scenario: Code block [auto, TestBackend + insta]
- GIVEN an assistant message with text, a fenced block of two lines, and more text
- WHEN rendered
- THEN the two code lines use the code style and the surrounding text uses the plain style

#### Scenario: No other markdown [auto]
- GIVEN an assistant message `**bold** # title | a | b |`
- WHEN rendered
- THEN the text appears literally, unstyled

#### Scenario: Unclosed fence while streaming [auto]
- GIVEN a streaming message that so far reads `ok` then a line starting with three backticks and one code line
- WHEN rendered
- THEN the code line uses the code style and nothing panics

#### Scenario: Long line wraps [auto]
- GIVEN a message line longer than the pane width
- WHEN rendered
- THEN the line wraps within the pane and no row is drawn outside it

### Requirement: Transcript scrolling
PageUp and PageDown MUST scroll the transcript by one page. While the view is at the bottom it MUST follow new deltas. While scrolled up the view MUST stay on the same content when deltas arrive. Sending a message MUST return the view to the bottom. Scrolling MUST clamp at both ends.

#### Scenario: Follow at the bottom [auto]
- GIVEN the view at the bottom
- WHEN deltas make the transcript longer
- THEN the last line stays visible

#### Scenario: Pinned when scrolled up [auto]
- GIVEN the view scrolled up 10 lines
- WHEN 5 more lines of delta arrive
- THEN the same content stays visible

#### Scenario: Send returns to the bottom [auto]
- GIVEN the view scrolled up
- WHEN a message is sent
- THEN the view is at the bottom

### Requirement: Engine guidance states
Before a message is sent, and again on every send attempt, the engine for the chat task MUST be resolved (see chat-engines). When the engine is not ready the pane MUST show a guidance state in the transcript area and MUST NOT send anything; the composer text is kept. The engine MUST NEVER be switched silently: no other engine is tried and the assignment is never changed. The guidance MUST update without reopening the pane when a detection result arrives or when a saved menu change alters the assignment. Messages (the exact text of the first two is normative, the others MUST contain the quoted parts):
- unassigned (neither `chat` nor `general` has an engine): `No engine assigned to chat` and the hint to open the menu with `Ctrl+Space m`;
- undetected (engine not `available`): the engine id and its status text, and the hint to start or install it and refresh from the menu with `r`;
- unsupported (`opencode`): `not supported in chat yet`;
- no jail (`codex` or `gemini` on a system without a working CLI jail, see chat-engines): `<engine>: not available on this system yet`;
- CLI gated off (an adapter exists but a manual gate failed, see chat-engines): the engine id and `gated off`;
- server with no model, or a model that is not detected: the engine id, the model name when set, and `no model` or `download required`.
While the first detection result has not arrived, the pane MUST show `checking engines…` and a send MUST wait for the result instead of failing.

#### Scenario: Unassigned [auto]
- GIVEN neither `chat` nor `general` is assigned
- WHEN the pane is shown and send is attempted
- THEN the guidance contains `No engine assigned to chat`, no ChatSend is emitted and the composer text is kept

#### Scenario: Undetected [auto]
- GIVEN `chat` is `ollama` / `llama3:8b` and ollama is `not running`
- WHEN send is attempted
- THEN the guidance names `ollama` and `not running`, no ChatSend is emitted and no other engine is used

#### Scenario: Unsupported opencode [auto]
- GIVEN `chat` is `opencode` and opencode is `available`
- WHEN send is attempted
- THEN the guidance reads `not supported in chat yet` and no ChatSend is emitted

#### Scenario: CLI gated off [auto]
- GIVEN `chat` is `codex`, codex is `available`, and the codex adapter is gated off
- WHEN send is attempted
- THEN the guidance names `codex` and `gated off`, and no process is started

#### Scenario: No jail [auto]
- GIVEN `chat` is `gemini`, gemini is `available`, and the CLI jail is unavailable
- WHEN send is attempted
- THEN the guidance reads `gemini: not available on this system yet`, no process is started and the composer text is kept

#### Scenario: Model not downloaded [auto]
- GIVEN `chat` is `ollama` / `old:7b` and ollama lists only `llama3:8b`
- WHEN send is attempted
- THEN the guidance names `old:7b` and `download required` and no ChatSend is emitted

#### Scenario: Checking engines [auto]
- GIVEN no detection result has arrived and `chat` is `ollama` / `llama3:8b`
- WHEN the pane is shown
- THEN it shows `checking engines…`, and when the result arrives with ollama available and the model listed the guidance disappears without reopening

#### Scenario: Fix without reopening [auto]
- GIVEN the unassigned guidance is showing
- WHEN the menu saves `chat` as `ollama` / `llama3:8b` (available)
- THEN the guidance is gone and a send emits ChatSend

### Requirement: State survives modes, COPY and tabs
A chat pane's state (composer text and cursor, transcript, scroll position, running stream) MUST survive every mode change (PREFIX, RESIZE, COPY on another pane, MENU), focus changes, zoom, splits of other panes and tab switches. A running stream MUST keep running in the background while its pane is not focused, not visible, or its tab is not active; deltas MUST keep accumulating and a final event MUST persist the exchange. While MENU is open no key or paste reaches the composer.

#### Scenario: Stream across a tab switch [auto]
- GIVEN a stream running in a chat pane of tab 1 and tab 2 becomes active
- WHEN deltas `ab` and `cd` and the final event arrive
- THEN returning to tab 1 shows the assistant message `abcd`, the pane is idle and the exchange was persisted

#### Scenario: Stream during COPY [auto]
- GIVEN COPY on a shell pane and a stream running in a chat pane of the same tab
- WHEN deltas arrive and COPY is left
- THEN the chat transcript contains them and the shell pane's COPY state was not affected by them

#### Scenario: Composer survives MENU [auto]
- GIVEN the composer holding `draft` and the menu opened and closed
- WHEN the chat pane is shown again
- THEN the composer still holds `draft`

#### Scenario: Keys do not reach the composer in MENU [auto]
- GIVEN MENU open over a focused chat pane with composer `draft`
- WHEN `x` is typed and a paste arrives
- THEN the composer still holds `draft`

#### Scenario: Zoom and focus [auto]
- GIVEN a streaming chat pane next to a shell pane
- WHEN focus moves to the shell, the shell is zoomed and unzoomed
- THEN the chat pane's transcript and stream are intact

#### Scenario: Stream survives a long background run [manual, Kitty]
- GIVEN a long answer streaming in a chat pane
- WHEN the user switches tabs, enters COPY elsewhere, and returns
- THEN the answer is complete and was not duplicated or lost

### Requirement: Yank
Ctrl+Y on a focused chat pane MUST copy the text of the last finished (or cancelled) assistant message to the clipboard through the existing SetClipboard path, subject to the same size cap as copy-selection, and MUST show the notice `Copied N characters` (singular for 1) through the existing notice slot. With no assistant message, or an empty one, it MUST show `Nothing to copy`. Over the cap it MUST show `Selection too large to copy`. Ctrl+B does the same for the last fenced code block of the last assistant message (the code lines without the fence lines); with no code block it MUST show `Nothing to copy`. Yank MUST NOT change the transcript.

#### Scenario: Yank last answer [auto]
- GIVEN an assistant message `hello`
- WHEN Ctrl+Y is pressed
- THEN a SetClipboard effect carries `hello` and the notice reads `Copied 5 characters`

#### Scenario: Nothing to yank [auto]
- GIVEN a chat with no assistant message
- WHEN Ctrl+Y is pressed
- THEN no SetClipboard effect is emitted and the notice reads `Nothing to copy`

#### Scenario: Yank the last code block [auto]
- GIVEN an assistant message with text, a fenced block `let x = 1;` and more text
- WHEN Ctrl+B is pressed
- THEN a SetClipboard effect carries `let x = 1;` without the fence lines; with no fenced block the notice reads `Nothing to copy`

#### Scenario: Yank is clean text [auto]
- GIVEN an assistant message that was sanitized on arrival
- WHEN Ctrl+Y is pressed
- THEN the copied text contains no control or escape characters

### Requirement: Closing a chat pane
Closing a chat pane (confirmed `w q`, closing its tab, or quitting) MUST cancel its stream, keep the partial answer persisted, and make every later event for that pane be dropped silently. A conversation is already saved, so closing never asks about unsaved chat data.

#### Scenario: Close while streaming [auto]
- GIVEN a chat pane streaming `Hel`
- WHEN the pane is closed
- THEN a ChatCancel effect is emitted, `Hel` is persisted as a partial message and the pane is gone

#### Scenario: Stale events after close [auto]
- GIVEN a closed chat pane
- WHEN a late delta, final or error event for it arrives
- THEN it is ignored with no panic and no effect

### Requirement: Context truncation marker
When the history sent to the engine omits older messages because of the client-side cap (see chat-engines), the transcript MUST show the system line `earlier messages are not sent to the model` once, at the position of the oldest message that is sent. The marker MUST NOT be stored.

#### Scenario: Marker shown [auto]
- GIVEN a conversation longer than the history cap
- WHEN a message is sent
- THEN the marker appears before the oldest sent message and is absent from the stored conversation

#### Scenario: Short conversation has no marker [auto]
- GIVEN a conversation within the cap
- WHEN a message is sent
- THEN no marker is shown
