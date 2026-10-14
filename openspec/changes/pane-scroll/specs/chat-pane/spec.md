# Delta for Chat Pane

Tags: [auto] = `cargo test`; [manual] = real Kitty. Scenarios without a tag are [auto]. Everything else in the chat pane spec is unchanged.

Archive-order dependency: the requirements below are introduced by the not yet archived ai-chat change (`openspec/changes/ai-chat/specs/chat-pane/spec.md`, requirements "Transcript scrolling" and "State survives modes, COPY and tabs"). pane-scroll MUST be archived AFTER ai-chat; this delta then replaces those texts.

## MODIFIED Requirements

### Requirement: Transcript scrolling
PageUp and PageDown MUST scroll the transcript by one page. The transcript MUST also scroll by lines: one line per `j`/`k` in COPY, half a page per Ctrl+d/Ctrl+u in COPY, to the top and bottom with `gg` and `G` in COPY (see modal-input, Chat COPY keys), and 3 lines per mouse wheel step (see mouse-input), up or down. Line, half-page and wheel steps are computed on the rendered (wrapped) lines. While the view is at the bottom it MUST follow new deltas. While scrolled up the view MUST stay on the same content when deltas arrive. Reaching the bottom by a scroll step (wheel down, `j`, `G` or a page) puts the view back in follow mode. Sending a message MUST return the view to the bottom. Scrolling MUST clamp at both ends, and a transcript shorter than the viewport does not scroll.
(Previously: only PageUp and PageDown scrolled, by whole pages)

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

### Requirement: State survives modes, COPY and tabs
A chat pane's state (composer text and cursor, transcript, scroll position, running stream) MUST survive every mode change (PREFIX, RESIZE, COPY on another pane, MENU), focus changes, zoom, splits of other panes and tab switches, with one exception: COPY on the chat pane itself. Leaving that COPY (Esc, `q`, or a focus or tab change, see copy-mode-scrollback) returns the transcript to the bottom and follow mode; a view scrolled by the mouse wheel or by PageUp/PageDown WITHOUT COPY is not reset by focus changes or tab switches. A running stream MUST keep running in the background while its pane is not focused, not visible, or its tab is not active; deltas MUST keep accumulating and a final event MUST persist the exchange. While MENU is open no key or paste reaches the composer.
(Previously: COPY was never available on a chat pane, so no exception existed)

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
