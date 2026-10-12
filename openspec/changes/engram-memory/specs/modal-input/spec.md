# Delta for Modal Input

All scenarios [auto] unless tagged. The Chat pane input table and its swallow rule are unchanged except for the keys added below, which are exempt from the swallow rule. The key bindings are defaults for design to confirm, chosen in the style of Ctrl+Y, Ctrl+B, Ctrl+T and Ctrl+R (Ctrl+N save note, Ctrl+O toggle the memory block, Ctrl+E summary draft, as in the design), and are an open question until confirmed in Kitty (manual check, with M8).

## ADDED Requirements

### Requirement: Memory keys
In TERMINAL, with a chat pane focused and the conversation view shown, three keys MUST be added to the Chat pane input table:

| Key | Effect |
|---|---|
| Ctrl+N | open the note editor on the last answer (see chat-memory) |
| Ctrl+E | request the end-of-conversation summary (see chat-memory) |
| Ctrl+O | expand or collapse the memory block (see chat-memory); it never touches the reasoning block, which stays on Ctrl+R |

Repeat events of Ctrl+N, Ctrl+E and Ctrl+O MUST be ignored, as for Ctrl+Y and Ctrl+B. With memory off or not ready, Ctrl+N and Ctrl+E only show their notices and Ctrl+O does nothing when no memory line exists. These keys MUST NEVER reach a PTY and MUST NOT type anything into the composer. In the conversation list, the tag editor and the delete confirmation they do nothing.

#### Scenario: Ctrl+N opens the editor [auto]
- GIVEN a conversation with an answer and a ready memory
- WHEN Ctrl+N is pressed
- THEN the note editor opens, no WritePty is emitted and the composer text is unchanged

#### Scenario: Ctrl+E requests a summary [auto]
- GIVEN an idle conversation with an answer and a ready memory
- WHEN Ctrl+E is pressed
- THEN one summary request effect is emitted and no WritePty is emitted

#### Scenario: Ctrl+O and Ctrl+R are independent [auto]
- GIVEN a last turn with a collapsed reasoning block and a collapsed memory line
- WHEN Ctrl+O is pressed, then Ctrl+R
- THEN the memory line expands first and the reasoning block expands second, each only on its own key

#### Scenario: Repeat ignored [auto]
- GIVEN a focused chat pane
- WHEN Repeat events for Ctrl+N, Ctrl+E and Ctrl+O arrive
- THEN nothing happens and no effect is emitted

#### Scenario: Memory off [auto]
- GIVEN memory off
- WHEN Ctrl+N and Ctrl+E are pressed
- THEN only the notice `Memory is off; enable it in the menu (Ctrl+Space m)` appears and no effect is emitted

#### Scenario: Other views ignore them [auto]
- GIVEN the conversation list shown
- WHEN Ctrl+N, Ctrl+E and Ctrl+O are pressed
- THEN nothing changes and nothing is typed

### Requirement: Memory view keys
While the note editor or the summary editor is open in a chat pane, keys act as follows and every other key that is not a printable character MUST be swallowed (no effect, no WritePty); Ctrl+C MUST NEVER emit Quit or a WritePty:

| Key | Effect |
|---|---|
| printable characters, Backspace, Delete, Left, Right, Home, End | edit the focused text field (on the category field, Left and Right cycle the category and printable characters do nothing) |
| Up, Down | move the cursor between lines of the content field |
| Enter | newline in the content field; in the title field, save (note editor); in the summary editor, newline |
| Tab, Shift+Tab | next or previous field (the summary editor has one field) |
| Ctrl+S | save (note) or approve (summary), including the second redaction pass |
| Esc | discard the view |
| Ctrl+Space | enter PREFIX as in any pane |

A paste is inserted into the focused text field with control and format characters dropped and CRLF turned into newline, never saved, and clipped at the field cap. Repeat is handled like Press for editing keys and ignored for Ctrl+S and Esc. In the start-server confirmation only an unmodified lowercase `y` confirms and any other key declines. While MENU is open no key or paste reaches a memory view.

#### Scenario: Typing and field cap [auto]
- GIVEN the title field holding 120 characters
- WHEN another character is typed
- THEN the title is unchanged

#### Scenario: Category cycles [auto]
- GIVEN the category field on `decision`
- WHEN Right is pressed, and then Left twice
- THEN the category is `architecture`, then `decision`, then `preference` (wrap)

#### Scenario: Enter in the title saves, in the content adds a line [auto]
- GIVEN a valid note editor with the title field focused, and in another run the content field focused
- WHEN Enter is pressed
- THEN the first emits one save effect and the second inserts a newline and emits none

#### Scenario: Ctrl+S saves, Esc discards [auto]
- GIVEN a valid note editor
- WHEN Ctrl+S is pressed, and in another run Esc
- THEN the first emits one save effect and the second emits none

#### Scenario: Paste into the focused field [auto]
- GIVEN the content field focused
- WHEN a paste of `x\r\ny\u{1b}[31m` arrives
- THEN the field holds `x`, newline, `y[31m` and nothing is saved

#### Scenario: Unlisted keys swallowed, Ctrl+C never quits [auto]
- GIVEN the note editor open
- WHEN F5, Ctrl+X and Ctrl+C are pressed
- THEN nothing changes and no Quit or WritePty is emitted

#### Scenario: Server offer accepts only y [auto]
- GIVEN the start-server confirmation
- WHEN `Y`, `n`, Enter and then (in another run) `y` are pressed
- THEN the first three decline and only `y` confirms

#### Scenario: Prefix still works [auto]
- GIVEN the note editor open
- WHEN Ctrl+Space then `w` then `v` are pressed
- THEN a split is made and the editor is still open in its pane

#### Scenario: Real keys in Kitty [manual, Kitty]
- GIVEN a chat with an answer and memory ready, in Kitty
- WHEN the user presses Ctrl+N, edits the fields, saves with Ctrl+S, and presses Ctrl+O and Ctrl+E
- THEN each key is received as listed and none is intercepted by the terminal
