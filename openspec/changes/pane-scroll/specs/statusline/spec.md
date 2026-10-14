# Delta for Statusline

Tags: [auto] = `cargo test`; scenarios without a tag are [auto]. Existing statusline snapshots MUST remain valid except those rendered in COPY (which gain the key hint) and those with a scrolled focused pane; snapshots of TERMINAL with the focused pane at the bottom or live are unaffected. The mode block, mode colors and the other requirements are unchanged.

Archive-order dependency: the "Chat pane segments" and "Mode block" requirements (`CHAT` label) come from the not yet archived ai-chat change; pane-scroll MUST be archived after ai-chat so that `CHAT` exists as the label this delta extends.

## ADDED Requirements

### Requirement: COPY key hint
While the mode is COPY and no notice is shown, the path segment MUST show a one-line key hint instead of the cwd, in the same style and with the same whole-entry clipping rule as the PREFIX key hint (entries joined by ` · `, clipped by whole entries ending with `…`, never a partial entry). The mode block keeps the label `COPY` (and its position indicator and selection marker); the hint does not repeat the label. For a shell pane the hint is `j/k scroll · v select · y copy · Esc exit`. For a chat pane (no selection or yank in this change) it is `j/k scroll · Esc exit`. A notice (for example `Copied N characters`, `nothing to scroll here` or a config notice) replaces the hint until the next key, as it replaces the cwd. On a very narrow width the mode block takes priority, the hint may be empty and rendering MUST NOT panic. The hint is static text per pane kind; it does not change with the selection state.

#### Scenario: Shell COPY hint [auto, TestBackend + insta]
- GIVEN COPY on a shell pane on a wide terminal
- WHEN rendered
- THEN the mode block reads `COPY` (with its position indicator) and the path segment shows `j/k scroll · v select · y copy · Esc exit`

#### Scenario: Chat COPY hint [auto, TestBackend + insta]
- GIVEN COPY on a chat pane on a wide terminal
- WHEN rendered
- THEN the path segment shows `j/k scroll · Esc exit`, with no `select` or `copy` entry

#### Scenario: Chat COPY always shows a position [auto]
- GIVEN COPY on a chat pane whose view is at the bottom
- WHEN rendered
- THEN the mode block reads `COPY ↑0/{total}` (the position is shown even at offset 0), and `↑N/{total}` once scrolled

#### Scenario: Hint with a selection [auto]
- GIVEN COPY on a shell pane with a character selection
- WHEN rendered
- THEN the mode block shows `VISUAL` as before and the path segment still shows the shell hint

#### Scenario: Notice replaces the hint [auto]
- GIVEN COPY on a shell pane after a yank that stays in COPY with the notice `Nothing to copy`
- WHEN rendered
- THEN the path segment shows the notice, not the hint, and after the next key the hint is back

#### Scenario: Hint gone after leaving COPY [auto]
- GIVEN COPY with the hint shown
- WHEN Esc is pressed
- THEN the statusline shows TERMINAL (or CHAT) and the cwd again

#### Scenario: Hint clipped by whole entries [auto]
- GIVEN COPY on a shell pane and a path segment too narrow for the full hint
- WHEN rendered
- THEN only the entries that fit are shown followed by `…`, with no partial entry

#### Scenario: Narrow width [auto]
- GIVEN COPY on any pane and a terminal width from 0 to 40
- WHEN rendered
- THEN nothing panics and the mode block is intact whenever it fits

### Requirement: Scroll position outside COPY
While the mode is TERMINAL and the focused pane is scrolled back without COPY (a main-screen shell pane with a non-zero offset, or a chat pane whose view is above the bottom), the mode block MUST show the position `↑{offset}/{total}` right after the mode label (`TERMINAL` or `CHAT`), before `[Z]`, using the same placement, format and path-budget rule as the COPY position indicator (see copy-mode-scrollback): `offset` is the rows (lines) above the live bottom and `total` the rows (lines) of history (for a chat pane, the scrollable lines of the transcript). It MUST NOT be shown at offset 0, on the alternate screen, or when the focused pane is not scrolled; scrolled panes that are not focused show only their pane badge. It MUST follow wheel scrolling and anchored output, the path budget shrinks by its width, and narrow widths MUST NOT panic.

#### Scenario: Shell pane scrolled by wheel [auto, TestBackend + insta]
- GIVEN TERMINAL with the focused main-screen pane at offset 12 of 340 rows of history
- WHEN rendered
- THEN the mode block reads `TERMINAL ↑12/340`

#### Scenario: Chat pane scrolled [auto]
- GIVEN a focused chat pane whose view is 9 lines above the bottom of a transcript with 120 scrollable lines
- WHEN rendered
- THEN the mode block reads `CHAT ↑9/120`

#### Scenario: Nothing at the bottom [auto]
- GIVEN the focused pane live (offset 0)
- WHEN rendered
- THEN the mode block reads `TERMINAL` (or `CHAT`) with no position, as before

#### Scenario: Unfocused scrolled pane [auto]
- GIVEN the focused pane live and another pane scrolled
- WHEN rendered
- THEN the mode block carries no position

#### Scenario: Follows output and wheel [auto]
- GIVEN the focused shell pane at `↑10/N`
- WHEN 5 lines scroll out, and then a wheel-down step arrives
- THEN the block reads `↑15/N+5`, then `↑12/N+5`

#### Scenario: Returns to live [auto]
- GIVEN the focused pane at `↑3/N`
- WHEN a key is written to its PTY
- THEN the block reads `TERMINAL` with no position

#### Scenario: Alternate screen [auto]
- GIVEN a focused alternate-screen pane
- WHEN rendered after wheel events
- THEN the mode block reads `TERMINAL` with no position

#### Scenario: Zoom marker order [auto]
- GIVEN a zoomed scrolled focused pane
- WHEN rendered
- THEN the order is the label, then `↑offset/total`, then `[Z]`

#### Scenario: Narrow width [auto]
- GIVEN a scrolled focused pane and widths from 0 to 40
- WHEN rendered
- THEN nothing panics
