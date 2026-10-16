# Pane Commands Specification

## Purpose

Run a command from a chat in the user's own shell pane: the target-pane rule, the confirmation, command validation, the write, and the accept-time rechecks. Engine-agnostic: it works for every engine from a fenced block of an answer, and it is the same path the `run_in_pane` tool uses (see agent-tools). The approval only TYPES the command; lazarobox never sends Enter, so the user reviews the line in their own shell and runs it. Tags: [auto] = `cargo test` (pure state machine, fake PTY effects, `TestBackend`); [manual] = real Kitty; [design-confirm] = behaviour fixed by this spec whose mechanism design must verify before the scenario becomes [auto].

Terms: "target pane" = the shell (PTY) pane a command is written to. "Shell pane" = a pane that owns a PTY (never a chat pane). "Bracketed paste on" = the target pane's terminal state has bracketed paste enabled. "Command" = the text to type, before validation. "Pending confirm" = the confirmation prompt shown at the composer of a chat pane.

## Requirements

### Requirement: Run action on a bash block
A chat pane with the conversation view shown MUST offer an action that opens a pending confirm for the last code block shown in the last finished assistant message, when its fence language is a shell language (`bash`, `sh`, `shell`, `zsh`; the exact list is pinned by design). A block without a language, with another language, an unclosed block, and a block of a message still streaming MUST NOT be runnable, and an earlier shell block MUST NOT be offered in place of a last block that is not runnable (the user runs what they see). The block is found with the same fence rule the transcript uses to draw code. The action MUST work for every engine, with no tool support in the engine or the model, and MUST be gated by `[ai.tools]` (see configuration). The action MUST NOT write anything by itself: only the confirm can. The binding is the prefix key `e` (see modal-input). The confirm is state of the chat pane drawn over its composer, not a global input mode.

#### Scenario: Action opens the confirm [auto]
- GIVEN an idle chat whose last answer holds a ```` ```bash ```` block `cd ~/dev/curso-rust` and a shell pane 2 with cwd `~/dev`
- WHEN the run action is used
- THEN a pending confirm shows `run in pane 2 (~/dev)` and the exact command, and no WritePty is emitted

#### Scenario: Not a shell block [auto]
- GIVEN a last answer whose only block is ```` ```rust ````, or a block with no language, or an unclosed block
- WHEN the run action is used
- THEN no confirm opens, the notice says there is no runnable command, and no effect is emitted

#### Scenario: Streaming message [auto]
- GIVEN a stream running whose partial message already holds a closed bash block
- WHEN the run action is used
- THEN no confirm opens until the message is finished

#### Scenario: Any engine [auto]
- GIVEN the same answer produced by a fake `ollama`, `claude`, `codex` and `gemini` engine with no tool support
- WHEN the run action is used on each
- THEN each opens the same confirm

#### Scenario: Tools off [auto]
- GIVEN `[ai.tools]` is off
- WHEN the run action is used
- THEN no confirm opens and the notice says tools are off

### Requirement: Target pane
The target pane MUST be the last focused shell pane of the chat's tab. When the chat was opened from a shell pane and no other shell pane was focused since, that pane is the target. The prompt MUST name it as `run in pane N (<cwd>)` where N is the pane's number in the tab and `<cwd>` is its effective cwd (see terminal-session), abbreviated with `~` inside `$HOME` and sanitized. The cycle key, plain `p` while the confirm is pending, MUST move the target to the next shell pane of the tab, with wrap, before the user accepts; the prompt updates. A chat pane, and any pane of another tab, MUST NEVER be a target. With no shell pane in the tab the action MUST be refused with a notice and no confirm opens.

#### Scenario: Last focused shell pane [auto]
- GIVEN shell panes 1 and 2 and a chat, with focus order 2, then the chat
- WHEN the run action is used
- THEN the prompt names pane 2

#### Scenario: Cycle [auto]
- GIVEN a pending confirm on pane 1 with shell panes 1, 2 and 3
- WHEN `p` is pressed three times
- THEN the prompt names pane 2, then 3, then 1, and nothing is written

#### Scenario: Chat is never a target [auto]
- GIVEN a tab with only chat panes
- WHEN the run action is used
- THEN no confirm opens and the notice says there is no shell pane

#### Scenario: Other tabs excluded [auto]
- GIVEN a shell pane in tab 2 and a chat with no shell pane in tab 1
- WHEN the run action is used in tab 1
- THEN the action is refused

#### Scenario: Prompt text is sanitized [auto]
- GIVEN a target cwd containing an escape sequence in its name
- WHEN the prompt is rendered
- THEN the label contains no control or escape characters

### Requirement: Approval types, never executes
Only an unmodified lowercase `y` press MUST accept a pending confirm. Accepting MUST write the validated command into the target pane and nothing else: no Enter, no newline after the command, no carriage return. Only `n` and Esc MUST decline (nothing is written). `Y`, Enter, a modified `y` and every other key MUST NOT answer: they are swallowed and the confirm stays pending (see modal-input, Approval keys). Repeat and Release events MUST NOT answer. One accept MUST produce exactly one write; a second `y` after the confirm closed MUST do nothing. There MUST be no setting, key or flag that skips the confirm, and no "always allow".

#### Scenario: y types without Enter [auto]
- GIVEN a pending confirm for `cd ~/dev/curso-rust` on pane 2
- WHEN `y` is pressed
- THEN exactly one WritePty addressed to pane 2 is emitted, its bytes end with the command text (or the end of the bracketed paste) and contain no `\r` or trailing `\n` outside the paste

#### Scenario: Decline writes nothing [auto]
- GIVEN a pending confirm
- WHEN `n` and Esc are pressed in separate runs
- THEN no WritePty is emitted and the confirm is closed each time

#### Scenario: Other keys are swallowed [auto]
- GIVEN a pending confirm
- WHEN `Y`, Enter, Space and Ctrl+y are pressed
- THEN no WritePty is emitted and the confirm is still pending

#### Scenario: Repeat does not answer [auto]
- GIVEN a pending confirm
- WHEN a Repeat event for `y` arrives
- THEN the confirm is unchanged and nothing is written

#### Scenario: One accept, one write [auto]
- GIVEN a confirm accepted with `y`
- WHEN `y` is pressed again
- THEN no further WritePty is emitted

#### Scenario: Shell stays unexecuted [manual, Kitty]
- GIVEN a bash block `echo hi` and an idle shell pane
- WHEN the confirm is accepted
- THEN `echo hi` appears on the shell's input line, is not executed, and runs only after the user presses Enter in that pane

### Requirement: Command validation
Before a confirm opens and again at accept time, the command MUST be validated and, when it fails, REFUSED (never silently cleaned): the user and the model are told it was refused and nothing is written. A command MUST be refused when it is empty or blank after removing trailing newlines, when it holds ESC, any C0 or C1 control other than `\n` and `\t`, DEL, bidi controls or other format characters (see the output sanitizer in chat-engines), or when it exceeds the length cap (pinned by design). Trailing newlines and trailing blanks MUST be removed so no Enter is implied. The text shown in the prompt MUST be exactly the text that will be written, rendered visibly (control characters are never drawn raw).

#### Scenario: Escape sequence refused [auto]
- GIVEN a command `ls\u{1b}[31m` supplied by a tool call
- WHEN it is validated
- THEN it is refused, no confirm opens and no WritePty is emitted

#### Scenario: Embedded paste terminator [auto]
- GIVEN a command `echo x\u{1b}[201~; rm -rf ~` (an early end of the bracketed paste)
- WHEN it is validated
- THEN it is refused because of the ESC, and the same text without the ESC (`echo x[201~`) is valid and written as plain text

#### Scenario: Other controls refused [auto]
- GIVEN commands holding `\u{07}`, `\u{00}`, `\u{7f}`, `\u{9b}`, a lone `\r`, and U+202E
- WHEN each is validated
- THEN each is refused

#### Scenario: Newline and tab allowed [auto]
- GIVEN the command `for f in *; do\n\techo $f\ndone`
- WHEN it is validated with bracketed paste on
- THEN it is accepted unchanged

#### Scenario: Trailing newline removed [auto]
- GIVEN the command `cd /tmp\n\n`
- WHEN it is validated
- THEN the command is `cd /tmp` and the prompt shows `cd /tmp`

#### Scenario: Empty and over-long [auto]
- GIVEN an empty command, a blank command, and one over the cap
- WHEN each is validated
- THEN each is refused

#### Scenario: Prompt shows the exact bytes [auto]
- GIVEN a command with a tab and a newline
- WHEN the confirm is rendered
- THEN the tab and newline are shown visibly and the written bytes equal the validated command

### Requirement: Write respects bracketed paste
The write MUST be one atomic paste of the validated command. With bracketed paste on in the target pane, it MUST be framed as one bracketed paste (start marker, command, end marker) so a multi-line command is not run line by line. With bracketed paste off, a single-line command MUST be written as plain text, and a command with a newline or a tab MUST be refused with a notice (without the framing the shell would execute or complete as it reads). The framing MUST be decided from the target pane's state at accept time, not at prompt time.

#### Scenario: Multi-line with bracketed paste on [auto]
- GIVEN the command `a\nb` and bracketed paste on in pane 2
- WHEN `y` is pressed
- THEN one WritePty carries start marker, `a\nb`, end marker, and no Enter

#### Scenario: Multi-line without bracketed paste [auto]
- GIVEN the command `a\nb` and bracketed paste off in pane 2
- WHEN `y` is pressed
- THEN nothing is written and the notice says the shell cannot take a multi-line command safely

#### Scenario: Single line without bracketed paste [auto]
- GIVEN `cd /tmp` and bracketed paste off
- WHEN `y` is pressed
- THEN one WritePty carries exactly `cd /tmp`

#### Scenario: State read at accept time [auto]
- GIVEN a confirm opened with bracketed paste on, and the shell turned it off before `y`
- WHEN `y` is pressed with a multi-line command
- THEN the write is refused

#### Scenario: Real shells [manual, Kitty]
- GIVEN bash, zsh and fish panes
- WHEN a two-line command is accepted in each
- THEN it appears as an editable two-line buffer in each and none of it runs until Enter

### Requirement: Accept-time rechecks
At accept time, before any byte is written, the target MUST be rechecked: the pane MUST still exist, MUST still be a shell pane of the same tab, and MUST NOT be on the alternate screen (a full-screen app such as nvim, htop or a pager). When a recheck fails the write MUST be refused with a notice naming the reason, and the confirm MUST close; no byte is written to any other pane. When the target pane was closed while the confirm was pending, `y` MUST be refused and the cycle key MAY select another shell pane; a command is never redirected to another pane without the prompt naming it.

#### Scenario: Full-screen app refused [auto]
- GIVEN a pending confirm on pane 2 and pane 2 enters the alternate screen
- WHEN `y` is pressed
- THEN nothing is written and the notice says pane 2 runs a full-screen app

#### Scenario: Pane closed before approval [auto]
- GIVEN a pending confirm on pane 2 and pane 2's shell exits before the user answers
- WHEN `y` is pressed
- THEN no WritePty is emitted to any pane, the notice says the pane is gone, and no panic occurs

#### Scenario: Closed pane then cycle [auto]
- GIVEN the same race with pane 1 still open
- WHEN `p` is pressed and the prompt names pane 1, then `y` is pressed
- THEN one WritePty is addressed to pane 1 only

#### Scenario: Never redirected silently [auto]
- GIVEN a pending confirm on pane 2 and focus moves to pane 3
- WHEN `y` is pressed
- THEN the write, if any, is addressed to pane 2

#### Scenario: Stale events after close [auto]
- GIVEN a closed target pane
- WHEN a late output or exit event for it arrives while the confirm is pending
- THEN it is ignored with no panic

#### Scenario: Half-typed line [design-confirm]
- GIVEN the target shell has user text on its input line
- WHEN the confirm is accepted
- THEN the behaviour pinned by design (refuse, or type after the existing text with the prompt warning about it) applies, and the prompt text says which

### Requirement: Confirm lifecycle
A chat pane MUST have at most one pending confirm. The confirm MUST be closed (nothing written) when its chat pane is closed, when the stream that requested it is cancelled, when tools are turned off, and when the app quits. Opening a second confirm while one is pending MUST replace nothing: the new request is refused with a notice. A confirm MUST survive mode changes, focus changes and tab switches of its chat pane, and `y` MUST reach it only while that chat pane is focused in TERMINAL.

#### Scenario: Close pane cancels [auto]
- GIVEN a pending confirm in a chat pane
- WHEN the chat pane is closed
- THEN the confirm is gone and no WritePty is emitted

#### Scenario: Survives a tab switch [auto]
- GIVEN a pending confirm and a switch to another tab and back
- WHEN the chat pane is shown
- THEN the same confirm is pending and unanswered

#### Scenario: Second request refused [auto]
- GIVEN a pending confirm
- WHEN the run action is used again
- THEN the pending confirm is unchanged and a notice says one is already open

#### Scenario: Keys elsewhere do not answer [auto]
- GIVEN a pending confirm in chat pane A and focus on a shell pane
- WHEN `y` is pressed
- THEN `y` goes to the shell pane's PTY as normal input and the confirm stays pending

### Requirement: No output capture
The system MUST NOT read back what the command printed, and MUST NOT send any pane output to the model. The outcome of a run action is a system line only (`sent to pane N; press Enter there to run it` or `cancelled`), which is never stored and never sent to the model.

#### Scenario: Outcome line only [auto]
- GIVEN an accepted confirm
- WHEN the pane produces output
- THEN the transcript holds only the outcome system line and the next request's history holds no pane output
