# Chat Pane Specification

## Purpose

The chat pane is a second kind of pane. It lives in the tile tree next to shell panes, owns its own conversation state (composer, transcript, stream), and talks to the engine assigned to the `chat` task (see chat-engines). It shows plain text and fenced code blocks only and does not see terminal output. Persistence is in chat-history, prompts in prompt-profiles, key routing in modal-input. Tags: [auto] = `cargo test` (pure state machine, fake engine, `TestBackend`); [manual] = real machine.

Terms: "idle" = no stream running; "streaming" = a request was sent and the answer is not finished; "system line" = a transcript line produced by lazarobox (error, notice, marker). System lines are never stored in the database and never sent to the model. "Origin cwd" = the effective cwd of the pane the chat was opened from. "Reasoning" = engine thinking text delivered as thinking deltas, separate from the answer (see chat-engines); it is shown but never stored and never sent back to an engine (see chat-history).

## Requirements

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

### Requirement: Stream lifecycle
A send MUST append the user message, show a waiting state and start one request. Until the first byte arrives the pane MUST show `waiting for the model…`, and when the first byte has not arrived after 3 seconds it MUST show `still waiting for the model…` instead (a cold model load is not a hang). Each text delta MUST be appended to the assistant message in arrival order and drawn incrementally. A final event MUST finish the message, return the pane to idle and persist the exchange (see chat-history). An error MUST add an error system line, keep any partial text received before it, and return the pane to idle. A stream that ends without a final event (cut stream) MUST keep the partial text, add the system line `stream interrupted`, and return to idle. A pane MUST run at most one stream at a time; several chat panes MAY stream at once.

#### Scenario: First-byte wait [auto]
- GIVEN a send was issued and no byte arrived
- WHEN 1 second passes, then 3 seconds
- THEN the pane shows `waiting for the model…` and then `still waiting for the model…`

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
A chat pane with a running stream MUST expose a phase, a start instant and the model of the request as sent. The phase is `thinking` from the send until the first non-empty text delta arrives (this covers waiting for the first byte and a stream that has only delivered reasoning), and `answering` from the first non-empty text delta until the terminal event. An idle pane has no phase. The statusline reads these values (see statusline). The pane's own waiting texts are unchanged: `waiting for the model…` and, after 3 seconds without a first byte, `still waiting for the model…`; a thinking delta counts as the first byte, after which the waiting text is replaced by the reasoning block.

#### Scenario: Phase while waiting [auto]
- GIVEN a send was issued and no byte arrived
- WHEN the phase is read, and again after 3 seconds
- THEN it is `thinking` both times, and the pane shows `waiting for the model…` and then `still waiting for the model…`

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
- GIVEN a stream showing `still waiting for the model…`
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

### Requirement: No-jail guidance on macOS (S1 refusal, S4 copy)
When `codex` or `gemini` is refused on macOS because Seatbelt is unavailable, the pane MUST keep the existing `<engine>: not available on this system yet` guidance, MUST add the macOS reason (Seatbelt not enforcing) in place of any "needs Linux" hint, MUST send nothing and MUST keep the composer text. A jail error (exit 71) MUST show as a jail error line, not as a CLI error. `claude` on macOS MUST never show the no-jail guidance. On Linux the guidance is unchanged.

#### Scenario: No jail on macOS [auto]
- GIVEN macOS, `chat` is `gemini`, gemini is `available` and the Seatbelt probe is Unavailable
- WHEN send is attempted
- THEN the guidance starts with `gemini: not available on this system yet`, names the Seatbelt reason, no process is started and the composer text is kept

#### Scenario: Exit 71 shows a jail error [auto]
- GIVEN macOS and a turn whose helper exits 71
- WHEN the turn ends
- THEN the transcript shows a jail error, not a CLI error

#### Scenario: claude on macOS [auto]
- GIVEN macOS, an Unavailable probe, `chat` is `claude` and claude is `available`
- WHEN send is attempted
- THEN no no-jail guidance is shown and the message is sent

#### Scenario: Linux guidance unchanged [auto]
- GIVEN Linux and an Unavailable jail
- WHEN send is attempted
- THEN the guidance equals the existing `No jail` text

### Requirement: Run action on code blocks
A chat pane MUST offer the run-in-pane action of pane-commands on the last shell code block of the last finished answer. When such a block exists and tools are on, the pane MUST show a one-line hint under that block naming the binding; the hint MUST NOT be stored, yanked or sent to the model. The action opens the pending confirm (see Approval prompt). Yank (Ctrl+B) MUST be unchanged.

#### Scenario: Hint under the block [auto, TestBackend + insta]
- GIVEN a finished answer with a bash block and tools on
- WHEN rendered
- THEN a hint line naming the run binding is drawn under the block

#### Scenario: No hint otherwise [auto, TestBackend + insta]
- GIVEN an answer with no shell block, or tools off, or a streaming message
- WHEN rendered
- THEN no hint line is drawn and the snapshot equals the one without this requirement

#### Scenario: Hint is not content [auto]
- GIVEN the hint shown
- WHEN Ctrl+Y and Ctrl+B are pressed and the next request is built
- THEN the clipboard text and the history contain no hint

### Requirement: Tool rows
Each tool event MUST be drawn inline in the transcript at its position in the turn, as a row distinct from messages, system lines and reasoning: a call row with the tool name and the short argument summary (for example `find_dir curso-rust`), and a result row with its status (`ok`, `error`, `denied`) and the short summary. A call and its result MUST read as a pair. Row text is already sanitized, wrapped at the pane width, shown literally (no markdown, no fences) and capped. Rows are system-produced: they are not messages, do not count toward the 400 messages per conversation, are never stored in the database, never yanked and never sent to the model. Text of an answer that imitates a row MUST NOT get the row style.

#### Scenario: Call and result rows [auto, TestBackend + insta]
- GIVEN a turn with a `find_dir` call returning `~/dev/curso-rust`
- WHEN rendered
- THEN a call row `find_dir curso-rust` and a result row `ok` with the summary appear between the surrounding text, in the tool-row style

#### Scenario: Denied row [auto, TestBackend + insta]
- GIVEN a declined `run_in_pane`
- WHEN rendered
- THEN the result row reads `denied`

#### Scenario: Rows are literal and sanitized [auto]
- GIVEN a result summary holding a line of three backticks, `# x` and an escape sequence
- WHEN rendered
- THEN everything is shown literally without code styling and no control character is drawn

#### Scenario: Rows are not stored or sent [auto]
- GIVEN a conversation with rows, then a send
- WHEN the store commands and the engine history are inspected
- THEN neither contains a row, and the message count is unchanged by rows

#### Scenario: Imitation is plain text [auto, TestBackend + insta]
- GIVEN an assistant answer whose text looks like a tool row
- WHEN rendered
- THEN it uses the plain answer style

#### Scenario: Rows follow the scroll rules [auto]
- GIVEN a transcript at the bottom, and another scrolled up
- WHEN tool rows arrive
- THEN the first follows the bottom and the second stays pinned

#### Scenario: Tiny pane with rows [auto]
- GIVEN a pane from 1x1 to 10x2 with rows and a pending confirm
- WHEN rendered
- THEN nothing panics

### Requirement: Approval prompt
A pending confirm or tool approval MUST be drawn at the composer (replacing the composer's hint line, not the transcript), naming what will happen: for the pane command `run in pane N (<cwd>)` and the exact command text drawn visibly; for `read_file` on a cloud engine `read <path>` with the sanitized path; for a user tool the server, tool and arguments; plus the keys (`y` run/allow, `n` decline, `p` target pane for the pane command). The prompt is an overlay of the chat pane's own state, not a global input mode. The composer text and cursor MUST be kept untouched under it. While a prompt is pending the pane MUST show it even when scrolled. A multi-line or long command MUST be wrapped and clipped to a visible maximum with the clipping marked, and the full text is what `y` writes; if the command or path cannot be shown in full in the available space (more than six wrapped rows, or a pane too short) the prompt MUST say so ("pane too small to show it all: enlarge it or press n") and show no part of it, and `y` MUST NOT approve it until a resize makes the whole text visible; `n`, Esc and Ctrl+C still deny. The same function decides what is drawn and whether `y` may approve. A prompt the model opened (not one the user opened with `Prefix e`) MUST ignore `y`, `n` and Esc for a short arming delay (400 ms) so a key typed for the composer cannot answer it, and its keys line shows `…` until then; Ctrl+C always cancels.

#### Scenario: Pane command prompt [auto, TestBackend + insta]
- GIVEN a pending confirm for `cd ~/dev/curso-rust` on pane 2 with cwd `~/dev`
- WHEN rendered in a 60x16 pane
- THEN the prompt reads `run in pane 2 (~/dev)`, the command and the key hints appear at the composer

#### Scenario: Composer kept [auto]
- GIVEN a composer holding `draft` and a pending confirm
- WHEN the confirm is declined
- THEN the composer still holds `draft`

#### Scenario: Long command [auto, TestBackend + insta]
- GIVEN a command of 20 lines
- WHEN the prompt is rendered
- THEN the visible part is marked as clipped and the prompt states the line count

#### Scenario: Prompt in a scrolled transcript [auto]
- GIVEN a transcript scrolled up and a pending confirm
- WHEN rendered
- THEN the prompt is still visible at the composer

### Requirement: Streaming with a pending approval
While an approval is pending the pane stays in its streaming state (one stream at a time, send refused), and the stream phase is unchanged. Cancel MUST stop the stream and deny the approval. Closing the pane MUST deny it and write nothing. A pending approval MUST survive modes, focus changes, zoom and tab switches (as the stream does), and a send MUST be refused with the existing system line. The waiting texts of the first byte do not apply while an approval is pending.

#### Scenario: Cancel denies [auto]
- GIVEN a streaming chat with a pending `run_in_pane` approval
- WHEN cancel is requested
- THEN a ChatCancel effect is emitted, the approval result is `denied`, nothing is written and the pane is idle

#### Scenario: Close denies [auto]
- GIVEN the same state
- WHEN the pane is closed
- THEN the result is `denied`, no WritePty is emitted and late events are ignored

#### Scenario: Send is refused [auto]
- GIVEN a pending approval and composer text `next`
- WHEN send is requested
- THEN no ChatSend is emitted and the composer still holds `next`

#### Scenario: Survives a tab switch [auto]
- GIVEN a pending approval and a switch to another tab and back
- WHEN the pane is shown
- THEN the same approval is pending

### Requirement: Untrusted tool output in the transcript
Tool results shown in the pane MUST be drawn only as rows (see Tool rows) with sanitized text; the pane MUST NOT interpret anything in a tool result (no fences, no links, no escape sequences), and MUST label rows from user MCP servers with the server name so their origin is visible.

#### Scenario: Server origin visible [auto, TestBackend + insta]
- GIVEN a result row from user server `fs`
- WHEN rendered
- THEN the row names `fs`

#### Scenario: Hostile result [auto]
- GIVEN a result with a clickable-link escape sequence and a bidi control
- WHEN rendered
- THEN neither reaches the screen

### Requirement: Batch review view
A proposed write batch, and a `git_commit` review, MUST open as a full-pane view inside the chat pane, in the same family as the list, memory and tag views. The view MUST render every row of the batch with a fixed row model, and the renderer and the approving key MUST share one coverage count. The view MUST show how many rows remain unseen and keep the approving key disabled until all are shown and the arm delay has passed. The view MUST support scrolling, and MUST work in zoom and after a resize. It MUST show the total number of files, the added and removed lines, a warning on build files, and for a commit the message and the signing notice. `n` and Esc MUST reject the whole batch.

#### Scenario: Remaining count [auto, TestBackend + insta]
- GIVEN a batch of 30 rows with 10 shown
- WHEN the view renders
- THEN it shows 20 rows remaining and the approve key reads as disabled

#### Scenario: Scroll covers rows [auto]
- GIVEN a batch taller than the pane
- WHEN the user scrolls to the end
- THEN the coverage count reaches the total and the approve key is enabled after the arm delay

#### Scenario: Resize keeps coverage [auto]
- GIVEN rows already covered
- WHEN the pane is resized
- THEN the covered rows stay covered and no row is lost

#### Scenario: Reject [auto]
- GIVEN an open review
- WHEN `n` is pressed
- THEN the view closes, the batch is discarded and the transcript gets a system line

#### Scenario: Reject waits like approve, except when it is the only exit [auto]
- GIVEN a reviewable batch whose review was first drawn at t
- WHEN `n` or Esc is pressed before t + 800 ms
- THEN nothing happens, and at t + 800 ms (the same delay as `y`) they reject the batch
- AND on a batch that cannot be reviewed (too large or empty) `n` and Esc reject at once

#### Scenario: Only drawn rows count [auto]
- GIVEN a review opened in a background tab, a hidden pane or under another chat view
- WHEN the pane is resized or scrolled without being drawn
- THEN no row counts as seen and the arm delay has not started, until a frame draws the review

#### Scenario: The countdown ends on screen [auto]
- GIVEN a drawn review counting down with no stream running
- WHEN the arm delay elapses
- THEN the screen is redrawn once more and the header reads `y ready`

#### Scenario: The review owns the pane [auto, TestBackend]
- GIVEN an open review in a pane with the AUTO badge or a scrolled transcript
- WHEN the pane renders and the wheel turns over it
- THEN the badges do not cover the counter row, the wheel scrolls the review and never the transcript, and other mouse events do nothing
- AND the keys row cuts from the right with `n reject` first, then `y`, then the scroll hints

### Requirement: Staged batch indicator
While the chat holds a staged batch the pane MUST show an indicator with the number of staged entries. An orphan batch (after a cancelled turn) MUST show a distinct indicator and MUST offer the keys to review it or discard it. The indicator MUST be visible in every chat view, focused or not, and MUST disappear when the batch is applied or discarded.

#### Scenario: Indicator count [auto, TestBackend + insta]
- GIVEN 3 staged entries
- WHEN the pane renders
- THEN the indicator shows 3

#### Scenario: Orphan indicator and keys [auto]
- GIVEN a turn cancelled with a staged batch
- WHEN the pane renders and the review key is pressed
- THEN the orphan indicator is visible and the review view opens

#### Scenario: Gone after discard [auto]
- GIVEN an orphan batch
- WHEN it is discarded
- THEN the indicator disappears

### Requirement: Closing discards a batch
Closing a chat pane that holds a staged or orphan batch MUST discard the batch and MUST show a notice that names the number of discarded entries. Nothing is written to disk by the close. Quitting the app MUST do the same for every chat.

#### Scenario: Close notice [auto]
- GIVEN a chat with 2 staged entries
- WHEN the pane is closed
- THEN the batch is gone, the disk is untouched and a notice names 2 entries

#### Scenario: Quit [auto]
- GIVEN two chats with staged batches
- WHEN the app quits
- THEN both batches are discarded and a notice is shown

### Requirement: Approval mode badge and system lines
A chat pane with tools on in `auto` mode MUST show the badge ` AUTO ` in amber, and in `run` mode the badge ` RUN ` in red, both high contrast, in every chat view, focused or not; `ask` shows none. When the pane is narrower than the badge (below 6 columns for ` AUTO `, below 5 for ` RUN `) the badge MUST shrink to `A` or `R`; the badge text is bold on the base background colour. The badge is system-drawn: never stored, yanked or sent to the model. Changing the mode (see modal-input) MUST add a system line naming the new mode and saying what it does: `ask` asks for each command; `auto` types commands without Enter; `run` types and runs a short list of navigation commands (`cd`, `pwd`, `ls`, `tree`, `mkdir`) and sends their output to the model; the `run` line reads `approval: run (cd, pwd, ls, tree, mkdir run at once and their output goes to the model; other commands ask)`. System lines are never stored or sent to the model. After a `cd` ran under pane-execution the transcript MUST show the system line `pane N: <exact command> → <cwd>`.
(Previously: the `auto` marker was specified only inside agent-tools, in one unspecified high-contrast style, and there was no run mode)

#### Scenario: Badges [auto, TestBackend + insta]
- GIVEN chats in `auto`, `run` and `ask` with tools on
- WHEN rendered
- THEN ` AUTO ` is amber, ` RUN ` is red and `ask` has no badge

#### Scenario: Narrow pane [auto, TestBackend + insta]
- GIVEN a chat in `run` rendered at 4 columns, and one in `auto` at 5 columns
- WHEN rendered
- THEN the badges read `R` and `A`, a `run` chat at 5 columns still reads ` RUN `, and nothing panics

#### Scenario: System line on change [auto]
- GIVEN a chat in `auto`
- WHEN the mode changes to `run`
- THEN the system line reads `approval: run (cd, pwd, ls, tree, mkdir run at once and their output goes to the model; other commands ask)`; it is not in the store or the next request

#### Scenario: Cd line shows the exact command [auto]
- GIVEN `cd ~/dev/curso-rust` run and settled
- WHEN rendered
- THEN a system line reads `pane N: cd ~/dev/curso-rust → <cwd>`

#### Scenario: Tools off shows nothing [auto, TestBackend + insta]
- GIVEN tools off and `approval = "run"`
- WHEN rendered
- THEN no badge appears and the snapshot equals the one without this change

### Requirement: Run result rows
Within the Tool rows rules, the result row of a `run_in_pane` call MUST start with the exact command and then read `ran in pane N` (with the cwd or the line count), `ran in pane N — still running`, `ran — output not available`, `typed into pane N — no Enter` or `denied`, with at most a short sanitized preview of the output; the full bounded output goes to the model only and is never stored in the transcript.

#### Scenario: Ran row [auto, TestBackend + insta]
- GIVEN a settled `ls -l` with 30 output lines
- WHEN rendered
- THEN the row starts with `ls -l`, reads `ran in pane N` and shows at most a capped preview, not all 30 lines

#### Scenario: Timed out row [auto, TestBackend + insta]
- GIVEN a run that hit the timeout
- WHEN rendered
- THEN the row starts with the exact command and reads `ran in pane N — still running`

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

### Requirement: Per-pane ORCH mode and badge [S1]
Each chat pane MUST have its own ORCH mode, off for a new pane, held in memory only (not saved, not restored). Ctrl+O toggles it (see modal-input). A pane MUST show an ORCH badge, in the same place and style family as AUTO and RUN (placed beside the existing mode badge), only while it is orchestrating: the global `enabled` gate is on AND the pane's ORCH mode is on. The badge MUST be absent otherwise. Only an orchestrating pane runs its chat turns on the `orchestrate` assignment; the other panes keep the `chat` assignment. While a run is active in the pane the toggle MUST NOT change the mode and the footer hint says so. With the global gate off the toggle does nothing.

#### Scenario: Badge follows both switches [auto, TestBackend + insta]
- GIVEN a chat pane
- WHEN `enabled` is false; then true with ORCH off; then true with ORCH on
- THEN the badge is absent, absent, then present

#### Scenario: Toggle is per pane [auto]
- GIVEN `enabled` true and two chat panes A and B, both with ORCH off
- WHEN Ctrl+O is pressed in A
- THEN A shows ORCH and B does not

#### Scenario: Toggle does nothing with the gate off [auto]
- GIVEN `enabled` false
- WHEN Ctrl+O is pressed in a chat pane
- THEN the pane mode and render are unchanged and no key reaches a PTY

#### Scenario: Toggle blocked during a run [auto]
- GIVEN an orchestrating pane with an active run
- WHEN Ctrl+O is pressed
- THEN the mode stays on and the footer hint says to cancel the run first

### Requirement: Worker rows [S1]
A worker MUST appear as a transcript row (kind, `engine>model`, status, elapsed), updated in place. The row MUST NOT be sent as history, MUST NOT be saved as a message, and MUST be included in yank and COPY like tool rows.

#### Scenario: Row rendered [auto, TestBackend + insta]
- GIVEN a `review` worker running on `claude>opus` for 12 s
- WHEN the pane is rendered
- THEN a row shows `review`, `claude>opus`, `running` and `12s`

### Requirement: Announced workers [S1]
A worker MUST be announced by its transcript row and start at once: no prompt, key or answer is needed to start a run or a worker (user decision, 2026-10-29: "lo avisas pero no pides permisos"). Its reads follow the run's read scope and the pane's approval mode (agent-orchestrator, Read scope of a run). The composer text MUST be kept.

#### Scenario: Worker starts without a prompt [auto]
- GIVEN an orchestrating pane whose composer holds `draft`
- WHEN the orchestrator spawns a `review` worker
- THEN its row is shown, the worker starts with no prompt and no key pressed, and the composer still holds `draft`

### Requirement: Failure alert [S1, S2]
A worker failure MUST show an inline alert in the pane with the worker label, the error class [S2] and the provider message, and the options: [S1] retry, cancel; [S2] retry, change model, change provider, cancel. The alert stays until the user chooses; it is not dismissed by a timer or by new output. While it is shown the pane MUST say the run is paused. When a run-scoped override is active the pane MUST show it in the run state and mark the affected worker rows (`engine>model` plus an override marker).

#### Scenario: Alert content [auto, TestBackend + insta]
- GIVEN a `code` worker failed with 429 `rate limit reached`
- WHEN the pane is rendered
- THEN the alert names the worker, shows class `rate_limit` and the message, and lists the four options

#### Scenario: Alert persists [auto]
- GIVEN an alert shown
- WHEN time passes and sibling output arrives
- THEN the alert is still shown with the same options

### Requirement: Approval label and queue [S1, S3]
Every approval prompt raised by a worker MUST show a label `worker n/m <kind> <engine>><model>`. [S3] The single pending approval limit MUST be replaced for workers by a FIFO queue showing one prompt at a time with the count waiting; each answer applies to its own prompt only. [S1] With one worker, at most one prompt is pending. A prompt MUST never be answered by a timer.

#### Scenario: Label shown [auto, TestBackend + insta]
- GIVEN worker 2 of 3, kind `review` on `claude>opus`, asks for a read
- WHEN the pane is rendered
- THEN the prompt shows `worker 2/3 review claude>opus` and the sanitized path

#### Scenario: Queue count [S3, auto]
- GIVEN three pending asks
- WHEN the pane is rendered
- THEN one prompt is shown and a `2 more waiting` indicator

### Requirement: Handoff persistence hint [S4]
When a run turned memory on the pane MUST show `memory on for this run` once per run (never again for the same run, however many handoffs follow); when handoffs are not persisted it MUST show `handoffs not persisted` once per run; and a verdict `off_course` or `blocked` MUST be shown as a transcript line.

#### Scenario: Hint once [auto]
- GIVEN memory off, engram unreachable and a run with three workers
- WHEN the run ends
- THEN `handoffs not persisted` was shown once

### Requirement: The `/cost` command
`/cost` MUST be a user-only composer command, recognized when the composer text, trimmed of surrounding whitespace, is exactly `/cost` (case-sensitive). Sending it MUST NOT create a user message, MUST NOT emit ChatSend, MUST NOT start a stream, MUST NOT contact any engine, MUST NOT be stored in the conversation and MUST NOT enter the history of later requests. It clears the composer and appends the `/cost` view (see `ai-usage`) as system lines. It MUST work while a stream is running in the pane (the send-refusal rule does not apply) and in a conversation with no messages. `/cost` followed by anything (`/cost now`) MUST show the system line `/cost takes no arguments` and keep the composer text. The view's lines are system lines: not stored, not sent to an engine, not copied as part of an answer. If the pane has no engine configured, `/cost` still works.

#### Scenario: No message, no request [auto]
- GIVEN an idle chat and the composer holding `/cost`
- WHEN send is requested
- THEN no user message is added, no ChatSend effect is emitted, the composer is empty and the view lines are appended

#### Scenario: Works during a stream [auto]
- GIVEN a stream running and the composer holding `/cost`
- WHEN send is requested
- THEN the view is appended, the stream is unaffected and no wait-or-cancel line appears

#### Scenario: Not in the history [auto]
- GIVEN `/cost` was run and then `hello` is sent
- WHEN the request is built
- THEN the history holds `hello` only and nothing from `/cost`

#### Scenario: Not stored [auto]
- GIVEN a conversation where `/cost` was run
- WHEN the store is read
- THEN it holds no message with `/cost` or the view text

#### Scenario: Arguments refused [auto]
- GIVEN the composer holding `/cost now`
- WHEN send is requested
- THEN the system line `/cost takes no arguments` appears and the composer still holds `/cost now`

#### Scenario: Other text is a normal message [auto]
- GIVEN the composer holding `what does /cost do`
- WHEN send is requested
- THEN it is sent as a normal user message

#### Scenario: Pending read [auto]
- GIVEN a slow store
- WHEN `/cost` is sent
- THEN `reading usage…` shows at once and the view replaces it when the data arrives, without blocking keys

### Requirement: Per-answer usage line
After a turn that ended with a final event, the pane MUST append one dim system line below the answer, in the format of `ai-usage` (Usage text formats), when the `ai.usage.line` setting is on (default on). For a turn with no record the line reads `usage: no data`. For a cancelled, interrupted or errored turn the line appears only when the turn has a record (it is then marked `partial`). The line is not part of the assistant message: not stored, not sent to an engine, not included by yank of an answer, and not shown after resume. With the setting off, no usage line appears and nothing else changes. The line appears before any threshold note of the same turn. When the turn is the coordinator's turn of an orchestrator run, the line starts `usage (coordinator):` instead of `usage:` (every other part is unchanged, `no data` included): it is the coordinator's own usage, not the run total, and the workers' usage is recorded apart (see `ai-usage`, Worker usage).

#### Scenario: Line after a claude turn [auto, TestBackend + insta]
- GIVEN a claude turn with 2 in, 4 out, cost 0.004936 and mode `subscription`
- WHEN the turn ends
- THEN the line `usage: in 2 · out 4 · $0.0049 list-price equivalent` appears below the answer

#### Scenario: Local engine [auto]
- GIVEN an ollama turn with 12 in and 34 out
- WHEN the turn ends
- THEN the line reads `usage: in 12 · out 34`

#### Scenario: No data [auto]
- GIVEN a turn whose engine reported nothing
- WHEN the turn ends
- THEN the line reads `usage: no data`

#### Scenario: Coordinator label [auto]
- GIVEN an orchestrator turn with 12 in and 34 out
- WHEN the turn ends
- THEN the line reads `usage (coordinator): in 12 · out 34`, and a plain chat turn still reads `usage: in 12 · out 34`

#### Scenario: Turned off [auto]
- GIVEN `ai.usage.line` off
- WHEN a turn ends
- THEN no usage line appears

#### Scenario: Cancelled without usage [auto]
- GIVEN a turn cancelled before any usage was reported
- WHEN the pane returns to idle
- THEN no usage line appears

#### Scenario: Not stored or resumed [auto]
- GIVEN a turn with a usage line
- WHEN the store is read and the conversation is resumed
- THEN neither holds the line

#### Scenario: Yank excludes it [auto]
- GIVEN an answer followed by its usage line
- WHEN the answer is yanked
- THEN the yanked text does not contain the usage line

### Requirement: Threshold notes in the pane
Threshold notes (see `ai-usage`, Thresholds) MUST appear as dim system lines after the usage line of the turn that crossed them, in this order when several apply: turn, daily tokens, daily dollars. They MUST NOT block the composer, delay a send, hide the answer or change focus, and MUST NOT play a sound or notification of their own. They are shown whether or not the usage line is on.

#### Scenario: Note after the usage line [auto]
- GIVEN `turn_tokens` 100 and a turn of 300 tokens with the usage line on
- WHEN the turn ends
- THEN the usage line is followed by `turn used 300 tokens (limit 100)`

#### Scenario: Usage line off, note on [auto]
- GIVEN the usage line off and a crossed threshold
- WHEN the turn ends
- THEN only the note appears

#### Scenario: Composer stays usable [auto]
- GIVEN a note was shown
- WHEN a message is typed and sent
- THEN it is sent normally
