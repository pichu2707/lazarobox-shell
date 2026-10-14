# Chat Pane Specification

## Purpose

The chat pane is a second kind of pane. It lives in the tile tree next to shell panes, owns its own conversation state (composer, transcript, stream), and talks to the engine assigned to the `chat` task (see chat-engines). It shows plain text and fenced code blocks only and does not see terminal output. Persistence is in chat-history, prompts in prompt-profiles, key routing in modal-input. Tags: [auto] = `cargo test` (pure state machine, fake engine, `TestBackend`); [manual] = real machine.

Terms: "idle" = no stream running; "streaming" = a request was sent and the answer is not finished; "system line" = a transcript line produced by lazarobox (error, notice, marker). System lines are never stored in the database and never sent to the model. "Origin cwd" = the effective cwd of the pane the chat was opened from. "Reasoning" = engine thinking text delivered as thinking deltas, separate from the answer (see chat-engines); it is shown but never stored and never sent back to an engine (see chat-history).

## Requirements

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

### Requirement: Stream phase
A chat pane with a running stream MUST expose a phase, a start instant and the model of the request as sent. The phase is `thinking` from the send until the first non-empty text delta arrives (this covers waiting for the first byte and a stream that has only delivered reasoning), and `answering` from the first non-empty text delta until the terminal event. An idle pane has no phase. The statusline reads these values (see statusline). The pane's own waiting texts are unchanged: `waiting for the model…` and, after 3 seconds without a first byte, `loading model…`; a thinking delta counts as the first byte, after which the waiting text is replaced by the reasoning block.

#### Scenario: Phase while waiting [auto]
- GIVEN a send was issued and no byte arrived
- WHEN the phase is read, and again after 3 seconds
- THEN it is `thinking` both times, and the pane shows `waiting for the model…` and then `loading model…`

#### Scenario: Phase with reasoning only [auto]
- GIVEN a stream that delivered only thinking deltas
- WHEN the phase is read
- THEN it is `thinking`

#### Scenario: Phase flips on the first answer text [auto]
- GIVEN a stream in phase `thinking` that delivered reasoning
- WHEN the first non-empty text delta arrives
- THEN the phase is `answering` and stays so until the terminal event

#### Scenario: No phase when idle [auto]
- GIVEN a stream that ends by final, by error, by cut stream or by cancel
- WHEN the terminal event is handled
- THEN the pane has no phase, no start instant and no streaming flag

#### Scenario: Thinking delta replaces the waiting text [auto]
- GIVEN a stream showing `loading model…`
- WHEN the first thinking delta arrives
- THEN the waiting text is gone and the reasoning block is drawn

#### Scenario: Request model is kept [auto]
- GIVEN a send with model `llama3:8b` and the `chat` assignment changed in the menu while it streams
- WHEN the pane is read
- THEN it still reports `llama3:8b` for that stream

### Requirement: Reasoning block
When an engine sends thinking deltas, the pane MUST draw the reasoning of the turn in the transcript, before that turn's answer, as plain text in the `text_muted` style, every line prefixed by the gutter `┊ `, growing incrementally while it streams. Reasoning text is sanitized with the same sanitizer rules as answer text (its own sanitizer state), wrapped at the pane width less the gutter, and shown literally: no markdown and no fences are interpreted. Reasoning is not a message: it does not count toward the 400 messages per conversation and is not part of yank. When the first non-empty text delta of the turn arrives, or the final event arrives with no answer text, the block MUST collapse once into the single line `▸ reasoning (N.Ns)`, where N.N is the seconds from the first thinking delta to the collapse, with one decimal. The toggle key (Ctrl+R, see modal-input) MUST expand the most recent reasoning block of the conversation, showing the line `▾ reasoning (N.Ns)` followed by the gutter-prefixed dimmed text, and collapse it again on the next press; it does nothing while that reasoning is still streaming or when there is no reasoning. A block is collapsed by default. A thinking delta that arrives after the block collapsed MUST be dropped. A turn with no thinking delta MUST draw no reasoning line, and the pane renders exactly as before for engines without reasoning.

If the stream is cut, cancelled or fails while reasoning is streaming, the partial reasoning MUST be kept and collapsed to the same single line, and the existing outcome is unchanged: cancel shows no new system line, an error shows its error system line, a cut stream shows `stream interrupted`; no empty assistant message remains when no answer text arrived.

#### Scenario: Reasoning streams dimmed with a gutter [auto, TestBackend + insta]
- GIVEN a stream that delivered thinking deltas `Let me` and ` check`
- WHEN rendered
- THEN the transcript shows `┊ Let me check` in `text_muted` above the (empty) answer, and a further thinking delta extends that text

#### Scenario: Collapse on the first answer text [auto, TestBackend + insta]
- GIVEN reasoning that streamed for 2.3 seconds (injected clock)
- WHEN the first text delta `Hi` arrives
- THEN the reasoning lines are replaced by the single line `▸ reasoning (2.3s)` followed by the answer `Hi`

#### Scenario: Collapse on final with no answer text [auto]
- GIVEN reasoning that streamed for 1.0 seconds and no text delta
- WHEN the final event arrives
- THEN the block shows `▸ reasoning (1.0s)`, no empty assistant message exists and the pane is idle

#### Scenario: Toggle expands and collapses [auto, TestBackend + insta]
- GIVEN a collapsed block `▸ reasoning (2.3s)` over the reasoning `Let me check`
- WHEN the toggle key is pressed, and then pressed again
- THEN the transcript first shows `▾ reasoning (2.3s)` followed by `┊ Let me check` dimmed, and then the single collapsed line again

#### Scenario: Toggle while streaming reasoning does nothing [auto]
- GIVEN a stream whose reasoning is still streaming
- WHEN the toggle key is pressed
- THEN the transcript is unchanged and the block collapses only when the answer text arrives

#### Scenario: Toggle with no reasoning [auto]
- GIVEN a conversation with no reasoning block
- WHEN the toggle key is pressed
- THEN nothing changes and no effect is emitted

#### Scenario: Late thinking delta dropped [auto]
- GIVEN a block that collapsed because answer text arrived
- WHEN another thinking delta arrives in the same turn
- THEN the block text, the answer and the collapsed line are unchanged

#### Scenario: Reasoning is literal text [auto]
- GIVEN thinking text containing a line starting with three backticks, `# heading` and `*bold*`
- WHEN rendered streaming and expanded
- THEN all of it is shown literally behind the gutter with no code styling and no panic

#### Scenario: Reasoning is sanitized [auto]
- GIVEN a thinking delta `a\u{1b}[31mb` and another `c\u{1b}]0;t\u{07}d`
- WHEN the block is rendered expanded
- THEN it reads `┊ abcd` with no control or escape characters

#### Scenario: Engine without reasoning [auto, TestBackend + insta]
- GIVEN a stream with only text deltas
- WHEN rendered at every step
- THEN no reasoning line appears and the snapshots equal those of the pane without this requirement

#### Scenario: Cancel while thinking [auto]
- GIVEN a stream that delivered only reasoning `Let me`
- WHEN cancel is requested
- THEN a ChatCancel effect is emitted, the reasoning is kept as the collapsed line `▸ reasoning (N.Ns)`, no empty assistant message remains, no new system line is added and the pane is idle

#### Scenario: Error while thinking [auto]
- GIVEN a stream that delivered only reasoning `Let me`
- WHEN an error event `connection reset` arrives
- THEN the reasoning is kept collapsed, the usual error system line is shown, no empty assistant message remains and the pane is idle

#### Scenario: Cut stream while thinking [auto]
- GIVEN a stream that delivered only reasoning and then the connection closes without a final event
- WHEN the end of the stream is detected
- THEN the reasoning is kept collapsed, the system line `stream interrupted` is shown and the pane is idle

#### Scenario: Reasoning is not yanked [auto]
- GIVEN a conversation whose last turn had reasoning and the answer `hello`
- WHEN Ctrl+Y and Ctrl+B are pressed
- THEN the clipboard text is `hello` (or the code block) and never contains reasoning

#### Scenario: Reasoning follows the scroll rules [auto]
- GIVEN a transcript at the bottom, and another scrolled up
- WHEN thinking deltas that add lines arrive
- THEN the first follows the bottom and the second stays pinned, as for answer text

#### Scenario: Two chats reason independently [auto]
- GIVEN two chat panes each streaming reasoning
- WHEN a thinking delta arrives for the first
- THEN only the first transcript changes

#### Scenario: Tiny pane with reasoning [auto]
- GIVEN a pane from 1x1 to 10x2, streaming reasoning, collapsed and expanded
- WHEN rendered
- THEN nothing panics

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
PageUp and PageDown MUST scroll the transcript by one page. The transcript MUST also scroll by lines: one line per `j`/`k` in COPY, half a page per Ctrl+d/Ctrl+u in COPY, to the top and bottom with `gg` and `G` in COPY (see modal-input, Chat COPY keys), and 3 lines per mouse wheel step (see mouse-input), up or down. Line, half-page and wheel steps are computed on the rendered (wrapped) lines. While the view is at the bottom it MUST follow new deltas. While scrolled up the view MUST stay on the same content when deltas arrive. Reaching the bottom by a scroll step (wheel down, `j`, `G` or a page) puts the view back in follow mode. Sending a message MUST return the view to the bottom. Scrolling MUST clamp at both ends, and a transcript shorter than the viewport does not scroll.

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

#### Scenario: Page keys still scroll by page [auto]
- GIVEN a long transcript at the bottom with a 20-line viewport
- WHEN PageUp is pressed
- THEN the view moves up one page

#### Scenario: Line step [auto]
- GIVEN a long transcript at the bottom
- WHEN the line-up step is applied once and the line-down step once
- THEN the view is 1 line above the bottom, then at the bottom again

#### Scenario: Wheel step is 3 lines [auto]
- GIVEN a long transcript at the bottom
- WHEN one wheel-up event arrives
- THEN the view is 3 lines above the bottom

#### Scenario: Line steps clamp [auto]
- GIVEN the view 2 lines above the bottom
- WHEN a wheel-down event arrives
- THEN the view is at the bottom (not below) and follows new deltas

#### Scenario: Wrapped lines count as lines [auto]
- GIVEN a message line that wraps into 3 rendered lines
- WHEN the line-up step is applied 3 times from below it
- THEN the whole wrapped message has been scrolled through

#### Scenario: Half page [auto]
- GIVEN a 20-line transcript viewport
- WHEN a half-page step is applied
- THEN the view moves 10 lines

#### Scenario: Short transcript [auto]
- GIVEN a transcript shorter than the viewport
- WHEN any scroll step is applied
- THEN nothing changes

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
A chat pane's state (composer text and cursor, transcript, scroll position, running stream) MUST survive every mode change (PREFIX, RESIZE, COPY on another pane, MENU), focus changes, zoom, splits of other panes and tab switches, with one exception: COPY on the chat pane itself. Leaving that COPY with Esc or `q` (see copy-mode-scrollback) returns the transcript to the bottom and follow mode. Ctrl+Space inside chat COPY enters PREFIX and ends COPY without resetting the view, and a focus change or tab switch never resets a chat transcript, however it was scrolled (mouse wheel, PageUp/PageDown or COPY). A running stream MUST keep running in the background while its pane is not focused, not visible, or its tab is not active; deltas MUST keep accumulating and a final event MUST persist the exchange. While MENU is open no key or paste reaches the composer.

#### Scenario: Stream across a tab switch [auto]
- GIVEN a stream running in a chat pane of tab 1 and tab 2 becomes active
- WHEN deltas `ab` and `cd` and the final event arrive
- THEN returning to tab 1 shows the assistant message `abcd`, the pane is idle and the exchange was persisted

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

#### Scenario: Stream during COPY [auto]
- GIVEN COPY on a shell pane and a stream running in a chat pane of the same tab
- WHEN deltas arrive and COPY is left
- THEN the chat transcript contains them and the shell pane's COPY state was not affected by them

#### Scenario: Stream during COPY on the chat itself [auto]
- GIVEN COPY on the chat pane scrolled up 8 lines and its stream running
- WHEN 4 lines of delta arrive and Esc is pressed
- THEN the same content stayed visible while in COPY, the deltas are in the transcript and the view is at the bottom after Esc

#### Scenario: Wheel scroll survives a focus change [auto]
- GIVEN a chat pane scrolled up 6 lines with the mouse wheel and not in COPY
- WHEN focus moves to another pane and back
- THEN the chat pane is still 6 lines above the bottom

#### Scenario: Wheel scroll survives a tab switch [auto]
- GIVEN the same scrolled chat pane
- WHEN the user switches to another tab and returns
- THEN the view is still 6 lines above the bottom

#### Scenario: Composer and stream untouched by scrolling [auto]
- GIVEN a chat pane with composer text `hi` and a running stream
- WHEN the transcript is scrolled by the wheel and by COPY keys
- THEN the composer text and cursor are unchanged and the stream continues

#### Scenario: Scroll in COPY on the real terminal [manual, Kitty]
- GIVEN a long chat answer in Kitty
- WHEN the user scrolls with the wheel, then enters COPY with `Ctrl+Space [` and uses j/k, Ctrl+u/d, gg, G, then Esc
- THEN the transcript scrolls smoothly, the `[+N]` badge tracks it and Esc returns to the last line

### Requirement: Yank
Ctrl+Y on a focused chat pane MUST copy the text of the last finished (or cancelled) assistant message to the clipboard through the existing SetClipboard path, subject to the same size cap as copy-selection, and MUST show the notice `Copied N characters` (singular for 1) through the existing notice slot. With no assistant message, or an empty one, it MUST show `Nothing to copy`. Over the cap it MUST show `Selection too large to copy`. Ctrl+B does the same for the last fenced code block of the last assistant message (the code lines without the fence lines); with no code block it MUST show `Nothing to copy`. Yank MUST NOT change the transcript. Reasoning is never part of what is yanked (see Reasoning block).

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

### Requirement: Memory views in the pane
The chat pane MUST support three memory views, each drawn inside the pane area in place of the conversation view while it is open, like the conversation list and the tag editor: the note editor (category, title and content fields, the focused field marked, a one-line key hint), the summary editor (the content field alone, with the line `Summary (not saved until you approve)`), and the start-server confirmation (the offer text of engram-client with `(y/n)`). While one of them is open the composer text, the transcript, the scroll position and any running stream MUST be kept and keep updating underneath, and closing the view returns to the conversation view unchanged. A pane has at most one memory view at a time. All text drawn is already sanitized text in styled spans, and rendering MUST NOT panic at any pane size down to 1x1.

#### Scenario: Note editor drawn [auto, TestBackend + insta]
- GIVEN the note editor with category `decision`, title `Use Axum` and two lines of content in a 50x16 pane
- WHEN rendered
- THEN the three fields are visible with the focused one marked, and the key hint is on the last row

#### Scenario: Summary editor drawn [auto, TestBackend + insta]
- GIVEN the summary editor with a proposed summary
- WHEN rendered
- THEN the line `Summary (not saved until you approve)` and the content field are visible

#### Scenario: Stream continues under a view [auto]
- GIVEN a stream running and the note editor opened over the previous answer
- WHEN deltas and the final event arrive, then the editor is closed
- THEN the transcript holds the full new answer and the composer text is as before

#### Scenario: Composer survives the view [auto]
- GIVEN the composer holding `draft` and the note editor opened and discarded
- WHEN the conversation view is shown again
- THEN the composer still holds `draft`

#### Scenario: Tiny pane [auto]
- GIVEN a pane of any size from 1x1 to 10x2
- WHEN each memory view is rendered
- THEN nothing panics

#### Scenario: Memory off changes nothing [auto, TestBackend + insta]
- GIVEN memory off
- WHEN the existing chat pane snapshots are rendered
- THEN they equal those rendered without this requirement

### Requirement: Memory hint lines
Memory system lines (`memory unavailable: <reason>`, the server-started line, the notes-remain lines of chat-history, deletion results) MUST be system lines: visually distinct from messages, never stored, never sent to the model. The line `memory unavailable: <reason>` MUST be shown at most once per conversation view, and a system line MUST never repeat because of a repeated probe.

#### Scenario: Once per view [auto]
- GIVEN an unavailable memory and two failed memory-key probes in one conversation view
- WHEN the transcript is read
- THEN `memory unavailable: engram is not running` appears once

#### Scenario: Not stored, not sent [auto]
- GIVEN a conversation with the memory hint line
- WHEN a message is sent and the store commands and the engine history are inspected
- THEN neither contains the hint
