# Delta for Chat Pane

All existing requirements stay as they are. The additions below only apply when tools are on and a tool event, a run action or a pending confirm exists; with tools off the pane MUST render and behave exactly as before. Tags as in the main spec.

## ADDED Requirements

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
A pending confirm or tool approval MUST be drawn at the composer (replacing the composer's hint line, not the transcript), naming what will happen: for the pane command `run in pane N (<cwd>)` and the exact command text drawn visibly; for `read_file` on a cloud engine `read <path>` with the sanitized path; for a user tool the server, tool and arguments; plus the keys (`y` run/allow, `n` decline, `p` target pane for the pane command). The prompt is an overlay of the chat pane's own state, not a global input mode. The composer text and cursor MUST be kept untouched under it. While a prompt is pending the pane MUST show it even when scrolled. A multi-line or long command MUST be wrapped and clipped to a visible maximum with the clipping marked, and the full text is what `y` writes; if the command cannot be shown in full in the available space the prompt MUST say so rather than hide it.

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
