# Delta for Copy Mode Scrollback

All scenarios [auto] unless tagged. Shell-pane COPY (cursor, selection, yank, motions) is unchanged. COPY now also applies to chat panes, with line scrolling only: no cursor, selection or yank on a chat pane in this change (deferred to a follow-up; `Ctrl+Y` and `Ctrl+B` still copy the last answer or code block).

Archive-order dependency: the requirement "COPY on a chat pane" is introduced by the not yet archived ai-chat change (`openspec/changes/ai-chat/specs/copy-mode-scrollback/spec.md`) as a refusal. pane-scroll MUST be archived AFTER ai-chat; this delta then replaces that requirement. The ai-chat requirement "Chat streams continue during COPY" is kept as is.

## MODIFIED Requirements

### Requirement: COPY on a chat pane
Entering COPY (`[` in PREFIX) while the focused pane is a chat pane MUST succeed when the pane shows the conversation view: the mode becomes COPY and the transcript is scrolled by lines through the keys of modal-input (Chat COPY keys). COPY on a chat pane has no cursor, no selection and no yank: the vt100 state is not involved and no COPY cursor state is created. When the chat pane shows the conversation list, the tag editor, the note editor, the summary draft, a delete confirmation or an engine guidance state with no transcript, entering COPY MUST leave the mode TERMINAL and show the notice `nothing to scroll here` through the existing notice slot until the next key. The previous refusal notice `copy mode is not available in chat panes; Ctrl+Y copies the last answer` MUST NOT exist any more.
(Previously: entering COPY on a chat pane was refused with the notice `copy mode is not available in chat panes; Ctrl+Y copies the last answer`)

#### Scenario: Enter on a chat pane
- GIVEN a focused chat pane in PREFIX showing a conversation
- WHEN `[` is pressed
- THEN the mode is COPY, no notice is shown, the composer text is unchanged and no WritePty is emitted

#### Scenario: Enter on the conversation list
- GIVEN a focused chat pane in PREFIX showing the conversation list
- WHEN `[` is pressed
- THEN the mode is TERMINAL and the notice `nothing to scroll here` is shown until the next key

#### Scenario: Notice clears on the next key
- GIVEN the notice is shown
- WHEN any key is pressed
- THEN the notice is gone and the statusline shows the chat segments again

#### Scenario: COPY on a shell pane beside a chat pane
- GIVEN a shell pane focused next to a chat pane
- WHEN `Ctrl+Space [` is pressed
- THEN the mode is COPY on the shell pane as before and the chat pane keeps its scroll position

## ADDED Requirements

### Requirement: Chat COPY navigation
In COPY on a chat pane the transcript MUST scroll by lines (see chat-pane, Transcript scrolling): `j`/`k` one line down/up, Ctrl+d/Ctrl+u half a page of the transcript viewport down/up, `gg` to the top of the transcript, `G` to the bottom. Movement MUST clamp at both ends with no error. The view MUST stay on the same content when deltas arrive (pinned, the stream keeps running and being saved); `G` or reaching the bottom does not leave COPY. Leaving COPY (Esc or `q`) returns the transcript to the bottom and follow mode, and sets the mode to TERMINAL. Isolation: no key is delivered to the composer or to any PTY while in chat COPY.

#### Scenario: Line motion
- GIVEN a transcript of 100 lines in a 20-line viewport at the bottom, in COPY
- WHEN `k` is pressed
- THEN the view is 1 line above the bottom; `j` returns it

#### Scenario: Half page
- GIVEN a chat pane in COPY with a 20-line viewport
- WHEN Ctrl+u is pressed
- THEN the view moves up 10 lines; Ctrl+d moves it down 10

#### Scenario: Top and bottom
- GIVEN a chat pane in COPY mid transcript
- WHEN `g` `g` then `G` are pressed
- THEN the view is at the top, then at the bottom

#### Scenario: Clamping
- GIVEN the view at the top of the transcript
- WHEN `k` or Ctrl+u is pressed
- THEN the view is unchanged; likewise `j`/Ctrl+d at the bottom

#### Scenario: Short transcript
- GIVEN a transcript shorter than the viewport, in COPY
- WHEN `k`, Ctrl+u and `gg` are pressed
- THEN the view is unchanged and nothing panics

#### Scenario: Pinned while a stream runs
- GIVEN COPY on a chat pane scrolled up 10 lines with a stream running
- WHEN 5 lines of delta arrive
- THEN the same content stays visible

#### Scenario: Exit returns to follow
- GIVEN COPY on a chat pane scrolled up
- WHEN Esc (or `q`) is pressed
- THEN the mode is TERMINAL and the last transcript line is visible and follows new deltas

#### Scenario: Composer untouched
- GIVEN a chat pane with composer text `hi` in COPY
- WHEN `j`, `x`, Enter and Backspace are pressed
- THEN the composer text is `hi`, no ChatSend and no WritePty is emitted

#### Scenario: Lone g
- GIVEN `g` pending in chat COPY
- WHEN any non-`g` key is pressed
- THEN the pending `g` is discarded

#### Scenario: Per-pane scroll state
- GIVEN a shell pane and a chat pane, COPY on the chat pane scrolled up 10 lines
- WHEN the shell pane's view offset is inspected
- THEN it is unchanged (0)

#### Scenario: Focus change exits chat COPY
- GIVEN COPY on a chat pane scrolled up
- WHEN focus moves to another pane or the tab changes
- THEN the mode is TERMINAL and the chat transcript is at the bottom and following

### Requirement: Scrolled view badge
Any pane whose view is scrolled back MUST show the badge `[+N]` in its top-right corner, over the content rectangle: for a shell pane N is the scrollback offset (rows above the live bottom, whether the pane is in COPY or only scrolled by the wheel); for a chat pane N is the number of transcript lines below the viewport. The badge MUST NOT be shown at offset 0 (live, or following), on the alternate screen, or on a pane too narrow to hold it. It MUST update as the view moves and as anchored output arrives, MUST NOT shift the pane's content or its PTY size, and MUST NOT panic at any pane size. The badge is a drawing overlay: it is not part of the pane's text, selection or yank.

#### Scenario: Shell pane scrolled by wheel
- GIVEN a main-screen pane at offset 12 and not in COPY
- WHEN rendered
- THEN `[+12]` is drawn in the pane's top-right corner

#### Scenario: Not shown when live
- GIVEN a pane at offset 0
- WHEN rendered
- THEN no badge is drawn

#### Scenario: Shown in COPY
- GIVEN COPY on a pane at offset 7
- WHEN rendered
- THEN `[+7]` is drawn in that pane

#### Scenario: Chat pane scrolled
- GIVEN a chat pane whose view is 9 lines above the bottom
- WHEN rendered
- THEN `[+9]` is drawn in its top-right corner, and no badge is drawn when it is at the bottom

#### Scenario: Anchored output updates the badge
- GIVEN a scrolled shell pane at `[+10]`
- WHEN 5 lines of output arrive
- THEN the badge reads `[+15]`

#### Scenario: Unfocused scrolled pane
- GIVEN a scrolled pane that is not focused
- WHEN rendered
- THEN its badge is drawn

#### Scenario: Tiny pane
- GIVEN a scrolled pane 3 columns wide
- WHEN rendered
- THEN no badge is drawn and nothing panics

#### Scenario: Alternate screen
- GIVEN an alternate-screen pane
- WHEN rendered
- THEN no badge is drawn
