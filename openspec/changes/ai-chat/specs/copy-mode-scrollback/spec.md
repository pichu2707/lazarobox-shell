# Delta for Copy Mode Scrollback

All scenarios [auto] unless tagged. COPY applies to shell panes only; the vt100 state, cursor and selection model are unchanged. The rest of this spec is not modified.

## ADDED Requirements

### Requirement: COPY on a chat pane
Entering COPY (`[` in PREFIX) while the focused pane is a chat pane MUST be refused: the mode returns to TERMINAL, no COPY state is created, no vt100 state is touched, and the notice `copy mode is not available in chat panes; Ctrl+Y copies the last answer` is shown through the existing notice slot until the next key. COPY on a shell pane next to a chat pane is unchanged.

#### Scenario: Refused on a chat pane
- GIVEN a focused chat pane in PREFIX
- WHEN `[` is pressed
- THEN mode is TERMINAL, no selection or cursor state exists and the notice is shown

#### Scenario: Notice clears on the next key
- GIVEN the refusal notice is shown
- WHEN any key is pressed
- THEN the notice is gone and the statusline shows the chat segments again

#### Scenario: COPY on a shell pane beside a chat pane
- GIVEN a shell pane focused next to a chat pane
- WHEN `Ctrl+Space [` is pressed
- THEN mode is COPY on the shell pane as before

### Requirement: Chat streams continue during COPY
While COPY is active on a shell pane, chat panes (in any tab) MUST keep streaming: deltas MUST keep being applied and saved, and nothing in COPY MUST be affected by them. Leaving COPY MUST show the accumulated chat state.

#### Scenario: Stream while in COPY
- GIVEN COPY on the left shell pane scrolled up 10 lines and a chat pane streaming on the right
- WHEN 3 deltas arrive
- THEN the shell viewport and selection are unchanged and the chat transcript holds the 3 deltas

#### Scenario: Focus moves to the chat pane
- GIVEN COPY on the left shell pane and a streaming chat pane on the right
- WHEN focus moves to the chat pane
- THEN COPY exits per the existing rule, the left viewport returns to the bottom and the stream continues
