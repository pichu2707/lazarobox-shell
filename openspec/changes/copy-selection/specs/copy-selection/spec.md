# Copy Selection Specification

## Purpose

Vim-style character and line selection in COPY and yank to the system clipboard via OSC 52. All scenarios [auto] unless tagged. "Row" means an absolute row of the pane's scrollback plus screen; "cursor" is the COPY cursor (see copy-mode-scrollback).

## Requirements

### Requirement: Selection kinds
In COPY, `v` MUST start a character selection and `V` a line selection, anchored at the cursor's current cell/row. Pressing the same key again MUST clear the selection; pressing the other key MUST switch the kind and keep the anchor. While a selection exists, every cursor motion (`h` `j` `k` `l` `0` `$` `gg` `G` Ctrl+d Ctrl+u) MUST extend it from the anchor, in either direction. The selection is per focused pane.

#### Scenario: Start character selection
- GIVEN COPY with the cursor at row 5, col 3 and no selection
- WHEN `v` is pressed
- THEN the selection kind is character, anchored at (5, 3), and the statusline shows `VISUAL`

#### Scenario: Start line selection
- GIVEN COPY with no selection
- WHEN `V` is pressed
- THEN the selection kind is line and the statusline shows `V-LINE`

#### Scenario: Same key clears
- GIVEN a character selection
- WHEN `v` is pressed
- THEN there is no selection and the marker is gone; mode stays COPY

#### Scenario: Other key switches kind
- GIVEN a character selection anchored at (5, 3)
- WHEN `V` is pressed
- THEN the kind is line, the anchor is still row 5, and the marker reads `V-LINE`

#### Scenario: Motion extends backwards
- GIVEN `v` pressed at (5, 3)
- WHEN `h` is pressed twice
- THEN the selection covers (5, 1) through (5, 3) inclusive

### Requirement: Text extraction
The selected text MUST be built from the pane's cells as follows. A character selection covers both end cells inclusively. A line selection covers whole rows. Rows are joined with `\n`, with no `\n` after the last row. Rows that are soft-wrapped into the next row MUST be joined with no `\n`. Trailing blank cells MUST be trimmed from every row that ends a logical line and from the last row of the selection. A soft-wrapped row that is not the last row MUST keep its trailing spaces, because they are real content at the wrap point. A wide character MUST be copied once, even when either of its two cells is selected. Rows outside the viewport MUST be included.

#### Scenario: Character selection within a row
- GIVEN a row `hello world`, cursor at col 0
- WHEN `v`, `l` x4 are pressed
- THEN the extracted text is `hello`

#### Scenario: Multi-row line selection
- GIVEN rows `foo` and `bar` (padded with blanks)
- WHEN the cursor is on `foo` and `V`, `j` are pressed
- THEN the extracted text is `foo\nbar`

#### Scenario: Soft-wrapped rows
- GIVEN a 10-column pane where `abcdefghijklmn` wrapped onto two rows
- WHEN both rows are selected with `V`
- THEN the extracted text is `abcdefghijklmn`

#### Scenario: Soft-wrapped row keeps trailing spaces
- GIVEN a 10-column pane where `hello     world` wrapped after col 10, so row 1 is `hello` plus 5 trailing spaces and row 2 is `world`
- WHEN both rows are selected with `V`
- THEN the extracted text is `hello     world` (5 spaces kept)

#### Scenario: Trailing blanks trimmed
- GIVEN a row `ab` followed by blank cells
- WHEN the selection covers the whole row with `v` and `$`
- THEN the extracted text is `ab`

#### Scenario: Wide character once
- GIVEN a row `a漢b` (the wide character occupies cols 1-2)
- WHEN `v` is pressed at col 2 and `l` once
- THEN the extracted text is `漢b`

#### Scenario: Selection spans scrollback
- GIVEN a selection anchored at a row that is above the viewport
- WHEN the text is extracted
- THEN it includes the off-screen rows and the viewport offset is unchanged afterwards

### Requirement: Yank to the clipboard via OSC 52
`y` with a selection MUST emit one `SetClipboard` effect carrying the extracted text, which the runtime writes to the host terminal as `ESC ] 52 ; c ; <base64 of the UTF-8 text> BEL`. After `y` the mode MUST be TERMINAL with the viewport at the bottom, and the selection dropped. `y` with no selection MUST do nothing. No key in COPY, including `y`, MUST be written to the PTY.

#### Scenario: Yank emits OSC 52
- GIVEN the extracted text `hello`
- WHEN `y` is pressed
- THEN the written sequence is `\x1b]52;c;aGVsbG8=\x07`, mode is TERMINAL, the viewport is at the bottom and no WritePty is emitted

#### Scenario: Yank without selection
- GIVEN COPY with no selection
- WHEN `y` is pressed
- THEN no effect is emitted and mode stays COPY

#### Scenario: Paste elsewhere [manual, Kitty]
- GIVEN `v`, a motion, then `y`
- WHEN pasting into another application
- THEN the pasted text equals the selection exactly

#### Scenario: Line yank runs nothing [manual, Kitty]
- GIVEN a line yank of a shell command
- WHEN pasting into a shell prompt
- THEN the text appears without a trailing newline and nothing runs

#### Scenario: Yank over ssh [manual, Kitty]
- GIVEN lazarobox running in an ssh session
- WHEN `y` is pressed on a selection
- THEN the text reaches the local clipboard

### Requirement: Size cap
If the base64 payload would exceed 100,000 bytes, `y` MUST emit no `SetClipboard` effect, MUST keep COPY and the selection, and MUST set the notice `Selection too large to copy`. A payload of exactly 100,000 bytes MUST be sent.

#### Scenario: At the cap
- GIVEN a selection of 75,000 bytes (base64 length 100,000)
- WHEN `y` is pressed
- THEN `SetClipboard` is emitted

#### Scenario: Over the cap
- GIVEN a selection of 75,001 bytes (base64 length 100,004)
- WHEN `y` is pressed
- THEN no `SetClipboard` is emitted, mode stays COPY with the selection, and the notice reads `Selection too large to copy`

#### Scenario: Oversized in a real terminal [manual, Kitty]
- GIVEN an oversized selection
- WHEN `y` is pressed
- THEN the too-large notice is shown and the clipboard is unchanged

### Requirement: Selection lifetime
The selection MUST be dropped on `q`, `i`, `y`, a host Resize (including a layout change that resizes the focused pane), a switch between the main and the alternate screen, a focus or active-tab change, and on any exit of COPY. Esc with a selection MUST clear it and stay in COPY. Output arriving while selecting MUST NOT move the selection off its content (anchor and cursor are absolute rows). If history eviction removes the anchor row, the anchor MUST clamp to column 0 of the oldest retained row without panic (the selection shrinks and is kept).

#### Scenario: Esc clears then exits
- GIVEN a selection in COPY
- WHEN Esc is pressed, then Esc again
- THEN after the first the selection is gone and mode is COPY; after the second mode is TERMINAL

#### Scenario: `q` drops the selection
- GIVEN a selection in COPY
- WHEN `q` is pressed and COPY is re-entered
- THEN mode was TERMINAL after `q` and no selection exists

#### Scenario: Resize drops it
- GIVEN a selection in COPY
- WHEN a host Resize arrives
- THEN there is no selection and mode stays COPY

#### Scenario: Screen switch drops it
- GIVEN a selection in COPY on the main screen
- WHEN the child switches to the alternate screen (or back)
- THEN there is no selection and mode stays COPY

#### Scenario: Screen switch resets the cursor [auto]
- GIVEN COPY with the cursor moved up into scrollback
- WHEN the child switches to the alternate screen and back (or the host resizes)
- THEN the cursor is on the pane's live cursor, as when entering COPY, and the viewport is at the bottom

#### Scenario: Focus change drops it
- GIVEN a selection in COPY on the left pane
- WHEN focus moves to the right pane
- THEN mode is TERMINAL and no selection exists

#### Scenario: Output keeps the selection on its content
- GIVEN a selection on rows containing `foo`
- WHEN 5 new lines of output arrive
- THEN the selected text is still `foo`

#### Scenario: Eviction of the anchor
- GIVEN a selection whose anchor row is evicted at the scrollback cap
- WHEN new output arrives
- THEN the anchor is (oldest retained row, col 0), the selection is kept and nothing panics

### Requirement: Selection highlight
Selected cells MUST be rendered with a style that differs from unselected cells; a line selection MUST highlight each selected row across the full pane width. The highlight MUST follow the selection when the viewport scrolls and MUST NOT appear without a selection.

#### Scenario: Character highlight [TestBackend + insta]
- GIVEN `v` and `l` x2 on `hello`
- WHEN rendered
- THEN cells `hel` carry the selection style and `lo` do not

#### Scenario: Line highlight [TestBackend + insta]
- GIVEN `V` on a row
- WHEN rendered
- THEN every cell of that row carries the selection style

#### Scenario: No selection
- GIVEN COPY with no selection
- WHEN rendered
- THEN no cell carries the selection style
